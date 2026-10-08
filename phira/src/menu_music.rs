//! Menu playback shares the UI audio manager; decoding never blocks menu input.
use crate::{dir, get_data, scene::fs_from_path};
use anyhow::{bail, Result};
use prpr::{task::Task, ui::UI_AUDIO};
use rand::{seq::SliceRandom, thread_rng};
use sasa::{AudioClip, Music, MusicParams};

struct Loaded {
    clip: AudioClip,
    title: String,
    path: Option<String>,
    start: f64,
    gain: f32,
}

#[derive(Default)]
pub struct MenuMusic {
    pub music: Option<Music>,
    pub title: String,
    pub duration: f64,
    pub custom: bool,
    pub user_paused: bool,
    task: Option<Task<Result<Loaded>>>,
    gain: f32,
    last_path: Option<String>,
    active: bool,
    low_pass: f32,
    library_paths: Vec<String>,
    attempted: bool,
}

fn preview_position(start: f32, duration: f64) -> f64 {
    if start.is_finite() && start >= 0. && (start as f64) < duration {
        start as f64
    } else {
        0.
    }
}

impl MenuMusic {
    pub fn loading(&self) -> bool {
        self.task.is_some()
    }
    pub fn set_low_pass(&mut self, value: f32) -> Result<()> {
        self.low_pass = value;
        if let Some(music) = &mut self.music {
            music.set_low_pass(value)?;
        }
        Ok(())
    }
    pub fn position(&self) -> f64 {
        self.music.as_ref().map_or(0., |it| it.position()).clamp(0., self.duration)
    }
    pub fn set_active(&mut self, active: bool) -> Result<()> {
        self.active = active;
        if let Some(music) = &mut self.music {
            if active && !self.user_paused {
                music.fade_in(0.5)?;
            } else {
                music.pause()?;
            }
        }
        Ok(())
    }
    pub fn leave(&mut self) {
        self.active = false;
        if let Some(music) = &mut self.music {
            let _ = music.fade_out(0.5);
        }
    }
    pub fn toggle(&mut self) -> Result<()> {
        self.user_paused = !self.user_paused;
        if let Some(music) = &mut self.music {
            if self.active && !self.user_paused {
                music.play()?;
            } else {
                music.pause()?;
            }
        }
        Ok(())
    }
    pub fn refresh(&mut self) {
        self.task = None;
        self.music = None;
        self.title.clear();
        self.duration = 0.;
        self.attempted = false;
        self.custom = dir::find_appearance_audio("bgm").is_some();
        self.request();
    }
    pub fn random(&mut self) {
        if !self.custom {
            self.request();
        }
    }
    fn request(&mut self) {
        if self.task.is_some() {
            return;
        }
        self.attempted = true;
        let custom_path = dir::find_appearance_audio("bgm");
        self.custom = custom_path.is_some();
        let mut paths: Vec<String> = get_data().charts.iter().map(|it| it.local_path.clone()).collect();
        self.library_paths = paths.clone();
        paths.shuffle(&mut thread_rng());
        // With several usable charts, avoid immediately selecting the current song.
        if paths.len() > 1 {
            if let Some(index) = paths.iter().position(|it| Some(it) == self.last_path.as_ref()) {
                let previous = paths.remove(index);
                paths.push(previous);
            }
        }
        self.task = Some(Task::new(async move {
            if let Some(path) = custom_path {
                let clip = AudioClip::new(std::fs::read(&path)?)?;
                if clip.length() <= 0. {
                    bail!("empty background music");
                }
                let title = std::fs::read_to_string(path.with_file_name("bgm-title.txt"))
                    .ok()
                    .filter(|it| !it.trim().is_empty())
                    .unwrap_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
                let gain = prpr::audio::music_normalization_gain(&clip);
                return Ok(Loaded {
                    clip,
                    title,
                    path: None,
                    start: 0.,
                    gain,
                });
            }
            for path in paths {
                let result: Result<Loaded> = async {
                    let mut fs = fs_from_path(&path)?;
                    let info = prpr::fs::load_info(fs.as_mut()).await?;
                    let clip = AudioClip::new(fs.load_file(&info.music).await?)?;
                    if clip.length() <= 0. {
                        bail!("empty chart music");
                    }
                    let start = preview_position(info.preview_start, clip.length());
                    let gain = prpr::audio::music_normalization_gain(&clip);
                    Ok(Loaded {
                        clip,
                        title: info.name,
                        path: Some(path.clone()),
                        start,
                        gain,
                    })
                }
                .await;
                match result {
                    Ok(loaded) => return Ok(loaded),
                    Err(err) => tracing::warn!(?err, ?path, "skipping unavailable menu music"),
                }
            }
            bail!("no playable local chart music")
        }));
    }
    pub fn update_volume(&mut self) -> Result<()> {
        if let Some(music) = &mut self.music {
            let config = &get_data().config;
            music.set_amplifier(config.music_volume(config.volume_bgm) * if config.uniform_loudness { self.gain } else { 1. })?;
        }
        Ok(())
    }
    pub fn update(&mut self) -> Result<()> {
        if let Some(result) = self.task.as_mut().and_then(Task::take) {
            self.task = None;
            match result {
                Ok(loaded) => {
                    let config = &get_data().config;
                    let music = UI_AUDIO.with(|it| {
                        it.borrow_mut().create_music(
                            loaded.clip.clone(),
                            MusicParams {
                                amplifier: config.music_volume(config.volume_bgm) * if config.uniform_loudness { loaded.gain } else { 1. },
                                loop_mix_time: if self.custom { 0. } else { -1. },
                                command_buffer_size: 64,
                                ..Default::default()
                            },
                        )
                    })?;
                    self.music = Some(music);
                    self.duration = loaded.clip.length();
                    self.title = loaded.title;
                    self.last_path = loaded.path;
                    self.gain = loaded.gain;
                    let music = self.music.as_mut().unwrap();
                    music.set_low_pass(self.low_pass)?;
                    music.seek_to(loaded.start)?;
                    if self.active && !self.user_paused {
                        music.fade_in(0.5)?;
                    }
                }
                Err(err) => {
                    self.music = None;
                    self.title.clear();
                    self.duration = 0.;
                    tracing::warn!(?err, "menu music unavailable");
                }
            }
        }
        if self.active && !self.user_paused && !self.custom && self.task.is_none() {
            let changed = self.music.is_none() && self.library_paths != get_data().charts.iter().map(|it| it.local_path.clone()).collect::<Vec<_>>();
            if (!self.attempted || changed) && self.music.is_none() || self.music.is_some() && self.position() >= self.duration - 0.005 {
                self.request();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_preview_starts_at_zero_instead_of_silence() {
        for start in [-1., f32::NAN, f32::INFINITY, 100., 101.] {
            assert_eq!(preview_position(start, 100.), 0.);
        }
        assert_eq!(preview_position(42., 100.), 42.);
    }
    #[test]
    fn manual_pause_survives_scene_and_app_resume() {
        let mut player = MenuMusic::default();
        player.set_active(true).unwrap();
        player.toggle().unwrap();
        player.leave();
        player.set_active(true).unwrap();
        assert!(player.user_paused);
        player.toggle().unwrap();
        assert!(!player.user_paused);
    }

    pub(crate) async fn playback_regression() {
        use crate::{data::LocalChart, get_data_mut, DATA_PATH};
        use macroquad::prelude::next_frame;
        let root = tempfile::tempdir_in("target/judgement-panel-qa").unwrap();
        let previous_root = DATA_PATH.lock().unwrap().replace(root.path().to_string_lossy().into_owned());
        let old_charts = std::mem::take(&mut get_data_mut().charts);
        // A short, silent WAV tests the actual decoder and UI audio stream without playing user music.
        let count = 8000u32 * 3;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + count * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((count * 2).to_le_bytes());
        wav.resize(wav.len() + count as usize * 2, 0);
        for (path, title, preview) in [("a", "Menu A", 1.), ("b", "Menu B", 1.)] {
            let chart = std::path::PathBuf::from(dir::charts().unwrap()).join(path);
            std::fs::create_dir_all(&chart).unwrap();
            let info = prpr::info::ChartInfo {
                name: title.into(),
                music: "song.wav".into(),
                preview_start: preview,
                ..Default::default()
            };
            std::fs::write(chart.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
            std::fs::write(chart.join("song.wav"), &wav).unwrap();
            get_data_mut().charts.push(LocalChart {
                info: info.into(),
                local_path: path.into(),
                record: None,
                mods: Default::default(),
                played_unlock: false,
            });
        }
        async fn loaded(player: &mut MenuMusic) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while player.loading() {
                player.update().unwrap();
                next_frame().await;
                assert!(std::time::Instant::now() < deadline, "menu music load must complete");
            }
            for _ in 0..3 {
                next_frame().await;
            }
        }
        let mut player = MenuMusic::default();
        player.set_active(true).unwrap();
        player.refresh();
        loaded(&mut player).await;
        assert!(!player.custom && player.duration == 3.);
        assert!(player.position() >= 1. && player.position() < 1.5, "start from chart preview");
        let first = player.title.clone();
        player.random();
        loaded(&mut player).await;
        assert_ne!(first, player.title, "skip the immediately previous chart");
        player.toggle().unwrap();
        for _ in 0..3 {
            next_frame().await;
        }
        let stopped = player.position();
        player.set_active(false).unwrap();
        player.set_active(true).unwrap();
        for _ in 0..5 {
            next_frame().await;
        }
        assert!((player.position() - stopped).abs() < 0.02, "manual pause survives menu resume");
        player.toggle().unwrap();
        player.music.as_mut().unwrap().seek_to(2.98).unwrap();
        let before_end = player.title.clone();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while player.title == before_end {
            next_frame().await;
            player.update().unwrap();
            assert!(std::time::Instant::now() < deadline, "end advances to another chart");
        }
        let source = root.path().join("自定义音乐.wav");
        std::fs::write(&source, &wav).unwrap();
        dir::import_appearance_audio("bgm", &source).unwrap();
        player.refresh();
        loaded(&mut player).await;
        assert!(player.custom && player.title == "自定义音乐.wav");
        player.random();
        assert!(!player.loading(), "custom BGM disables random");
        player.music.as_mut().unwrap().seek_to(2.98).unwrap();
        for _ in 0..10 {
            next_frame().await;
            player.update().unwrap();
        }
        assert!(player.position() < 1., "custom BGM loops at the end");
        dir::clear_appearance_audio("bgm").unwrap();
        player.refresh();
        loaded(&mut player).await;
        assert!(!player.custom && player.title.starts_with("Menu "));
        player.set_active(false).unwrap();
        drop(player);
        get_data_mut().charts = old_charts;
        *DATA_PATH.lock().unwrap() = previous_root;
        println!("Menu music: actual WAV decode, preview seek, no-repeat random, pause/resume, track end, custom loop and reset passed.");
    }
}

#[cfg(test)]
pub(crate) use tests::playback_regression;
