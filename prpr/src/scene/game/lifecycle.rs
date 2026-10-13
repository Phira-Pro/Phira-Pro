//! App lifecycle only: keep the practice pause state and invalidate GPU caches.
//! This does not reset notes, the practice segment, bookmarks, or playback rate.
use super::*;

impl GameScene {
    pub(super) fn background_pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.paused_before_background.get_or_insert(tm.paused());
        self.practice_panel.hide();
        self.practice_base.cancel();
        self.pause_rewind = None;
        self.music.pause()?;
        if !tm.paused() {
            tm.pause();
        }
        self.chart.blocked_touches.clear();
        self.block_audio.suspend(&mut self.music);
        Ok(())
    }

    pub(super) fn foreground_resume(&mut self, tm: &mut TimeManager) {
        self.last_update_time = tm.real_time();
        self.fps_last_frame_time = tm.real_time();
        // Surface restoration can keep the same dimensions. Reallocate lazily
        // on the next valid render frame, never in an Android lifecycle callback.
        self.res.invalidate_render_surface();
        let was_paused = self.paused_before_background.take().unwrap_or(tm.paused());
        if !was_paused && matches!(self.state, State::Starting | State::BeforeMusic) && tm.paused() {
            tm.resume();
        }
    }
}
