//! Gameplay HUD and application controls are distinct render layers.
//! The coordinator composites chart/HUD/noise/effects first; controls never enter
//! a chart snapshot or postprocess target.
use super::*;

impl GameScene {
    pub(super) fn draw_failure_screen(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<bool> {
        let now = tm.real_time();
        let Some(failure) = &mut self.failure else { return Ok(false) };
        let Some(color) = failure.flash_color(now) else { return Ok(false) };
        let show_controls = failure.controls_alpha(now) > 0. && !self.hide_ui;
        let camera = Camera2D {
            render_target: self.res.camera.render_target,
            ..ui.camera()
        };
        // Clear the entire destination, including any bars outside the fitted chart.
        set_camera(&camera);
        clear_background(color);
        if show_controls {
            self.res.update_size(ui.viewport);
            self.draw_controls(tm, ui)?;
            set_camera(&camera);
        }
        Ok(true)
    }

    pub(super) fn draw_gameplay_ui(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let old_top = std::mem::replace(&mut ui.top, 1. / self.res.aspect_ratio);
        let result = self.ui(ui, tm);
        ui.top = old_top;
        result
    }

    pub(super) fn draw_controls(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&Camera2D {
            zoom: self.res.camera.zoom,
            viewport: self.res.camera.viewport,
            render_target: self.res.camera.render_target,
            ..Default::default()
        });
        let old_top = std::mem::replace(&mut ui.top, 1. / self.res.aspect_ratio);
        let result = self.overlay_ui(ui, tm);
        ui.top = old_top;
        result
    }
}
