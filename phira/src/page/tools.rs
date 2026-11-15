//! Local input and frame-time diagnostics.
use super::{Page, SharedState};
use crate::get_data;
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    ui::{DRectButton, Ui},
};
use std::borrow::Cow;
#[cfg(all(test, record, target_os = "windows"))]
pub(crate) mod qa;

pub struct ToolsPage {
    buttons: Vec<DRectButton>,
    sample: prpr::diagnostics::Snapshot,
    last_sample: f32,
    persistent: bool,
    frame_mean: f32,
    frame_p95: f32,
}

impl ToolsPage {
    pub fn new() -> Self {
        Self {
            buttons: (0..4).map(|_| DRectButton::new()).collect(),
            sample: Default::default(),
            last_sample: f32::NEG_INFINITY,
            persistent: prpr::diagnostics::persistent(),
            frame_mean: 0.,
            frame_p95: 0.,
        }
    }
}

impl Page for ToolsPage {
    fn label(&self) -> Cow<'static, str> {
        "问题诊断".into()
    }

    fn enter(&mut self, _: &mut SharedState) -> Result<()> {
        self.last_sample = f32::NEG_INFINITY;
        Ok(())
    }

    fn exit(&mut self) -> Result<()> {
        if !prpr::diagnostics::persistent() {
            prpr::diagnostics::set_running(false);
        }
        Ok(())
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        if s.t - self.last_sample >= 0.2 {
            self.sample = prpr::diagnostics::preview();
            let mut frames = self.sample.frames_ms.clone();
            frames.sort_by(f32::total_cmp);
            self.frame_mean = frames.iter().sum::<f32>() / frames.len().max(1) as f32;
            self.frame_p95 = frames.get(frames.len().saturating_sub(1) * 95 / 100).copied().unwrap_or(0.);
            self.last_sample = s.t;
        }
        crate::replay::poll_export();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        for (i, b) in self.buttons.iter_mut().enumerate() {
            if !b.touch(touch, s.t) {
                continue;
            }
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
                    data["android_display"] = crate::android_display_diagnostics().unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));
                }
                crate::replay::request_export_bytes(
                    format!("PhiraPro-diagnostics-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")),
                    serde_json::to_vec_pretty(&data)?,
                );
            } else if i == 3 {
                prpr::diagnostics::set_running(!prpr::diagnostics::running());
            }
            self.last_sample = f32::NEG_INFINITY;
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
                let work = recent.iter().map(|f| f.callback_work_ms).sum::<f64>() / recent.len().max(1) as f64;
                ui.text(format!(
                    "请求 {} Hz  帧循环 {:.1} Hz  处理 {:.2}ms  偏好 {:.0}/{:.0}/{:.0}",
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
                    "引导式访问 {}  低电量 {}  温控 {}  重试 {}/2  申请 #{}",
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
                ui.fill_rect(Rect::new(graph.x + i as f32 * step, graph.bottom() - h, step.max(0.001), h), if *v > 16.7 { ORANGE } else { SKYBLUE });
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
        });
        Ok(())
    }
}
