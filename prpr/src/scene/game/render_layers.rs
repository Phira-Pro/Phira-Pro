//! Gameplay HUD and application controls are distinct render layers.
//! The coordinator composites chart/HUD/noise/effects first; controls never enter
//! a chart snapshot or postprocess target.
use super::*;

impl GameScene {
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
