//! User-facing display/audio preferences and the hidden-cover editor.
use super::{item_row_h, render_switch, render_title, request_mobile_file, right_rect, ChooseButton, L10N_LOCAL};
use crate::{dir, get_data, get_data_mut, scene::BGM_VOLUME_UPDATED};
use anyhow::{Context, Result};
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::SafeTexture,
    scene::{request_input, return_input, show_error, show_message, take_input},
    ui::{DRectButton, Slider, Ui},
};
use std::sync::atomic::Ordering;

pub(super) struct ExperienceList {
    font: ChooseButton,
    language: Option<String>,
    theoretical: DRectButton,
    loudness: DRectButton,
    level: Slider,
    background: DRectButton,
    dim: DRectButton,
    color: [DRectButton; 2],
    image: [DRectButton; 2],
    reset: [DRectButton; 2],
    editor: Option<CoverEditor>,
    dirty: bool,
}

fn font_options() -> Vec<String> {
    [
        tl!("font-smallest"),
        tl!("font-smaller"),
        tl!("font-medium"),
        tl!("font-larger"),
        tl!("font-largest"),
    ]
    .into_iter()
    .map(|s| s.into_owned())
    .collect()
}

impl ExperienceList {
    pub fn new() -> Self {
        Self {
            font: ChooseButton::new()
                .with_options(font_options())
                .with_selected(get_data().config.font_size.min(4)),
            language: get_data().language.clone(),
            theoretical: DRectButton::new(),
            loudness: DRectButton::new(),
            level: Slider::new(0.0..1.0, 0.01).with_button_step(0.5),
            background: DRectButton::new(),
            dim: DRectButton::new(),
            color: std::array::from_fn(|_| DRectButton::new()),
            image: std::array::from_fn(|_| DRectButton::new()),
            reset: std::array::from_fn(|_| DRectButton::new()),
            editor: None,
            dirty: false,
        }
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if let Some(editor) = &mut self.editor {
            if editor.touch(touch, t) {
                match editor.apply() {
                    Ok(()) => self.dirty = true,
                    Err(err) => show_error(err),
                }
                self.editor = None;
            } else if editor.cancelled {
                self.editor = None;
            }
            return true;
        }
        self.font.top_touch(touch, t)
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        if self.font.touch(touch, t) {
            return Ok(Some(false));
        }
        let config = &mut get_data_mut().config;
        if self.theoretical.touch(touch, t) {
            config.theoretical_score ^= true;
            return Ok(Some(true));
        }
        if self.loudness.touch(touch, t) {
            config.uniform_loudness ^= true;
            BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
            return Ok(Some(true));
        }
        if config.uniform_loudness {
            if let Some(changed) = self.level.touch(touch, t, &mut config.loudness) {
                BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
                return Ok(Some(changed));
            }
        }
        if self.background.touch(touch, t) {
            config.fixed_background ^= true;
            return Ok(Some(true));
        }
        if config.fixed_background && self.dim.touch(touch, t) {
            request_input("fixed-background", InputBox::new().default_text(format!("{:.2}", config.background_dim)));
            return Ok(Some(false));
        }
        for side in 0..2 {
            if self.color[side].touch(touch, t) {
                let color = if side == 0 { config.hide_upper_color } else { config.hide_lower_color };
                self.editor = Some(CoverEditor::color(side, color));
                return Ok(Some(false));
            }
            if self.image[side].touch(touch, t) {
                #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
                if let Some(path) = super::pick_image(&tl!("cover-import")) {
                    match CoverEditor::image(side, &path) {
                        Ok(editor) => self.editor = Some(editor),
                        Err(err) => show_error(err),
                    }
                }
                request_mobile_file(if side == 0 { "hide_upper_import" } else { "hide_lower_import" });
                return Ok(Some(false));
            }
            if self.reset[side].touch(touch, t) {
                if side == 0 {
                    config.hide_upper_image = None;
                    config.hide_upper_color = 0;
                } else {
                    config.hide_lower_image = None;
                    config.hide_lower_color = 0;
                }
                return Ok(Some(true));
            }
        }
        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        if self.language != get_data().language {
            self.language = get_data().language.clone();
            self.font.set_options(font_options());
        }
        self.font.update(t);
        if self.font.changed() {
            let config = &mut get_data_mut().config;
            config.font_size = self.font.selected();
            prpr::ui::FONT_DISPLAY_SCALE.store(config.font_scale().to_bits(), Ordering::Relaxed);
            self.dirty = true;
        }
        if let Some((id, text)) = take_input() {
            if id == "fixed-background" {
                match text.trim().parse::<f32>() {
                    Ok(value) if value.is_finite() && (0.0..=1.0).contains(&value) => {
                        get_data_mut().config.background_dim = value;
                        self.dirty = true;
                    }
                    _ => {
                        show_message(tl!("background-invalid")).error();
                    }
                }
            } else {
                return_input(id, text);
            }
        }
        #[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
        if let Some((id, path)) = prpr::scene::take_file() {
            if matches!(id.as_str(), "hide_upper_import" | "hide_lower_import") {
                let side = usize::from(id == "hide_lower_import");
                match CoverEditor::image(side, std::path::Path::new(&path)) {
                    Ok(editor) => self.editor = Some(editor),
                    Err(err) => show_error(err),
                }
            } else {
                prpr::scene::return_file(id, path);
            }
        }
        Ok(std::mem::take(&mut self.dirty))
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let rr = right_rect(r.w);
        let mut h = 0.;
        macro_rules! item { ($($body:tt)*) => {{ $($body)* ui.dy(item_row_h()); h += item_row_h(); }}; }
        let config = &get_data().config;
        item! { render_title(ui, tl!("font-display-size"), None); self.font.render(ui, rr, t); }
        item! { render_title(ui, tl!("theoretical-score"), Some(tl!("theoretical-score-sub"))); render_switch(ui, rr, t, &mut self.theoretical, config.theoretical_score); }
        item! { render_title(ui, tl!("uniform-loudness"), Some(tl!("uniform-loudness-sub"))); render_switch(ui, rr, t, &mut self.loudness, config.uniform_loudness); }
        if config.uniform_loudness {
            item! { render_title(ui, tl!("uniform-level"), None); self.level.render(ui, rr, t, config.loudness, format!("{:.2}", config.loudness)); }
        }
        item! { render_title(ui, tl!("fixed-background"), None); render_switch(ui, rr, t, &mut self.background, config.fixed_background); }
        if config.fixed_background {
            item! { render_title(ui, tl!("background-dim"), None); self.dim.render_text(ui, rr, t, format!("{:.2}", config.background_dim), 0.5, false); }
        }
        for side in 0..2 {
            item! {
                render_title(ui, if side == 0 { tl!("upper-cover-color") } else { tl!("lower-cover-color") }, None);
                let color = if side == 0 { config.hide_upper_color } else { config.hide_lower_color };
                self.color[side].render_text_color(ui, rr, t, format!("#{color:06X}"), 0.5, true, Color::from_hex_rgb(color));
            }
            item! { render_title(ui, if side == 0 { tl!("upper-cover-image") } else { tl!("lower-cover-image") }, None); self.image[side].render_text(ui, rr, t, tl!("cover-import"), 0.5, false); }
            item! { render_title(ui, tl!("cover-reset"), None); self.reset[side].render_text(ui, rr, t, tl!("font-reset-btn"), 0.5, false); }
        }
        (r.w, h)
    }

    pub fn render_top(&mut self, ui: &mut Ui, t: f32) {
        self.font.render_top(ui, t, 1.);
        if let Some(editor) = &mut self.editor {
            let scale = ((ui.top - 0.02) / 0.52).clamp(0.5, 1.);
            editor.ui_scale = scale;
            ui.with(prpr::core::Matrix::new_scaling(scale), |ui| editor.render(ui, t));
        }
    }
}

struct CoverEditor {
    ui_scale: f32,
    side: usize,
    hsv: [f32; 3],
    source: Option<image::RgbaImage>,
    texture: Option<SafeTexture>,
    center: Vec2,
    zoom: f32,
    zoom_slider: Slider,
    palette: Rect,
    hue_rect: Rect,
    image_rect: Rect,
    dragging: Option<(u64, Vec2)>,
    confirm: DRectButton,
    cancel: DRectButton,
    cancelled: bool,
}

impl CoverEditor {
    fn color(side: usize, color: u32) -> Self {
        let c = Color::from_hex_rgb(color);
        let max = c.r.max(c.g).max(c.b);
        let min = c.r.min(c.g).min(c.b);
        let d = max - min;
        let hue = if d == 0. {
            0.
        } else if max == c.r {
            ((c.g - c.b) / d).rem_euclid(6.) / 6.
        } else if max == c.g {
            ((c.b - c.r) / d + 2.) / 6.
        } else {
            ((c.r - c.g) / d + 4.) / 6.
        };
        Self {
            ui_scale: 1.,
            side,
            hsv: [hue, if max > 0. { d / max } else { 0. }, max],
            source: None,
            texture: None,
            center: vec2(0.5, 0.5),
            zoom: 1.,
            zoom_slider: Slider::new(1.0..4.0, 0.05),
            palette: Rect::default(),
            hue_rect: Rect::default(),
            image_rect: Rect::default(),
            dragging: None,
            confirm: DRectButton::new(),
            cancel: DRectButton::new(),
            cancelled: false,
        }
    }

    fn image(side: usize, path: &std::path::Path) -> Result<Self> {
        anyhow::ensure!(std::fs::metadata(path)?.len() <= 32 * 1024 * 1024, "图片不能超过 32 MB");
        let mut reader = image::ImageReader::new(std::io::Cursor::new(std::fs::read(path)?)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(128 * 1024 * 1024);
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        reader.limits(limits);
        let image = reader.decode().context("无法读取遮挡图片")?;
        let mut editor = Self::color(side, 0);
        let preview = image.thumbnail(1024, 1024).to_rgba8();
        editor.texture = Some(Texture2D::from_rgba8(preview.width() as u16, preview.height() as u16, preview.as_raw()).into());
        editor.source = Some(image.to_rgba8());
        Ok(editor)
    }

    fn selected_crop(&self) -> Option<Rect> {
        let image = self.source.as_ref()?;
        let ratio = screen_width() / screen_height() / 0.9;
        let mut w = image.width() as f32;
        let mut h = w / ratio;
        if h > image.height() as f32 {
            h = image.height() as f32;
            w = h * ratio;
        }
        w /= self.zoom;
        h /= self.zoom;
        Some(Rect::new(
            (self.center.x * image.width() as f32 - w / 2.).clamp(0., image.width() as f32 - w),
            (self.center.y * image.height() as f32 - h / 2.).clamp(0., image.height() as f32 - h),
            w,
            h,
        ))
    }

    fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.confirm.touch(touch, t) {
            return true;
        }
        if self.cancel.touch(touch, t) {
            self.cancelled = true;
            return false;
        }
        if self.source.is_some() {
            self.zoom_slider.touch(touch, t, &mut self.zoom);
            if touch.phase == TouchPhase::Started && self.image_rect.contains(touch.position) {
                self.dragging = Some((touch.id, touch.position));
            }
            if let Some((id, previous)) = &mut self.dragging {
                if *id == touch.id {
                    self.center += (touch.position - *previous) / vec2(self.image_rect.w, self.image_rect.h);
                    self.center = self.center.clamp(Vec2::ZERO, Vec2::ONE);
                    *previous = touch.position;
                    if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                        self.dragging = None;
                    }
                }
            }
        } else if matches!(touch.phase, TouchPhase::Started | TouchPhase::Moved | TouchPhase::Stationary) {
            if self.palette.contains(touch.position) {
                self.hsv[1] = (touch.position.x - self.palette.x) / self.palette.w;
                self.hsv[2] = 1. - (touch.position.y - self.palette.y) / self.palette.h;
            }
            if self.hue_rect.contains(touch.position) {
                self.hsv[0] = (touch.position.x - self.hue_rect.x) / self.hue_rect.w;
            }
        }
        false
    }

    fn apply(&self) -> Result<()> {
        let color = hsv(self.hsv[0], self.hsv[1], self.hsv[2]);
        let hex = ((color.r * 255.).round() as u32) << 16 | ((color.g * 255.).round() as u32) << 8 | (color.b * 255.).round() as u32;
        let path = if let (Some(image), Some(crop)) = (&self.source, self.selected_crop()) {
            let crop = image::imageops::crop_imm(image, crop.x as u32, crop.y as u32, crop.w.max(1.) as u32, crop.h.max(1.) as u32).to_image();
            let image = image::DynamicImage::ImageRgba8(crop).resize(2048, 2048, image::imageops::FilterType::Lanczos3);
            let path = format!("{}/hide-{}.png", dir::appearance()?, if self.side == 0 { "upper" } else { "lower" });
            image.save(&path)?;
            Some(path)
        } else {
            None
        };
        let config = &mut get_data_mut().config;
        if self.side == 0 {
            config.hide_upper_color = hex;
            config.hide_upper_image = path;
        } else {
            config.hide_lower_color = hex;
            config.hide_lower_image = path;
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(
            Rect::new(-1. / self.ui_scale, -ui.top / self.ui_scale, 2. / self.ui_scale, ui.top * 2. / self.ui_scale),
            Color::new(0., 0., 0., 0.85),
        );
        ui.text(tl!("cover-editor")).pos(0., -0.47).anchor(0.5, 0.5).size(0.65).draw();
        if let (Some(image), Some(texture), Some(crop)) = (&self.source, &self.texture, self.selected_crop()) {
            let scale = (1.25 / image.width() as f32).min(0.50 / image.height() as f32);
            let r = Rect::new(-(image.width() as f32) * scale / 2., -0.30, image.width() as f32 * scale, image.height() as f32 * scale);
            self.image_rect = ui.rect_to_global(r);
            ui.fill_rect(r, (**texture, r));
            let c = Rect::new(r.x + crop.x * scale, r.y + crop.y * scale, crop.w * scale, crop.h * scale);
            let dark = Color::new(0., 0., 0., 0.65);
            for slab in [
                Rect::new(r.x, r.y, r.w, c.y - r.y),
                Rect::new(r.x, c.bottom(), r.w, r.bottom() - c.bottom()),
                Rect::new(r.x, c.y, c.x - r.x, c.h),
                Rect::new(c.right(), c.y, r.right() - c.right(), c.h),
            ] {
                ui.fill_rect(slab, dark);
            }
            frame(ui, c, false);
            let small = Rect::new(c.x, if self.side == 0 { c.y } else { c.bottom() - c.h / 9. }, c.w, c.h / 9.);
            frame(ui, small, true);
            self.zoom_slider
                .render(ui, Rect::new(0.05, 0.24, 0.42, 0.06), t, self.zoom, format!("{:.2}×", self.zoom));
            ui.text(tl!("cover-crop-help")).pos(0., 0.35).anchor(0.5, 0.5).size(0.35).draw();
        } else {
            let r = Rect::new(-0.55, -0.32, 1.1, 0.5);
            self.palette = ui.rect_to_global(r);
            for row in 0..12 {
                for col in 0..24 {
                    ui.fill_rect(
                        Rect::new(r.x + r.w * col as f32 / 24., r.y + r.h * row as f32 / 12., r.w / 24. + 0.001, r.h / 12. + 0.001),
                        hsv(self.hsv[0], col as f32 / 23., 1. - row as f32 / 11.),
                    );
                }
            }
            let hue = Rect::new(-0.55, 0.22, 1.1, 0.04);
            self.hue_rect = ui.rect_to_global(hue);
            for col in 0..48 {
                ui.fill_rect(Rect::new(hue.x + hue.w * col as f32 / 48., hue.y, hue.w / 48. + 0.001, hue.h), hsv(col as f32 / 48., 1., 1.));
            }
            let selected = hsv(self.hsv[0], self.hsv[1], self.hsv[2]);
            ui.fill_circle(r.x + r.w * self.hsv[1], r.y + r.h * (1. - self.hsv[2]), 0.012, WHITE);
            ui.fill_rect(Rect::new(-0.12, 0.30, 0.24, 0.06), selected);
        }
        self.cancel
            .render_text(ui, Rect::new(-0.48, 0.43, 0.40, 0.08), t, tl!("cover-cancel"), 0.45, false);
        self.confirm
            .render_text(ui, Rect::new(0.08, 0.43, 0.40, 0.08), t, tl!("cover-apply"), 0.45, true);
    }
}

fn hsv(h: f32, s: f32, v: f32) -> Color {
    let h = h.rem_euclid(1.) * 6.;
    let c = v * s;
    let x = c * (1. - (h % 2. - 1.).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    Color::new(r + m, g + m, b + m, 1.)
}

fn frame(ui: &mut Ui, r: Rect, dashed: bool) {
    for edge in [
        Rect::new(r.x, r.y, r.w, 0.002),
        Rect::new(r.x, r.bottom(), r.w, 0.002),
        Rect::new(r.x, r.y, 0.002, r.h),
        Rect::new(r.right(), r.y, 0.002, r.h),
    ] {
        if !dashed {
            ui.fill_rect(edge, WHITE);
            continue;
        }
        let horizontal = edge.w > edge.h;
        let length = if horizontal { edge.w } else { edge.h };
        for i in 0..(length / 0.025).ceil() as usize {
            let start = i as f32 * 0.025;
            let size = (length - start).min(0.013).max(0.);
            ui.fill_rect(
                if horizontal {
                    Rect::new(edge.x + start, edge.y, size, edge.h)
                } else {
                    Rect::new(edge.x, edge.y + start, edge.w, size)
                },
                WHITE,
            );
        }
    }
}
