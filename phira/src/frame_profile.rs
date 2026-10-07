//! Opt-in local frame timing. Disabled during normal play; no network telemetry.
use std::{
    fs::File,
    io::{BufWriter, Write},
    time::Instant,
};

pub struct FrameProfile {
    output: BufWriter<File>,
    origin: Instant,
    samples: Vec<[f64; 6]>,
}

impl FrameProfile {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("PHIRA_FRAME_PROFILE")?;
        let mut output = BufWriter::new(File::create(path).ok()?);
        writeln!(output, "seconds,interval_ms,pacing_ms,update_ms,render_ms,present_ms").ok()?;
        Some(Self {
            output,
            origin: Instant::now(),
            samples: Vec::with_capacity(1024),
        })
    }

    pub fn record(&mut self, start: Instant, interval: f64, pacing: f64, update: f64, render: f64, present: f64) {
        self.samples
            .push([start.duration_since(self.origin).as_secs_f64(), interval, pacing, update, render, present]);
        // Buffered batches avoid per-frame file I/O and bound retained memory.
        if self.samples.len() == 1024 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        for s in self.samples.drain(..) {
            let _ = writeln!(self.output, "{:.6},{:.4},{:.4},{:.4},{:.4},{:.4}", s[0], s[1], s[2], s[3], s[4], s[5]);
        }
        let _ = self.output.flush();
    }
}

impl Drop for FrameProfile {
    fn drop(&mut self) {
        self.flush();
    }
}
