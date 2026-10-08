//! One settings panel for selecting, copying, editing and saving judgement presets.
use super::{item_row_h, render_switch, right_rect, ChooseButton, L10N_LOCAL};
use crate::{
    get_data, get_data_mut,
    judgement_presets::{self, JudgePreset, JudgeSettings},
    save_data,
};
use anyhow::Result;
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    config::JudgeAlgorithm,
    ext::{semi_white, RectExt},
    scene::{request_input, return_input, show_error, take_input},
    ui::{DRectButton, Scroll, Ui},
};

const PHIGROS: &str = "builtin-phigros";
const PRO: &str = "builtin-pro";
const DETAILED: &str = "builtin-detailed";

#[cfg(all(test, target_os = "windows"))]
mod tests;

struct Draft {
    id: Option<String>,
    name: String,
    settings: JudgeSettings,
}
pub(super) struct JudgementList {
    selector: ChooseButton,
    od: ChooseButton,
    od_names: Vec<String>,
    ids: Vec<Option<String>>,
    names: Vec<String>,
    add: DRectButton,
    edit: DRectButton,
    delete: DRectButton,
    save: DRectButton,
    cancel: DRectButton,
    name: DRectButton,
    algorithm: DRectButton,
    numbers: Vec<DRectButton>,
    sides: Vec<DRectButton>,
    switches: Vec<DRectButton>,
    draft: Option<Draft>,
    dirty: bool,
    discard_input: bool,
}

// All numeric inputs share a finite parser; selecting Phigros exposes its additional parameters.
fn number(s: &mut JudgeSettings, i: usize) -> &mut f64 {
    match i {
        5 => &mut s.phigros.drag_ms,
        6 => &mut s.phigros.flick_ratio,
        7 => &mut s.phigros.tap_width,
        8 => &mut s.phigros.special_width,
        9 => &mut s.phigros.bad_edge,
        10 => &mut s.phigros.metric_divisor,
        11 => &mut s.phigros.protection_ms,
        13 => &mut s.phigros.hold_tail_ms,
        14 => &mut s.phigros.hold_delayed_miss_ms,
        15 => &mut s.phigros.special_early_ms,
        16 => &mut s.phigros.strict_perfect_ms,
        17 => &mut s.phigros.strict_good_ms,
        18 => &mut s.phigros.strict_bad_ms,
        19 => &mut s.phigros.flick_speed,
        20 => &mut s.phigros.flick_dpi,
        21 => &mut s.phigros.flick_sample_hz,
        22 => &mut s.phigros.flick_multiplier,
        23 => &mut s.phigros.flick_projection_min,
        24 => &mut s.phigros.bad_shrink_factor,
        25 => &mut s.phigros.device_dpi,
        _ => unreachable!(),
    }
}
fn number_value(s: &JudgeSettings, i: usize) -> f64 {
    if i < 4 {
        s.windows_ms[i] as f64
    } else if i == 4 {
        s.late_ms as f64
    } else if i == 12 {
        s.phigros.hold_safe_frames as f64
    } else {
        let mut clone = s.clone();
        *number(&mut clone, i)
    }
}
fn switch(s: &mut JudgeSettings, i: usize) -> &mut bool {
    match i {
        0 => &mut s.drag_protect,
        1 => &mut s.flick_protect,
        2 => &mut s.strict,
        3 => &mut s.fullscreen,
        4 => &mut s.no_combo_score,
        5 => &mut s.hold_tail,
        6 => &mut s.phigros.frame_compensation,
        7 => &mut s.phigros.late_overrun,
        8 => &mut s.grading.perfect_plus,
        9 => &mut s.grading.detailed,
        _ => unreachable!(),
    }
}
fn setting_indices(s: &JudgeSettings) -> Vec<usize> {
    let mut indices = vec![];
    if s.grading.perfect_plus {
        indices.push(0);
    }
    indices.push(1);
    if s.grading.detailed {
        indices.push(26);
    }
    indices.push(2);
    if s.grading.detailed {
        indices.extend([27, 28]);
    }
    indices.push(3);
    indices.extend(4..if s.algorithm == JudgeAlgorithm::Phigros { 26 } else { 5 });
    indices
}
fn switch_indices(s: &JudgeSettings) -> Vec<usize> {
    let mut indices = vec![8, 9];
    indices.extend(0..if s.algorithm == JudgeAlgorithm::Phigros { 8 } else { 6 });
    indices
}
fn number_label(i: usize) -> String {
    tl!(format!("preset-number-{i}")).into_owned()
}
fn switch_label(i: usize) -> String {
    tl!(format!("preset-switch-{i}")).into_owned()
}

fn render_title<'a>(ui: &mut Ui, title: impl Into<std::borrow::Cow<'a, str>>, subtitle: Option<std::borrow::Cow<'a, str>>) {
    ui.text(title.into())
        .pos(0.035, item_row_h() * if subtitle.is_some() { 0.32 } else { 0.5 })
        .anchor(0., 0.5)
        .size(0.48)
        .max_width(1.08)
        .draw();
    if let Some(subtitle) = subtitle {
        ui.text(subtitle)
            .pos(0.035, item_row_h() * 0.76)
            .anchor(0., 0.5)
            .size(0.28)
            .max_width(1.55)
            .color(semi_white(0.6))
            .draw();
    }
}
fn sided(i: usize) -> bool {
    i >= 26 || i < 4 || matches!(i, 5 | 6 | 16 | 17 | 18)
}
fn side_value(s: &JudgeSettings, i: usize, late: bool) -> f64 {
    let side = usize::from(late);
    match i {
        0..=3 => s.timing.sides(s.windows_ms)[side][i] as f64,
        26..=28 => (if late { s.grading.late_ms[i - 26] } else { s.grading.early_ms[i - 26] }) as f64,
        5 => s.phigros.drag_sides()[side],
        6 => s.phigros.flick_sides()[side],
        16..=18 => s.phigros.strict_sides()[side][i - 16],
        _ => unreachable!(),
    }
}
fn set_side(s: &mut JudgeSettings, i: usize, late: bool, value: f64) {
    let side = usize::from(late);
    match i {
        0..=3 => s.timing.set(s.windows_ms, i, late, value as f32),
        26..=28 => {
            if late {
                s.grading.late_ms[i - 26] = value as f32;
            } else {
                s.grading.early_ms[i - 26] = value as f32;
            }
        }
        5 => {
            let mut v = s.phigros.drag_sides();
            v[side] = value;
            s.phigros.drag_sides_ms = Some(v);
        }
        6 => {
            let mut v = s.phigros.flick_sides();
            v[side] = value;
            s.phigros.flick_ratio_sides = Some(v);
        }
        16..=18 => {
            let mut v = s.phigros.strict_sides()[side];
            let i = i - 16;
            v[i] = value;
            for j in i + 1..3 {
                v[j] = v[j].max(v[j - 1]);
            }
            for j in (0..i).rev() {
                v[j] = v[j].min(v[j + 1]);
            }
            if late {
                s.phigros.strict_late_ms = Some(v);
            } else {
                s.phigros.strict_early_ms = Some(v);
            }
        }
        _ => unreachable!(),
    }
    s.normalize();
}
fn fmt(v: f64) -> String {
    format!("{v:.3}").trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl JudgementList {
    pub fn is_editing(&self) -> bool {
        self.draft.is_some()
    }
    pub fn new() -> Self {
        let mut list = Self {
            selector: ChooseButton::new().with_compact_popup(),
            od_names: Vec::new(),
            od: ChooseButton::new()
                .with_compact_popup()
                .with_options(vec![String::new()])
                .with_selected(0),
            ids: Vec::new(),
            names: Vec::new(),
            add: DRectButton::new(),
            edit: DRectButton::new(),
            delete: DRectButton::new(),
            save: DRectButton::new(),
            cancel: DRectButton::new(),
            name: DRectButton::new(),
            algorithm: DRectButton::new(),
            sides: (0..58).map(|_| DRectButton::new()).collect(),
            numbers: (0..29).map(|_| DRectButton::new()).collect(),
            switches: (0..10).map(|_| DRectButton::new()).collect(),
            draft: None,
            dirty: false,
            discard_input: false,
        };
        list.refresh();
        list
    }
    fn refresh(&mut self) {
        let data = get_data();
        let current = JudgeSettings::capture(&data.config);
        let mut names = vec![
            tl!("preset-phigros").into_owned(),
            tl!("preset-pro").into_owned(),
            tl!("preset-detailed").into_owned(),
        ];
        self.ids = vec![Some(PHIGROS.into()), Some(PRO.into()), Some(DETAILED.into())];
        let mut settings = vec![JudgeSettings::phigros(), JudgeSettings::default(), JudgeSettings::detailed()];
        for p in &data.judge_presets {
            names.push(p.name.clone());
            self.ids.push(Some(p.id.clone()));
            settings.push(p.settings.clone());
        }
        let selected = self
            .ids
            .iter()
            .zip(&settings)
            .position(|(id, s)| *id == data.judge_preset_id && *s == current)
            .or_else(|| settings.iter().position(|s| *s == current))
            .unwrap_or(settings.len());
        names.push(tl!("preset-current").into_owned());
        self.ids.push(None);
        if self.names != names {
            self.selector.set_options(names.clone());
            self.names = names;
        }
        if self.selector.selected() != selected {
            self.selector.set_selected(selected);
        }
    }
    fn selection(&self) -> Option<&str> {
        self.ids.get(self.selector.selected()).and_then(|s| s.as_deref())
    }
    fn activate(&mut self) {
        let id = self.selection().map(str::to_owned);
        let settings = match id.as_deref() {
            Some(PHIGROS) => Some(JudgeSettings::phigros()),
            Some(PRO) => Some(JudgeSettings::default()),
            Some(DETAILED) => Some(JudgeSettings::detailed()),
            Some(id) => get_data().judge_presets.iter().find(|p| p.id == id).map(|p| p.settings.clone()),
            None => None,
        };
        if let Some(settings) = settings {
            let data = get_data_mut();
            let old = (data.config.clone(), data.judge_preset_id.clone());
            settings.apply(&mut data.config);
            data.judge_preset_id = id;
            if let Err(err) = save_data() {
                let data = get_data_mut();
                (data.config, data.judge_preset_id) = old;
                show_error(err);
                self.refresh();
            }
        }
    }
    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.od.top_touch(touch, t) || (self.draft.is_none() && self.selector.top_touch(touch, t))
    }
    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        if let Some(mut draft) = self.draft.take() {
            if self.cancel.touch(touch, t) {
                self.discard_input = true;
                self.draft = None;
                return Ok(Some(false));
            }
            if self.save.touch(touch, t) {
                let name = match judgement_presets::valid_name(&draft.name, &get_data().judge_presets, draft.id.as_deref()) {
                    Ok(name) => name,
                    Err(err) => {
                        show_error(anyhow::anyhow!(tl!(err.to_string()).into_owned()));
                        self.draft = Some(draft);
                        return Ok(Some(false));
                    }
                };
                if name == tl!("preset-phigros").as_ref() || name == tl!("preset-pro").as_ref() || name == tl!("preset-detailed").as_ref() {
                    show_error(anyhow::anyhow!(tl!("preset-name-duplicate").into_owned()));
                    self.draft = Some(draft);
                    return Ok(Some(false));
                }
                draft.settings.normalize();
                let preset = JudgePreset {
                    id: draft.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    name,
                    settings: draft.settings.clone(),
                };
                let data = get_data_mut();
                let old = (data.config.clone(), data.judge_presets.clone(), data.judge_preset_id.clone());
                if let Some(p) = data.judge_presets.iter_mut().find(|p| p.id == preset.id) {
                    *p = preset.clone();
                } else {
                    data.judge_presets.push(preset.clone());
                }
                preset.settings.apply(&mut data.config);
                data.judge_preset_id = Some(preset.id);
                if let Err(err) = save_data() {
                    let data = get_data_mut();
                    (data.config, data.judge_presets, data.judge_preset_id) = old;
                    show_error(err);
                    self.draft = Some(draft);
                    return Ok(Some(false));
                }
                self.draft = None;
                self.refresh();
                return Ok(Some(true));
            }
            if self.name.touch(touch, t) {
                self.discard_input = false;
                request_input("preset-name", InputBox::new().default_text(&draft.name));
                self.draft = Some(draft);
                return Ok(Some(false));
            }
            let result = self.touch_controls(touch, t, &mut draft.settings);
            self.draft = Some(draft);
            return result;
        }
        if self.selector.touch(touch, t) {
            return Ok(Some(false));
        }
        let adding = self.add.touch(touch, t);
        let editing = !adding && self.edit.touch(touch, t);
        if adding || editing {
            self.discard_input = false;
            let existing = self.selection().and_then(|id| get_data().judge_presets.iter().find(|p| p.id == id));
            // "Add" always copies to a new ID; built-ins are never overwritten.
            self.draft = Some(Draft {
                id: if editing { existing.map(|p| p.id.clone()) } else { None },
                name: if editing {
                    existing.map(|p| p.name.clone()).unwrap_or_default()
                } else {
                    String::new()
                },
                settings: JudgeSettings::capture(&get_data().config),
            });
            return Ok(Some(false));
        }
        if self.delete.touch(touch, t) {
            if let Some(id) = self.selection().map(str::to_owned).filter(|s| s != PHIGROS && s != PRO && s != DETAILED) {
                let data = get_data_mut();
                let old = (data.judge_presets.clone(), data.judge_preset_id.clone());
                data.judge_presets.retain(|p| p.id != id);
                data.judge_preset_id = None;
                if let Err(err) = save_data() {
                    let data = get_data_mut();
                    (data.judge_presets, data.judge_preset_id) = old;
                    show_error(err);
                    self.refresh();
                    return Ok(Some(false));
                }
                self.refresh();
                return Ok(Some(true));
            }
        }
        let mut settings = JudgeSettings::capture(&get_data().config);
        let previous = settings.clone();
        let result = self.touch_controls(touch, t, &mut settings)?;
        if previous != settings {
            self.apply_current(settings);
        }
        Ok(result)
    }
    fn apply_current(&mut self, mut settings: JudgeSettings) {
        settings.normalize();
        let data = get_data_mut();
        let old = (data.config.clone(), data.judge_preset_id.clone());
        settings.apply(&mut data.config);
        data.judge_preset_id = None;
        if let Err(err) = save_data() {
            let data = get_data_mut();
            (data.config, data.judge_preset_id) = old;
            show_error(err);
        }
        self.refresh();
    }
    fn touch_controls(&mut self, touch: &Touch, t: f32, settings: &mut JudgeSettings) -> Result<Option<bool>> {
        if self.od.touch(touch, t) {
            return Ok(Some(false));
        }
        if self.algorithm.touch(touch, t) {
            settings.algorithm = if settings.algorithm == JudgeAlgorithm::Phigros {
                JudgeAlgorithm::PhiraPro
            } else {
                JudgeAlgorithm::Phigros
            };
            return Ok(Some(false));
        }
        for i in setting_indices(settings) {
            if sided(i) {
                for late in [false, true] {
                    if self.sides[i * 2 + usize::from(late)].touch(touch, t) {
                        let value = side_value(settings, i, late);
                        self.discard_input = false;
                        request_input(
                            format!("preset-side-{i}-{}", usize::from(late)),
                            InputBox::new().default_text(if late || i == 6 { fmt(value) } else { format!("-{}", fmt(value)) }),
                        );
                        return Ok(Some(false));
                    }
                }
                continue;
            }
            if self.numbers[i].touch(touch, t) {
                self.discard_input = false;
                request_input(format!("preset-value-{i}"), InputBox::new().default_text(number_value(settings, i).to_string()));
                return Ok(Some(false));
            }
        }
        for i in switch_indices(settings) {
            if self.switches[i].touch(touch, t) {
                *switch(settings, i) ^= true;
                return Ok(Some(false));
            }
        }
        Ok(None)
    }
    pub fn update(&mut self, t: f32) -> Result<bool> {
        self.od.update(t);
        if self.od.changed() && self.od.selected() > 0 {
            let od = self.od.selected() as i8 - 16;
            if let Some(draft) = &mut self.draft {
                draft.settings.apply_osu_mania_od(od);
            } else {
                let mut settings = JudgeSettings::capture(&get_data().config);
                settings.apply_osu_mania_od(od);
                self.apply_current(settings);
            }
        }
        self.selector.update(t);
        if self.draft.is_none() && self.selector.changed() {
            self.activate();
        }
        let live = self.draft.is_none();
        let before = JudgeSettings::capture(&get_data().config);
        if live {
            self.draft = Some(Draft {
                id: None,
                name: String::new(),
                settings: before.clone(),
            });
        }
        let result = self.update_inner(t);
        if live {
            let settings = self.draft.take().unwrap().settings;
            if settings != before {
                self.apply_current(settings);
            }
            self.refresh();
        }
        result
    }
    fn update_inner(&mut self, _t: f32) -> Result<bool> {
        if let Some((id, text)) = take_input() {
            if self.discard_input && id.starts_with("preset-") {
                return Ok(false);
            }
            if let Some(draft) = &mut self.draft {
                if id == "preset-name" {
                    draft.name = text.trim().to_owned();
                    return Ok(false);
                }
                if let Some(key) = id.strip_prefix("preset-side-") {
                    if let Some((i, side)) = key
                        .split_once('-')
                        .and_then(|(i, side)| Some((i.parse::<usize>().ok()?, side.parse::<usize>().ok()?)))
                        .filter(|(i, side)| *i < 29 && sided(*i) && *side < 2)
                    {
                        match text
                            .trim()
                            .trim_end_matches("ms")
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite() && v.abs() <= 10000. && (side == 0 || *v >= 0.))
                        {
                            Some(v) => set_side(&mut draft.settings, i, side == 1, v.abs()),
                            None => show_error(anyhow::anyhow!(tl!("judge-window-invalid").into_owned())),
                        }
                    }
                    return Ok(false);
                }
                if let Some(i) = id.strip_prefix("preset-value-").and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < 26) {
                    match text.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0. && *v <= 10000.) {
                        Some(v) => {
                            if i < 4 {
                                let mut cfg = prpr::config::Config::default();
                                draft.settings.apply(&mut cfg);
                                cfg.set_judge_window(i, v as f32);
                                draft.settings.windows_ms = JudgeSettings::capture(&cfg).windows_ms;
                            } else if i == 4 {
                                draft.settings.late_ms = v as f32;
                            } else if i == 12 {
                                draft.settings.phigros.hold_safe_frames = v as i32;
                            } else {
                                *number(&mut draft.settings, i) = v;
                            }
                            draft.settings.normalize();
                        }
                        None => show_error(anyhow::anyhow!(tl!("judge-window-invalid").into_owned())),
                    }
                    return Ok(false);
                }
            }
            if id.starts_with("preset-") {
                return Ok(false);
            } // Discard input from a cancelled editor.
            return_input(id, text);
        }
        if self.draft.is_none() {
            self.refresh();
        }
        Ok(std::mem::take(&mut self.dirty))
    }
    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let rr = right_rect(r.w);
        let mut h = 0.;
        macro_rules! row { ($($body:tt)*) => {{ $($body)* ui.dy(item_row_h()); h += item_row_h(); }}; }
        if let Some(mut draft) = self.draft.take() {
            row! { render_title(ui, tl!("preset-name"), None); self.name.render_text(ui, rr, t, if draft.name.is_empty() { tl!("preset-name-empty").into_owned() } else { draft.name.clone() }, 0.42, false); }
            h += self.render_controls(ui, r, t, &mut draft.settings);
            row! { render_title(ui, tl!("preset-save"), None); self.save.render_text(ui, rr, t, tl!("preset-save"), 0.42, true); }
            row! { render_title(ui, tl!("preset-cancel"), None); self.cancel.render_text(ui, rr, t, tl!("preset-cancel"), 0.42, false); }
            self.draft = Some(draft);
        } else {
            row! { render_title(ui, tl!("preset-select"), Some(tl!("preset-select-sub"))); }
            row! { self.selector.render(ui, Rect::new(r.w - 0.61, 0.02, 0.56, item_row_h() - 0.04), t); }
            row! { render_title(ui, tl!("preset-add"), None); self.add.render_text(ui, rr, t, tl!("preset-add"), 0.42, false); }
            let custom = self.selection().is_some_and(|id| id != PHIGROS && id != PRO && id != DETAILED);
            row! { render_title(ui, if custom { tl!("preset-edit") } else { tl!("preset-copy") }, None); self.edit.render_text(ui, rr, t, if custom { tl!("preset-edit") } else { tl!("preset-copy") }, 0.42, false); }
            if custom {
                row! { render_title(ui, tl!("preset-delete"), Some(tl!("preset-delete-sub"))); self.delete.render_text(ui, rr, t, tl!("preset-delete"), 0.42, false); }
            }
            let mut current = JudgeSettings::capture(&get_data().config);
            h += self.render_controls(ui, r, t, &mut current);
        }
        (r.w, h)
    }
    fn render_controls(&mut self, ui: &mut Ui, r: Rect, t: f32, settings: &mut JudgeSettings) -> f32 {
        let rr = right_rect(r.w);
        let mut h = 0.;
        macro_rules! row { ($($body:tt)*) => {{ $($body)* ui.dy(item_row_h()); h += item_row_h(); }}; }
        let mut options = vec![tl!("preset-od-custom").into_owned()];
        options.extend((-15..=15).map(|od| format!("OD {od:+}")));
        if self.od_names != options {
            self.od.set_options(options.clone());
            self.od_names = options;
        }
        self.od.set_selected(settings.selected_osu_mania_od().map_or(0, |od| (od + 16) as usize));
        row! { render_title(ui, tl!("preset-od"), Some(tl!("preset-od-sub"))); self.od.render(ui, Rect::new(r.w - 0.61, 0.02, 0.56, item_row_h() - 0.04), t); }
        for i in [8, 9] {
            row! { render_title(ui, switch_label(i), None); let enabled = *switch(settings, i); render_switch(ui, rr, t, &mut self.switches[i], enabled); }
        }
        row! { render_title(ui, tl!("preset-algorithm"), None); self.algorithm.render_text(ui, rr, t, if settings.algorithm == JudgeAlgorithm::Phigros { tl!("preset-phigros-short") } else { tl!("preset-pro-short") }, 0.42, false); }
        row! {
            render_title(ui, tl!("preset-windows"), None);
            ui.text(tl!("preset-early")).pos(r.w - 0.43, item_row_h() * 0.5).anchor(0.5, 0.5).size(0.34).draw();
            ui.text(tl!("preset-late")).pos(r.w - 0.15, item_row_h() * 0.5).anchor(0.5, 0.5).size(0.34).draw();
        }
        for i in setting_indices(settings) {
            if i == 4 {
                row! { render_title(ui, tl!("preset-rules"), None); }
            }
            if sided(i) {
                row! {
                    ui.fill_rect(Rect::new(0.015, 0.005, r.w - 0.03, item_row_h() - 0.01), Color::new(0.12, 0.17, 0.22, 0.65));
                    render_title(ui, number_label(i), None);
                    for late in [false, true] {
                        let value = side_value(settings, i, late);
                        let label = if i == 6 { fmt(value) } else { format!("{}{} ms", if late { "+" } else { "-" }, fmt(value)) };
                        let button = Rect::new(r.w - if late { 0.28 } else { 0.56 }, (item_row_h() - 0.075) / 2., 0.25, 0.075);
                        self.sides[i * 2 + usize::from(late)].render_text(ui, button, t, label, 0.36, false);
                    }
                }
            } else {
                row! { render_title(ui, number_label(i), None); self.numbers[i].render_text(ui, rr, t, fmt(number_value(settings, i)), 0.42, false); }
            }
        }
        if settings.algorithm == JudgeAlgorithm::Phigros {
            row! { render_title(ui, tl!("preset-dpi"), Some(tl!("preset-dpi-sub"))); }
        }
        for i in switch_indices(settings).into_iter().filter(|i| *i < 8) {
            row! { render_title(ui, switch_label(i), None); let enabled = *switch(settings, i); render_switch(ui, rr, t, &mut self.switches[i], enabled); }
        }
        h
    }
    pub fn render_top(&mut self, ui: &mut Ui, t: f32) {
        self.od.render_top(ui, t, 1.);
        if self.draft.is_none() {
            self.selector.render_top(ui, t, 1.);
        }
    }
}

/// Standalone overlay opened from Settings → Chart; no separate settings tab.
pub(super) struct JudgementPage {
    list: JudgementList,
    scroll: Scroll,
}
impl JudgementPage {
    pub fn new() -> Self {
        Self {
            list: JudgementList::new(),
            scroll: Scroll::new(),
        }
    }
}
impl super::Page for JudgementPage {
    fn label(&self) -> std::borrow::Cow<'static, str> {
        tl!("judgement-settings")
    }
    fn update(&mut self, state: &mut super::SharedState) -> Result<()> {
        self.scroll.update(state.t);
        self.list.update(state.t)?;
        Ok(())
    }
    fn touch(&mut self, touch: &Touch, state: &mut super::SharedState) -> Result<bool> {
        if self.list.top_touch(touch, state.t) || self.scroll.touch(touch, state.t) {
            return Ok(true);
        }
        let editing = self.list.is_editing();
        if let Some(changed) = self.list.touch(touch, state.t)? {
            if editing != self.list.is_editing() || changed {
                self.scroll.y_scroller.reset();
            }
            self.scroll.y_scroller.halt();
            return Ok(true);
        }
        Ok(false)
    }
    fn render(&mut self, ui: &mut Ui, state: &mut super::SharedState) -> Result<()> {
        let r = ui.content_rect().feather(-0.015);
        let t = state.t;
        state.render_fader(ui, |ui| {
            ui.fill_rect(r, Color::new(0.075, 0.11, 0.15, 0.94));
            self.scroll.size((r.w, r.h));
            ui.dx(r.x);
            ui.dy(r.y);
            self.scroll.render(ui, |ui| self.list.render(ui, r, t));
        });
        Ok(())
    }
    fn render_top(&mut self, ui: &mut Ui, state: &mut super::SharedState) -> Result<()> {
        self.list.render_top(ui, state.t);
        Ok(())
    }
    fn on_back_pressed(&mut self, _state: &mut super::SharedState) -> bool {
        if self.list.draft.take().is_some() {
            self.list.discard_input = true;
            self.scroll.y_scroller.reset();
            true
        } else {
            false
        }
    }
}
