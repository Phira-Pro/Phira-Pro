//! Shared modal color picker. Changes stay local until Apply, including Default.
use super::L10N_LOCAL;
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    core::Matrix,
    ext::RectExt,
    scene::{request_input, return_input, show_message, take_input},
    ui::{DRectButton, Ui},
};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) enum ColorAction {
    Apply(Option<u32>),
    Cancel,
}

#[derive(Clone, Copy)]
enum ColorDrag {
    Palette,
    Hue,
    Rgb(usize),
}

pub(super) struct ColorPicker {
    title: String,
    value: Option<u32>,
    original: u32,
    default: Option<u32>,
    fallback: u32,
    hsv: [f32; 3],
    input_id: String,
    palette: Rect,
    hue: Rect,
    drag: Option<(u64, ColorDrag)>,
    rgb: [DRectButton; 3],
    rgb_rect: [Rect; 3],
    hex: DRectButton,
    reset: DRectButton,
    cancel: DRectButton,
    apply: DRectButton,
    swatches: [DRectButton; 10],
}

const SWATCHES: [u32; 10] = [
    0xffffff, 0x000000, 0xff3b30, 0xff9500, 0xffd60a, 0x34c759, 0x5ac8fa, 0x0a84ff, 0xaf52de, 0xff2d55,
];

impl ColorPicker {
    #[cfg(all(test, target_os = "windows"))]
    pub(super) fn test_button(&self, name: &str) -> Vec2 {
        super::qa::center(match name {
            "apply" => &self.apply,
            "reset" => &self.reset,
            "cancel" => &self.cancel,
            _ => &self.hex,
        })
    }
    #[cfg(all(test, target_os = "windows"))]
    pub(super) fn test_input(&self, text: &str) {
        return_input(self.input_id.clone(), text.into());
    }
    pub fn new(title: String, value: Option<u32>, default: Option<u32>, fallback: u32) -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let original = value.unwrap_or(fallback) & 0xffffff;
        Self {
            title,
            value,
            original,
            default,
            fallback,
            hsv: rgb_to_hsv(original),
            input_id: format!("color-picker-{}", SERIAL.fetch_add(1, Ordering::Relaxed)),
            palette: Rect::default(),
            hue: Rect::default(),
            drag: None,
            rgb: std::array::from_fn(|_| DRectButton::new()),
            rgb_rect: [Rect::default(); 3],
            hex: DRectButton::new(),
            reset: DRectButton::new(),
            cancel: DRectButton::new(),
            apply: DRectButton::new(),
            swatches: std::array::from_fn(|_| DRectButton::new()),
        }
    }
    fn selected(&self) -> u32 {
        let c = hsv(self.hsv);
        ((c.r * 255.).round() as u32) << 16 | ((c.g * 255.).round() as u32) << 8 | (c.b * 255.).round() as u32
    }
    fn select(&mut self, color: u32) {
        self.value = Some(color & 0xffffff);
        self.hsv = rgb_to_hsv(color);
    }
    fn select_channel(&mut self, i: usize, channel: u8) {
        let shift = (2 - i) * 8;
        self.select((self.selected() & !(255 << shift)) | ((channel as u32) << shift));
    }
    pub fn update(&mut self) {
        if let Some((id, text)) = take_input() {
            if id == self.input_id {
                let value = text.trim().strip_prefix('#').unwrap_or(text.trim());
                match u32::from_str_radix(value, 16)
                    .ok()
                    .filter(|_| value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    Some(color) => self.select(color),
                    None => {
                        show_message(tl!("color-invalid")).error();
                    }
                }
            } else if let Some(i) = id
                .strip_prefix(&format!("{}-rgb-", self.input_id))
                .and_then(|i| i.parse::<usize>().ok())
                .filter(|i| *i < 3)
            {
                match text.trim().parse::<u8>() {
                    Ok(channel) => self.select_channel(i, channel),
                    Err(_) => {
                        show_message(tl!("color-rgb-invalid")).error();
                    }
                }
            } else if !id.starts_with("color-picker-") {
                return_input(id, text);
            }
        }
    }
    pub fn touch(&mut self, touch: &Touch, t: f32) -> Option<ColorAction> {
        if self.cancel.touch(touch, t) {
            return Some(ColorAction::Cancel);
        }
        if self.apply.touch(touch, t) {
            return Some(ColorAction::Apply(self.value));
        }
        if self.reset.touch(touch, t) {
            self.hsv = rgb_to_hsv(self.default.unwrap_or(self.fallback));
            self.value = self.default;
        }
        if self.hex.touch(touch, t) {
            request_input(&self.input_id, InputBox::new().default_text(format!("#{:06X}", self.selected())));
        }
        for i in 0..SWATCHES.len() {
            if self.swatches[i].touch(touch, t) {
                self.select(SWATCHES[i]);
            }
        }
        for i in 0..3 {
            if self.rgb[i].touch(touch, t) {
                request_input(
                    format!("{}-rgb-{i}", self.input_id),
                    InputBox::new().default_text(format!("{}", (self.selected() >> ((2 - i) * 8)) & 255)),
                );
            }
        }
        if touch.phase == TouchPhase::Started && self.drag.is_none() {
            if self.palette.contains(touch.position) {
                self.drag = Some((touch.id, ColorDrag::Palette));
            } else if self.hue.contains(touch.position) {
                self.drag = Some((touch.id, ColorDrag::Hue));
            } else {
                for i in 0..3 {
                    if self.rgb_rect[i].feather(0.018).contains(touch.position) {
                        self.drag = Some((touch.id, ColorDrag::Rgb(i)));
                        break;
                    }
                }
            }
        }
        if let Some((id, hue)) = self.drag {
            if id == touch.id {
                if touch.phase != TouchPhase::Cancelled {
                    match hue {
                        ColorDrag::Hue => self.hsv[0] = ((touch.position.x - self.hue.x) / self.hue.w).clamp(0., 1.),
                        ColorDrag::Palette => {
                            self.hsv[1] = ((touch.position.x - self.palette.x) / self.palette.w).clamp(0., 1.);
                            self.hsv[2] = (1. - (touch.position.y - self.palette.y) / self.palette.h).clamp(0., 1.);
                        }
                        ColorDrag::Rgb(i) => {
                            let r = self.rgb_rect[i];
                            self.select_channel(i, (((touch.position.x - r.x) / r.w).clamp(0., 1.) * 255.).round() as u8);
                        }
                    }
                    self.value = Some(self.selected());
                }
                if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    self.drag = None;
                }
            }
        }
        None
    }
    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        // Dim the whole viewport before scaling the dialog, so even wide screens are covered.
        ui.fill_rect(Rect::new(-1., -ui.top, 2., ui.top * 2.), Color::new(0., 0., 0., 0.72));
        let scale = ((ui.top - 0.025) / 0.46).min(1.);
        ui.with(Matrix::new_scaling(scale), |ui| {
            ui.fill_path(&Rect::new(-0.74, -0.46, 1.48, 0.92).rounded(0.025), Color::new(0.10, 0.14, 0.19, 1.));
            ui.text(&self.title).pos(-0.68, -0.405).anchor(0., 0.5).size(0.52).max_width(1.34).draw();
            let r = Rect::new(-0.68, -0.33, 0.67, 0.49);
            self.palette = ui.rect_to_global(r);
            ui.fill_rect(r, (WHITE, (r.x, r.y), hsv([self.hsv[0], 1., 1.]), (r.right(), r.y)));
            ui.fill_rect(r, (Color::new(0., 0., 0., 0.), (r.x, r.y), BLACK, (r.x, r.bottom())));
            let p = vec2(r.x + r.w * self.hsv[1], r.y + r.h * (1. - self.hsv[2]));
            ui.stroke_circle(p.x, p.y, 0.014, 0.006, BLACK);
            ui.stroke_circle(p.x, p.y, 0.014, 0.003, WHITE);
            let r = Rect::new(-0.68, 0.19, 0.67, 0.035);
            self.hue = ui.rect_to_global(r);
            for i in 0..6 {
                let x = r.x + r.w * i as f32 / 6.;
                let end = x + r.w / 6.;
                ui.fill_rect(
                    Rect::new(x, r.y, r.w / 6. + 0.0001, r.h),
                    (hsv([i as f32 / 6., 1., 1.]), (x, r.y), hsv([(i + 1) as f32 / 6., 1., 1.]), (end, r.y)),
                );
            }
            let x = r.x + r.w * self.hsv[0];
            ui.fill_path(&Rect::new(x - 0.006, r.y - 0.006, 0.012, r.h + 0.012).rounded(0.004), WHITE);
            let color = self.selected();
            for (x, c, label) in [
                (0.04, self.original, tl!("color-original")),
                (
                    0.37,
                    color,
                    if self.value.is_none() {
                        tl!("color-current-default")
                    } else {
                        tl!("color-current")
                    },
                ),
            ] {
                ui.fill_path(&Rect::new(x, -0.33, 0.28, 0.115).rounded(0.012), Color::from_hex_rgb(c));
                ui.text(label).pos(x + 0.14, -0.184).anchor(0.5, 0.5).size(0.31).draw();
            }
            self.hex
                .render_text(ui, Rect::new(0.04, -0.143, 0.61, 0.07), t, format!("#{color:06X}"), 0.42, false);
            for i in 0..3 {
                let y = -0.035 + i as f32 * 0.085;
                ui.text(["R", "G", "B"][i]).pos(0.04, y + 0.025).anchor(0., 0.5).size(0.35).draw();
                let channel = ((color >> ((2 - i) * 8)) & 255) as f32;
                self.rgb[i].render_text(ui, Rect::new(0.09, y, 0.13, 0.05), t, format!("{channel:.0}"), 0.35, false);
                let r = Rect::new(0.28, y + 0.022, 0.37, 0.006);
                self.rgb_rect[i] = ui.rect_to_global(r);
                let shift = (2 - i) * 8;
                let base = color & !(255 << shift);
                ui.fill_rect(r, (Color::from_hex_rgb(base), (r.x, r.y), Color::from_hex_rgb(base | (255 << shift)), (r.right(), r.y)));
                let x = r.x + r.w * channel / 255.;
                ui.fill_circle(x, r.center().y, 0.012, WHITE);
                ui.stroke_circle(x, r.center().y, 0.012, 0.002, BLACK);
            }
            for (i, c) in SWATCHES.into_iter().enumerate() {
                let r = Rect::new(-0.68 + i as f32 * 0.136, 0.26, 0.11, 0.055);
                self.swatches[i].render_text(ui, r, t, "", 0.3, false);
                ui.fill_path(&r.feather(-0.004).rounded(0.008), Color::from_hex_rgb(c));
            }
            self.cancel
                .render_text(ui, Rect::new(-0.68, 0.355, 0.40, 0.07), t, tl!("cover-cancel"), 0.4, false);
            self.reset
                .render_text(ui, Rect::new(-0.20, 0.355, 0.40, 0.07), t, tl!("color-reset"), 0.4, false);
            self.apply
                .render_text(ui, Rect::new(0.28, 0.355, 0.40, 0.07), t, tl!("cover-apply"), 0.4, true);
        });
    }
}

fn rgb_to_hsv(hex: u32) -> [f32; 3] {
    let c = Color::from_hex_rgb(hex & 0xffffff);
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let h = if d == 0. {
        0.
    } else if max == c.r {
        ((c.g - c.b) / d).rem_euclid(6.) / 6.
    } else if max == c.g {
        ((c.b - c.r) / d + 2.) / 6.
    } else {
        ((c.r - c.g) / d + 4.) / 6.
    };
    [h, if max > 0. { d / max } else { 0. }, max]
}
fn hsv([h, s, v]: [f32; 3]) -> Color {
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

#[cfg(all(test, target_os = "windows"))]
pub(super) fn render_regression(painter: &mut prpr::ui::TextPainter) {
    use super::qa::{capture, center};
    let touch = |phase, position| Touch {
        id: 910,
        phase,
        position,
        time: 0.,
    };
    for lang in ["zh-CN", "en-US"] {
        prpr_l10n::set_prefered_locale(Some(lang.parse().unwrap()));
        for (w, h, ratio) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
            let mut ui = Ui::new(painter, Some((0, 0, w, h)));
            set_camera(&ui.camera());
            clear_background(Color::new(0.10, 0.13, 0.18, 1.));
            let mut picker = ColorPicker::new(tl!("item-touch-color").into_owned(), Some(0xff3b30), Some(0xff3b30), 0);
            picker.render(&mut ui, 2.);
            capture(&mut ui, &format!("color-picker-{lang}-{ratio}"));
            for button in [&picker.cancel, &picker.apply, &picker.reset, &picker.hex] {
                let p = center(button);
                assert!(p.x.abs() < 0.95 && p.y.abs() < ui.top - 0.02, "dialog controls must fit the viewport");
            }
            // Use the platform input return path, with validation preserving the last good value.
            picker.test_input("#12AB34");
            picker.update();
            assert_eq!(picker.selected(), 0x12ab34);
            picker.test_input("+FFFFF");
            picker.update();
            assert_eq!(picker.selected(), 0x12ab34);
            picker.test_input("#12345Z");
            picker.update();
            assert_eq!(picker.selected(), 0x12ab34);
            return_input(format!("{}-rgb-1", picker.input_id), "255".into());
            picker.update();
            assert_eq!(picker.selected(), 0x12ff34);
            return_input(format!("{}-rgb-1", picker.input_id), "256".into());
            picker.update();
            assert_eq!(picker.selected(), 0x12ff34);
            // Continuous color area captures one finger and clamps drags past its boundary.
            let r = picker.palette;
            picker.touch(&touch(TouchPhase::Started, r.center()), 2.);
            let before = picker.selected();
            let mut other = touch(TouchPhase::Moved, vec2(r.right() + 0.1, r.y - 0.1));
            other.id += 1;
            picker.touch(&other, 2.);
            assert_eq!(picker.selected(), before, "second finger must not steal a palette drag");
            picker.touch(&touch(TouchPhase::Moved, vec2(r.x - 0.1, r.y - 0.1)), 2.);
            assert_eq!(picker.selected(), 0xffffff);
            picker.touch(&touch(TouchPhase::Ended, r.center()), 2.);
            // RGB line touches independently control the selected channel.
            picker.select(0x123456);
            let r = picker.rgb_rect[2];
            picker.touch(&touch(TouchPhase::Started, r.center()), 2.);
            picker.touch(&touch(TouchPhase::Ended, vec2(r.right() + 0.1, r.center().y)), 2.);
            assert_eq!(picker.selected(), 0x1234ff);
            // Reset is staged, and Apply returns a value; Cancel never returns a saved value.
            let p = picker.test_button("reset");
            picker.touch(&touch(TouchPhase::Started, p), 2.);
            picker.touch(&touch(TouchPhase::Ended, p), 2.);
            let p = picker.test_button("apply");
            picker.touch(&touch(TouchPhase::Started, p), 2.);
            assert!(matches!(picker.touch(&touch(TouchPhase::Ended, p), 2.), Some(ColorAction::Apply(Some(0xff3b30)))));
            let p = picker.test_button("cancel");
            picker.touch(&touch(TouchPhase::Started, p), 2.);
            assert!(matches!(picker.touch(&touch(TouchPhase::Ended, p), 2.), Some(ColorAction::Cancel)));
            picker.default = None;
            let p = picker.test_button("reset");
            picker.touch(&touch(TouchPhase::Started, p), 2.);
            picker.touch(&touch(TouchPhase::Ended, p), 2.);
            let p = picker.test_button("apply");
            picker.touch(&touch(TouchPhase::Started, p), 2.);
            assert!(
                matches!(picker.touch(&touch(TouchPhase::Ended, p), 2.), Some(ColorAction::Apply(None))),
                "resource-pack reset must preserve the None sentinel"
            );
        }
    }
    println!("Color picker: real render/touch, single-finger drag capture, RGB/HEX validation, reset/apply/cancel at three aspects passed");
}
