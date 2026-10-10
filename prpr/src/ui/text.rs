use super::Ui;
use crate::{
    core::{Matrix, Point, Vector},
    ext::{get_viewport, RectExt},
};
use glyph_brush::{
    ab_glyph::{Font, FontArc, ScaleFont},
    BrushAction, BrushError, GlyphBrush, GlyphBrushBuilder, GlyphCruncher, HorizontalAlign, Section,
};
use macroquad::{
    miniquad::{Texture, TextureParams},
    prelude::*,
};
use once_cell::sync::Lazy;
use std::{borrow::Cow, cell::RefCell, thread::LocalKey};
use tracing::debug;

mod layout;
use layout::{layout_text, InkCache, LayoutOptions};

thread_local! {
    static MULTILINGUAL_FALLBACK: std::cell::RefCell<Option<FontArc>> = const { std::cell::RefCell::new(None) };
}

/// Install the last-resort script font before constructing UI painters. Keep
/// the primary/custom face and its existing CJK fallback ahead of this font.
pub fn set_multilingual_fallback(font: FontArc) {
    MULTILINGUAL_FALLBACK.with(|slot| *slot.borrow_mut() = Some(font));
}

/// Reject invalid line metrics before glyph layout divides by font height.
/// Parsing an SFNT alone does not guarantee that it can be rasterized safely.
pub fn parse_font(bytes: Vec<u8>) -> anyhow::Result<FontArc> {
    let font = FontArc::try_from_vec(bytes).map_err(|_| anyhow::anyhow!("无效或不支持的字体，请使用 TTF / OTF"))?;
    let metrics = [
        font.ascent_unscaled(),
        font.descent_unscaled(),
        font.line_gap_unscaled(),
        font.height_unscaled(),
    ];
    anyhow::ensure!(metrics.iter().all(|n| n.is_finite()) && font.height_unscaled() > 0. && font.glyph_count() > 0, "字体度量无效，无法安全显示");
    Ok(font)
}

#[must_use = "DrawText does nothing until you 'draw' it"]
pub struct DrawText<'a, 's, 'ui> {
    pub ui: &'ui mut Ui<'a>,
    text: Option<Cow<'s, str>>,
    size: f32,
    pos: (f32, f32),
    anchor: (f32, f32),
    color: Color,
    max_width: Option<f32>,
    baseline: bool,
    multiline: bool,
    scale: Matrix,
    h_align: HorizontalAlign,
}

impl<'a, 's, 'ui> DrawText<'a, 's, 'ui> {
    pub(crate) fn new(ui: &'ui mut Ui<'a>, text: Cow<'s, str>) -> Self {
        Self {
            ui,
            text: Some(text),
            size: 1.,
            pos: (0., 0.),
            anchor: (0., 0.),
            color: WHITE,
            max_width: None,
            baseline: true,
            multiline: false,
            scale: Matrix::identity(),
            h_align: HorizontalAlign::Left,
        }
    }

    pub fn h_center(mut self) -> Self {
        self.h_align = HorizontalAlign::Center;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn pos(mut self, x: f32, y: f32) -> Self {
        self.pos = (x, y);
        self
    }

    pub fn anchor(mut self, x: f32, y: f32) -> Self {
        self.anchor = (x, y);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = Some(max_width);
        self
    }

    pub fn no_baseline(mut self) -> Self {
        self.baseline = false;
        self
    }

    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    pub fn scale(mut self, scale: Matrix) -> Self {
        self.scale = scale;
        self
    }

    fn get_scale(&self, w: i32) -> f32 {
        0.04 * self.size * w as f32 * f32::from_bits(super::FONT_DISPLAY_SCALE.load(std::sync::atomic::Ordering::Relaxed))
    }

    fn bounds(&self, (_, _, w, h): (f32, f32, f32, f32)) -> Rect {
        let vp = get_viewport();
        let s = 2. / vp.2 as f32;
        let mut rect = Rect::new(self.pos.0, self.pos.1, w * s, h * s);
        rect.x -= rect.w * self.anchor.0;
        rect.y -= rect.h * self.anchor.1;
        rect
    }

    fn measure_inner<'c>(&mut self, text: &'c str, painter: &mut Option<&mut TextPainter>) -> (Section<'c>, (f32, f32, f32, f32)) {
        let vp = get_viewport();
        let scale = self.get_scale(vp.2);

        let default_text_painter = &mut self.ui.text_painter;
        let painter = painter.as_deref_mut().unwrap_or(default_text_painter);
        layout_text(
            &mut painter.brush,
            &mut painter.ink_cache,
            text,
            LayoutOptions {
                scale,
                primary_scale: painter.primary_scale,
                max_width: self.max_width.map(|w| w * vp.2 as f32 / 2.),
                baseline: self.baseline,
                multiline: self.multiline,
                h_align: self.h_align,
                color: self.color.into(),
            },
        )
    }

    pub fn measure_with_font(&mut self, mut painter: Option<&mut TextPainter>) -> Rect {
        let text = self.text.take().unwrap();
        let (_, bound) = self.measure_inner(&text, &mut painter);
        self.text = Some(text);
        self.bounds(bound)
    }

    pub fn measure_using(&mut self, font: &'static LocalKey<RefCell<Option<TextPainter>>>) -> Rect {
        font.with(|it| self.measure_with_font(it.borrow_mut().as_mut()))
    }

    #[inline]
    pub fn measure(&mut self) -> Rect {
        self.measure_with_font(None)
    }

    pub fn draw_with_font(&mut self, mut painter: Option<&mut TextPainter>) -> Rect {
        let text = std::mem::take(&mut self.text).unwrap();
        let (section, bound) = self.measure_inner(&text, &mut painter);
        let rect = self.bounds(bound);
        if self.scale == Matrix::identity() && !self.ui.rect_visible(rect.feather(0.008)) {
            self.text = Some(text);
            return rect;
        }
        let vp = get_viewport();
        let s = vp.2 as f32 / 2.;
        if let Some(painter) = &mut painter {
            painter.brush.queue(section);
        } else {
            self.ui.text_painter.brush.queue(section);
        }
        self.ui
            .with((Matrix::new_scaling(1. / s) * self.scale).append_translation(&Vector::new(rect.x - bound.0 / s, rect.y - bound.1 / s)), |ui| {
                /* ui.apply(|ui| {
                    let tr = Matrix::identity();
                    if let Some(painter) = painter {
                        painter.submit(tr, ui.alpha);
                    } else {
                        ui.text_painter.submit(tr, ui.alpha);
                    }
                }); */
                if let Some(painter) = painter {
                    painter.submit(ui.transform, ui.alpha);
                } else {
                    ui.text_painter.submit(ui.transform, ui.alpha);
                }
            });
        self.text = Some(text);
        rect
    }

    pub fn draw_using(&mut self, font: &'static LocalKey<RefCell<Option<TextPainter>>>) -> Rect {
        font.with(|it| self.draw_with_font(it.borrow_mut().as_mut()))
    }

    #[inline]
    pub fn draw(&mut self) -> Rect {
        self.draw_with_font(None)
    }
}

static TEXTURE_DIM: Lazy<u32> = Lazy::new(|| unsafe {
    use miniquad::gl::*;
    let mut size = 0;
    glGetIntegerv(GL_MAX_TEXTURE_SIZE, &mut size);
    (size as u32).min(2048)
});

#[derive(Clone)]
struct MyVertex {
    pos: (f32, f32),
    uv: (f32, f32),
    color: Color,
}
impl MyVertex {
    pub fn new(x: f32, y: f32, u: f32, v: f32, color: Color) -> Self {
        Self {
            pos: (x, y),
            uv: (u, v),
            color,
        }
    }
}

pub struct TextPainter {
    primary_scale: f32,
    ink_cache: InkCache,
    brush: GlyphBrush<[MyVertex; 4]>,
    cache_texture: Texture2D,
    data_buffer: Vec<u8>,
    vertices_buffer: Vec<MyVertex>,
    draw_vertices: Vec<Vertex>,
    draw_indices: Vec<u16>,
}

impl TextPainter {
    pub fn new(font: FontArc, fallback: Option<FontArc>) -> Self {
        let mut fonts = vec![font];
        if let Some(fallback) = fallback {
            fonts.push(fallback);
        }
        MULTILINGUAL_FALLBACK.with(|font| fonts.extend(font.borrow().iter().cloned()));
        let mut brush = GlyphBrushBuilder::using_fonts(fonts).build();
        let dim = *TEXTURE_DIM;
        brush.resize_texture(dim, dim);
        // TODO optimize
        let cache_texture = Self::new_cache_texture(brush.texture_dimensions());
        Self {
            primary_scale: 1.,
            ink_cache: InkCache::default(),
            brush,
            cache_texture,
            data_buffer: Vec::new(),
            vertices_buffer: Vec::new(),
            draw_vertices: Vec::new(),
            draw_indices: Vec::new(),
        }
    }

    /// Match visible glyph height, rather than treating stroke weight as size.
    /// Use common glyphs actually present in both fonts; bound extreme metrics.
    pub fn normalize_to(&mut self, reference: &FontArc) {
        let font = &self.brush.fonts()[0];
        let mut ratios = Vec::new();
        for c in ['国', '中', 'H', 'M', '0'] {
            let (a, b) = (font.glyph_id(c), reference.glyph_id(c));
            if a.0 == 0 || b.0 == 0 {
                continue;
            }
            if let (Some(a), Some(b)) = (font.outline_glyph(a.with_scale(1000.)), reference.outline_glyph(b.with_scale(1000.))) {
                let (a, b) = (a.px_bounds().height(), b.px_bounds().height());
                if a > 0. && b > 0. {
                    ratios.push(b / a);
                }
            }
        }
        ratios.sort_by(f32::total_cmp);
        self.primary_scale = ratios.get(ratios.len() / 2).copied().unwrap_or(1.).clamp(0.5, 2.);
    }

    fn new_cache_texture(dim: (u32, u32)) -> Texture2D {
        debug!("creating cache texture: {}x{}", dim.0, dim.1);
        Texture2D::from_miniquad_texture(Texture::new_render_texture(
            unsafe { get_internal_gl() }.quad_context,
            TextureParams {
                width: dim.0,
                height: dim.1,
                filter: FilterMode::Linear,
                format: miniquad::TextureFormat::RGBA8,
                wrap: miniquad::TextureWrap::Clamp,
            },
        ))
    }

    pub fn line_gap(&self, scale: f32) -> f32 {
        self.brush.fonts()[0].as_scaled(scale).line_gap()
    }

    fn submit(&mut self, tr: Matrix, alpha: f32) {
        let mut flushed = false;
        loop {
            match self.brush.process_queued(
                |rect, tex_data| unsafe {
                    if !flushed {
                        get_internal_gl().flush();
                        flushed = true;
                    }
                    self.data_buffer.clear();
                    self.data_buffer.reserve(tex_data.len() * 4);
                    for alpha in tex_data {
                        self.data_buffer.extend_from_slice(&[255, 255, 255, *alpha]);
                    }
                    self.cache_texture.raw_miniquad_texture_handle().update_texture_part(
                        get_internal_gl().quad_context,
                        rect.min[0] as _,
                        rect.min[1] as _,
                        rect.width() as _,
                        rect.height() as _,
                        &self.data_buffer,
                    );
                },
                |vertex| {
                    let pos = &vertex.pixel_coords;
                    let uv = &vertex.tex_coords;
                    let color: Color = vertex.extra.color.into();
                    [
                        MyVertex::new(pos.min.x, pos.min.y, uv.min.x, uv.min.y, color),
                        MyVertex::new(pos.max.x, pos.min.y, uv.max.x, uv.min.y, color),
                        MyVertex::new(pos.min.x, pos.max.y, uv.min.x, uv.max.y, color),
                        MyVertex::new(pos.max.x, pos.max.y, uv.max.x, uv.max.y, color),
                    ]
                },
            ) {
                Err(BrushError::TextureTooSmall { suggested }) => {
                    if !flushed {
                        unsafe { get_internal_gl() }.flush();
                        flushed = true;
                    }
                    let new_texture = Self::new_cache_texture(suggested);
                    self.cache_texture.delete();
                    self.cache_texture = new_texture;
                    self.brush.resize_texture(suggested.0, suggested.1);
                }
                Ok(BrushAction::Draw(vertices)) => {
                    self.vertices_buffer.clear();
                    self.vertices_buffer.extend(vertices.into_iter().flatten());
                    self.redraw(tr, alpha);
                    break;
                }
                Ok(BrushAction::ReDraw) => {
                    self.redraw(tr, alpha);
                    break;
                }
            }
        }
    }

    fn redraw(&mut self, tr: Matrix, alpha: f32) {
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(Some(self.cache_texture));
        // One geometry submission per text batch, rather than per glyph.
        // 512 glyphs stay within macroquad's default draw-call capacities.
        for vertices in self.vertices_buffer.chunks(512 * 4) {
            self.draw_vertices.clear();
            self.draw_indices.clear();
            for quad in vertices.chunks_exact(4) {
                let start = self.draw_vertices.len() as u16;
                for vertex in quad {
                    let pos = tr.transform_point(&Point::new(vertex.pos.0, vertex.pos.1));
                    // ReDraw reuses glyph vertices across UI scopes and frames.
                    // Apply the current fade here rather than baking it into the cache.
                    let mut color = vertex.color;
                    color.a *= alpha;
                    self.draw_vertices.push(Vertex::new(pos.x, pos.y, 0., vertex.uv.0, vertex.uv.1, color));
                }
                self.draw_indices.extend([start, start + 2, start + 3, start, start + 1, start + 3]);
            }
            gl.geometry(&self.draw_vertices, &self.draw_indices);
        }
    }
}

impl Drop for TextPainter {
    fn drop(&mut self) {
        crate::ext::queue_texture_deletion(self.cache_texture);
    }
}
