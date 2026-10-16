//! Keep startup failures observable instead of silently closing the window.
use std::{
    path::PathBuf,
    sync::{Mutex, Once},
};

mod journal;

static STAGE: Mutex<&'static str> = Mutex::new("native bootstrap");
static JOURNAL: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Write before graphics initialization on iOS, so a native crash can leave a last checkpoint.
/// Creating a readable document also makes normal installations visible to file sharing.
pub fn begin() {
    static BEGIN: Once = Once::new();
    BEGIN.call_once(|| {
        let Some(path) = report_path() else { return };
        let Some(directory) = path.parent() else { return };
        match journal::prepare(directory, &identity()) {
            Ok(path) => {
                if let Ok(mut journal) = JOURNAL.lock() {
                    *journal = Some(path);
                }
                stage("native bootstrap");
            }
            Err(error) => miniquad::warn!("Could not create startup journal: {}", error),
        }
    });
}

fn identity() -> String {
    format!(
        "Phira Pro {} / build {} / {}\nUTC: {}",
        crate::PRO_VERSION,
        env!("PHIRA_BUILD_NUMBER"),
        env!("GIT_HASH"),
        chrono::Utc::now().to_rfc3339()
    )
}

fn append(message: &str) {
    if let Ok(journal) = JOURNAL.try_lock() {
        if let Some(path) = journal.as_ref() {
            let _ = journal::append(path, &format!("{} {message}", chrono::Utc::now().to_rfc3339()));
        }
    }
}

pub fn stage(name: &'static str) {
    if let Ok(mut stage) = STAGE.lock() {
        *stage = name;
    }
    append(&format!("Stage: {name}"));
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
    let report = format!("{}\nStage: {stage}\n{message}\n", identity());
    append(&format!("Error at {stage}: {message}"));
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
