//! Native GL ownership and lifetime for the editor's bounded frame mailbox.
use anyhow::Result;
use prpr::{chart_video::{ChartVideoRenderer, VideoOptions, VisualOptions}, ext::poll_future, ui::TextPainter};
use rpen_host::{PreviewBridge, PreviewPosition, PreviewStart};
use std::{future::Future, pin::Pin};

type Loading = Pin<Box<dyn Future<Output = Result<(String, ChartVideoRenderer)>>>>;
#[derive(Default)]
pub struct Preview {
    loading: Option<Loading>,
    loading_id: String,
    renderer: Option<(String, ChartVideoRenderer)>,
    last: Option<PreviewPosition>,
}
async fn load(start: PreviewStart) -> Result<(String, ChartVideoRenderer)> {
    let source = tempfile::Builder::new().suffix(".pez").tempfile()?;
    std::fs::write(source.path(), start.bytes)?;
    let config = crate::get_data().config.clone();
    let options = VideoOptions {
        width: start.width, height: start.height, fps: 30, hitsound: false,
        sample_count: config.sample_count.clamp(1, 4),
        res_pack_path: crate::get_data().selected_respack_path()?,
        visuals: VisualOptions {
            offset_indicator: Some(config.offset_indicator), ap_fc_indicator: config.ap_fc_indicator,
            double_hint: config.double_hint, particle: config.particle, show_acc: config.show_acc,
            show_judgement_details: config.show_judgement_details, show_perfect_combo: config.show_perfect_combo,
            show_early_late: config.show_early_late,
        },
        ..VideoOptions::default()
    };
    let renderer = ChartVideoRenderer::preview_with_config(source.path().to_owned(), options, config).await?;
    Ok((start.id, renderer))
}
impl Preview {
    pub fn update(&mut self, bridge: &PreviewBridge) {
        if let Some(start) = bridge.take_start() {
            self.renderer = None;
            self.last = None;
            self.loading_id = start.id.clone();
            self.loading = Some(Box::pin(load(start)));
        }
        if self.loading.is_some() && bridge.position(&self.loading_id).is_none() { self.loading = None; }
        if let Some(task) = &mut self.loading {
            if let Some(result) = poll_future(task.as_mut()) {
                self.loading = None;
                match result {
                    Ok((id, renderer)) if bridge.position(&id).is_some() => self.renderer = Some((id, renderer)),
                    Ok(_) => {},
                    Err(error) => bridge.fail(&self.loading_id, format!("原生预览加载失败：{error:#}")),
                }
            }
        }
        if self.renderer.as_ref().is_some_and(|(id, _)| bridge.position(id).is_none()) { self.renderer = None; self.last = None; }
    }
    pub fn render(&mut self, bridge: &PreviewBridge, painter: &mut TextPainter) {
        let Some((id, renderer)) = &mut self.renderer else { return; };
        let Some(position) = bridge.position(id) else { return; };
        if self.last == Some(position) { return; }
        macroquad::prelude::push_camera_state();
        let result = renderer.preview_music_frame(painter, position.time, position.show_hud);
        macroquad::prelude::pop_camera_state();
        match result {
            Ok(()) => { let options = renderer.options(); bridge.publish(id, options.width, options.height, renderer.frame_bytes()); self.last = Some(position); }
            Err(error) => { bridge.fail(id, format!("原生预览绘制失败：{error:#}")); self.renderer = None; }
        }
    }
}
