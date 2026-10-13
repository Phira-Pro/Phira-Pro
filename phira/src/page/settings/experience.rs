//! User-facing display/audio preferences and the hidden-cover editor.
use super::{item_row_h, render_switch, render_title, request_mobile_file, right_rect, ChooseButton, ColorAction, ColorPicker, L10N_LOCAL};
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
    color_editor: Option<(usize, ColorPicker)>,
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
            color_editor: None,
            dirty: false,
        }
    }

    pub fn close_editor(&mut self) -> bool {
        self.color_editor.take().is_some() || self.editor.take().is_some()
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if let Some((side, picker)) = &mut self.color_editor {
            if let Some(action) = picker.touch(touch, t) {
                if let ColorAction::Apply(Some(color)) = action {
                    let config = &mut get_data_mut().config;
                    if *side == 0 {
                        config.hide_upper_color = color;
                        config.hide_upper_image = None;
                    } else {
                        config.hide_lower_color = color;
                        config.hide_lower_image = None;
                    }
                    self.dirty = true;
                }
                self.color_editor = None;
            }
            return true;
        }
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
            config.theoretical_score = config.judge_grading.perfect_plus && !config.theoretical_score;
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
                self.color_editor = Some((
                    side,
                    ColorPicker::new(
                        if side == 0 { tl!("upper-cover-color") } else { tl!("lower-cover-color") }.into_owned(),
                        Some(color),
                        Some(0),
                        0,
                    ),
                ));
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
        if let Some((_, picker)) = &mut self.color_editor {
            picker.update();
        }
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
        item! { render_title(ui, tl!("theoretical-score"), Some(if config.judge_grading.perfect_plus { tl!("theoretical-score-sub") } else { tl!("theoretical-score-disabled") })); render_switch(ui, rr, t, &mut self.theoretical, config.theoretical_score); }
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
        if let Some((_, picker)) = &mut self.color_editor {
            picker.render(ui, t);
        }
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
    source: Option<image::RgbaImage>,
    texture: Option<SafeTexture>,
    center: Vec2,
    zoom: f32,
    zoom_slider: Slider,
    image_rect: Rect,
    dragging: Option<(u64, Vec2)>,
    confirm: DRectButton,
    cancel: DRectButton,
    cancelled: bool,
}

impl CoverEditor {
    fn new(side: usize) -> Self {
        Self {
            ui_scale: 1.,
            side,
            source: None,
            texture: None,
            center: vec2(0.5, 0.5),
            zoom: 1.,
            zoom_slider: Slider::new(1.0..4.0, 0.05),
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
        let mut editor = Self::new(side);
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
        }
        false
    }

    fn apply(&self) -> Result<()> {
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
            config.hide_upper_color = 0;
            config.hide_upper_image = path;
        } else {
            config.hide_lower_color = 0;
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
        }
        self.cancel
            .render_text(ui, Rect::new(-0.48, 0.43, 0.40, 0.08), t, tl!("cover-cancel"), 0.45, false);
        self.confirm
            .render_text(ui, Rect::new(0.08, 0.43, 0.40, 0.08), t, tl!("cover-apply"), 0.45, true);
    }
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

#[cfg(all(test, target_os = "windows"))]
pub(super) fn render_regression(painter: &mut prpr::ui::TextPainter) {
    use super::qa::{capture, center};
    let touch = |phase, position| Touch {
        id: 930,
        phase,
        position,
        time: 0.,
    };
    for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        let mut list = ExperienceList::new();
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        for side in 0..2 {
            let height = ui.scope(|ui| list.render(ui, Rect::new(0., 0., 1.6, 8.), 2.).1);
            set_camera(&ui.camera());
            clear_background(Color::new(0.10, 0.13, 0.18, 1.));
            ui.scope(|ui| {
                ui.dx(-0.8);
                ui.dy(-height + (6 - side * 3) as f32 * item_row_h() - 0.15);
                list.render(ui, Rect::new(0., 0., 1.6, 8.), 2.);
            });
            let p = center(&list.color[side]);
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                list.touch(&touch(phase, p), 2.).unwrap();
            }
            list.render_top(&mut ui, 2.);
            capture(&mut ui, &format!("hide-color-picker-{side}-{ratio}"));
            list.color_editor.as_ref().unwrap().1.test_input("#F4A619");
            list.update(2.).unwrap();
            assert!(list.close_editor(), "Back cancels the color dialog");
            let config = &get_data().config;
            assert_eq!(if side == 0 { config.hide_upper_color } else { config.hide_lower_color }, 0);
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                list.touch(&touch(phase, p), 2.).unwrap();
            }
            list.render_top(&mut ui, 2.);
            list.color_editor.as_ref().unwrap().1.test_input("#F4A619");
            list.update(2.).unwrap();
            let p = list.color_editor.as_ref().unwrap().1.test_button("apply");
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                assert!(list.top_touch(&touch(phase, p), 2.));
            }
            assert!(list.update(2.).unwrap());
            let config = &get_data().config;
            assert_eq!(if side == 0 { config.hide_upper_color } else { config.hide_lower_color }, 0xf4a619);
            // Existing hide-cover reset still clears color and image together.
            let p = center(&list.reset[side]);
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                list.touch(&touch(phase, p), 2.).unwrap();
            }
            let config = &get_data().config;
            assert_eq!(if side == 0 { config.hide_upper_color } else { config.hide_lower_color }, 0);
        }
    }
    println!("Hide colors: shared picker, Back cancellation, Apply and existing cover reset passed at three aspects");
}
