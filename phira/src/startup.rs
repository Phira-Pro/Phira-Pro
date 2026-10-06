//! Keep startup failures observable instead of silently closing the window.
use std::{
    path::PathBuf,
    sync::{Mutex, Once},
};

static STAGE: Mutex<&'static str> = Mutex::new("native bootstrap");

pub fn stage(name: &'static str) {
    if let Ok(mut stage) = STAGE.lock() {
        *stage = name;
    }
    tracing::info!(stage = name, "startup");
}

fn report_path() -> Option<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::{NSSearchPathDirectory, NSSearchPathDomainMask, NSSearchPathForDirectoriesInDomains};
        let paths = NSSearchPathForDirectoriesInDomains(NSSearchPathDirectory::DocumentDirectory, NSSearchPathDomainMask::UserDomainMask, true);
        return paths
            .firstObject()
            .map(|path| PathBuf::from(path.to_string()).join("PhiraPro-startup-error.txt"));
    }
    #[cfg(not(target_os = "ios"))]
    {
        let root = crate::DATA_PATH.try_lock().ok()?.clone();
        // Android's path callback may itself fail before a writable directory
        // exists. Do not write into an arbitrary working directory in that case.
        #[cfg(target_os = "android")]
        let root = PathBuf::from(root?);
        #[cfg(not(target_os = "android"))]
        let root = root.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        Some(root.join("data/PhiraPro-startup-error.txt"))
    }
}

pub fn report(message: &str) {
    let stage = STAGE.try_lock().map(|stage| *stage).unwrap_or("unknown");
    let report = format!("Phira Pro {}\nStage: {stage}\n{message}\n", crate::PRO_VERSION);
    miniquad::error!("{}", report);
    if let Some(path) = report_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, report);
    }
}

pub fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            report(&info.to_string());
            previous(info);
        }));
    });
}

/// A recoverable initialization error must not complete macroquad's main
/// future: completing it requests termination, which looks like a crash.
pub async fn show_failure(error: anyhow::Error) {
    let message = format!("{error:#}");
    report(&message);
    loop {
        use macroquad::prelude::*;
        clear_background(BLACK);
        draw_text("Phira Pro could not start", 24., 48., 28., WHITE);
        draw_text("See PhiraPro-startup-error.txt for details.", 24., 80., 20., WHITE);
        for (i, line) in message.lines().take(8).enumerate() {
            draw_text(line, 24., 116. + i as f32 * 24., 18., WHITE);
        }
        next_frame().await;
    }
}
