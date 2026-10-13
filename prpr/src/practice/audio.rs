//! Streaming practice audio. The song clock always measures chart seconds;
//! pitch preservation changes grain spacing, never the judgement clock.
//! WSOLA reference: Roelands & Verhelst, ICASSP 1993.
use anyhow::Result;
use sasa::{AudioClip, Frame, Music, MusicParams, Renderer};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Weak,
};
#[path = "low_pass.rs"]
mod low_pass;

pub enum Player {
    Native(Music),
    Practice(PracticePlayer),
}
impl From<Music> for Player {
    fn from(music: Music) -> Self {
        Self::Native(music)
    }
}
impl Player {
    pub fn pause(&mut self) -> Result<()> {
        match self {
            Self::Native(m) => m.pause(),
            Self::Practice(m) => m.send(Command::Pause(true)),
        }
    }
    pub fn play(&mut self) -> Result<()> {
        match self {
            Self::Native(m) => m.play(),
            Self::Practice(m) => m.send(Command::Pause(false)),
        }
    }
    pub fn paused(&mut self) -> bool {
        match self {
            Self::Native(m) => m.paused(),
            Self::Practice(m) => m.state.paused.load(Ordering::Relaxed),
        }
    }
    pub fn position(&self) -> f64 {
        match self {
            Self::Native(m) => m.position(),
            Self::Practice(m) => f64::from_bits(m.state.position.load(Ordering::Relaxed)),
        }
    }
    pub fn seek_to(&mut self, time: f64) -> Result<()> {
        match self {
            Self::Native(m) => m.seek_to(time),
            Self::Practice(m) => {
                m.send(Command::Seek(time.max(0.)))?;
                m.state.position.store(time.max(0.).to_bits(), Ordering::Relaxed);
                Ok(())
            }
        }
    }
    pub fn try_set_playback_rate(&mut self, rate: f64) -> bool {
        match self {
            Self::Native(m) => m.try_set_playback_rate(rate),
            Self::Practice(m) => m.tx.send(Command::Rate(rate.clamp(0.1, 2.))).is_ok(),
        }
    }
    pub fn try_set_low_pass_filter(&mut self, cutoff: Option<f32>, q: f32, seconds: f32) -> bool {
        match self {
            Self::Native(m) => m.try_set_low_pass_filter(cutoff, q, seconds),
            Self::Practice(m) => m.tx.send(Command::Filter(cutoff, q, seconds)).is_ok(),
        }
    }
    pub fn configure_practice(&mut self, pitch: bool, correct: bool) -> Result<()> {
        if let Self::Practice(m) = self {
            m.send(Command::Options(pitch, correct))?;
        }
        Ok(())
    }
    pub fn schedule(&mut self, events: Vec<(f64, AudioClip)>) -> Result<()> {
        if let Self::Practice(m) = self {
            m.send(Command::Schedule(events))?;
        }
        Ok(())
    }
}
struct State {
    position: AtomicU64,
    paused: AtomicBool,
}
pub struct PracticePlayer {
    // Lifecycle/control commands must not wait for an audio callback: Android
    // can suspend that callback while the UI is processing background events.
    tx: mpsc::Sender<Command>,
    state: Arc<State>,
}
impl PracticePlayer {
    fn send(&self, cmd: Command) -> Result<()> {
        self.tx.send(cmd).map_err(|e| anyhow::anyhow!("practice audio: {e}"))
    }
}
enum Command {
    Pause(bool),
    Seek(f64),
    Rate(f64),
    Options(bool, bool),
    Filter(Option<f32>, f32, f32),
    Schedule(Vec<(f64, AudioClip)>),
}

pub struct Stream {
    clip: AudioClip,
    state: Weak<State>,
    rx: mpsc::Receiver<Command>,
    position: f64,
    rate: f64,
    amplifier: f32,
    sfx_volume: f32,
    paused: bool,
    pitch: bool,
    correct: bool,
    events: Vec<(f64, AudioClip)>,
    next: usize,
    active: Vec<(usize, f64)>,
    grain: Vec<Frame>,
    tail: Vec<Frame>,
    cursor: usize,
    hop: usize,
    sample_rate: u32,
    primed: bool,
    filter: low_pass::LowPass,
}
impl Stream {
    pub fn new(clip: AudioClip, params: MusicParams, sfx_volume: f32, events: Vec<(f64, AudioClip)>) -> (Player, Self) {
        let state = Arc::new(State {
            position: AtomicU64::new(0),
            paused: AtomicBool::new(true),
        });
        let (tx, rx) = mpsc::channel();
        let stream = Self {
            clip,
            state: Arc::downgrade(&state),
            rx,
            position: 0.,
            rate: params.playback_rate,
            amplifier: params.amplifier,
            sfx_volume,
            paused: true,
            pitch: false,
            correct: false,
            events,
            next: 0,
            active: Vec::with_capacity(64),
            grain: vec![Frame::default(); 4096],
            tail: vec![Frame::default(); 4096],
            cursor: 0,
            hop: 0,
            sample_rate: 0,
            primed: false,
            filter: Default::default(),
        };
        (Player::Practice(PracticePlayer { tx, state }), stream)
    }
    fn reset(&mut self) {
        self.primed = false;
        self.cursor = self.hop;
        self.next = self.events.partition_point(|(t, _)| *t < self.position - 1e-7);
        self.active.clear();
        self.filter.clear_history();
    }
    fn prepare(&mut self, sr: u32) {
        if self.sample_rate != sr {
            self.sample_rate = sr;
            self.hop = (sr as usize / 50).clamp(1, 4096);
            self.reset();
        }
        while let Ok(command) = self.rx.try_recv() {
            match command {
                Command::Pause(p) => self.paused = p,
                Command::Seek(t) => {
                    self.position = t;
                    self.reset();
                }
                Command::Rate(r) => {
                    self.rate = r;
                    self.reset();
                }
                Command::Options(p, c) => {
                    if self.pitch != p {
                        self.primed = false;
                        self.cursor = self.hop;
                    }
                    if self.correct != c {
                        self.active.clear();
                        self.next = self.events.partition_point(|(t, _)| *t < self.position);
                    }
                    self.pitch = p;
                    self.correct = c;
                }
                Command::Filter(c, q, s) => self.filter.set(c, q, s),
                Command::Schedule(mut events) => {
                    events.sort_by(|a, b| a.0.total_cmp(&b.0));
                    self.events = events;
                    self.reset();
                }
            }
        }
        self.publish();
    }
    fn publish(&self) {
        if let Some(s) = self.state.upgrade() {
            s.position.store(self.position.to_bits(), Ordering::Relaxed);
            s.paused.store(self.paused, Ordering::Relaxed);
        }
    }
    fn sample(&self, time: f64) -> Frame {
        if time < 0. {
            Frame::default()
        } else {
            self.clip.sample(time).unwrap_or_default()
        }
    }
    fn grain(&mut self) {
        let dt = 1. / self.sample_rate as f64;
        let mut start = self.position;
        if self.primed {
            // Both channels use one offset. Search around the absolute song
            // clock, so successive alignment corrections cannot accumulate drift.
            let search = self.sample_rate as i32 / 125;
            let mut best = f64::NEG_INFINITY;
            let mut shift = 0;
            for candidate in (-search..=search).step_by(8) {
                let mut dot = 0.;
                let mut energy = 1e-12;
                for i in (0..self.hop).step_by(8) {
                    let a = self.tail[i];
                    let b = self.sample(self.position + (candidate as f64 + i as f64) * dt);
                    dot += (a.0 * b.0 + a.1 * b.1) as f64;
                    energy += (b.0 * b.0 + b.1 * b.1) as f64;
                }
                let score = dot / energy.sqrt();
                if score > best {
                    best = score;
                    shift = candidate;
                }
            }
            start += shift as f64 * dt;
        }
        for i in 0..self.hop {
            let current = self.sample(start + i as f64 * dt);
            let mix = (i as f32 / self.hop as f32).clamp(0., 1.);
            self.grain[i] = if self.primed {
                self.tail[i] * (1. - mix) + current * mix
            } else {
                current
            };
            self.tail[i] = self.sample(start + (i + self.hop) as f64 * dt);
        }
        self.primed = true;
        self.cursor = 0;
    }
    fn frame(&mut self) -> Frame {
        if self.paused {
            return Frame::default();
        }
        if self.position >= self.clip.length() {
            self.paused = true;
            return Frame::default();
        }
        let frame = if self.pitch && (self.rate - 1.).abs() > 1e-6 {
            if self.cursor >= self.hop {
                self.grain();
            }
            let frame = self.grain[self.cursor];
            self.cursor += 1;
            frame
        } else {
            self.sample(self.position)
        };
        let mut frame = self.filter.process(frame * self.amplifier, self.sample_rate);
        if self.correct {
            while let Some((t, _)) = self.events.get(self.next) {
                if *t > self.position + 1e-9 {
                    break;
                }
                self.active.push((self.next, (self.position - t).max(0.) / self.rate));
                self.next += 1;
            }
            let events = &self.events;
            self.active.retain_mut(|(id, time)| {
                let Some(sound) = events[*id].1.sample(*time) else { return false };
                frame = frame + sound * self.sfx_volume;
                *time += 1. / self.sample_rate as f64;
                true
            });
        }
        self.position += self.rate / self.sample_rate as f64;
        frame
    }
}
impl Renderer for Stream {
    fn alive(&self) -> bool {
        self.state.strong_count() > 0
    }
    fn render_mono(&mut self, sr: u32, data: &mut [f32]) {
        self.prepare(sr);
        for sample in data {
            *sample += self.frame().avg();
        }
        self.publish();
    }
    fn render_stereo(&mut self, sr: u32, data: &mut [f32]) {
        self.prepare(sr);
        for samples in data.chunks_exact_mut(2) {
            let Frame(l, r) = self.frame();
            samples[0] += l;
            samples[1] += r;
        }
        self.publish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_commands_do_not_wait_for_a_suspended_audio_callback() {
        let sr = 8000;
        let (player, mut stream) = Stream::new(AudioClip::from_raw(vec![Frame::default(); sr as usize], sr), MusicParams::default(), 0., vec![]);
        let Player::Practice(player) = player else { unreachable!() };
        let (done, finished) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            // No callback drains the commands until this worker completes.
            // The previous 64-slot sync channel blocks here during suspension.
            for _ in 0..512 {
                if player.send(Command::Pause(true)).is_err() {
                    return player;
                }
            }
            player.send(Command::Seek(0.25)).unwrap();
            done.send(()).unwrap();
            player
        });
        let completed = finished.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
        if !completed {
            drop(stream);
            let _ = worker.join();
            panic!("UI lifecycle blocked while the audio callback was suspended");
        }
        let player = worker.join().unwrap();
        let mut silence = [0.; 128];
        stream.render_stereo(sr, &mut silence);
        assert_eq!(f64::from_bits(player.state.position.load(Ordering::Relaxed)), 0.25);
        assert!(player.state.paused.load(Ordering::Relaxed));
        assert!(silence.iter().all(|v| *v == 0.));
    }
    #[test]
    fn changing_practice_rate_updates_the_audio_clock_without_stale_multiplier() {
        let sr = 8000;
        let clip = AudioClip::from_raw(vec![Frame::default(); sr as usize * 20], sr);
        let (mut player, mut stream) = Stream::new(clip, MusicParams::default(), 0., vec![]);
        for pitch in [false, true] {
            player.configure_practice(pitch, false).unwrap();
            for rate in [0.1, 0.73, 1., 1.01, 1.25, 2., 1.] {
                player.pause().unwrap();
                player.seek_to(0.5).unwrap();
                assert!(player.try_set_playback_rate(rate));
                let mut buffer = vec![0.; sr as usize / 10 * 2];
                stream.render_stereo(sr, &mut buffer);
                assert!((player.position() - 0.5).abs() < 1e-9);
                player.play().unwrap();
                stream.render_stereo(sr, &mut buffer);
                assert!((player.position() - (0.5 + rate * 0.1)).abs() < 1e-8, "pitch={pitch}, rate={rate}");
            }
        }
    }
    #[test]
    fn pitch_clock_seek_pause_and_cues() {
        let sr = 16000;
        let clip = AudioClip::from_raw(
            (0..sr * 3)
                .map(|i| {
                    let s = (std::f32::consts::TAU * 400. * i as f32 / sr as f32).sin();
                    Frame(s, -s)
                })
                .collect(),
            sr,
        );
        for rate in [0.5, 0.75, 1.25, 2.] {
            let (mut player, mut stream) = Stream::new(
                clip.clone(),
                MusicParams {
                    playback_rate: rate,
                    ..Default::default()
                },
                1.,
                vec![],
            );
            player.configure_practice(true, false).unwrap();
            player.play().unwrap();
            let mut samples = vec![0.; sr as usize * 2];
            stream.render_stereo(sr, &mut samples);
            assert!((player.position() - rate).abs() < 1e-8);
            let crossings = samples
                .chunks_exact(2)
                .skip(sr as usize / 4)
                .map(|x| x[0])
                .collect::<Vec<_>>()
                .windows(2)
                .filter(|x| x[0] <= 0. && x[1] > 0.)
                .count();
            assert!((crossings as i32 - 300).abs() < 12, "rate {rate}: frequency drift, crossings={crossings}");
            assert!(samples.chunks_exact(2).all(|x| (x[0] + x[1]).abs() < 1e-6));
            player.pause().unwrap();
            let mut silence = [0.; 320];
            stream.render_stereo(sr, &mut silence);
            assert_eq!(silence, [0.; 320]);
            player.seek_to(1.).unwrap();
            stream.render_stereo(sr, &mut silence);
            assert_eq!(player.position(), 1.);
        }
        let silence = AudioClip::from_raw(vec![Frame::default(); sr as usize], sr);
        let click = AudioClip::from_raw(vec![Frame(1., 1.); 80], sr);
        let (mut p, mut stream) = Stream::new(silence, MusicParams::default(), 1., vec![(0.25, click)]);
        p.configure_practice(false, true).unwrap();
        p.play().unwrap();
        let mut out = vec![0.; sr as usize];
        stream.render_mono(sr, &mut out);
        assert_eq!(out.iter().position(|&x| x > 0.), Some(4000));
        assert_eq!(out.iter().filter(|&&x| x > 0.).count(), 80);
        p.seek_to(0.3).unwrap();
        let mut out = [0.; 800];
        stream.render_mono(sr, &mut out);
        assert!(out.iter().all(|&x| x == 0.));
        p.seek_to(0.).unwrap();
        let mut out = vec![0.; 8000];
        stream.render_mono(sr, &mut out);
        assert_eq!(out.iter().filter(|&&x| x > 0.).count(), 80);
    }
}
