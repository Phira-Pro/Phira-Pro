use super::{super::L10N_LOCAL, LanePackage, Layout};
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::semi_black,
    scene::{request_input, return_input, show_message, take_input},
    ui::{DRectButton, Ui},
};
use std::borrow::Cow;

const FIELDS: [Field; 4] = [Field::Positions, Field::Spacing, Field::Height, Field::Speed];

#[derive(Clone, Copy)]
enum Field {
    Positions,
    Spacing,
    Height,
    Speed,
}

impl Field {
    fn id(self) -> &'static str {
        match self {
            Self::Positions => "lane-import-positions",
            Self::Spacing => "lane-import-spacing",
            Self::Height => "lane-import-height",
            Self::Speed => "lane-import-speed",
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Positions => itl!("lane-import-positions"),
            Self::Spacing => itl!("lane-import-spacing"),
            Self::Height => itl!("lane-import-height"),
            Self::Speed => itl!("lane-import-speed"),
        }
    }

    fn prompt(self, lanes: usize) -> Cow<'static, str> {
        match self {
            Self::Positions => itl!("lane-import-positions-help", "count" => lanes).into(),
            Self::Spacing => itl!("lane-import-spacing-help"),
            Self::Height => itl!("lane-import-height-help"),
            Self::Speed => itl!("lane-import-speed-help"),
        }
    }
}

pub(in crate::scene) struct ImportDialog {
    package: LanePackage,
    difficulty: usize,
    layout: Layout,
    difficulty_buttons: [DRectButton; 2],
    field_buttons: [DRectButton; 4],
    cancel: DRectButton,
    import: DRectButton,
}

impl ImportDialog {
    pub(in crate::scene) fn new(package: LanePackage) -> Self {
        let lanes = package.difficulties[0].lanes;
        Self {
            package,
            difficulty: 0,
            layout: Layout {
                positions: Layout::evenly_spaced(lanes, 1200. / lanes as f32),
                line_y: -300.,
                speed: 12.,
            },
            difficulty_buttons: std::array::from_fn(|_| DRectButton::new().with_radius(0.012)),
            field_buttons: std::array::from_fn(|_| DRectButton::new().with_radius(0.012)),
            cancel: DRectButton::new().with_radius(0.012),
            import: DRectButton::new().with_radius(0.012),
        }
    }

    fn change_difficulty(&mut self, direction: isize) {
        let count = self.package.difficulties.len();
        self.difficulty = (self.difficulty as isize + direction).rem_euclid(count as isize) as usize;
        let lanes = self.package.difficulties[self.difficulty].lanes;
        if lanes != self.layout.positions.len() {
            self.layout.positions = Layout::evenly_spaced(lanes, 1200. / lanes as f32);
        }
    }

    fn field_value(&self, field: Field) -> String {
        match field {
            Field::Positions => self.layout.positions.iter().map(f32::to_string).collect::<Vec<_>>().join(", "),
            Field::Spacing => {
                let positions = &self.layout.positions;
                let spacing = positions[1] - positions[0];
                if positions.windows(2).all(|pair| ((pair[1] - pair[0]) - spacing).abs() < 0.001) {
                    spacing.to_string()
                } else {
                    String::new()
                }
            }
            Field::Height => self.layout.line_y.to_string(),
            Field::Speed => self.layout.speed.to_string(),
        }
    }

    fn apply_field(&mut self, field: Field, text: &str) -> bool {
        let lanes = self.layout.positions.len();
        match field {
            Field::Positions => {
                let values = text
                    .split(|c: char| c == ',' || c == '，' || c.is_whitespace())
                    .filter(|it| !it.is_empty())
                    .map(str::parse)
                    .collect::<Result<Vec<f32>, _>>();
                if let Ok(positions) = values {
                    if Layout::validate_positions(&positions, lanes) {
                        self.layout.positions = positions;
                        return true;
                    }
                }
            }
            Field::Spacing => {
                if let Ok(spacing) = text.trim().parse::<f32>() {
                    let positions = Layout::evenly_spaced(lanes, spacing);
                    if Layout::validate_positions(&positions, lanes) {
                        self.layout.positions = positions;
                        return true;
                    }
                }
            }
            Field::Height => {
                if let Ok(value) = text.trim().parse::<f32>() {
                    if (-450. ..=450.).contains(&value) {
                        self.layout.line_y = value;
                        return true;
                    }
                }
            }
            Field::Speed => {
                if let Ok(value) = text.trim().parse::<f32>() {
                    if value.is_finite() && value > 0. && value <= 1000. {
                        self.layout.speed = value;
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(in crate::scene) fn update(&mut self) {
        if let Some((id, text)) = take_input() {
            if let Some(field) = FIELDS.into_iter().find(|field| field.id() == id) {
                if !self.apply_field(field, &text) {
                    show_message(field.prompt(self.layout.positions.len())).warn();
                }
            } else {
                return_input(id, text);
            }
        }
    }

    /// Some(true) confirms, Some(false) cancels.
    pub(in crate::scene) fn touch(&mut self, touch: &Touch, t: f32) -> Option<bool> {
        if self.package.difficulties.len() > 1 {
            for (index, direction) in [-1, 1].into_iter().enumerate() {
                if self.difficulty_buttons[index].touch(touch, t) {
                    self.change_difficulty(direction);
                    return None;
                }
            }
        }
        for (index, field) in FIELDS.into_iter().enumerate() {
            if matches!(field, Field::Spacing) && self.layout.positions.len() == 1 {
                continue;
            }
            if self.field_buttons[index].touch(touch, t) {
                request_input(
                    field.id(),
                    InputBox::new()
                        .title(field.label())
                        .prompt(field.prompt(self.layout.positions.len()))
                        .default_text(self.field_value(field)),
                );
                return None;
            }
        }
        if self.cancel.touch(touch, t) {
            return Some(false);
        }
        self.import.touch(touch, t).then_some(true)
    }

    pub(in crate::scene) fn into_conversion(self) -> (LanePackage, usize, Layout) {
        (self.package, self.difficulty, self.layout)
    }

    pub(in crate::scene) fn render(&mut self, ui: &mut Ui, t: f32) {
        let count = self.package.difficulties.len();
        let chart = &self.package.difficulties[self.difficulty];
        let h = (ui.screen_rect().h - 0.04).min(0.78);
        let panel = Rect::new(-0.76, -h / 2., 1.52, h);
        ui.fill_rect(ui.screen_rect(), semi_black(0.62));
        ui.fill_rect(panel, Color::from_rgba(26, 30, 38, 255));
        ui.text(itl!("lane-import-title"))
            .pos(0., panel.y + h * 0.085)
            .size(0.62)
            .anchor(0.5, 0.5)
            .no_baseline()
            .draw();

        let left = panel.x + 0.065;
        let right = panel.right() - 0.065;
        let selector_y = panel.y + h * 0.18;
        if count > 1 {
            let side_w = 0.10;
            let side_h = h * 0.10;
            self.difficulty_buttons[0].render_text(ui, Rect::new(left, selector_y, side_w, side_h), t, "<", 0.46, false);
            self.difficulty_buttons[1].render_text(ui, Rect::new(right - side_w, selector_y, side_w, side_h), t, ">", 0.46, false);
        }
        ui.text(format!("{}   {}K   {}/{}", chart.info.level, chart.lanes, self.difficulty + 1, count))
            .pos(0., selector_y + h * 0.05)
            .size(0.47)
            .max_width(1.08)
            .anchor(0.5, 0.5)
            .no_baseline()
            .draw();

        for (index, field) in FIELDS.into_iter().enumerate() {
            if matches!(field, Field::Spacing) && chart.lanes == 1 {
                continue;
            }
            let y = panel.y + h * (0.32 + index as f32 * 0.121);
            ui.text(field.label())
                .pos(left, y + h * 0.046)
                .size(0.43)
                .max_width(0.34)
                .anchor(0., 0.5)
                .no_baseline()
                .draw();
            let value = self.field_value(field);
            let text = if value.is_empty() {
                itl!("lane-import-custom-layout")
            } else {
                value.into()
            };
            self.field_buttons[index].render_text(ui, Rect::new(left + 0.37, y, 0.49, h * 0.092), t, text, 0.43, false);
        }

        let preview = Rect::new(left + 0.90, panel.y + h * 0.32, 0.49, h * 0.46);
        ui.fill_rect(preview, semi_black(0.6));
        let line_y = preview.y + (450. - self.layout.line_y) / 900. * preview.h;
        let accent = ui.accent();
        ui.fill_rect(Rect::new(preview.x, line_y - 0.002, preview.w, 0.004), accent);
        for &position in &self.layout.positions {
            let x = preview.x + (position + 675.) / 1350. * preview.w;
            ui.fill_rect(Rect::new(x - 0.0015, preview.y, 0.003, line_y - preview.y), Color::new(1., 1., 1., 0.18));
            ui.fill_rect(Rect::new(x - 0.007, line_y - 0.004, 0.014, 0.008), WHITE);
        }

        let y = panel.y + h * 0.84;
        self.cancel
            .render_text(ui, Rect::new(right - 0.58, y, 0.27, h * 0.105), t, ttl!("cancel"), 0.46, false);
        self.import
            .render_text(ui, Rect::new(right - 0.27, y, 0.27, h * 0.105), t, itl!("lane-import-confirm"), 0.46, true);
    }
}
