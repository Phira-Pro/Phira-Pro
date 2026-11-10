use super::*;
use sasa::{
    backend::{
        cpal::{CpalBackend, CpalSettings},
        BackendSetup,
    },
    AudioManager, Backend,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

// Keep the callback stopped until recovery, just as an interrupted device does.
// The real backend then drains the deliberately full mixer/music queues.
struct InterruptedBackend {
    inner: CpalBackend,
    starts: Arc<AtomicUsize>,
}

impl Backend for InterruptedBackend {
    fn setup(&mut self, setup: BackendSetup) -> Result<()> {
        self.inner.setup(setup)
    }
    fn start(&mut self) -> Result<()> {
        if self.starts.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            self.inner.start()
        }
    }
    fn consume_broken(&self) -> bool {
        self.starts.load(Ordering::SeqCst) == 1
    }
}

pub(in crate::scene) fn interrupted_audio() -> (AudioManager, Arc<AtomicUsize>) {
    let starts = Arc::new(AtomicUsize::new(0));
    let audio = AudioManager::new(InterruptedBackend {
        inner: CpalBackend::new(CpalSettings::default()),
        starts: starts.clone(),
    })
    .unwrap();
    (audio, starts)
}

#[test]
#[ignore = "requires desktop OpenGL/audio; run separately with --ignored --test-threads=1"]
fn results_retry_recovers_before_full_audio_queue() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    macroquad::Window::from_config(
        Conf {
            window_title: "Results retry audio regression".into(),
            headless: true,
            ..Default::default()
        },
        async {
            set_pc_assets_folder("assets");
            next_frame().await;
            crate::core::init_assets();
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(directory.path().join("chart.json"), br#"{"formatVersion":3,"offset":0,"judgeLineList":[]}"#).unwrap();
            std::fs::copy("assets/ending.ogg", directory.path().join("song.ogg")).unwrap();
            let fs = Box::new(crate::fs::ExternalFileSystem(Arc::new(crate::dir::Dir::new(directory.path()).unwrap())));
            let info = ChartInfo {
                music: "song.ogg".into(),
                ..Default::default()
            };
            let mut config = Config::default();
            config.volume_music = 0.;
            config.volume_sfx = 0.;
            config.shader_pre_render = false;
            config.sample_count = 1;
            let texture: SafeTexture = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]).into();
            let mut future = Box::pin(GameScene::new(GameMode::Normal, info, config, fs, None, texture.clone(), texture, None, None, None));
            let mut game = loop {
                if let Some(result) = crate::ext::poll_future(future.as_mut()) {
                    break result.unwrap();
                }
                next_frame().await;
            };
            let mut tm = TimeManager::default();
            game.enter(&mut tm, None).unwrap();
            for _ in 0..3 {
                let mut ending = EndingScene::new(
                    game.ending_background.clone(),
                    game.res.illustration.clone(),
                    game.res.player.clone(),
                    game.res.icons.clone(),
                    game.res.icon_retry.clone(),
                    game.res.icon_proceed.clone(),
                    game.res.mod_icons.clone(),
                    game.res.info.clone(),
                    game.judge.result(false),
                    &game.res.config,
                    game.res.res_pack.ending.clone(),
                    None,
                    None,
                    0,
                    None,
                    None,
                    None,
                )
                .unwrap();
                ending.enter(&mut tm, None).unwrap();
                ending.pause(&mut tm).unwrap();
                ending.check_interrupted_resume(&mut tm);
                drop(ending);

                let (mut audio, starts) = interrupted_audio();
                let pending: Vec<_> = (0..16)
                    .map(|_| audio.create_music(game.res.music.clone(), MusicParams::default()).unwrap())
                    .collect();
                game.res.audio = audio;
                game.enter(&mut tm, None).unwrap();
                assert_eq!(starts.load(Ordering::SeqCst), 2, "retry must restart hidden gameplay audio before adding a player");
                assert!(matches!(game.state, State::Starting));
                assert!(!tm.paused());
                game.update(&mut tm).unwrap();
                drop(pending);
                next_frame().await;
            }
        },
    );
}
