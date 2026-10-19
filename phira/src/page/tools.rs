//! Display preferences and local diagnostics share controls, not gameplay state.
use super::{Page, SharedState};
use crate::{get_data, get_data_mut, save_data};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    ui::{DRectButton, Scroll, Ui},
};
use std::borrow::Cow;
#[cfg(all(test, record, target_os = "windows"))]
pub(crate) mod qa;
pub struct ToolsPage {
    diagnostic: bool,
    buttons: Vec<DRectButton>,
    sample: prpr::diagnostics::Snapshot,
    scroll: Scroll,
    last_sample: f32,
    persistent: bool,
    frame_mean: f32,
    frame_p95: f32,
}
impl ToolsPage {
    pub fn new(diagnostic: bool) -> Self {
        Self {
            diagnostic,
            buttons: (0..9).map(|_| DRectButton::new()).collect(),
            sample: Default::default(),
            scroll: Scroll::new(),
            last_sample: f32::NEG_INFINITY,
            persistent: prpr::diagnostics::persistent(),
            frame_mean: 0.,
            frame_p95: 0.,
        }
    }
}
impl Page for ToolsPage {
    fn label(&self) -> Cow<'static, str> {
        if self.diagnostic { "问题诊断" } else { "画质设置" }.into()
    }
    fn enter(&mut self, _: &mut SharedState) -> Result<()> {
        self.last_sample = f32::NEG_INFINITY;
        Ok(())
    }
    fn exit(&mut self) -> Result<()> {
        if self.diagnostic && !prpr::diagnostics::persistent() {
            prpr::diagnostics::set_running(false);
        }
        Ok(())
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if self.diagnostic {
            if s.t - self.last_sample >= 0.2 {
                self.sample = prpr::diagnostics::preview();
                let mut frames = self.sample.frames_ms.clone();
                frames.sort_by(f32::total_cmp);
                self.frame_mean = frames.iter().sum::<f32>() / frames.len().max(1) as f32;
                self.frame_p95 = frames.get(frames.len().saturating_sub(1) * 95 / 100).copied().unwrap_or(0.);
                self.last_sample = s.t;
            }
            crate::replay::poll_export();
        }
        Ok(())
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if !self.diagnostic && self.scroll.touch(touch, s.t) {
            for b in &mut self.buttons {
                b.cancel(s.t);
            }
            return Ok(true);
        }
        for (i, b) in self.buttons.iter_mut().enumerate() {
            if !b.touch(touch, s.t) {
                continue;
            }
            if self.diagnostic {
                if i == 0 {
                    prpr::diagnostics::clear();
                } else if i == 2 {
                    self.persistent = !self.persistent;
                    prpr::diagnostics::set_persistent(self.persistent);
                } else if i == 1 {
                    let sample = prpr::diagnostics::snapshot();
                    let c = &get_data().config;
                    #[allow(unused_mut)] // Android adds the cached platform display state below.
                    let mut data = serde_json::json!({"version":crate::PRO_VERSION,"build":env!("PHIRA_BUILD_NUMBER"),"platform":std::env::consts::OS,"viewport":[screen_width(),screen_height()],"sampling":prpr::diagnostics::running(),"requested_refresh_hz":c.preferred_refresh_rate,"maximum_refresh_hz":f32::from_bits(prpr::ui::MAX_DEVICE_REFRESH_RATE.load(std::sync::atomic::Ordering::Relaxed)),"fps_limit":c.desktop_fps_limit,"aspect_ratio":c.aspect_ratio,"sample_count":c.sample_count,"fxaa":c.fxaa,"shader_pre_render":c.shader_pre_render,"noise_glow":c.noise_glow,"noise_refraction":c.noise_refraction,"judge_algorithm":format!("{:?}",c.judge_algorithm),"configured_windows_seconds":c.judge_windows().early,"input":sample});
                    #[cfg(target_os = "android")]
                    {
                        data["android_display"] =
                            crate::android_display_diagnostics().unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));
                    }
                    crate::replay::request_export_bytes(
                        format!("PhiraPro-diagnostics-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")),
                        serde_json::to_vec_pretty(&data)?,
                    );
                } else if i == 3 {
                    prpr::diagnostics::set_running(!prpr::diagnostics::running());
                }
                self.last_sample = f32::NEG_INFINITY;
            } else {
                let c = &mut get_data_mut().config;
                match i {
                    0 => {
                        c.particle = false;
                        c.noise_glow = false;
                        c.noise_refraction = false;
                        c.sample_count = 1;
                        c.fxaa = false;
                    }
                    1 => {
                        c.particle = true;
                        c.noise_glow = true;
                        c.noise_refraction = true;
                        c.sample_count = 1;
                        c.fxaa = false;
                    }
                    2 => {
                        c.particle = true;
                        c.noise_glow = true;
                        c.noise_refraction = true;
                        c.sample_count = 4;
                        c.fxaa = false;
                    }
                    3 => c.particle ^= true,
                    4 => c.noise_glow ^= true,
                    5 => c.noise_refraction ^= true,
                    6 => {
                        c.sample_count = match c.sample_count {
                            1 => 2,
                            2 => 4,
                            _ => 1,
                        }
                    }
                    7 => c.fxaa ^= true,
                    _ => {}
                }
                save_data()?;
            }
            return Ok(true);
        }
        // MainScene owns the header/back button. Diagnostic sampling happens
        // independently; it must not consume touches outside its own controls.
        Ok(false)
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui| {
            let r = ui.content_rect().feather(-0.025);
            ui.fill_path(&r.feather(0.012).rounded(0.008), semi_black(0.35));
            for b in &mut self.buttons {
                b.invalidate();
            }
            if self.diagnostic {
                self.buttons[0].render_text(ui, Rect::new(r.x + 0.32, r.y, 0.25, 0.08), t, "清空采样", 0.36, false);
                self.buttons[1].render_text(ui, Rect::new(r.right() - 0.28, r.y, 0.28, 0.08), t, "导出诊断", 0.36, false);
                self.buttons[3].render_text(
                    ui,
                    Rect::new(r.x, r.y, 0.29, 0.08),
                    t,
                    if prpr::diagnostics::running() { "停止采样" } else { "开始采样" },
                    0.36,
                    prpr::diagnostics::running(),
                );
                self.buttons[2].render_text(
                    ui,
                    Rect::new(r.x + 0.60, r.y, 0.45, 0.08),
                    t,
                    if self.persistent {
                        "跨页面采样：开"
                    } else {
                        "跨页面采样：关"
                    },
                    0.34,
                    self.persistent,
                );
                ui.text(format!(
                    "{} · {}×{} · 主循环间隔 {:.2}ms / P95 {:.2}ms · {}",
                    std::env::consts::OS,
                    screen_width(),
                    screen_height(),
                    self.frame_mean,
                    self.frame_p95,
                    if prpr::diagnostics::running() { "采样中" } else { "已停止" }
                ))
                .pos(r.x, r.y + 0.11)
                .size(0.42)
                .max_width(r.w)
                .draw();
                ui.text(format!("当前 {} 指 · 最多 {} 指 · 系统取消 {} 次", self.sample.active.len(), self.sample.max_fingers, self.sample.cancels))
                    .pos(r.x, r.y + 0.17)
                    .size(0.43)
                    .draw();
                let extra = if let Some(state) = &self.sample.display_state {
                    let recent: Vec<_> = self.sample.display_frames.iter().rev().take(120).collect();
                    let intervals: Vec<_> = recent.iter().filter_map(|f| f.callback_interval_ms).collect();
                    let hz = if intervals.is_empty() {
                        0.
                    } else {
                        1000. * intervals.len() as f64 / intervals.iter().sum::<f64>()
                    };
                    let work = recent.iter().map(|f| f.glk_display_ms).sum::<f64>() / recent.len().max(1) as f64;
                    ui.text(format!(
                        "请求 {} Hz · 显示回调 {:.1} Hz · GLKView.display {:.2}ms · 回显区间 {:.0}/{:.0}/{:.0}",
                        state.requested_hz, hz, work, state.preferred_range[0], state.preferred_range[1], state.preferred_range[2]
                    ))
                    .pos(r.x, r.y + 0.23)
                    .size(0.31)
                    .max_width(r.w)
                    .draw();
                    let thermal = match state.thermal_state {
                        0 => "正常",
                        1 => "轻度",
                        2 => "严重",
                        _ => "临界",
                    };
                    ui.text(format!(
                        "引导式访问 {} · 低电量 {} · 温控 {} · 恢复 {}/2 · 显示链接 #{}",
                        if state.guided_access { "开" } else { "关" },
                        if state.low_power { "开" } else { "关" },
                        thermal,
                        state.recovery_attempts,
                        state.generation
                    ))
                    .pos(r.x, r.y + 0.29)
                    .size(0.31)
                    .max_width(r.w)
                    .draw();
                    0.12
                } else {
                    0.
                };
                ui.text("多指按住、移动和抬起测试。开启跨页面采样后可返回游玩再导出；记录仅保存在本次运行内存。")
                    .pos(r.x, r.y + 0.23 + extra)
                    .size(0.32)
                    .max_width(r.w)
                    .draw();
                let graph = Rect::new(r.x, r.y + 0.30 + extra, r.w, 0.16);
                ui.fill_path(&graph.rounded(0.01), semi_black(0.3));
                let step = graph.w / self.sample.frames_ms.len().max(1) as f32;
                for (i, v) in self.sample.frames_ms.iter().enumerate() {
                    let h = (*v / 50.).clamp(0., 1.) * graph.h;
                    ui.fill_rect(
                        Rect::new(graph.x + i as f32 * step, graph.bottom() - h, step.max(0.001), h),
                        if *v > 16.7 { ORANGE } else { SKYBLUE },
                    );
                }
                for (i, event) in self
                    .sample
                    .events
                    .iter()
                    .rev()
                    .take(((r.h - 0.50 - extra) / 0.045).max(0.) as usize)
                    .enumerate()
                {
                    ui.text(event)
                        .pos(r.x, r.y + 0.49 + extra + i as f32 * 0.045)
                        .size(0.28)
                        .max_width(r.w)
                        .color(semi_white(0.65))
                        .draw();
                }
                for (id, p) in prpr::diagnostics::active_touches() {
                    let x = p[0] / screen_width() * 2. - 1.;
                    let y = (p[1] / screen_height() * 2. - 1.) * ui.top;
                    ui.fill_circle(x, y, 0.035, Color::new(0.3, 0.8, 1., 0.5));
                    ui.text(format!("{id}")).pos(x, y).anchor(0.5, 0.5).no_baseline().size(0.3).draw();
                }
            } else {
                let mut scroll = std::mem::take(&mut self.scroll);
                scroll.size((r.w, r.h));
                ui.scope(|ui| {
                    ui.dx(r.x);
                    ui.dy(r.y);
                    scroll.render(ui, |ui| {
                        let r = Rect::new(0., 0., r.w, r.h);
                        let c = &get_data().config;
                        for (i, label) in ["低耗", "标准", "高质量"].iter().enumerate() {
                            self.buttons[i].render_text(
                                ui,
                                Rect::new(r.x + i as f32 * r.w / 3., r.y, r.w / 3. - 0.012, 0.085),
                                t,
                                *label,
                                0.44,
                                false,
                            );
                        }
                        let labels = [
                            format!("打击粒子：{}", if c.particle { "开" } else { "关" }),
                            format!("噪域辉光：{}", if c.noise_glow { "开" } else { "关" }),
                            format!("噪域折射：{}", if c.noise_refraction { "开" } else { "关" }),
                            format!("抗锯齿采样：{}×", c.sample_count),
                            format!("FXAA：{}", if c.fxaa { "开" } else { "关" }),
                        ];
                        for (i, label) in labels.iter().enumerate() {
                            self.buttons[i + 3].render_text(ui, Rect::new(r.x, r.y + 0.12 + i as f32 * 0.105, r.w, 0.085), t, label, 0.43, false);
                        }
                        ui.text("噪域继续使用原有贴图、颜色与区域边界。画质设置不改变触点屏蔽范围；下次加载谱面生效。")
                            .pos(r.x, r.y + 0.68)
                            .size(0.32)
                            .max_width(r.w)
                            .multiline()
                            .draw();
                        (r.w, 0.82)
                    });
                });
                self.scroll = scroll;
            }
        });
        Ok(())
    }
}
