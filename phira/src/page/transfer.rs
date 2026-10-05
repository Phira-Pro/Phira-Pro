prpr_l10n::tl_file!("settings");

// 「数据迁移 / 备份还原」设置子页。

use super::{Page, SharedState};
use crate::{data::LocalChart, get_data, get_data_mut, migrate, save_data, transfer};
use anyhow::{anyhow, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{semi_white, RectExt},
    scene::{show_error, show_message},
    task::Task,
    ui::{DRectButton, Scroll, Ui},
};
use std::{
    borrow::Cow,
    collections::HashSet,
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};

/// 是否为支持系统原生文件对话框的桌面平台。
#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
const DESKTOP: bool = true;
#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
const DESKTOP: bool = false;

#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
fn pick_json() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("data.json", &["json"]).pick_file()
}

#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
fn pick_json() -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
fn pick_save(default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(default_name).save_file()
}

#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
fn pick_save(_default_name: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
fn pick_zip() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("Phira 备份", &["zip"]).pick_file()
}

#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
fn pick_zip() -> Option<PathBuf> {
    None
}

pub struct TransferPage {
    scroll: Scroll,
    scan: Option<transfer::Scan>,
    import_config: bool,
    status: Option<Result<String>>,
    /// 后台任务：备份 / 还原 / 导入（完成后给出提示）。
    busy: Option<Task<Result<String>>>,
    btn_pick: DRectButton,
    btn_import: DRectButton,
    btn_import_config: DRectButton,
    btn_backup: DRectButton,
    btn_restore: DRectButton,

    /// Phira Pro：从官服导入「我有成绩的谱面」。
    mig_min_diff: usize,
    mig_min_rating: usize,
    mig_ranked_only: bool,
    mig_skip: bool,
    btn_mig_min_diff: DRectButton,
    btn_mig_min_rating: DRectButton,
    btn_mig_ranked: DRectButton,
    btn_mig_skip: DRectButton,
    btn_mig_start: DRectButton,
    /// Phira Pro：批量导入「官方客户端批量导出」的谱面包（复用 `scene/main.rs` 的 `_import` 路径）。
    btn_chart_import: DRectButton,
    mig_prog: Option<Arc<migrate::Progress>>,
    mig_task: Option<Task<Result<Vec<LocalChart>>>>,
}

impl TransferPage {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            scan: None,
            import_config: true,
            status: None,
            busy: None,
            btn_pick: DRectButton::new().with_radius(0.008),
            btn_import: DRectButton::new().with_radius(0.008),
            btn_import_config: DRectButton::new().with_radius(0.008),
            btn_backup: DRectButton::new().with_radius(0.008),
            btn_restore: DRectButton::new().with_radius(0.008),
            mig_min_diff: 0,
            mig_min_rating: 0,
            mig_ranked_only: false,
            mig_skip: true,
            btn_mig_min_diff: DRectButton::new().with_radius(0.008),
            btn_mig_min_rating: DRectButton::new().with_radius(0.008),
            btn_mig_ranked: DRectButton::new().with_radius(0.008),
            btn_mig_skip: DRectButton::new().with_radius(0.008),
            btn_mig_start: DRectButton::new().with_radius(0.008),
            btn_chart_import: DRectButton::new().with_radius(0.008),
            mig_prog: None,
            mig_task: None,
        }
    }

    fn backup_name() -> String {
        format!("phira-backup-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"))
    }

    fn start_task(&mut self, f: impl FnOnce() -> Result<String> + Send + 'static) {
        if self.busy.is_some() {
            return;
        }
        show_message(tl!("transfer-working")).ok();
        self.status = None;
        self.busy = Some(Task::new(async move { f() }));
    }
}

impl Page for TransferPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("transfer-label")
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if let Some(task) = &mut self.busy {
            if let Some(res) = task.take() {
                self.busy = None;
                match res {
                    Ok(msg) => {
                        self.status = Some(Ok(msg.clone()));
                        show_message(msg).ok();
                    }
                    Err(err) => {
                        self.status = Some(Err(anyhow::anyhow!(format!("{err:#}"))));
                        show_error(err.context(tl!("transfer-failed")));
                    }
                }
            }
        }
        // Phira Pro：官服成绩谱面迁移任务。
        if let Some(task) = &mut self.mig_task {
            if let Some(res) = task.take() {
                self.mig_task = None;
                match res {
                    Ok(charts) => {
                        let mut added = 0usize;
                        {
                            let data = get_data_mut();
                            for c in charts {
                                if !data.charts.iter().any(|it| it.local_path == c.local_path) {
                                    data.charts.push(c);
                                    added += 1;
                                }
                            }
                        }
                        save_data()?;
                        s.reload_local_charts();
                        let msg = tl!("migrate-imported", "count" => added.to_string());
                        self.status = Some(Ok(msg.clone()));
                        show_message(msg).ok();
                    }
                    Err(err) => {
                        self.status = Some(Err(anyhow!("{err:#}")));
                        show_error(err.context(tl!("transfer-failed")));
                    }
                }
            }
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.busy.is_some() {
            // 处理中忽略其它点击
            return Ok(true);
        }

        if self.btn_pick.touch(touch, t) {
            if !DESKTOP {
                self.status = Some(Ok(tl!("transfer-unsupported").into_owned()));
                show_message(tl!("transfer-unsupported")).error();
                return Ok(true);
            }
            if let Some(path) = pick_json() {
                self.status = Some(match transfer::scan(&path) {
                    Ok(scan) => {
                        let msg = tl!(
                            "transfer-scan",
                            "charts" => scan.charts.to_string(),
                            "respacks" => scan.respacks.to_string(),
                            "appearance" => scan.appearance.to_string()
                        );
                        self.scan = Some(scan);
                        show_message(msg.clone()).ok();
                        Ok(msg)
                    }
                    Err(err) => {
                        let msg = format!("{err:#}");
                        show_error(anyhow::anyhow!("{}: {}", tl!("transfer-failed"), msg));
                        Err(anyhow::anyhow!(msg))
                    }
                });
            }
            return Ok(true);
        }

        if self.btn_import_config.touch(touch, t) {
            self.import_config ^= true;
            return Ok(true);
        }

        if self.btn_import.touch(touch, t) {
            if let Some(scan) = self.scan.clone() {
                let import_config = self.import_config;
                self.start_task(move || {
                    let v = transfer::import(&scan, import_config)?;
                    Ok(tl!(
                        "transfer-done",
                        "charts" => v.charts.to_string(),
                        "respacks" => v.respacks.to_string(),
                        "appearance" => v.appearance.to_string()
                    ))
                });
            }
            return Ok(true);
        }

        if self.btn_backup.touch(touch, t) {
            if !DESKTOP {
                self.status = Some(Ok(tl!("transfer-unsupported").into_owned()));
                show_message(tl!("transfer-unsupported")).error();
                return Ok(true);
            }
            if let Some(path) = pick_save(&Self::backup_name()) {
                self.start_task(move || {
                    let n = transfer::create_backup(&path)?;
                    Ok(tl!("backup-created", "count" => n.to_string()))
                });
            }
            return Ok(true);
        }

        if self.btn_restore.touch(touch, t) {
            if !DESKTOP {
                self.status = Some(Ok(tl!("transfer-unsupported").into_owned()));
                show_message(tl!("transfer-unsupported")).error();
                return Ok(true);
            }
            if let Some(path) = pick_zip() {
                self.start_task(move || {
                    let n = transfer::restore_backup(&path)?;
                    Ok(tl!("backup-restored", "count" => n.to_string()))
                });
            }
            return Ok(true);
        }

        // Phira Pro：从官服导入「我有成绩的谱面」。
        if self.btn_mig_min_diff.touch(touch, t) {
            self.mig_min_diff = (self.mig_min_diff + 1) % migrate::MIN_DIFF_PRESETS.len();
            return Ok(true);
        }
        if self.btn_mig_min_rating.touch(touch, t) {
            self.mig_min_rating = (self.mig_min_rating + 1) % migrate::MIN_RATING_PRESETS.len();
            return Ok(true);
        }
        if self.btn_mig_ranked.touch(touch, t) {
            self.mig_ranked_only ^= true;
            return Ok(true);
        }
        if self.btn_mig_skip.touch(touch, t) {
            self.mig_skip ^= true;
            return Ok(true);
        }
        if self.btn_mig_start.touch(touch, t) {
            // 运行中再点 = 取消本次迁移。
            if self.mig_task.is_some() {
                if let Some(p) = &self.mig_prog {
                    p.cancel.store(true, Ordering::Relaxed);
                }
                return Ok(true);
            }
            let Some(me) = get_data().me.as_ref().map(|it| it.id) else {
                show_error(anyhow!(tl!("migrate-need-login")));
                return Ok(true);
            };
            let downloaded: HashSet<i32> = get_data().charts.iter().filter_map(|it| it.info.id).collect();
            let filter = migrate::Filter {
                min_difficulty: migrate::MIN_DIFF_PRESETS[self.mig_min_diff],
                min_rating: migrate::MIN_RATING_PRESETS[self.mig_min_rating],
                ranked_only: self.mig_ranked_only,
                skip_downloaded: self.mig_skip,
            };
            let p = Arc::new(migrate::Progress::default());
            self.mig_prog = Some(Arc::clone(&p));
            self.mig_task = Some(Task::new(async move { migrate::run(me, downloaded, filter, p).await }));
            return Ok(true);
        }

        // Phira Pro：批量导入官方导出的谱面包。
        if self.btn_chart_import.touch(touch, t) {
            prpr::scene::request_file("_import");
            return Ok(true);
        }

        Ok(false)
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let mut cr = ui.content_rect();
        cr.x += 0.025;
        cr.w -= 0.025;

        s.render_fader(ui, |ui| {
            let outer = cr.feather(-0.005);
            self.scroll.size((outer.w, outer.h));
            ui.dx(outer.x);
            ui.dy(outer.y);
            self.scroll.render(ui, |ui| {
                let w = outer.w;
                let mut y = 0.;

                // 小标题 / 说明注释的字号。原先用 0.135 / 0.095，在页面缩放下只有几像素，
                // 完全看不清；这里对齐设置页的标题/副标题字号。
                const HEAD_SIZE: f32 = 0.6;
                const DESC_SIZE: f32 = 0.35;

                // ---------------- 数据迁移 ----------------
                ui.text(tl!("transfer-label")).pos(0.004, y).anchor(0., 0.).size(HEAD_SIZE).color(WHITE).draw();
                y += HEAD_SIZE * 0.17;

                let dh = ui
                    .text(tl!("transfer-desc"))
                    .pos(0.004, y)
                    .anchor(0., 0.)
                    .multiline()
                    .max_width(w - 0.008)
                    .size(DESC_SIZE)
                    .color(semi_white(0.75))
                    .draw()
                    .h;
                y += dh + 0.05;

                self.btn_pick.render_text(ui, Rect::new(0., y, w, 0.095), t, tl!("transfer-pick"), 0.55, false);
                y += 0.095 + 0.014;

                if let Some(scan) = &self.scan {
                    let name = scan.name.clone().unwrap_or_default();
                    let msg = if name.is_empty() {
                        tl!(
                            "transfer-scan",
                            "charts" => scan.charts.to_string(),
                            "respacks" => scan.respacks.to_string(),
                            "appearance" => scan.appearance.to_string()
                        )
                    } else {
                        format!(
                            "{} · {}",
                            name,
                            tl!(
                                "transfer-scan",
                                "charts" => scan.charts.to_string(),
                                "respacks" => scan.respacks.to_string(),
                                "appearance" => scan.appearance.to_string()
                            )
                        )
                    };
                    let mh = ui
                        .text(msg)
                        .pos(0.004, y)
                        .anchor(0., 0.)
                        .multiline()
                        .max_width(w - 0.008)
                        .size(DESC_SIZE)
                        .color(semi_white(0.9))
                        .draw()
                        .h;
                    y += mh + 0.04;

                    // 用 l10n 的「开 / 关」而不是 ✓ / ✗：内置字体缺这两个符号，
                    // 缺字不占宽会让整段文字在按钮里整体左偏，看起来"没居中"。
                    let state = if self.import_config { crate::ttl!("switch-on") } else { crate::ttl!("switch-off") };
                    let cfg_label = format!("{}：{}", tl!("transfer-import-config"), state);
                    self.btn_import_config.render_text(ui, Rect::new(0., y, w, 0.095), t, &cfg_label, 0.55, false);
                    y += 0.095 + 0.012;

                    self.btn_import.render_text(ui, Rect::new(0., y, w, 0.095), t, tl!("transfer-import"), 0.55, false);
                    y += 0.095 + 0.02;
                } else {
                    y += 0.02;
                }

                // ---------------- 从官服导入我的成绩谱面 ----------------
                ui.text(tl!("migrate-label")).pos(0.004, y).anchor(0., 0.).size(HEAD_SIZE).color(WHITE).draw();
                y += HEAD_SIZE * 0.17;

                let dh = ui
                    .text(tl!("migrate-desc"))
                    .pos(0.004, y)
                    .anchor(0., 0.)
                    .multiline()
                    .max_width(w - 0.008)
                    .size(DESC_SIZE)
                    .color(semi_white(0.75))
                    .draw()
                    .h;
                y += dh + 0.04;

                let on_off = |on: bool| if on { crate::ttl!("switch-on") } else { crate::ttl!("switch-off") };

                let d = migrate::MIN_DIFF_PRESETS[self.mig_min_diff];
                let label = if d > 0. {
                    format!("{}：{d:.1}", tl!("migrate-min-diff"))
                } else {
                    format!("{}：{}", tl!("migrate-min-diff"), crate::ttl!("migrate-unlimited"))
                };
                self.btn_mig_min_diff.render_text(ui, Rect::new(0., y, w, 0.095), t, &label, 0.55, false);
                y += 0.095 + 0.012;

                let r = migrate::MIN_RATING_PRESETS[self.mig_min_rating];
                let label = if r > 0. {
                    format!("{}：{r:.2}", tl!("migrate-min-rating"))
                } else {
                    format!("{}：{}", tl!("migrate-min-rating"), crate::ttl!("migrate-unlimited"))
                };
                self.btn_mig_min_rating.render_text(ui, Rect::new(0., y, w, 0.095), t, &label, 0.55, false);
                y += 0.095 + 0.012;

                let label = format!("{}：{}", tl!("migrate-ranked-only"), on_off(self.mig_ranked_only));
                self.btn_mig_ranked.render_text(ui, Rect::new(0., y, w, 0.095), t, &label, 0.55, false);
                y += 0.095 + 0.012;

                let label = format!("{}：{}", tl!("migrate-skip"), on_off(self.mig_skip));
                self.btn_mig_skip.render_text(ui, Rect::new(0., y, w, 0.095), t, &label, 0.55, false);
                y += 0.095 + 0.016;

                let label = if self.mig_task.is_some() { tl!("migrate-cancel") } else { tl!("migrate-start") };
                self.btn_mig_start.render_text(ui, Rect::new(0., y, w, 0.095), t, label, 0.55, false);
                y += 0.095 + 0.012;

                if let Some(p) = &self.mig_prog {
                    let msg = p.status();
                    if !msg.is_empty() {
                        let mh = ui
                            .text(msg)
                            .pos(0.004, y)
                            .anchor(0., 0.)
                            .multiline()
                            .max_width(w - 0.008)
                            .size(DESC_SIZE)
                            .color(semi_white(0.9))
                            .draw()
                            .h;
                        y += mh + 0.02;
                    }
                }
                y += 0.02;

                // ---------------- 批量导入谱面（官方导出包） ----------------
                ui.text(tl!("chart-import-label")).pos(0.004, y).anchor(0., 0.).size(HEAD_SIZE).color(WHITE).draw();
                y += HEAD_SIZE * 0.17;
                let dh = ui
                    .text(tl!("chart-import-desc"))
                    .pos(0.004, y)
                    .anchor(0., 0.)
                    .multiline()
                    .max_width(w - 0.008)
                    .size(DESC_SIZE)
                    .color(semi_white(0.75))
                    .draw()
                    .h;
                y += dh + 0.04;
                self.btn_chart_import
                    .render_text(ui, Rect::new(0., y, w, 0.095), t, tl!("chart-import-btn"), 0.55, false);
                y += 0.095 + 0.02;

                // ---------------- 备份与还原 ----------------
                ui.text(tl!("backup-label")).pos(0.004, y).anchor(0., 0.).size(HEAD_SIZE).color(WHITE).draw();
                y += HEAD_SIZE * 0.17;

                let dh = ui
                    .text(tl!("backup-desc"))
                    .pos(0.004, y)
                    .anchor(0., 0.)
                    .multiline()
                    .max_width(w - 0.008)
                    .size(DESC_SIZE)
                    .color(semi_white(0.75))
                    .draw()
                    .h;
                y += dh + 0.05;

                self.btn_backup.render_text(ui, Rect::new(0., y, w, 0.095), t, tl!("backup-create"), 0.55, false);
                y += 0.095 + 0.012;
                self.btn_restore.render_text(ui, Rect::new(0., y, w, 0.095), t, tl!("backup-restore"), 0.55, false);
                y += 0.095 + 0.02;

                // ---------------- 状态 ----------------
                if let Some(status) = &self.status {
                    let (text, color) = match status {
                        Ok(msg) => (msg.clone(), semi_white(0.9)),
                        Err(err) => (format!("{}: {err}", tl!("transfer-failed")), RED),
                    };
                    let mh = ui
                        .text(text)
                        .pos(0.004, y)
                        .anchor(0., 0.)
                        .multiline()
                        .max_width(w - 0.008)
                        .size(DESC_SIZE)
                        .color(color)
                        .draw()
                        .h;
                    y += mh + 0.02;
                }

                (w, y + 0.02)
            });
        });
        if self.busy.is_some() {
            ui.full_loading_simple(t);
        }
        Ok(())
    }
}
