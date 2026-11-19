use crate::{
    AVCodecContext, AVFormatContext, AVFrame, AVPacket, AVPixelFormat, AVRational, Error, Result, SwsContext, VideoStreamFormat,
};
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};
use tracing::error;

pub struct Video {
    stream_format: VideoStreamFormat,
    frame_rate: AVRational,
    time_base: AVRational,
    duration: f64,
    start_time: i64,

    position: Arc<(Mutex<DecodePosition>, Condvar)>,
    frame: Arc<Mutex<DecodedFrame>>,
    decode_thread: Option<JoinHandle<()>>,
}

struct DecodePosition {
    timestamp: i64,
    rewind: bool,
    generation: u64,
}

struct DecodedFrame {
    frame: AVFrame,
    timestamp: i64,
    generation: u64,
}

impl Video {
    pub fn open(file: impl AsRef<str>, pix_fmt: AVPixelFormat) -> Result<Self> {
        let mut format_ctx = AVFormatContext::new()?;
        format_ctx.open_input(file.as_ref())?;
        format_ctx.find_stream_info()?;

        let video_stream = format_ctx.streams().into_iter().find(|it| it.is_video()).ok_or(Error::NoVideoStream)?;
        let base = video_stream.time_base();
        let time_base = base.to_f64();
        let duration = video_stream.duration() as f64 * time_base;
        let duration = if duration > 0. { duration } else { format_ctx.duration_seconds() };
        let start_time = video_stream.start_time();
        let frame_rate = video_stream.frame_rate();

        let decoder = video_stream.find_decoder()?;
        let mut codec_ctx = AVCodecContext::new(decoder, video_stream.codec_params(), Some(pix_fmt))?;

        let stream_format = codec_ctx.video_stream_format();
        let out_format = VideoStreamFormat {
            pix_fmt,
            ..stream_format.clone()
        };

        let mut sws = SwsContext::new(stream_format.clone(), out_format.clone())?;
        let mut in_frame = AVFrame::new()?;
        let mut out_frame = AVFrame::new()?;
        out_frame.set_video_format(&out_format);
        out_frame.get_buffer()?;

        let mut buf_frame = AVFrame::new()?;
        buf_frame.set_video_format(&out_format);
        buf_frame.get_buffer()?;

        let position = Arc::new((Mutex::new(DecodePosition { timestamp: start_time, rewind: false, generation: 0 }), Condvar::new()));
        let frame = Arc::new(Mutex::new(DecodedFrame { frame: buf_frame, timestamp: -1, generation: 0 }));

        let video_index = video_stream.index();

        let decode_thread = std::thread::spawn({
            let position = position.clone();
            let frame = frame.clone();

            move || {
                let mut run = || -> Result<()> {
                    let mut packet = AVPacket::new()?;
                    let mut current_ts = -1;

                    let mut catching_up_to: Option<i64> = None;

                    loop {
                        let (ts, rewind, generation) = {
                            let mut guard = position.0.lock().unwrap();
                            loop {
                                let ts = guard.timestamp;
                                if ts == i64::MAX {
                                    return Ok(());
                                }

                                if std::mem::take(&mut guard.rewind) {
                                    break (ts, true, guard.generation);
                                }
                                let diff_sec = (current_ts - ts) as f64 * time_base;
                                if current_ts != -1 && diff_sec > 0.0 && diff_sec < 0.5 {
                                    guard = position.1.wait(guard).unwrap();
                                } else {
                                    break (ts, false, guard.generation);
                                }
                            }
                        };

                        if rewind || (current_ts - ts) as f64 * time_base > 0.5 {
                            let is_already_catching_up = catching_up_to.is_some_and(|target| ((ts - target) as f64 * time_base).abs() < 0.5);

                            if rewind || !is_already_catching_up {
                                format_ctx.seek_frame(video_index, ts, crate::ffi::AVSEEK_FLAG_BACKWARD)?;
                                codec_ctx.flush_buffers();

                                catching_up_to = Some(ts);
                                current_ts = -1;
                                continue;
                            }
                        }

                        if !format_ctx.read_frame(&mut packet)? {
                            drop(position.1.wait_while(position.0.lock().unwrap(), |request| {
                                request.timestamp == ts && !request.rewind
                            }).unwrap());
                            continue;
                        }

                        if packet.stream_index() != video_index {
                            continue;
                        }

                        codec_ctx.send_packet(&packet)?;
                        let mut sent = false;
                        while codec_ctx.receive_frame(&mut in_frame)? {
                            current_ts = in_frame.pts();

                            if catching_up_to.is_some_and(|target| current_ts >= target) {
                                catching_up_to = None;
                            }

                            if !sent && current_ts >= ts {
                                sws.scale(&in_frame, &mut out_frame);
                                let mut guard = frame.lock().unwrap();
                                std::mem::swap(&mut guard.frame, &mut out_frame);
                                guard.timestamp = current_ts;
                                guard.generation = generation;
                                sent = true;
                            }
                        }
                    }
                };

                if let Err(e) = run() {
                    error!("decode error: {e:?}");
                    frame.lock().unwrap().timestamp = -1;
                }
            }
        });

        Ok(Self {
            stream_format,
            frame_rate,
            time_base: base,
            duration,
            start_time,

            frame,
            position,
            decode_thread: Some(decode_thread),
        })
    }

    pub fn stream_format(&self) -> VideoStreamFormat {
        self.stream_format.clone()
    }

    pub fn frame_rate(&self) -> AVRational {
        self.frame_rate
    }

    pub fn time_base(&self) -> AVRational {
        self.time_base
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn elapsed_to_timestamp(&self, elapsed: f64) -> i64 {
        let time_base = self.time_base();
        self.start_time.saturating_add((elapsed * time_base.den as f64 / time_base.num as f64).round() as i64)
    }

    pub fn seek(&self, timestamp: i64) {
        let mut guard = self.position.0.lock().unwrap();
        guard.timestamp = timestamp;
        self.position.1.notify_one();
    }

    pub fn rewind(&self) {
        let mut guard = self.position.0.lock().unwrap();
        guard.timestamp = self.start_time;
        guard.rewind = true;
        guard.generation = guard.generation.wrapping_add(1);
        self.position.1.notify_one();
    }

    pub fn with_frame<R>(&self, mut f: impl FnMut(&AVFrame, i64) -> R) -> R {
        let generation = self.position.0.lock().unwrap().generation;
        let guard = self.frame.lock().unwrap();
        // A decoder may finish its last packet after a rewind was requested.
        let timestamp = if guard.generation == generation { guard.timestamp } else { -1 };
        f(&guard.frame, timestamp)
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.seek(i64::MAX);
        if let Some(handle) = self.decode_thread.take() {
            handle.join().unwrap();
        }
    }
}
