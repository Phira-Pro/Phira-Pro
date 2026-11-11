//! Ordered ordinary-line lowering. The cache contains the actual immutable
//! vertex/index streams consumed by GL, with logical source IDs (never GL IDs).
//! Model/colour palettes remove per-line state changes without sorting alpha.
//! GLSL evaluation remains a low-quality precision candidate, not a lossless claim.
use crate::core::{Chart, JudgeLineKind, Resource};
use anyhow::{ensure, Context, Result};
use macroquad::{prelude::*, window::get_internal_gl};
use miniquad::{
    Bindings, Buffer, BufferLayout, BufferType, Pipeline, PipelineParams, Shader, ShaderMeta, UniformBlockLayout, UniformDesc, UniformType,
    VertexAttribute, VertexFormat,
};
use sha2::{Digest, Sha256};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex},
};

/// Runtime ownership of the cache generation that supplied this renderer.
/// This lease is never part of the serialized immutable mesh.
pub struct CacheLease {
    manager: Arc<super::cache::RenderCache>,
    ticket: super::cache::Ticket,
}
impl CacheLease {
    pub fn new(manager: Arc<super::cache::RenderCache>, ticket: super::cache::Ticket) -> Self {
        Self { manager, ticket }
    }
    pub fn is_current(&self) -> bool {
        self.manager.is_current(&self.ticket)
    }
}

const HEADER: usize = 116;
const MAX_LINES: usize = 65_536;
const MAGIC: &[u8; 8] = b"PHRLN001";
fn abi() -> [u8; 32] {
    Sha256::digest(b"phira-line-gpu-v1/ordered/params-xy-wide-thin-slot/rgba8-signed-width").into()
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Param {
    data: [f32; 4],
}
pub struct Mesh {
    source: [u8; 32],
    length: f32,
    palette: usize,
    ids: Vec<usize>,
    vertices: Vec<Param>,
    indices: Vec<u16>,
}
pub fn supported_ids(chart: &Chart) -> Vec<usize> {
    chart
        .order
        .iter()
        .copied()
        .filter(|&id| matches!(chart.lines[id].kind, JudgeLineKind::Normal))
        .collect()
}
pub fn cache_key(source: [u8; 32], length: f32, palette: usize, ids: &[usize]) -> String {
    let mut hash = Sha256::new();
    hash.update(abi());
    hash.update(source);
    hash.update(length.to_bits().to_le_bytes());
    hash.update((palette as u32).to_le_bytes());
    for &id in ids {
        hash.update((id as u32).to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}
// Preserve draw_line's loading-time arithmetic, including normalization rather
// than substituting thickness/2. Both width variants are baked into each vertex.
fn quad(length: f32, slot: usize) -> [Param; 4] {
    let x1 = -length;
    let x2 = length;
    let dx = x2 - x1;
    let y = |thickness: f32| dx / ((dx * dx).sqrt() / (thickness * 0.5));
    let wide = y(0.01);
    let thin = y(0.0076);
    [
        Param {
            data: [x1, wide, thin, slot as f32],
        },
        Param {
            data: [x1, -wide, -thin, slot as f32],
        },
        Param {
            data: [x2, wide, thin, slot as f32],
        },
        Param {
            data: [x2, -wide, -thin, slot as f32],
        },
    ]
}
fn domain(length: f32, palette: usize, ids: &[usize]) -> Result<()> {
    ensure!(length.is_finite() && length > 0. && (length * 2.).is_finite() && (length * 2.).powi(2).is_finite(), "Unsupported line geometry");
    ensure!((1..=32).contains(&palette) && !ids.is_empty() && ids.len() <= MAX_LINES, "Line mesh budget/capacity");
    let mut unique = std::collections::HashSet::new();
    ensure!(ids.iter().all(|&id| id <= u32::MAX as usize && unique.insert(id)), "Duplicate/invalid source line");
    ensure!(quad(length, 0).iter().flat_map(|v| v.data).all(|v| v.is_finite()), "Non-finite lowered vertex");
    Ok(())
}
pub fn compile(source: [u8; 32], length: f32, palette: usize, ids: Vec<usize>) -> Result<Mesh> {
    domain(length, palette, &ids)?;
    let mut vertices = Vec::with_capacity(ids.len() * 4);
    let mut indices = Vec::with_capacity(ids.len() * 6);
    for i in 0..ids.len() {
        let slot = i % palette;
        vertices.extend_from_slice(&quad(length, slot));
        let base = (slot * 4) as u16;
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    Ok(Mesh {
        source,
        length,
        palette,
        ids,
        vertices,
        indices,
    })
}
pub fn encode(mesh: &Mesh) -> Vec<u8> {
    let mut body = Vec::with_capacity(mesh.ids.len() * 80);
    for &id in &mesh.ids {
        body.extend_from_slice(&(id as u32).to_le_bytes());
    }
    for v in &mesh.vertices {
        for x in v.data {
            body.extend_from_slice(&x.to_le_bytes());
        }
    }
    for &i in &mesh.indices {
        body.extend_from_slice(&i.to_le_bytes());
    }
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&abi());
    bytes.extend_from_slice(&mesh.source);
    bytes.extend_from_slice(&(mesh.palette as u32).to_le_bytes());
    bytes.extend_from_slice(&mesh.length.to_le_bytes());
    bytes.extend_from_slice(&(mesh.ids.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(&body));
    bytes.extend_from_slice(&body);
    bytes
}
pub fn decode(bytes: &[u8], source: [u8; 32], length: f32, palette: usize, ids: &[usize]) -> Result<Mesh> {
    domain(length, palette, ids)?;
    ensure!(bytes.len() >= HEADER && &bytes[..8] == MAGIC && bytes[8..40] == abi() && bytes[40..72] == source, "Stale/invalid lowered line cache");
    let u = |o| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    ensure!(u(72) as usize == palette && u(76) == length.to_bits() && u(80) as usize == ids.len(), "Line cache shape/config changed");
    ensure!(bytes.len() == HEADER + ids.len() * 80, "Line cache counts/size mismatch");
    ensure!(bytes[84..116] == Sha256::digest(&bytes[HEADER..])[..], "Line cache checksum mismatch");
    let mut cursor = HEADER;
    for &id in ids {
        ensure!(u(cursor) as usize == id, "Line source order changed");
        cursor += 4;
    }
    let mut vertices = Vec::with_capacity(ids.len() * 4);
    for i in 0..ids.len() {
        for expected in quad(length, i % palette) {
            let data = std::array::from_fn(|_| {
                let v = f32::from_bits(u(cursor));
                cursor += 4;
                v
            });
            ensure!(data.map(f32::to_bits) == expected.data.map(f32::to_bits), "Invalid lowered line geometry/slot");
            vertices.push(Param { data });
        }
    }
    let mut indices = Vec::with_capacity(ids.len() * 6);
    for i in 0..ids.len() {
        let b = ((i % palette) * 4) as u16;
        for expected in [b, b + 1, b + 2, b + 2, b + 1, b + 3] {
            let value = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
            cursor += 2;
            ensure!(value == expected, "Invalid lowered triangle order");
            indices.push(value);
        }
    }
    Ok(Mesh {
        source,
        length,
        palette,
        ids: ids.to_vec(),
        vertices,
        indices,
    })
}

pub fn capability(force: bool, res: &Resource) -> Result<usize> {
    ensure!(!force, "Forced line GPU capability fallback");
    ensure!(res.config.sample_count == 1 && !res.config.chart_debug, "Line GPU approximation requires low quality and non-debug rendering");
    let (mut vectors, mut attributes) = (0, 0);
    unsafe {
        miniquad::gl::glGetIntegerv(0x8dfb, &mut vectors);
        miniquad::gl::glGetIntegerv(0x8869, &mut attributes);
    }
    ensure!(unsafe { miniquad::gl::glGetError() } == 0 && vectors >= 9 && attributes >= 1, "Line vertex capability probe failed");
    Ok(((vectors as usize - 4) / 5).min(32))
}
fn vertex(capacity: usize) -> String {
    format!(
        r#"#version 100
attribute vec4 params;
varying lowp vec4 color;
uniform mat4 Projection;
uniform vec4 PoseA[{capacity}]; uniform vec4 PoseB[{capacity}];
uniform vec4 PoseC[{capacity}]; uniform vec4 PoseD[{capacity}];
uniform vec4 RGBA[{capacity}];
void main() {{
    int i=int(params.w); vec4 c=RGBA[i];
    // Signed alpha encodes the width choice using exact small integer values.
    // Division is identical to the original RGBA8 vertex conversion.
    bool thin=c.a<0.; if(thin) c.a=-c.a-1.;
    mat4 model=mat4(PoseA[i],PoseB[i],PoseC[i],PoseD[i]);
    gl_Position=Projection*model*vec4(params.x,thin ? params.z : params.y,0.,1.);
    color=c/255.;
}}
"#
    )
}
const FRAGMENT: &str = r#"#version 100
varying lowp vec4 color; uniform sampler2D Texture;
void main(){ gl_FragColor=color*texture2D(Texture,vec2(0.,0.)); }
"#;
#[derive(Clone, Copy)]
struct Record {
    page: usize,
    slot: usize,
    start: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    page: usize,
    start: usize,
    count: usize,
}
impl Span {
    fn continues(self, record: Record) -> bool {
        self.page == record.page && self.start + self.count == record.start
    }
}
#[derive(Clone, PartialEq)]
struct Capture {
    projection: [f32; 16],
    pass: Option<miniquad::RenderPass>,
    viewport: Option<(i32, i32, i32, i32)>,
    clip: Option<(i32, i32, i32, i32)>,
}
struct Page {
    buffer: Buffer,
    uniforms: Vec<f32>,
    lines: usize,
}
struct Renderer {
    context_id: usize,
    lease: CacheLease,
    shader: Shader,
    _render_thread: PhantomData<Rc<()>>,
    records: Vec<Option<Record>>,
    order: Vec<usize>,
    cursor: usize,
    frame_valid: bool,
    pages: Vec<Page>,
    indices: Buffer,
    texture: miniquad::Texture,
    pipeline: Pipeline,
    length: u32,
    palette: usize,
    pending: Option<(Span, Capture)>,
    frames: u64,
    calls: u64,
    accepted: u64,
    transparent: u64,
    fallback: u64,
    draws: u64,
    loading_draws: u64,
    uniform_bytes: u64,
    static_bytes: usize,
}
pub(crate) struct State {
    renderer: Option<Renderer>,
    status: serde_json::Value,
}
impl Default for State {
    fn default() -> Self {
        Self {
            renderer: None,
            status: serde_json::json!({"requested":false,"installed":false}),
        }
    }
}
pub fn disable(chart: &Chart) {
    *chart.compiled_lines.borrow_mut() = State::default();
}
pub fn unavailable(chart: &Chart, reason: String) {
    *chart.compiled_lines.borrow_mut() = State {
        renderer: None,
        status: serde_json::json!({"requested":true,"installed":false,"fallback":reason}),
    };
}
pub fn stats(chart: &Chart) -> serde_json::Value {
    let state = chart.compiled_lines.borrow();
    match state.renderer.as_ref() {
        None => state.status.clone(),
        Some(r) => serde_json::json!({"requested":true,"installed":true,"frames":r.frames,"calls":r.calls,"accepted_lines":r.accepted,
        "transparent_lines":r.transparent,"fallback_lines":r.fallback,"queued_draws":r.draws,"uniform_payload_bytes":r.uniform_bytes,
        "loading_draws":r.loading_draws,"gameplay_queued_draws":r.draws-r.loading_draws,
        "static_buffer_bytes":r.static_bytes,"gameplay_buffer_upload_bytes":0,"palette_capacity":r.palette,"pages":r.pages.len(),
        "quality_class":"mild-lossy-pending-pixel-validation","context_id":r.context_id,
        "shader_program":r.shader.gl_internal_id(unsafe {get_internal_gl()}.quad_context),
        "lifecycle":"Chart owns renderer; deferred retirement runs on rendering thread. Managed context identity is not an EGL loss detector."}),
    }
}
pub fn install(mesh: Mesh, chart: &Chart, res: &Resource, lease: CacheLease) -> Result<()> {
    ensure!(res.config.sample_count == 1 && !res.config.chart_debug, "Low quality only");
    ensure!(supported_ids(chart) == mesh.ids && res.info.line_length.to_bits() == mesh.length.to_bits(), "Line source changed");
    ensure!(lease.is_current(), "Compiler work invalidated by clear/disable");
    let ctx = unsafe { get_internal_gl() }.quad_context;
    let shader = Shader::new(
        ctx,
        &vertex(mesh.palette),
        FRAGMENT,
        ShaderMeta {
            images: vec!["Texture".into()],
            uniforms: UniformBlockLayout {
                uniforms: vec![
                    UniformDesc::new("Projection", UniformType::Mat4),
                    UniformDesc::new("PoseA", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("PoseB", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("PoseC", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("PoseD", UniformType::Float4).array(mesh.palette),
                    UniformDesc::new("RGBA", UniformType::Float4).array(mesh.palette),
                ],
            },
        },
    )?;
    // The legacy pipeline allocates locations for its supplied attributes.
    // A driver assigning an incompatible location must fall back, not panic.
    let location = unsafe { miniquad::gl::glGetAttribLocation(shader.gl_internal_id(ctx).unwrap(), b"params\0".as_ptr().cast()) };
    if location != 0 || unsafe { miniquad::gl::glGetError() } != 0 {
        shader.delete(ctx);
        anyhow::bail!("Unsupported line attribute location");
    }
    let pipeline = Pipeline::with_params(
        ctx,
        &[BufferLayout::default()],
        &[VertexAttribute::new("params", VertexFormat::Float4)],
        shader,
        PipelineParams {
            color_blend: Some(miniquad::BlendState::new(
                miniquad::Equation::Add,
                miniquad::BlendFactor::Value(miniquad::BlendValue::SourceAlpha),
                miniquad::BlendFactor::OneMinusValue(miniquad::BlendValue::SourceAlpha),
            )),
            ..Default::default()
        },
    );
    let pages = mesh
        .vertices
        .chunks(mesh.palette * 4)
        .map(|v| Page {
            buffer: Buffer::immutable(ctx, BufferType::VertexBuffer, v),
            uniforms: vec![0.; 16 + mesh.palette * 20],
            lines: v.len() / 4,
        })
        .collect();
    let indices = Buffer::immutable(ctx, BufferType::IndexBuffer, &mesh.indices);
    let texture = miniquad::Texture::from_rgba8(ctx, 1, 1, &[255; 4]);
    let mut records = vec![None; chart.lines.len()];
    for (i, &id) in mesh.ids.iter().enumerate() {
        records[id] = Some(Record {
            page: i / mesh.palette,
            slot: i % mesh.palette,
            start: i * 6,
        });
    }
    let static_bytes = mesh.vertices.len() * std::mem::size_of::<Param>() + mesh.indices.len() * 2;
    let renderer = Renderer {
        context_id: ctx.context_id(),
        lease,
        shader,
        _render_thread: PhantomData,
        records,
        order: chart.order.clone(),
        cursor: 0,
        frame_valid: false,
        pages,
        indices,
        texture,
        pipeline,
        length: mesh.length.to_bits(),
        palette: mesh.palette,
        pending: None,
        frames: 0,
        calls: 0,
        accepted: 0,
        transparent: 0,
        fallback: 0,
        draws: 0,
        loading_draws: 0,
        uniform_bytes: 0,
        static_bytes,
    };
    ensure!(unsafe { miniquad::gl::glGetError() } == 0, "Line resources failed");
    // Clear/disable may happen while loading creates the resources. A rejected
    // renderer uses the same deferred Drop retirement as an installed one.
    ensure!(renderer.lease.is_current(), "Compiler work invalidated by clear/disable");
    chart.compiled_lines.borrow_mut().renderer = Some(renderer);
    Ok(())
}
pub fn preparation(chart: &Chart) -> serde_json::Value {
    chart.compiled_lines.borrow().status.clone()
}
pub(crate) fn record_preparation(chart: &Chart, report: serde_json::Value) {
    chart.compiled_lines.borrow_mut().status = report;
}
impl Renderer {
    fn flush(&mut self) {
        let Some((span, capture)) = self.pending.take() else { return };
        let page = &mut self.pages[span.page];
        page.uniforms[..16].copy_from_slice(&capture.projection);
        let uniforms = Arc::<[f32]>::from(page.uniforms.as_slice());
        let gl = unsafe { get_internal_gl() };
        let old = (gl.quad_gl.get_active_render_pass(), gl.quad_gl.get_viewport(), gl.quad_gl.get_scissor());
        gl.quad_gl.render_pass(capture.pass);
        gl.quad_gl.viewport(capture.viewport);
        gl.quad_gl.scissor(capture.clip);
        // All buffers are immutable; no per-frame orphan can race an older command.
        unsafe {
            gl.quad_gl.resident_geometry(
                self.pipeline,
                Bindings {
                    vertex_buffers: vec![page.buffer],
                    index_buffer: self.indices,
                    images: vec![self.texture],
                },
                uniforms,
                span.start as i32,
                span.count as i32,
            )
        };
        gl.quad_gl.render_pass(old.0);
        gl.quad_gl.viewport(old.1);
        gl.quad_gl.scissor(old.2);
        self.draws += 1;
        self.uniform_bytes += page.uniforms.len() as u64 * 4;
    }
}
/// Loading-only lifecycle probe. It queues the actual compiled VBO/IBO and
/// shader through the ordinary resident queue; the caller may retire this
/// Chart before consuming the queue. The shared 1px target is caller-owned.
pub fn queue_loading_sample(chart: &Chart, pass: miniquad::RenderPass) -> Result<()> {
    let mut state = chart.compiled_lines.borrow_mut();
    let r = state.renderer.as_mut().context("No compiled line renderer")?;
    ensure!(r.lease.is_current(), "Compiled line cache invalidated by clear/disable");
    ensure!(r.context_id == unsafe { get_internal_gl() }.quad_context.context_id(), "Managed context changed");
    r.flush();
    let model = Mat4::IDENTITY.to_cols_array();
    let page = &mut r.pages[0];
    for col in 0..4 {
        let offset = 16 + col * r.palette * 4;
        page.uniforms[offset..offset + 4].copy_from_slice(&model[col * 4..col * 4 + 4]);
    }
    let offset = 16 + 4 * r.palette * 4;
    page.uniforms[offset..offset + 4].copy_from_slice(&[255.; 4]);
    r.pending = Some((
        Span { page: 0, start: 0, count: 6 },
        Capture {
            projection: Mat4::from_scale(vec3(1. / f32::from_bits(r.length), 200., 1.)).to_cols_array(),
            pass: Some(pass),
            viewport: Some((0, 0, 1, 1)),
            clip: None,
        },
    ));
    r.flush();
    r.loading_draws += 1;
    Ok(())
}
/// Real immutable buffer, shader and sampler use before gameplay. Each page
/// and width branch gets its own readback: a later opaque draw must not allow
/// a tile renderer to discard all the earlier pages as covered work.
pub fn warm_loading(chart: &Chart, force_unavailable: bool) -> Result<serde_json::Value> {
    let begun = std::time::Instant::now();
    let native = crate::render_warm::NativeState::capture()?;
    native.client_pixels();
    let mut state = chart.compiled_lines.borrow_mut();
    let r = state.renderer.as_mut().context("No compiled renderer to warm")?;
    ensure!(r.lease.is_current(), "Compiled line cache invalidated by clear/disable");
    ensure!(r.context_id == unsafe { get_internal_gl() }.quad_context.context_id(), "Managed context changed");
    unsafe { get_internal_gl() }.flush();
    let (pass, reused) = crate::render_warm::target(force_unavailable, r.palette)?;
    r.flush();
    let mut readbacks = 0;
    let before_bytes = r.uniform_bytes;
    let page_line_counts: Vec<usize> = r.pages.iter().map(|p| p.lines).collect();
    for page_id in 0..r.pages.len() {
        crate::render_warm::reset(pass, r.palette)?;
        // Disjoint one-pixel columns force every slot to contribute. Fixed Y
        // projection distinguishes thin/wide at the first and last rows.
        for thin in [false, true] {
            let page = &mut r.pages[page_id];
            for slot in 0..page.lines {
                let center = 2. * (slot as f32 + 0.5) / r.palette as f32 - 1.;
                let model = Mat4::from_scale_rotation_translation(
                    vec3(1. / r.palette as f32, 1., 1.),
                    Quat::IDENTITY,
                    vec3(f32::from_bits(r.length) * center, 0., 0.),
                )
                .to_cols_array();
                for col in 0..4 {
                    let offset = 16 + col * r.palette * 4 + slot * 4;
                    page.uniforms[offset..offset + 4].copy_from_slice(&model[col * 4..col * 4 + 4]);
                }
                let offset = 16 + 4 * r.palette * 4 + slot * 4;
                let rgb = if thin { 0. } else { 255. };
                page.uniforms[offset..offset + 4].copy_from_slice(&[rgb, rgb, rgb, if thin { -256. } else { 255. }]);
            }
            let count = page.lines * 6;
            r.pending = Some((
                Span {
                    page: page_id,
                    start: page_id * r.palette * 6,
                    count,
                },
                Capture {
                    projection: Mat4::from_scale(vec3(1. / f32::from_bits(r.length), 200., 1.)).to_cols_array(),
                    pass: Some(pass),
                    viewport: Some((0, 0, r.palette as i32, 8)),
                    clip: None,
                },
            ));
            r.flush();
            r.loading_draws += 1;
            let pixels = crate::render_warm::read(pass, r.palette)?;
            for row in 0..8 {
                for slot in 0..r.palette {
                    let expected = if slot >= page_line_counts[page_id] {
                        [17, 31, 67, 89]
                    } else if !thin || row == 0 || row == 7 {
                        [255; 4]
                    } else {
                        [0, 0, 0, 255]
                    };
                    let offset = (row * r.palette + slot) * 4;
                    ensure!(pixels[offset..offset + 4] == expected, "Loading page {page_id}, slot {slot}, row {row}, thin={thin} sample differs");
                }
            }
            readbacks += 1;
        }
    }
    native.finish()?;
    Ok(serde_json::json!({"requested":true,"completed":true,"pages":r.pages.len(),"draws":readbacks,"readbacks":readbacks,
        "target_reused":reused,"context_id":r.context_id,"nominal_target_bytes":r.palette*8*4,"target_size":[r.palette,8],
        "page_line_counts":page_line_counts,"verified_slots":page_line_counts.iter().sum::<usize>(),"edge_width_verified":true,
        "native_transfer_state_restored":true,"static_buffer_bytes":r.static_bytes,
        "uniform_bytes":r.uniform_bytes-before_bytes,"wall_ms":begun.elapsed().as_secs_f64()*1000.,
        "scope":"Disjoint slot columns and width edges read during loading. Transfer/FBO/viewport restored; managed flush may end a retained pass. Not all native framebuffer/driver variants or EGL loss."}))
}
pub fn begin_chart(chart: &Chart) {
    let mut state = chart.compiled_lines.borrow_mut();
    // Check once at the chart boundary, before flushing any pending span.
    // Removing the owner queues retirement; Drop itself never calls GL.
    if state.renderer.as_ref().is_some_and(|r| !r.lease.is_current()) {
        state.renderer = None;
        state.status = serde_json::json!({"requested":true,"installed":false,
            "fallback":"Compiled line cache invalidated by clear/disable; reload the chart to prepare new resources."});
    }
    let current = unsafe { get_internal_gl() }.quad_context.context_id();
    if state.renderer.as_ref().is_some_and(|r| r.context_id != current) {
        state.renderer = None;
        state.status =
            serde_json::json!({"requested":true,"installed":false,"fallback":"Managed context changed; reload the chart to prepare new resources."});
    }
    if let Some(r) = state.renderer.as_mut() {
        r.flush();
        r.cursor = 0;
        r.frame_valid = true;
        r.frames += 1;
    }
}
pub fn before_line(chart: &Chart, id: usize, kind: &JudgeLineKind, res: &Resource) {
    let mut state = chart.compiled_lines.borrow_mut();
    if let Some(r) = state.renderer.as_mut() {
        if r.order.get(r.cursor) != Some(&id) {
            r.frame_valid = false;
        }
        r.cursor += 1;
        if !r.frame_valid
            || res.config.chart_debug
            || res.config.sample_count != 1
            || !matches!(kind, JudgeLineKind::Normal)
            || r.records.get(id).and_then(|v| *v).is_none()
        {
            r.flush();
        }
    }
}
pub fn end_chart(chart: &Chart) {
    let mut state = chart.compiled_lines.borrow_mut();
    if let Some(r) = state.renderer.as_mut() {
        r.flush();
        r.frame_valid = false;
    }
}
pub fn try_render(chart: &Chart, id: usize, length: f32, thin: bool, color: Color, res: &Resource) -> bool {
    let mut state = chart.compiled_lines.borrow_mut();
    let Some(r) = state.renderer.as_mut() else { return false };
    r.calls += 1;
    let gl = unsafe { get_internal_gl() };
    let model = gl.quad_gl.get_model_matrix().to_cols_array();
    let capture = Capture {
        projection: gl.quad_gl.get_projection_matrix().to_cols_array(),
        pass: gl.quad_gl.get_active_render_pass(),
        viewport: gl.quad_gl.get_viewport(),
        clip: gl.quad_gl.get_scissor(),
    };
    let record = r.records.get(id).and_then(|v| *v);
    if !r.frame_valid
        || record.is_none()
        || length.to_bits() != r.length
        || res.config.sample_count != 1
        || res.config.chart_debug
        || !gl.quad_gl.is_using_default_material()
        || gl.quad_gl.is_geometry_capture_enabled()
        || gl.quad_gl.is_depth_test_enabled()
        || !model.iter().chain(capture.projection.iter()).all(|v| v.is_finite())
        || ![color.r, color.g, color.b, color.a].iter().all(|v| v.is_finite())
    {
        r.flush();
        r.fallback += 1;
        return false;
    }
    // Match draw_line's logical state as well as its geometry. Later source
    // primitives must observe the same texture/mode even while this line's
    // custom draw is waiting in the ordered batch.
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    let record = record.unwrap();
    if r.pending.as_ref().is_some_and(|(span, key)| !span.continues(record) || *key != capture) {
        r.flush();
    }
    let rgba = [color.r, color.g, color.b, color.a].map(|v| (v * 255.) as u8 as f32);
    // Width is packed into the sign of integer alpha; all 0..255 values and
    // their signed encodings are exactly representable. No visual quantization
    // is added beyond the original Vertex::new RGBA8 conversion.
    if rgba[3] == 0. {
        r.transparent += 1;
    }
    let page = &mut r.pages[record.page];
    for col in 0..4 {
        let offset = 16 + col * r.palette * 4 + record.slot * 4;
        page.uniforms[offset..offset + 4].copy_from_slice(&model[col * 4..col * 4 + 4]);
    }
    let offset = 16 + 4 * r.palette * 4 + record.slot * 4;
    page.uniforms[offset..offset + 4].copy_from_slice(&[rgba[0], rgba[1], rgba[2], if thin { -rgba[3] - 1. } else { rgba[3] }]);
    if let Some((span, _)) = &mut r.pending {
        span.count += 6;
    } else {
        r.pending = Some((
            Span {
                page: record.page,
                start: record.start,
                count: 6,
            },
            capture,
        ));
    }
    r.accepted += 1;
    true
}
struct Retired {
    context_id: usize,
    pages: Vec<Page>,
    indices: Buffer,
    texture: miniquad::Texture,
    pipeline: Pipeline,
    shader: Shader,
}
static RETIRED: Mutex<Vec<Retired>> = Mutex::new(Vec::new());
/// Called by the existing main-thread texture retirement hook. Drop itself
/// never calls GL, including when an unsafe loading wrapper is cancelled.
pub fn flush_pending_deletions() {
    let retired = std::mem::take(&mut *RETIRED.lock().unwrap());
    if retired.is_empty() {
        return;
    }
    let mut gl = unsafe { get_internal_gl() };
    gl.flush();
    let context = gl.quad_context;
    for resource in retired {
        // A new managed context must not delete unrelated reused native names.
        // Native EGL loss within an unchanged managed context needs a separate
        // surface lifecycle notification; this check does not claim to detect it.
        if resource.context_id != context.context_id() {
            continue;
        }
        context.clear_resource_bindings();
        for page in resource.pages {
            page.buffer.delete();
        }
        resource.indices.delete();
        resource.texture.delete();
        let pipeline = resource.pipeline.delete(context);
        let shader = resource.shader.delete(context);
        debug_assert!(pipeline && shader, "Compiled-line resources were not exclusively owned");
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        RETIRED.lock().unwrap().push(Retired {
            context_id: self.context_id,
            pages: std::mem::take(&mut self.pages),
            indices: self.indices,
            texture: self.texture,
            pipeline: self.pipeline,
            shader: self.shader,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_lease_rejects_cleared_and_disabled_generations_without_gl() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let manager = Arc::new(super::super::cache::RenderCache::new(root.clone(), true).unwrap());
        let key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let before_clear = CacheLease::new(manager.clone(), manager.begin(key).unwrap().unwrap());
        assert!(before_clear.is_current());
        assert!(!root.exists());

        manager.clear_all().unwrap();
        assert!(!before_clear.is_current());
        let after_clear = CacheLease::new(manager.clone(), manager.begin(key).unwrap().unwrap());
        assert!(after_clear.is_current());

        manager.set_enabled(false);
        assert!(!before_clear.is_current());
        assert!(!after_clear.is_current());
        assert!(manager.begin(key).unwrap().is_none());
        manager.set_enabled(true);
        assert!(!before_clear.is_current());
        assert!(!after_clear.is_current());
        let after_reenable = CacheLease::new(manager.clone(), manager.begin(key).unwrap().unwrap());
        assert!(after_reenable.is_current());
        assert!(!root.exists());
    }
    #[test]
    fn lowered_blob_preserves_widths_order_and_pages_and_rejects_stale_data() {
        let mesh = compile([3; 32], 2.7, 2, vec![8, 2, 7, 1, 6]).unwrap();
        let blob = encode(&mesh);
        let decoded = decode(&blob, [3; 32], 2.7, 2, &mesh.ids).unwrap();
        assert_eq!(mesh.indices, decoded.indices);
        assert_eq!(mesh.ids, decoded.ids);
        for (a, b) in mesh.vertices.iter().zip(&decoded.vertices) {
            assert_eq!(a.data.map(f32::to_bits), b.data.map(f32::to_bits));
        }
        for length in [0., f32::NAN, f32::INFINITY, f32::MAX] {
            assert!(compile([3; 32], length, 2, vec![1]).is_err());
        }
        assert!(decode(&blob, [4; 32], 2.7, 2, &mesh.ids).is_err());
        assert!(decode(&blob, [3; 32], 2.71, 2, &mesh.ids).is_err());
        assert!(decode(&blob, [3; 32], 2.7, 3, &mesh.ids).is_err());
        assert!(decode(&blob, [3; 32], 2.7, 2, &[2, 8, 7, 1, 6]).is_err());
        for cut in [0, 7, 115, blob.len() - 1] {
            assert!(decode(&blob[..cut], [3; 32], 2.7, 2, &mesh.ids).is_err());
        }
        let mut corrupt = blob.clone();
        corrupt[HEADER + 9] ^= 1;
        assert!(decode(&corrupt, [3; 32], 2.7, 2, &mesh.ids).is_err());
        let mut hostile = blob.clone();
        hostile[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&hostile, [3; 32], 2.7, 2, &mesh.ids).is_err());
        // Even a recomputed checksum must not permit a changed triangle stream.
        let mut forged = blob;
        let end = forged.len();
        forged[end - 2..].copy_from_slice(&u16::MAX.to_le_bytes());
        let checksum = Sha256::digest(&forged[HEADER..]);
        forged[84..116].copy_from_slice(&checksum);
        assert!(decode(&forged, [3; 32], 2.7, 2, &mesh.ids).is_err());
    }
    #[test]
    fn width_encoding_covers_every_original_rgba8_alpha() {
        for alpha in 0..=255 {
            for thin in [false, true] {
                let alpha = alpha as f32;
                let encoded = if thin { -alpha - 1. } else { alpha };
                assert_eq!(encoded < 0., thin);
                let decoded = if encoded < 0. { -encoded - 1. } else { encoded };
                assert_eq!(decoded.to_bits(), alpha.to_bits());
            }
        }
        // Independent draw_line oracle uses generic endpoints and both widths.
        for len in [0.1, 1., 2.7, 999.] {
            let vertices = quad(len, 7);
            for (variant, width) in [(1, 0.01), (2, 0.0076)] {
                let (x1, x2, y1, y2) = (-len, len, 0f32, 0f32);
                let nx = -(y2 - y1);
                let ny = x2 - x1;
                let norm = (nx * nx + ny * ny).sqrt() / (width * 0.5);
                let tx = nx / norm;
                let ty = ny / norm;
                let positions = [[x1 + tx, y1 + ty], [x1 - tx, y1 - ty], [x2 + tx, y2 + ty], [x2 - tx, y2 - ty]];
                for (v, p) in vertices.iter().zip(positions) {
                    assert_eq!(v.data[0].to_bits(), p[0].to_bits());
                    assert_eq!(v.data[variant].to_bits(), p[1].to_bits());
                }
            }
        }
    }
    #[test]
    fn batching_merges_only_adjacent_source_triangles_without_crossing_barriers() {
        let mesh = compile([1; 32], 1., 2, vec![9, 3, 8, 2, 7]).unwrap();
        // Unsupported lines and CPU fallback deliberately break a span.
        let events = [Some(0), Some(1), None, Some(2), None, Some(4)];
        let mut spans = Vec::new();
        let mut pending: Option<Span> = None;
        let mut oracle = Vec::new();
        for event in events {
            let Some(i) = event else {
                if let Some(s) = pending.take() {
                    spans.push(s);
                }
                continue;
            };
            let record = Record {
                page: i / 2,
                slot: i % 2,
                start: i * 6,
            };
            oracle.extend_from_slice(&mesh.indices[i * 6..i * 6 + 6]);
            if pending.is_some_and(|s| !s.continues(record)) {
                spans.push(pending.take().unwrap());
            }
            if let Some(s) = &mut pending {
                s.count += 6;
            } else {
                pending = Some(Span {
                    page: record.page,
                    start: record.start,
                    count: 6,
                });
            }
        }
        if let Some(s) = pending {
            spans.push(s);
        }
        let actual = spans
            .iter()
            .flat_map(|s| mesh.indices[s.start..s.start + s.count].iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(actual, oracle);
        assert_eq!(spans.iter().map(|s| s.count).collect::<Vec<_>>(), [12, 6, 6]);
    }
}
