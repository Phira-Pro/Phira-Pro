prpr_l10n::tl_file!("record");

// 本地成绩详情页：从「本地记录」排行榜点开任意一条成绩，展示与正常游玩结算界面
// 一致的排版（背景 + 曲绘开屏动画 + 滚动分数 + 判定计数 + 连击条 + 判定分布图），
// 底部三个按钮依次为「返回」「导入 / 管理回放」「播放回放」。

use super::{fs_from_path, replay::ReplayScene};
use crate::{history, replay};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    core::{BOLD_FONT, PGR_FONT},
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt, SafeTexture, ScaleType, BLACK_TEXTURE},
    judge::{icon_index, PlayResult, HIST_BUCKETS, HIST_MAX_MS},
    scene::{show_error, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{clip_sector, DRectButton, Ui},
};
use std::{future::Future, pin::Pin};

pub struct RecordDetailScene {
    record: history::Record,

    rank_icons: [SafeTexture; 8],
    background: SafeTexture,
    illustration: SafeTexture,
    /// 曲绘加载任务：必须在主线程轮询（`LocalTask`），因为 macroquad 纹理只能主线程创建。
    /// 由 `LoadingScene::load` 同时给出清晰曲绘与高斯模糊背景（结算界面同款）。
    visual_task: LocalTask<Result<(SafeTexture, SafeTexture)>>,

    replay_path: Option<std::path::PathBuf>,
    replay_task: Option<Pin<Box<dyn Future<Output = Result<ReplayScene>>>>>,

    exit: bool,
    next: Option<NextScene>,
    btn_back: DRectButton,
    btn_replay: DRectButton,
    btn_open: DRectButton,
    btn_export: DRectButton,
}

impl RecordDetailScene {
    pub fn new(record: history::Record, rank_icons: [SafeTexture; 8]) -> Self {
        let replay_path = {
            let p = record
                .replay_file
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| replay::path_for(&record.key, record.time));
            if p.is_file() {
                Some(p)
            } else {
                None
            }
        };
        let visual_task = Self::make_visual_task(&record);
        Self {
            record,
            rank_icons,
            background: BLACK_TEXTURE.clone(),
            illustration: BLACK_TEXTURE.clone(),
            visual_task,
            replay_path,
            replay_task: None,
            exit: false,
            next: None,
            btn_back: DRectButton::new(),
            btn_replay: DRectButton::new(),
            btn_open: DRectButton::new(),
            btn_export: DRectButton::new(),
        }
    }

    /// 加载曲绘：用与游玩一致的 `LoadingScene::load` 同时得到清晰曲绘与高斯模糊背景。
    /// 该 future 由主线程轮询（`LocalTask`），所以内部创建纹理是安全的。
    fn make_visual_task(record: &history::Record) -> LocalTask<Result<(SafeTexture, SafeTexture)>> {
        let local_path = if let Some(path) = record.key.strip_prefix("local:") {
            path.to_owned()
        } else if let Some(id) = record.key.strip_prefix("id:") {
            id.parse::<i32>().ok().and_then(|id| {
                crate::get_data()
                    .charts
                    .iter()
                    .find(|it| it.info.id == Some(id))
                    .map(|it| it.local_path.clone())
            })?
        } else {
            return None;
        };
        Some(Box::pin(async move {
            let mut fs = fs_from_path(&local_path)?;
            let info = prpr::fs::load_info(fs.as_mut()).await?;
            let (illustration, background, _theme) = LoadingScene::load(fs.as_mut(), &info.illustration).await?;
            Ok((illustration, background))
        }))
    }

    /// 把历史记录转成结算界面同款 `PlayResult`。
    fn play_result(&self) -> PlayResult {
        let r = &self.record;
        let mut hist = [0u32; HIST_BUCKETS];
        for (i, v) in r.hist.iter().enumerate() {
            if i < HIST_BUCKETS {
                hist[i] = *v;
            }
        }
        PlayResult {
            score: r.score,
            accuracy: r.accuracy,
            max_combo: r.max_combo,
            num_of_notes: r.num_of_notes,
            // 历史记录把 Perfect+ 并入 Perfect：counts[0]=Perfect(含 P+)，后面依次 Good/Bad/Miss。
            counts: r.grade_counts.unwrap_or([r.counts[0], r.counts[1], r.counts[2], r.counts[3], 0, 0, 0, 0]),
            grading: r.grading.unwrap_or(prpr::config::JudgeGrading { perfect_plus: false, ..Default::default() }),
            early: 0,
            late: 0,
            std: r.std,
            mean: 0.,
            offsets: Vec::new(),
            hist,
            early_kind: [0; 8],
            late_kind: [0; 8],
        }
    }
}

impl Scene for RecordDetailScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        tm.reset();
        tm.seek_to(-0.2);
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if self.btn_back.touch(touch, t) {
            self.exit = true;
            return Ok(true);
        }
        if self.btn_replay.touch(touch, t) {
            if let Some(path) = self.replay_path.clone() {
                if self.replay_task.is_none() {
                    self.replay_task = Some(Box::pin(async move { ReplayScene::new(path).await }));
                }
            }
            return Ok(true);
        }
        if self.btn_export.touch(touch, t) {
            if let Some(path) = &self.replay_path {
                if let Err(err) = replay::request_export(path) {
                    show_error(err);
                }
            }
            return Ok(true);
        }
        if self.btn_open.touch(touch, t) {
            self.next = Some(NextScene::Overlay(Box::new(crate::page::replays::ReplayManager::new(Some(self.record.key.clone()), true))));
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        replay::poll_export();
        if let Some(task) = &mut self.visual_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.visual_task = None;
                match res {
                    Ok((illustration, background)) => {
                        self.illustration = illustration;
                        self.background = background;
                    }
                    Err(err) => tracing::warn!(?err, "failed to load chart illustration for record detail"),
                }
            }
        }
        if let Some(task) = &mut self.replay_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.replay_task = None;
                match res {
                    Ok(scene) => self.next = Some(NextScene::Overlay(Box::new(scene))),
                    Err(err) => show_error(err.context("打开回放失败")),
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        // —— 结算界面同款：背景 + 曲绘开屏动画 + 排版 ——
        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let top = 1. / asp;
        let t = tm.now() as f32;
        cam.render_target = None;
        set_camera(&cam);
        let sr = ui.screen_rect();
        // 背景：与结算界面一致——高斯模糊曲绘铺满（CropCenter）+ 30% 压暗。
        ui.fill_rect(sr, (*self.background, sr));
        ui.fill_rect(sr, Color::new(0., 0., 0., 0.3));

        if self.record.replay_result_unknown {
            ui.text(&self.record.name).pos(-0.9, -top + 0.15).size(0.8).max_width(1.8).draw();
            ui.text("旧回放未记录原始成绩；可播放录制事件，成绩数据保持未记录。")
                .pos(-0.9, -top + 0.3)
                .size(0.5)
                .max_width(1.8)
                .draw();
            self.btn_back
                .render_text(ui, Rect::new(-0.9, top - 0.16, 0.24, 0.09), t, "返回", 0.45, false);
            self.btn_replay
                .render_text(ui, Rect::new(-0.62, top - 0.16, 0.3, 0.09), t, "播放回放", 0.45, false);
            self.btn_export
                .render_text(ui, Rect::new(-0.29, top - 0.16, 0.3, 0.09), t, "导出回放", 0.45, false);
            if self.replay_task.is_some() {
                ui.full_loading_simple(t);
            }
            return Ok(());
        }

        fn ran(t: f32, l: f32, r: f32) -> f32 {
            ((t - l) / (r - l)).clamp(0., 1.)
        }

        let ct = vec2(-0.55, 1.2);
        let start = vec2(1.25, 0.9) - ct;
        let end = vec2(-0.15, -0.7) - ct;
        let angle_start = start.y.atan2(start.x) * 0.4;
        let angle_end = end.y.atan2(end.x);
        let center_angle = 1.8;

        let p = ran(t, 0.1, 1.8);
        let p = 1. - (1. - p).powi(3);
        let sector_start = p * (angle_end - angle_start - center_angle) + angle_start;
        let project_y = ct.y + (1. - ct.x) * (sector_start + center_angle).sin();
        let pf = ran(t, 2., 2.4);
        let res = self.play_result();

        if project_y < top {
            let c = ui.background();
            let y = -top + 0.12;
            let br = Rect::new(-1., y, 2., 0.34);
            ui.fill_rect(br, (c, (-1., y), Color { a: 0.1, ..c }, (1., y + 0.3)));

            let y = y - 0.07;
            ui.fill_rect(Rect::new(-1., y, 2., 0.07), Color { a: 0.3, ..c });
            let r = ui
                .text(&self.record.name)
                .pos(-0.53 + (1.2 - y) / 1.9 * 0.4, y + 0.012)
                .color(semi_white(0.6))
                .max_width(0.8)
                .size(0.56)
                .draw();
            ui.text(&self.record.level)
                .pos(0.97, r.y)
                .anchor(1., 0.)
                .size(0.56)
                .color(semi_white(0.7))
                .draw();

            let icon = &self.rank_icons[icon_index(res.score, res.max_combo == res.num_of_notes)];
            let p = ran(t, 1.7, 2.4).powi(2);
            let r = Rect::new(0.75, br.center().y, 0., 0.).feather(0.13 + (1. - p) * 0.05);
            ui.fill_rect(r, (**icon, r, ScaleType::Fit, semi_white(p)));

            let y = y + 0.16;
            let lf = -0.48 + (1.2 - y) / 1.9 * 0.4;
            let mut x = lf;
            let p = ran(t, 0.9, 2.6);
            let mut digits = Vec::with_capacity(7);
            let mut s = res.score;
            for _ in 0..7 {
                digits.push(s % 10);
                s /= 10;
            }
            digits.reverse();
            let s = 1.5;
            let sr = ui.text("0").size(s).measure_using(&PGR_FONT);
            let h = sr.h;
            ui.scissor(Rect::new(-1., y, 2., h + 0.01), |ui| {
                for (i, d) in digits.into_iter().enumerate() {
                    let p = (p * (1. + (0.16 * (6 - i) as f32).powi(2))).min(1.);
                    let p = 1. - (1. - p).powi(3);
                    let mut p = d as f32 + (1. - p) * 7.;
                    if p > 10. {
                        p -= 10.;
                    }
                    let up = p as u32;
                    let dw = (up + 1) % 10;
                    let o = -h * (p - up as f32);
                    ui.text(up.to_string())
                        .pos(x + sr.w / 2., y + o)
                        .anchor(0.5, 0.)
                        .size(s)
                        .draw_using(&PGR_FONT);
                    ui.text(dw.to_string())
                        .pos(x + sr.w / 2., y + h + o)
                        .anchor(0.5, 0.)
                        .size(s)
                        .draw_using(&PGR_FONT);
                    x += sr.w;
                }
            });

            let cl = semi_white(0.6);
            let ct = semi_white(0.8);
            let cs = semi_white(0.4);
            let s = 0.5;

            let r = ui
                .text(tl!("accuracy"))
                .pos(lf - 0.017, y + h + 0.03)
                .color(cl)
                .size(s)
                .draw_using(&BOLD_FONT);
            let r = ui
                .text(format!("{:.2}%", res.accuracy * 100.))
                .pos(r.right() + 0.02, r.y)
                .color(ct)
                .size(s)
                .draw_using(&BOLD_FONT);

            let r = ui.text("|").pos(r.right() + 0.03, r.y).color(cs).size(s).draw();

            let r = ui.text(tl!("max-combo"))
                .pos(r.right() + 0.03, r.y)
                .color(cl)
                .size(s)
                .draw_using(&BOLD_FONT);
            ui.text(format!("{}/{}", res.max_combo, res.num_of_notes))
                .pos(r.right() + 0.02, r.y)
                .size(s)
                .color(ct)
                .draw_using(&BOLD_FONT);

            // 判定计数（历史记录无 PERFECT+ 分档）
            let mut y = -top + 0.4 + ui.top * 0.3;
            let tp = y;
            // 与结算界面一致：判定列表 / 连击条的左基准从 -0.26 起算。
            // 之前误用了分数那一段的 `lf`（-0.48 起算），导致整体往左偏了约 0.2。
            let mut x = -0.26 + (1.2 - y) / 1.9 * 0.4;
            let lf = x;
            let s = 0.64;
            if !res.grading.detailed {
                for (id, title) in prpr::judge::visible_grades(res.grading) {
                    ui.text(title)
                        .pos(x, y)
                        .anchor(1., 0.)
                        .color(semi_white(0.6))
                        .size(s)
                        .draw_using(&BOLD_FONT);
                    let r = ui.text(res.counts[id].to_string()).pos(x + 0.06, y).size(s).draw_using(&BOLD_FONT);
                    let dy = r.h + 0.03;
                    y += dy;
                    x -= dy / 1.9 * 0.4;
                }
            }

            // 连击进度条
            let p = ran(t, 0.8, 1.8);
            let p = 1. - (1. - p).powi(3);
            let y = tp;
            let x = lf + 0.42;
            let r = ui
                .text(tl!("max-combo"))
                .pos(x, y)
                .anchor(1., 0.)
                .color(semi_white(0.6))
                .size(s)
                .draw_using(&BOLD_FONT);
            let mut r = Rect::new(r.right() + 0.03, r.y + 0.004, 0.45, r.h);
            let draw_par = |ui: &mut Ui, r: Rect, p: f32, c: Color| {
                let sl = 1.9 / 0.4;
                let w = p * r.w;
                let d = r.h / sl;
                let mut b = ui.builder(c);
                b.add(r.x, r.bottom());
                if w < d {
                    b.add(r.x + w, r.bottom());
                    b.add(r.x + w, r.bottom() - w * sl);
                    b.triangle(0, 1, 2);
                } else {
                    b.add(r.x + d, r.y);
                    b.add(r.x + w, r.y);
                    b.add(r.x + w.min(r.w - d), r.bottom());
                    b.triangle(0, 1, 2);
                    b.triangle(0, 2, 3);
                    if w + d > r.right() {
                        b.add(r.x + w, r.y + (r.w - w) * sl);
                        b.triangle(2, 3, 4);
                    }
                }
                b.commit();
            };
            draw_par(ui, r, 1., semi_black(0.4));
            let ct2 = r.center();
            let combo = (res.max_combo as f32 * p).round() as u32;
            let text = format!("{combo} / {}", res.num_of_notes);
            ui.text(&text)
                .pos(ct2.x, ct2.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.4)
                .draw_using(&BOLD_FONT);
            let p = combo as f32 / res.num_of_notes as f32;
            draw_par(ui, r, p, WHITE);
            r.w *= p;
            ui.scissor(r, |ui| {
                ui.text(text)
                    .pos(ct2.x, ct2.y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(BLACK)
                    .draw_using(&BOLD_FONT);
            });

            // 判定分布图（早 ← → 晚），与结算界面一致
            let hist = &res.hist;
            if hist.iter().any(|it| *it > 0) {
                let w = 0.9;
                let h = 0.085;
                let x0 = -0.26;
                let cy = -top + 0.43 + top * 0.15;
                let base = cy + h / 2.;
                let bw = w / HIST_BUCKETS as f32;
                let mid = HIST_BUCKETS as f32 / 2.;
                let max = hist.iter().copied().max().unwrap_or(1).max(1) as f32;
                let bucket_ms = (HIST_MAX_MS * 2. / HIST_BUCKETS as f64) as f32;
                let gold = Color::new(1., 0.84, 0.35, 0.95);
                let blue = Color::new(0.45, 0.72, 1., 0.92);
                let red = Color::new(1., 0.42, 0.42, 0.92);
                let grey = Color::new(0.62, 0.62, 0.66, 0.9);
                for (i, count) in hist.iter().enumerate() {
                    let bh = h * (*count as f32 / max);
                    let off = (i as f32 + 0.5) * bucket_ms - HIST_MAX_MS as f32;
                    let a = off.abs();
                    let color = if a <= 80. { gold } else if a <= 160. { blue } else if a <= 240. { red } else { grey };
                    ui.fill_rect(Rect::new(x0 + bw * i as f32 + bw * 0.1, base - bh, bw * 0.8, bh), color);
                }
                let zx = x0 + bw * mid;
                ui.fill_rect(Rect::new(zx - 0.0008, cy - h / 2., 0.0016, h), semi_white(0.4));
            }

            // 日期信息（右上角）
            ui.text(self.record.time_text())
                .pos(1. - 0.02, -top + 0.48)
                .anchor(1., 0.)
                .size(0.45)
                .color(semi_white(0.6))
                .draw_using(&BOLD_FONT);
        }

        // —— 曲绘开屏动画（与结算界面一致，会被扇形遮罩） ——
        clip_sector(ui, ct, sector_start, sector_start + center_angle, |ui| {
            ui.fill_rect(sr, (*self.illustration, sr));
        });
        let sector_start = (p * 1.4 - 0.3).max(0.) * (angle_end - angle_start - center_angle) + angle_start;
        clip_sector(ui, ct, sector_start, sector_start + center_angle * 0.5, |ui| {
            ui.fill_rect(sr, (*self.illustration, sr.feather(0.15)));
        });

        if project_y < top && res.grading.detailed {
            ui.alpha(pf, |ui| {
                prpr::ui::draw_judgement_grid(ui, &res, prpr::ui::judgement_grid_area(ui.top), false);
            });
        }

        // —— 底部按钮：返回 / 导入管理 / 播放回放，同一横排 ——
        ui.alpha(pf, |ui| {
            let t = tm.real_time() as f32;
            let grid = prpr::ui::judgement_grid_area(top);
            let width = if res.grading.detailed && grid.bottom() > top - 0.14 {
                // On wide phones the last judgement rows share this vertical
                // strip. Fit all three buttons to their right, without stacking.
                (0.96 - grid.right() - 0.01 - 2. * 0.02) / 3.
            } else {
                0.25
            };
            let mut r = Rect::new(0.96, top - 0.04, width, 0.1);
            r.x -= r.w;
            r.y -= r.h;
            let replay_exists = self.replay_path.is_some();
            self.btn_replay.render_shadow(ui, r, t, |ui, path| {
                ui.fill_path(
                    &path,
                    if replay_exists {
                        Color::from_hex_rgb(0x43a047)
                    } else {
                        Color::from_hex_rgb(0x37474f)
                    },
                );
                ui.text(if replay_exists { tl!("record-play") } else { tl!("record-no-replay") })
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.44)
                    .max_width(r.w - 0.02)
                    .draw_using(&BOLD_FONT);
            });
            r.x -= r.w + 0.02;
            let manage_label = "导入 / 管理回放";
            let manage_width = ui.text(manage_label).size(0.4).measure_using(&BOLD_FONT).w;
            let manage_size = 0.4 * ((r.w - 0.025) / manage_width).min(1.);
            self.btn_open.render_shadow(ui, r, t, |ui, path| {
                ui.fill_path(&path, Color::from_hex_rgb(0x455a64));
                ui.text(manage_label)
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(manage_size)
                    .max_width(r.w - 0.02)
                    .draw_using(&BOLD_FONT);
            });
            r.x -= r.w + 0.02;
            self.btn_back.render_shadow(ui, r, t, |ui, path| {
                ui.fill_path(&path, Color::from_hex_rgb(0x78909c));
                ui.text(tl!("record-back"))
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.44)
                    .max_width(r.w - 0.02)
                    .draw_using(&BOLD_FONT);
            });
            if self.replay_path.is_some() {
                self.btn_export
                    .render_text(ui, Rect::new(-0.96, top - 0.11, 0.24, 0.075), t, "导出回放", 0.4, false);
            }
        });

        if self.replay_task.is_some() {
            ui.full_loading_simple(t);
        }

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if let Some(n) = self.next.take() {
            return n;
        }
        if self.exit {
            NextScene::Pop
        } else {
            NextScene::None
        }
    }
}
