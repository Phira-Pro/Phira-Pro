//! The LIFE display reads normalized health; judgement owns all health rules.
use crate::{
    config::Config,
    core::{Matrix, PGR_FONT},
    ext::SafeTexture,
    ui::Ui,
};
use macroquad::prelude::*;
use miniquad::{BlendFactor, BlendState, BlendValue, Equation};

const SMOOTH_TIME: f32 = 0.16;
const FLASH_TIME: f32 = 0.15;
const DEFAULT_WIDTH: f32 = 0.6;

#[derive(Default)]
pub(super) struct LifeBar {
    display: Display,
    material: Option<LifeMaterial>,
    material_failed: bool,
}

struct Display {
    target: f32,
    life: f32,
    velocity: f32,
    flash: f32,
    time: Option<f64>,
    flow: f32,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            target: 1.,
            life: 1.,
            velocity: 0.,
            flash: 0.,
            time: None,
            flow: 0.,
        }
    }
}

impl Display {
    fn health_number(&self, capacity: u32) -> u32 {
        let points = self.target * capacity as f32;
        // Suppress rounding noise from converting normalized health to points.
        ((points - capacity as f32 * f32::EPSILON).ceil() as u32).max(u32::from(points > 0.))
    }

    fn observe(&mut self, health: f32, time: f64) {
        let health = health.clamp(0., 1.);
        let previous = self.time.replace(time);
        let Some(elapsed) = previous.map(|last| time - last).filter(|dt| (0. ..=0.5).contains(dt)) else {
            self.target = health;
            self.life = health;
            self.velocity = 0.;
            self.flash = 0.;
            self.flow = time.max(0.) as f32;
            return;
        };
        let dt = elapsed.min(0.1) as f32;
        self.flow += dt;
        self.flash = (self.flash - dt).max(0.);
        if health < self.target {
            self.flash = FLASH_TIME;
        }
        self.target = health;
        let omega = 2. / SMOOTH_TIME;
        let x = omega * dt;
        let decay = 1. / (1. + x + 0.48 * x * x + 0.235 * x * x * x);
        let change = self.life - health;
        let step = (self.velocity + omega * change) * dt;
        self.velocity = (self.velocity - omega * step) * decay;
        let next = health + (change + step) * decay;
        if (next - health).abs() < 0.0001 || (health - self.life) * (next - health) > 0. {
            self.life = health;
            self.velocity = 0.;
        } else {
            self.life = next.clamp(0., 1.);
        }
    }
}

struct LifeMaterial {
    material: Material,
    _noise: [SafeTexture; 2],
}

impl LifeMaterial {
    fn load() -> anyhow::Result<Self> {
        use UniformType::*;
        let noise = [
            load_noise(include_bytes!("../../../../assets/lifebar/noise-a.png"))?,
            load_noise(include_bytes!("../../../../assets/lifebar/noise-b.png"))?,
        ];
        let material = load_material(
            include_str!("life_bar.vert"),
            include_str!("life_bar.frag"),
            MaterialParams {
                pipeline_params: PipelineParams {
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                    )),
                    alpha_blend: Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::OneMinusValue(BlendValue::SourceAlpha))),
                    ..Default::default()
                },
                uniforms: [("Life", Float1), ("Flow", Float1), ("Tint", Float4), ("Opacity", Float1)]
                    .into_iter()
                    .map(|(name, ty)| (name.to_owned(), ty))
                    .collect(),
                textures: vec!["NoiseA".into(), "NoiseB".into()],
            },
        )?;
        material.set_texture("NoiseA", *noise[0]);
        material.set_texture("NoiseB", *noise[1]);
        Ok(Self { material, _noise: noise })
    }
}

impl Drop for LifeMaterial {
    fn drop(&mut self) {
        // The HUD quad is flushed before leaving draw().
        self.material.delete();
    }
}

fn load_noise(bytes: &[u8]) -> anyhow::Result<SafeTexture> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8();
    let image = image::imageops::flip_vertical(&image);
    let texture = Texture2D::from_rgba8(image.width() as u16, image.height() as u16, image.as_raw());
    texture.set_filter(FilterMode::Linear);
    texture
        .raw_miniquad_texture_handle()
        .set_wrap(unsafe { get_internal_gl() }.quad_context, miniquad::TextureWrap::Repeat);
    Ok(texture.into())
}

impl LifeBar {
    pub(super) fn reset(&mut self) {
        self.display = Display::default();
    }

    pub(super) fn draw(&mut self, ui: &mut Ui, config: &Config, health: f32, time: f64, opacity: f32) {
        self.display.observe(health, time);
        if opacity <= 0. {
            return;
        }
        if self.material.is_none() && !self.material_failed {
            match LifeMaterial::load() {
                Ok(material) => self.material = Some(material),
                Err(error) => {
                    tracing::warn!(%error, "LIFE shader unavailable; using the simple LIFE display");
                    self.material_failed = true;
                }
            }
        }
        let screen = ui.screen_rect();
        // Leave room for the rim even at the widest setting.
        let width_scale = config.hp_width / DEFAULT_WIDTH;
        let width = screen.w
            * if width_scale <= 1. {
                width_scale
            } else {
                1. + (width_scale - 1.) * 0.65
            };
        let height = screen.h * 0.1 * config.hp_height;
        let (r, g, b) = config.hp_color.rgb();
        let display = &self.display;
        if display.life > 0.0001 {
            if let Some(bar) = &self.material {
                let material = bar.material;
                material.set_uniform("Life", display.life);
                material.set_uniform("Flow", display.flow);
                material.set_uniform("Tint", vec4(r, g, b, 1.));
                material.set_uniform("Opacity", opacity);
                gl_use_material(material);
                draw_rectangle(-width / 2., screen.y, width, height, WHITE);
                gl_use_default_material();
                unsafe { get_internal_gl() }.flush();
            } else {
                let w = width * display.life * 0.66;
                for row in 0..12 {
                    let y = row as f32 / 12.;
                    draw_rectangle(-w / 2., screen.y + y * height, w, height / 12., Color::new(r, g, b, (1. - y) * opacity * 0.8));
                }
            }
        }
        let flash = display.flash / FLASH_TIME;
        let label_top = screen.y + screen.h * 0.03;
        let number = display.health_number(config.hp_capacity());
        ui.alpha(opacity, |ui| {
            let fill = Color::new(flash, 0., 0., 1.);
            let edge = Color::new((r * 0.9).max(0.05), (g * 0.9).max(0.05), (b * 0.9).max(0.05), 1.);
            let bottom = outlined_text(ui, &number.to_string(), label_top, 1.0, fill, edge);
            outlined_text(ui, "LIFE", bottom + 0.01, 0.4, fill, edge);
        });
    }
}

fn outlined_text(ui: &mut Ui, text: &str, y: f32, size: f32, fill: Color, edge: Color) -> f32 {
    let d = 4. / crate::ext::get_viewport().2.max(1) as f32;
    // Give the glyph atlas a larger raster to preserve the outline on scaled HUDs.
    let ss = 2.;
    let scale = Matrix::new_scaling(1. / ss);
    let bounds = ui.text(text).size(size * ss).measure_using(&PGR_FONT);
    let x = -bounds.w / (2. * ss);
    let c = d * std::f32::consts::FRAC_1_SQRT_2;
    for (dx, dy) in [(-d, 0.), (d, 0.), (0., -d), (0., d), (-c, -c), (c, -c), (-c, c), (c, c)] {
        ui.text(text)
            .pos(x + dx, y + dy)
            .size(size * ss)
            .scale(scale)
            .color(edge)
            .draw_using(&PGR_FONT);
    }
    ui.text(text).pos(x, y).size(size * ss).scale(scale).color(fill).draw_using(&PGR_FONT);
    y + bounds.h / ss
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HealthBarColor;

    #[test]
    fn new_life_uses_reference_red_when_upgrading_and_remembers_its_own_color() {
        let legacy: Config = serde_yaml::from_str("hpMode: true\nhpColor: Green").unwrap();
        assert!(legacy.hp_mode);
        assert_eq!(legacy.hp_color, HealthBarColor::Red);
        let configured: Config = serde_yaml::from_str("lifeBarColor: Blue").unwrap();
        let saved: Config = serde_yaml::from_str(&serde_yaml::to_string(&configured).unwrap()).unwrap();
        assert_eq!(saved.hp_color, HealthBarColor::Blue);
    }

    #[test]
    fn damage_flashes_and_changes_the_number_before_the_bar_contracts() {
        let mut display = Display::default();
        display.observe(1., 0.);
        display.observe(0.6, 1. / 60.);
        assert_eq!(display.health_number(100), 60);
        assert_eq!(display.health_number(200), 120);
        assert_eq!(display.flash, FLASH_TIME);
        assert!(display.life > 0.6 && display.life < 1.);
        for frame in 2..60 {
            display.observe(0.6, frame as f64 / 60.);
        }
        assert_eq!(display.flash, 0.);
        assert!((display.life - 0.6).abs() < 0.001);
    }

    #[test]
    fn healing_expands_without_a_flash_and_seeking_snaps_to_new_life() {
        let mut display = Display::default();
        display.observe(0.2, 1.);
        display.observe(0.7, 1.02);
        assert!(display.life > 0.2 && display.life < 0.7);
        assert_eq!(display.flash, 0.);
        display.observe(0., 0.5);
        assert_eq!(display.life, 0.);
        assert_eq!(display.flash, 0.);
    }
}
