//! Export presentation, isolated from account and score lifecycles.
use super::{FrameContext, PresentationOptions, RenderStage};
use crate::{
    ext::{semi_black, SafeTexture, ScaleType},
    info::ChartInfo,
    judge::PlayResult,
    ui::{TextPainter, Ui},
};
use macroquad::prelude::*;
use std::cell::RefCell;

pub(super) mod animation;
mod geometry;
mod loading;
mod result;

pub struct PresentationAssets {
    pub illustration: SafeTexture,
    pub background: SafeTexture,
    pub avatar: SafeTexture,
    pub icons: [SafeTexture; 8],
    pub challenge_frames: [Option<SafeTexture>; 5],
    pub font: Option<RefCell<TextPainter>>,
    pub font_line_height: f32,
}

pub fn apply_metadata(info: &mut ChartInfo, options: &PresentationOptions) {
    for (target, value) in [
        (&mut info.name, &options.metadata.title),
        (&mut info.composer, &options.metadata.composer),
        (&mut info.charter, &options.metadata.charter),
        (&mut info.illustrator, &options.metadata.illustrator),
        (&mut info.level, &options.metadata.difficulty),
    ] {
        if !value.trim().is_empty() {
            *target = value.clone();
        }
    }
}

pub fn background(ui: &mut Ui, assets: &PresentationAssets) {
    let area = ui.screen_rect();
    ui.fill_rect(area, (*assets.background, area, ScaleType::CropCenter));
    ui.fill_rect(area, semi_black(123. / 255.));
}

pub fn draw(ui: &mut Ui, context: &FrameContext, options: &PresentationOptions, info: &ChartInfo, assets: &PresentationAssets, result: &PlayResult) {
    match context.stage {
        RenderStage::Opening => loading::draw(ui, context, options, info, assets),
        RenderStage::Ending => result::draw(ui, context, options, info, assets, result),
        _ => {}
    }
}

/// Output typography is independent of the application's accessibility scale.
/// Restored on all return paths before the settings page is drawn again.
pub(super) struct FontScale(u32);
impl FontScale {
    pub fn native() -> Self {
        Self(crate::ui::FONT_DISPLAY_SCALE.swap(1f32.to_bits(), std::sync::atomic::Ordering::Relaxed))
    }
}
impl Drop for FontScale {
    fn drop(&mut self) {
        crate::ui::FONT_DISPLAY_SCALE.store(self.0, std::sync::atomic::Ordering::Relaxed);
    }
}
