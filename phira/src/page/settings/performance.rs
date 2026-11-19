//! Runtime quality, frame-rate and rendering-optimization settings.
use super::{render_setting_row, render_switch, render_title, right_rect, L10N_LOCAL};
use crate::popup::ChooseButton;
use crate::{get_data, get_data_mut};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::config::Config;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "android", target_os = "ios"))]
use prpr::config::UNLIMITED_FPS;
use prpr::ui::{DRectButton, Ui, PREFER_REDUCED_MOTION};
#[cfg(any(target_os = "android", target_os = "ios"))]
use std::borrow::Cow;
use std::sync::atomic::Ordering;

#[cfg(target_os = "android")]
const REFRESH_RATES: [u16; 8] = [60, 120, 144, 165, 185, 200, 240, UNLIMITED_FPS];
#[cfg(target_os = "ios")]
const REFRESH_RATES: [u16; 3] = [60, 120, UNLIMITED_FPS];
#[cfg(any(target_os = "windows", target_os = "linux"))]
const DESKTOP_FPS_LIMITS: [u16; 11] = [0, 60, 120, 144, 165, 185, 200, 240, 360, 480, UNLIMITED_FPS];
const HAS_FPS_SECTION: bool = cfg!(any(target_os = "windows", target_os = "linux", target_os = "android", target_os = "ios"));

fn apply_quality_preset(config: &mut Config, index: usize) {
    match index {
        0 => {
            config.particle = false;
            config.noise_glow = false;
            config.noise_refraction = false;
            config.sample_count = 1;
            config.fxaa = false;
        }
        1 => {
            config.particle = true;
            config.noise_glow = true;
            config.noise_refraction = true;
            config.sample_count = 1;
            config.fxaa = false;
        }
        _ => {
            config.particle = true;
            config.noise_glow = true;
            config.noise_refraction = true;
            config.sample_count = 4;
            config.fxaa = false;
        }
    }
}

fn aa_index(config: &Config) -> usize {
    if config.sample_count >= 4 {
        3
    } else if config.sample_count >= 2 {
        2
    } else if config.fxaa {
        1
    } else {
        0
    }
}

fn apply_antialiasing(config: &mut Config, index: usize) {
    match index {
        1 => {
            config.sample_count = 1;
            config.fxaa = true;
        }
        2 => {
            config.sample_count = 2;
            config.fxaa = false;
        }
        3 => {
            config.sample_count = 4;
            config.fxaa = false;
        }
        _ => {
            config.sample_count = 1;
            config.fxaa = false;
        }
    }
}

pub(super) struct PerformanceList {
    sections: [DRectButton; 4],
    section_open: [bool; 4],
    presets: [DRectButton; 3],
    particle: DRectButton,
    noise_glow: DRectButton,
    noise_refraction: DRectButton,
    antialiasing: ChooseButton,
    aa_lang: Option<String>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    desktop_fps: ChooseButton,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    desktop_fps_lang: Option<String>,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    refresh_rate: DRectButton,
    aggressive: DRectButton,
    block_simple: DRectButton,
    line_elision: DRectButton,
    shader_pre_render: DRectButton,
    font_atlas_pool: DRectButton,
    effect_render_optimized: DRectButton,
    reduced_motion: DRectButton,
}

impl PerformanceList {
    pub fn new() -> Self {
        Self {
            sections: std::array::from_fn(|_| DRectButton::new()),
            section_open: [true; 4],
            presets: std::array::from_fn(|_| DRectButton::new()),
            particle: DRectButton::new(),
            noise_glow: DRectButton::new(),
            noise_refraction: DRectButton::new(),
            antialiasing: ChooseButton::new()
                .with_compact_popup()
                .with_popup_height(0.42)
                .with_options(Self::aa_options())
                .with_selected(aa_index(&get_data().config)),
            aa_lang: get_data().language.clone(),
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            desktop_fps: ChooseButton::new().with_compact_popup().with_options(Self::fps_options()).with_selected(
                DESKTOP_FPS_LIMITS
                    .iter()
                    .position(|rate| *rate == get_data().config.desktop_fps_limit)
                    .unwrap_or(0),
            ),
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            desktop_fps_lang: get_data().language.clone(),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            refresh_rate: DRectButton::new(),
            aggressive: DRectButton::new(),
            block_simple: DRectButton::new(),
            line_elision: DRectButton::new(),
            shader_pre_render: DRectButton::new(),
            font_atlas_pool: DRectButton::new(),
            effect_render_optimized: DRectButton::new(),
            reduced_motion: DRectButton::new(),
        }
    }

    fn aa_options() -> Vec<String> {
        vec![
            tl!("item-aa-off").into_owned(),
            tl!("item-aa-fxaa").into_owned(),
            tl!("item-aa-msaa2").into_owned(),
            tl!("item-aa-msaa4").into_owned(),
        ]
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn fps_options() -> Vec<String> {
        DESKTOP_FPS_LIMITS
            .iter()
            .map(|rate| {
                if *rate == 0 {
                    tl!("item-desktop-fps-auto").into_owned()
                } else if *rate == UNLIMITED_FPS {
                    tl!("item-fps-unlimited").into_owned()
                } else {
                    format!("{rate} FPS")
                }
            })
            .collect()
    }

    pub fn open_all(&mut self) {
        self.section_open = [true; 4];
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.antialiasing.top_touch(touch, t) {
            return true;
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            self.desktop_fps.top_touch(touch, t)
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            let _ = (touch, t);
            false
        }
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        for (index, button) in self.sections.iter_mut().enumerate() {
            if index == 1 && !HAS_FPS_SECTION {
                continue;
            }
            if button.touch(touch, t) {
                self.section_open[index] ^= true;
                return Ok(Some(false));
            }
        }
        if self.section_open[3] && self.reduced_motion.touch(touch, t) {
            let data = get_data_mut();
            data.prefer_reduced_motion ^= true;
            PREFER_REDUCED_MOTION.store(data.prefer_reduced_motion, Ordering::Relaxed);
            return Ok(Some(true));
        }
        let data = get_data_mut();
        let config = &mut data.config;
        if self.section_open[0] {
            for (i, button) in self.presets.iter_mut().enumerate() {
                if button.touch(touch, t) {
                    apply_quality_preset(config, i);
                    self.antialiasing.set_selected(aa_index(config));
                    return Ok(Some(true));
                }
            }
            if self.particle.touch(touch, t) {
                config.particle ^= true;
                return Ok(Some(true));
            }
            if self.noise_glow.touch(touch, t) {
                config.noise_glow ^= true;
                return Ok(Some(true));
            }
            if self.noise_refraction.touch(touch, t) {
                config.noise_refraction ^= true;
                return Ok(Some(true));
            }
            if self.antialiasing.touch(touch, t) {
                return Ok(Some(false));
            }
        }
        if self.section_open[1] {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if self.desktop_fps.touch(touch, t) {
                return Ok(Some(false));
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            if self.refresh_rate.touch(touch, t) {
                let selected = REFRESH_RATES.iter().position(|rate| *rate == config.preferred_refresh_rate).unwrap_or(0);
                config.preferred_refresh_rate = REFRESH_RATES[(selected + 1) % REFRESH_RATES.len()];
                crate::set_refresh_rate(config.preferred_refresh_rate)?;
                return Ok(Some(true));
            }
        }
        if self.section_open[2] {
            if self.aggressive.touch(touch, t) {
                config.aggressive ^= true;
                return Ok(Some(true));
            }
            if self.block_simple.touch(touch, t) {
                config.block_area_simple ^= true;
                return Ok(Some(true));
            }
            if self.line_elision.touch(touch, t) {
                config.line_elision ^= true;
                return Ok(Some(true));
            }
            if self.shader_pre_render.touch(touch, t) {
                config.shader_pre_render ^= true;
                return Ok(Some(true));
            }
            if self.font_atlas_pool.touch(touch, t) {
                config.font_atlas_pool ^= true;
                prpr::ui::FONT_ATLAS_POOL.store(config.font_atlas_pool, Ordering::Relaxed);
                return Ok(Some(true));
            }
            if self.effect_render_optimized.touch(touch, t) {
                config.effect_render_optimized ^= true;
                prpr::core::EFFECT_RENDER_OPTIMIZED.store(config.effect_render_optimized, Ordering::Relaxed);
                return Ok(Some(true));
            }
        }
        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        let language = get_data().language.clone();
        if self.aa_lang != language {
            self.aa_lang = language.clone();
            self.antialiasing.set_options(Self::aa_options());
        }
        self.antialiasing.update(t);
        if self.antialiasing.changed() {
            apply_antialiasing(&mut get_data_mut().config, self.antialiasing.selected());
            return Ok(true);
        }
        self.antialiasing.set_selected(aa_index(&get_data().config));

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            if self.desktop_fps_lang != language {
                self.desktop_fps_lang = language;
                self.desktop_fps.set_options(Self::fps_options());
            }
            self.desktop_fps.update(t);
            if self.desktop_fps.changed() {
                get_data_mut().config.desktop_fps_limit = DESKTOP_FPS_LIMITS[self.desktop_fps.selected()];
                return Ok(true);
            }
            self.desktop_fps.set_selected(
                DESKTOP_FPS_LIMITS
                    .iter()
                    .position(|rate| *rate == get_data().config.desktop_fps_limit)
                    .unwrap_or(0),
            );
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let _ = t;
        Ok(false)
    }

    pub fn render_top(&mut self, ui: &mut Ui, t: f32) {
        self.antialiasing.render_top(ui, t, 1.);
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        self.desktop_fps.render_top(ui, t, 1.);
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => { render_setting_row!(ui, h, (w - 0.48).min(1.2); $($b)*) };
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;

        self.sections[0].render_text(
            ui,
            Rect::new(0.04, 0.005, w - 0.08, 0.085),
            t,
            format!("{}  {}", if self.section_open[0] { "−" } else { "+" }, tl!("performance-quality")),
            0.4,
            false,
        );
        ui.dy(0.11);
        h += 0.11;
        if self.section_open[0] {
            item! {
                render_title(ui, tl!("performance-preset"), Some(tl!("performance-preset-sub")));
            }
            let gap = 0.012;
            let margin = 0.02;
            let button_width = (w - margin * 2. - gap * 2.).max(0.) / 3.;
            for (i, button) in self.presets.iter_mut().enumerate() {
                let label = ["performance-preset-low", "performance-preset-standard", "performance-preset-high"][i];
                button.render_text(ui, Rect::new(margin + i as f32 * (button_width + gap), 0., button_width, 0.085), t, tl!(label), 0.4, false);
            }
            ui.dy(0.10);
            h += 0.10;

            item! {
                render_title(ui, tl!("item-particle"), Some(tl!("item-particle-sub")));
                render_switch(ui, rr, t, &mut self.particle, config.particle);
            }
            item! {
                render_title(ui, tl!("item-noise-glow"), Some(tl!("item-noise-glow-sub")));
                render_switch(ui, rr, t, &mut self.noise_glow, config.noise_glow);
            }
            item! {
                render_title(ui, tl!("item-noise-refraction"), Some(tl!("item-noise-refraction-sub")));
                render_switch(ui, rr, t, &mut self.noise_refraction, config.noise_refraction);
            }
            item! {
                render_title(ui, tl!("item-aa"), Some(tl!("item-aa-sub")));
                self.antialiasing.render(ui, rr, t);
            }
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "android", target_os = "ios"))]
        {
            self.sections[1].render_text(
                ui,
                Rect::new(0.04, 0.005, w - 0.08, 0.085),
                t,
                format!("{}  {}", if self.section_open[1] { "−" } else { "+" }, tl!("performance-fps")),
                0.4,
                false,
            );
            ui.dy(0.11);
            h += 0.11;
            if self.section_open[1] {
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                item! {
                    render_title(ui, tl!("item-desktop-fps"), Some(if cfg!(target_os = "linux") {
                        tl!("item-desktop-fps-linux-sub")
                    } else {
                        tl!("item-desktop-fps-sub")
                    }));
                    self.desktop_fps.render(ui, rr, t);
                }
                #[cfg(any(target_os = "android", target_os = "ios"))]
                item! {
                    render_title(ui, tl!("item-refresh-rate"), Some({
                        let max_rate = f32::from_bits(prpr::ui::MAX_DEVICE_REFRESH_RATE.load(Ordering::Relaxed));
                        if max_rate.is_finite() && max_rate > 0.0 {
                            Cow::Owned(tl!("item-refresh-rate-max", "rate" => format!("{max_rate:.0}")))
                        } else {
                            tl!("item-refresh-rate-sub")
                        }
                    }));
                    let label = if config.preferred_refresh_rate == UNLIMITED_FPS {
                        tl!("item-fps-unlimited")
                    } else {
                        Cow::Owned(format!("{} Hz", config.preferred_refresh_rate))
                    };
                    self.refresh_rate.render_text(ui, rr, t, label, 0.5, false);
                }
            }
        }

        self.sections[2].render_text(
            ui,
            Rect::new(0.04, 0.005, w - 0.08, 0.085),
            t,
            format!("{}  {}", if self.section_open[2] { "−" } else { "+" }, tl!("performance-render")),
            0.4,
            false,
        );
        ui.dy(0.11);
        h += 0.11;
        if self.section_open[2] {
            item! {
                render_title(ui, tl!("item-opt"), Some(tl!("item-opt-sub")));
                render_switch(ui, rr, t, &mut self.aggressive, config.aggressive);
            }
            item! {
                render_title(ui, tl!("item-block-simple"), Some(tl!("item-block-simple-sub")));
                render_switch(ui, rr, t, &mut self.block_simple, config.block_area_simple);
            }
            item! {
                render_title(ui, tl!("item-line-elision"), Some(tl!("item-line-elision-sub")));
                render_switch(ui, rr, t, &mut self.line_elision, config.line_elision);
            }
            item! {
                render_title(ui, tl!("item-shader-pre-render"), Some(tl!("item-shader-pre-render-sub")));
                render_switch(ui, rr, t, &mut self.shader_pre_render, config.shader_pre_render);
            }
            item! {
                render_title(ui, tl!("item-font-atlas-pool"), Some(tl!("item-font-atlas-pool-sub")));
                render_switch(ui, rr, t, &mut self.font_atlas_pool, config.font_atlas_pool);
            }
            item! {
                render_title(ui, tl!("item-effect-render-optimized"), Some(tl!("item-effect-render-optimized-sub")));
                render_switch(ui, rr, t, &mut self.effect_render_optimized, config.effect_render_optimized);
            }
        }

        self.sections[3].render_text(
            ui,
            Rect::new(0.04, 0.005, w - 0.08, 0.085),
            t,
            format!("{}  {}", if self.section_open[3] { "−" } else { "+" }, tl!("performance-interface")),
            0.4,
            false,
        );
        ui.dy(0.11);
        h += 0.11;
        if self.section_open[3] {
            item! {
                render_title(ui, tl!("item-prefer-reduced-motion"), Some(tl!("item-prefer-reduced-motion-sub")));
                render_switch(ui, rr, t, &mut self.reduced_motion, data.prefer_reduced_motion);
            }
        }
        (w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::{aa_index, apply_antialiasing, apply_quality_preset};

    #[test]
    fn quality_presets_cover_particles_noise_and_samples() {
        let mut config = prpr::config::Config::default();
        apply_quality_preset(&mut config, 0);
        assert!(!config.particle && !config.noise_glow && !config.noise_refraction);
        assert_eq!(config.sample_count, 1);
        assert!(!config.fxaa);

        apply_quality_preset(&mut config, 1);
        assert!(config.particle && config.noise_glow && config.noise_refraction);
        assert_eq!(config.sample_count, 1);
        assert!(!config.fxaa);

        apply_quality_preset(&mut config, 2);
        assert!(config.particle && config.noise_glow && config.noise_refraction);
        assert_eq!(config.sample_count, 4);
        assert!(!config.fxaa);
    }

    #[test]
    fn antialiasing_is_a_single_choice() {
        let mut config = prpr::config::Config::default();
        for (index, sample_count, fxaa) in [(0, 1, false), (1, 1, true), (2, 2, false), (3, 4, false)] {
            apply_antialiasing(&mut config, index);
            assert_eq!(config.sample_count, sample_count);
            assert_eq!(config.fxaa, fxaa);
            assert_eq!(aa_index(&config), index);
        }
    }
}
