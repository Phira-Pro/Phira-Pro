//! Trial adapter deliberately supplies no upload, history or score-save callback.
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::BLACK_TEXTURE,
    fs::{self, FileSystem, ZipFileSystem},
    scene::{GameMode, GameScene, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, Ui},
};
use rpen_host::PlayRequest;

pub async fn load(bytes: Vec<u8>, request: PlayRequest) -> Result<Box<dyn Scene>> {
    let mut fs: Box<dyn FileSystem> = Box::new(ZipFileSystem::new(bytes)?);
    let mut info = fs::load_info(fs.as_mut()).await?;
    info.id = None;
    info.use_rpen = Some(true);
    let mut config = crate::get_data().config.clone();
    config.offline_mode = true;
    config.practice_local_path = None;
    config.res_pack_path = crate::get_data().selected_respack_path()?;
    let (illustration, background, _) = LoadingScene::load(fs.as_mut(), &info.illustration)
        .await
        .unwrap_or_else(|_| (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone(), WHITE));
    let game = GameScene::new(GameMode::Exercise, info, config, fs, None, background, illustration, None, None, None).await?;
    Ok(Box::new(Trial {
        game,
        request,
        initialized: false,
        back: false,
        button: DRectButton::new(),
    }))
}
struct Trial {
    game: GameScene,
    request: PlayRequest,
    initialized: bool,
    back: bool,
    button: DRectButton,
}

fn return_to_editor(tm: &mut TimeManager) -> NextScene {
    // The scene stack restores the parent's position, but shares this clock.
    // A paused exercise must not freeze the workbench's next transition.
    if tm.paused() {
        tm.resume();
    }
    tm.speed = 1.;
    tm.adjust_time = false;
    NextScene::Pop
}

impl Scene for Trial {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.game.enter(tm, target)?;
        if !self.initialized {
            self.initialized = true;
            self.game.on_result(
                tm,
                Box::new(prpr::practice::tools::Selection {
                    start: self.request.start,
                    end: self.request.end.unwrap_or(f64::MAX),
                }),
            )?;
            tm.pause();
        }
        Ok(())
    }
    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.game.pause(tm)
    }
    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.game.resume(tm)
    }
    fn on_result(&mut self, tm: &mut TimeManager, result: Box<dyn std::any::Any>) -> Result<()> {
        self.game.on_result(tm, result)
    }
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.button.touch(touch, tm.real_time() as f32) {
            self.back = true;
            return Ok(true);
        }
        self.game.touch(tm, touch)
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.game.update(tm)
    }
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        self.game.render(tm, ui)?;
        let r = ui.content_rect();
        self.button
            .render_text(ui, Rect::new(r.right() - 0.47, r.y + 0.015, 0.44, 0.075), tm.real_time() as f32, "返回编辑器", 0.34, false);
        Ok(())
    }
    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if self.back {
            return_to_editor(tm)
        } else {
            self.game.next_scene(tm)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn returning_from_paused_or_running_trial_keeps_parent_animation_clock_moving() {
        for paused in [false, true] {
            let real = Rc::new(Cell::new(10.));
            let clock = Rc::clone(&real);
            let mut tm = TimeManager::manual(Box::new(move || clock.get()));
            tm.speed = 0.5;
            tm.adjust_time = true;
            if paused {
                tm.pause();
            }
            assert!(matches!(return_to_editor(&mut tm), NextScene::Pop));
            // Model the scene stack restoring the workbench's saved time.
            tm.seek_to(25.);
            real.set(11.);
            assert!((tm.now() - 26.).abs() < 1e-9);
            assert!(!tm.adjust_time);
        }
    }
}
