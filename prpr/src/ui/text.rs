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

use super::font_atlas_pool;
use std::sync::atomic::Ordering;

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

/// Edge length used when a painter allocates its atlas.
///
/// The glyph cache grows on demand, so on Linux desktop (three long-lived
/// painters in the UI) starting at 1024² leaves 12 MiB per painter free until
/// the cache is actually full. Glyph rasterization, layout, UVs and filtering
/// do not depend on the atlas size.
fn initial_texture_dim() -> u32 {
    #[cfg(target_os = "linux")]
    {
        (*TEXTURE_DIM).min(1024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        *TEXTURE_DIM
    }
}

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
    cache_context: usize,
    atlas: Option<Atlas>,
    atlas_blocked: bool,
    data_buffer: Vec<u8>,
    vertices_buffer: Vec<MyVertex>,
    draw_vertices: Vec<Vertex>,
    draw_indices: Vec<u16>,
    #[cfg(target_os = "linux")]
    trim_generation: u64,
}

/// Mirrored glyph-atlas state for the rotating pool. `shadow` is the
/// authoritative RGBA image the pool patches from; `writes` are this frame's
/// dirty rectangles.
struct Atlas {
    pool: font_atlas_pool::Pool,
    shadow: Vec<u8>,
    writes: Vec<[u32; 4]>,
    dirty: bool,
    enabled: bool,
}

fn context_id() -> usize {
    unsafe { get_internal_gl() }.quad_context as *mut miniquad::Context as usize
}

impl Atlas {
    /// Patch the mirror with the brush's alpha-only glyph data, using the same
    /// white-RGB expansion the single-texture path uploads.
    fn write(&mut self, width: u32, rect: (u32, u32, u32, u32), alphas: &[u8]) -> bool {
        let (x, y, w, h) = rect;
        if alphas.len() != (w as usize) * (h as usize) {
            return false;
        }
        for row in 0..h as usize {
            let dst_start = (((y as usize + row) * width as usize) + x as usize) * 4;
            let dst_end = dst_start + (w as usize) * 4;
            if dst_end > self.shadow.len() {
                return false;
            }
            let src_start = row * w as usize;
            for (dst, alpha) in self.shadow[dst_start..dst_end]
                .chunks_exact_mut(4)
                .zip(&alphas[src_start..src_start + w as usize])
            {
                dst.copy_from_slice(&[255, 255, 255, *alpha]);
            }
        }
        true
    }
}

impl TextPainter {
    pub fn new(font: FontArc, fallback: Option<FontArc>) -> Self {
        let mut fonts = vec![font];
        if let Some(fallback) = fallback {
            fonts.push(fallback);
        }
        MULTILINGUAL_FALLBACK.with(|font| fonts.extend(font.borrow().iter().cloned()));
        let mut brush = GlyphBrushBuilder::using_fonts(fonts).build();
        let dim = initial_texture_dim();
        brush.resize_texture(dim, dim);
        // TODO optimize
        let cache_texture = Self::new_cache_texture(brush.texture_dimensions());
        let mut painter = Self {
            primary_scale: 1.,
            ink_cache: InkCache::default(),
            brush,
            cache_texture,
            cache_context: context_id(),
            atlas: None,
            atlas_blocked: false,
            data_buffer: Vec::new(),
            vertices_buffer: Vec::new(),
            draw_vertices: Vec::new(),
            draw_indices: Vec::new(),
            #[cfg(target_os = "linux")]
            trim_generation: 0,
        };
        painter.ensure_atlas();
        painter
    }

    /// Keep the runtime switch and the pool state in step. Enabling re-uploads
    /// the authoritative mirror into fresh versions, so glyphs cached by the
    /// brush are never lost; disabling hands the current version back as an
    /// ordinary mutable texture.
    fn ensure_atlas(&mut self) {
        let wanted = super::FONT_ATLAS_POOL.load(Ordering::Relaxed);
        let active = self.atlas.as_ref().is_some_and(|atlas| atlas.enabled);
        if wanted == active {
            return;
        }
        if wanted {
            if !self.atlas_blocked {
                self.enable_atlas();
            }
        } else {
            self.disable_atlas();
        }
    }

    fn enable_atlas(&mut self) {
        // The fence backend is Android-only; other targets never probe GL or
        // allocate the mirror.
        #[cfg(not(target_os = "android"))]
        {
            self.atlas_blocked = true;
            return;
        }
        let (width, height) = self.brush.texture_dimensions();
        let Some(bytes) = (width as usize).checked_mul(height as usize).and_then(|size| size.checked_mul(4)) else {
            self.atlas_blocked = true;
            return;
        };
        let mut atlas = self.atlas.take().unwrap_or_else(|| Atlas {
            pool: font_atlas_pool::Pool::new(),
            shadow: Vec::new(),
            writes: Vec::new(),
            dirty: false,
            enabled: false,
        });
        // Desktop and other unsupported targets stop here, before allocating
        // the 16 MiB mirror for a pool that can never be used.
        if !atlas.pool.supported() {
            self.atlas = None;
            self.atlas_blocked = true;
            return;
        }
        atlas.shadow.resize(bytes, 0);
        match atlas.pool.warm(width, height, &atlas.shadow) {
            Some(texture) => {
                if texture != self.cache_texture && !atlas.pool.owns(self.cache_texture) && self.cache_context == context_id() {
                    unsafe { get_internal_gl() }.flush();
                    unsafe { get_internal_gl() }.quad_context.clear_resource_bindings();
                    self.cache_texture.delete();
                }
                self.cache_texture = texture;
                self.cache_context = context_id();
                atlas.enabled = true;
                self.atlas = Some(atlas);
            }
            None => {
                self.atlas = None;
                self.atlas_blocked = true;
            }
        }
    }

    fn disable_atlas(&mut self) {
        let Some(atlas) = &mut self.atlas else { return };
        if !atlas.enabled {
            return;
        }
        let (width, height) = self.brush.texture_dimensions();
        atlas.pool.release_to_caller(self.cache_texture, width, height, &atlas.shadow);
        atlas.enabled = false;
        atlas.dirty = false;
        atlas.writes.clear();
    }

    /// Publish this frame's glyph changes before the vertices sampling them.
    fn publish_atlas(&mut self) {
        let (width, height) = self.brush.texture_dimensions();
        let Some(atlas) = self.atlas.as_mut() else { return };
        if !atlas.enabled || !atlas.dirty {
            return;
        }
        let result = atlas.pool.publish(self.cache_texture, width, height, &atlas.shadow, &atlas.writes);
        atlas.writes.clear();
        atlas.dirty = false;
        if result.texture != self.cache_texture && !atlas.pool.owns(self.cache_texture) && self.cache_context == context_id() {
            unsafe { get_internal_gl() }.flush();
            unsafe { get_internal_gl() }.quad_context.clear_resource_bindings();
            self.cache_texture.delete();
        }
        self.cache_texture = result.texture;
        self.cache_context = context_id();
        if !result.valid || !result.pooled {
            // The pool restored the single-texture contract; never re-enable
            // from a state whose mirror may disagree with the uploaded image.
            self.atlas = None;
            self.atlas_blocked = true;
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

    /// Release a grown glyph atlas under critical system-memory pressure.
    /// The cache is rebuilt from the same fonts on demand, so this trades a
    /// one-off rasterization burst for memory, not image quality.
    #[cfg(target_os = "linux")]
    fn trim_atlas(&mut self) {
        let dim = initial_texture_dim();
        let (width, height) = self.brush.texture_dimensions();
        if width <= dim && height <= dim {
            return;
        }
        unsafe { get_internal_gl() }.flush();
        let pooled = self.atlas.as_ref().is_some_and(|atlas| atlas.pool.owns(self.cache_texture));
        self.atlas = None;
        if !pooled {
            self.cache_texture.delete();
        }
        self.cache_texture = Self::new_cache_texture((dim, dim));
        self.cache_context = context_id();
        self.brush.resize_texture(dim, dim);
        self.atlas_blocked = false;
        self.ensure_atlas();
        tracing::info!("trimmed font atlas {width}x{height} -> {dim}x{dim}");
    }

    pub fn line_gap(&self, scale: f32) -> f32 {
        self.brush.fonts()[0].as_scaled(scale).line_gap()
    }

    fn submit(&mut self, tr: Matrix, alpha: f32) {
        #[cfg(target_os = "linux")]
        {
            let generation = crate::desktop_memory::trim_generation();
            if generation != self.trim_generation {
                self.trim_generation = generation;
                self.trim_atlas();
            }
        }
        self.ensure_atlas();
        let (width, _) = self.brush.texture_dimensions();
        let mut flushed = false;
        #[cfg(target_os = "linux")]
        let mut recycled = false;
        loop {
            let outcome = {
                let atlas = &mut self.atlas;
                let data_buffer = &mut self.data_buffer;
                let cache_texture = self.cache_texture;
                self.brush.process_queued(
                    |rect, tex_data| {
                        let (x0, y0) = (rect.min[0], rect.min[1]);
                        let (x1, y1) = (rect.max[0], rect.max[1]);
                        if let Some(atlas) = atlas.as_mut().filter(|atlas| atlas.enabled) {
                            if atlas.write(width, (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)), tex_data) {
                                let write: [u32; 4] = [x0, y0, x1, y1];
                                if atlas.writes.last() != Some(&write) {
                                    atlas.writes.push(write);
                                }
                                atlas.dirty = true;
                            }
                            return;
                        }
                        if !flushed {
                            unsafe { get_internal_gl() }.flush();
                            flushed = true;
                        }
                        data_buffer.clear();
                        data_buffer.reserve(tex_data.len() * 4);
                        for alpha in tex_data {
                            data_buffer.extend_from_slice(&[255, 255, 255, *alpha]);
                        }
                        cache_texture.raw_miniquad_texture_handle().update_texture_part(
                            unsafe { get_internal_gl() }.quad_context,
                            rect.min[0] as _,
                            rect.min[1] as _,
                            rect.width() as _,
                            rect.height() as _,
                            data_buffer,
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
                )
            };
            match outcome {
                Err(BrushError::TextureTooSmall { suggested }) => {
                    if !flushed {
                        unsafe { get_internal_gl() }.flush();
                        flushed = true;
                    }
                    #[cfg(target_os = "linux")]
                    if suggested.0 > *TEXTURE_DIM || suggested.1 > *TEXTURE_DIM {
                        // The atlas already has the maximum edge. Doubling it
                        // again would allocate 4096² (64 MiB) and keep growing;
                        // recycle the glyph cache instead. The cache is a pure
                        // rasterization cache, so dropping it changes neither
                        // glyph pixels nor layout.
                        if recycled {
                            tracing::warn!("font atlas is full at {}x{}; skipping text this frame", *TEXTURE_DIM, *TEXTURE_DIM);
                            break;
                        }
                        recycled = true;
                        let max = *TEXTURE_DIM;
                        let pooled = self.atlas.as_ref().is_some_and(|atlas| atlas.pool.owns(self.cache_texture));
                        self.atlas = None;
                        if !pooled {
                            self.cache_texture.delete();
                        }
                        self.cache_texture = Self::new_cache_texture((max, max));
                        self.cache_context = context_id();
                        self.brush.resize_texture(max, max);
                        self.atlas_blocked = false;
                        self.ensure_atlas();
                        debug!("font atlas reached {max}x{max}; recycled glyph cache");
                        continue;
                    }
                    // Pool versions die with the old shape. Queue their
                    // deletion by dropping the state before replacing the
                    // texture the painter currently points at.
                    let pooled = self.atlas.as_ref().is_some_and(|atlas| atlas.pool.owns(self.cache_texture));
                    self.atlas = None;
                    if !pooled {
                        self.cache_texture.delete();
                    }
                    self.cache_texture = Self::new_cache_texture(suggested);
                    self.cache_context = context_id();
                    self.brush.resize_texture(suggested.0, suggested.1);
                    // glyph_brush re-rasterizes after a resize, so a zeroed
                    // mirror is the correct starting image.
                    self.atlas_blocked = false;
                    self.ensure_atlas();
                }
                Ok(BrushAction::Draw(vertices)) => {
                    self.vertices_buffer.clear();
                    self.vertices_buffer.extend(vertices.into_iter().flatten());
                    self.publish_atlas();
                    self.redraw(tr, alpha);
                    break;
                }
                Ok(BrushAction::ReDraw) => {
                    self.publish_atlas();
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
        // Pool-owned versions are queued by the pool itself; queueing the
        // current one here as well would delete the same GL name twice.
        let pooled = self.atlas.as_ref().is_some_and(|atlas| atlas.pool.owns(self.cache_texture));
        if !pooled {
            crate::ext::queue_texture_deletion(self.cache_texture);
        }
    }
}
