//! Fixed header/footer and independent list scrolling.
use super::*;
use prpr::ext::{semi_white, RectExt, SafeTexture};
const BH: f32 = 0.078;
const GAP: f32 = 0.014;
const RH: f32 = 0.19;
pub(super) fn entry_top(i: usize, n: usize, h: f32) -> f32 {
    (i as f32 * (RH + GAP)).min((n as f32 * (RH + GAP) + 0.25 - h).max(0.))
}
fn title(i: &Item) -> String {
    if i.meta.name.is_empty() {
        i.path.file_name().unwrap_or_default().to_string_lossy().into_owned()
    } else {
        i.meta.name.clone()
    }
}
fn status(i: &Item) -> &'static str {
    if i.binding.as_ref().is_none_or(|b| b.local_path.is_none()) && i.imported() {
        "待关联谱面"
    } else if i.binding.as_ref().is_some_and(|b| !b.verified) {
        "关联未校验"
    } else if i.imported() {
        "导入回放"
    } else {
        "自己录制"
    }
}
impl ReplayManager {
    pub(super) fn render_inner(&mut self, ui: &mut Ui, t: f32) {
        let a = if self.standalone {
            ui.screen_rect().feather(-0.04)
        } else {
            ui.content_rect().feather(-0.015)
        };
        for b in &mut self.buttons {
            b.invalidate();
        }
        ui.fill_rect(a, Color::new(0.075, 0.11, 0.15, 0.96));
        let x = a.x + 0.025;
        let w = a.w - 0.05;
        let bw = (w - 3. * GAP) / 4.;
        let top = a.y + 0.025;
        ui.text("回放管理").pos(x, top + 0.014).size(0.52).draw();
        let (search, close) = self.search_icons.get_or_insert_with(|| {
            (
                SafeTexture::from(Texture2D::from_file_with_format(include_bytes!("../../../../assets/search.png"), None)),
                SafeTexture::from(Texture2D::from_file_with_format(include_bytes!("../../../../assets/close.png"), None)),
            )
        });
        let r = crate::page::search_bar::rect(a.right() - 0.025, top, crate::page::search_bar::HEIGHT, self.query.is_empty());
        crate::page::search_bar::render(ui, &mut self.buttons[13], &mut self.search_clear, r, t, &self.query, search, close);
        let mut y = top + 0.115;
        for (c, (i, s)) in [
            (1, "导入回放"),
            (2, "刷新列表"),
            (14, ["最新录制", "曲名排序", "占用空间", "重要优先"][self.sort]),
            (15, if self.multi { "完成多选" } else { "多选回放" }),
        ]
        .iter()
        .enumerate()
        {
            self.buttons[*i].render_text(ui, Rect::new(x + c as f32 * (bw + GAP), y, bw, BH), t, *s, 0.35, *i == 15 && self.multi);
        }
        y += BH + GAP;
        for (c, s) in ["全部回放", "自己录制", "导入回放", "缺少谱面"].iter().enumerate() {
            self.buttons[c + 3].render_text(ui, Rect::new(x + c as f32 * (bw + GAP), y, bw, BH), t, *s, 0.35, self.filter == c);
        }
        y += BH + GAP;
        if self.multi {
            for (c, (i, s)) in [(16, "全选结果"), (17, "删除所选"), (18, "导出所选")].iter().enumerate() {
                self.buttons[*i].render_text(ui, Rect::new(x + c as f32 * (bw + GAP), y, bw, BH), t, *s, 0.35, false);
            }
            ui.text(format!("已选 {} 项", self.chosen.len()))
                .pos(x + 3. * (bw + GAP), y + 0.02)
                .size(0.35)
                .draw();
            y += BH + GAP;
        }
        if !self.status.is_empty() {
            let h = ui.text(&self.status).size(0.31).multiline().max_width(w).measure().h;
            ui.text(&self.status)
                .pos(x, y)
                .size(0.31)
                .multiline()
                .max_width(w)
                .color(Color::from_rgba(125, 194, 242, 255))
                .draw();
            y += h + 0.02;
        }
        let footer = a.bottom() - BH - 0.025;
        if !crate::plugins::replay_analysis::available().is_empty() {
            self.buttons[20].render_text(ui, Rect::new(x, footer, bw, BH), t, "PRCA", 0.36, false);
        }
        if self.standalone {
            self.buttons[0].render_text(ui, Rect::new(x + 3. * (bw + GAP), footer, bw, BH), t, "返回", 0.36, false);
        }
        let items = self.visible();
        ui.text(format!("{} 个回放", items.len()))
            .pos(x + bw + GAP, footer + 0.025)
            .size(0.3)
            .color(semi_white(0.65))
            .draw();
        let list = Rect::new(x, y, w, (footer - y - 0.025).max(0.08));
        self.scroll.size((list.w, list.h));
        let charts = crate::get_data()
            .charts
            .iter()
            .map(|c| (c.info.name.clone(), c.local_path.clone()))
            .collect::<Vec<_>>();
        self.rows
            .resize_with(if self.choosing_chart { charts.len() } else { items.len() }, RectButton::new);
        for row in &mut self.rows {
            row.set(ui, Rect::new(10., 10., 0., 0.));
        }
        ui.scope(|ui| {
            ui.dx(list.x);
            ui.dy(list.y);
            self.scroll.render(ui, |ui| {
                let mut y = 0.;
                if self.choosing_chart {
                    ui.text("选择录制时的本地谱面").pos(0.02, 0.02).size(0.4).draw();
                    self.buttons[10].render_text(ui, Rect::new(w - bw, 0., bw, BH), t, "取消关联", 0.35, false);
                    y = 0.11;
                    for (i, (name, path)) in charts.iter().enumerate() {
                        let r = Rect::new(0., y, w, RH);
                        ui.fill_path(&r.rounded(0.01), Color::new(0.12, 0.17, 0.22, 1.));
                        ui.text(name).pos(0.025, y + 0.02).size(0.44).max_width(w - 0.05).draw();
                        ui.text(path)
                            .pos(0.025, y + 0.105)
                            .size(0.29)
                            .color(semi_white(0.65))
                            .max_width(w - 0.05)
                            .draw();
                        self.rows[i].set(ui, r);
                        y += RH + GAP;
                    }
                } else {
                    for (i, item) in items.iter().enumerate() {
                        let expanded = !self.multi && self.selected.as_ref() == Some(&item.path);
                        let selected = if self.multi { self.chosen.contains(&item.path) } else { expanded };
                        let h = RH + if expanded { 2. * (BH + GAP) + 0.025 } else { 0. };
                        let card = Rect::new(0., y, w, h);
                        ui.fill_path(
                            &card.rounded(0.01),
                            if selected {
                                Color::new(0.14, 0.25, 0.33, 1.)
                            } else {
                                Color::new(0.11, 0.16, 0.21, 1.)
                            },
                        );
                        if selected {
                            ui.stroke_path(&card.rounded(0.01), 0.0025, Color::from_rgba(102, 193, 242, 255));
                        }
                        ui.text(title(item)).pos(0.025, y + 0.018).size(0.46).max_width(w - 0.25).draw();
                        ui.text(if self.multi {
                            if selected {
                                "已选中"
                            } else {
                                "未选中"
                            }
                        } else {
                            status(item)
                        })
                        .pos(w - 0.025, y + 0.037)
                        .anchor(1., 0.5)
                        .size(0.28)
                        .color(semi_white(0.65))
                        .draw();
                        let date = item
                            .meta
                            .recorded_at
                            .map(|time| crate::history::Record { time, ..Default::default() }.time_text())
                            .unwrap_or("时间未记录".into());
                        ui.text(format!("{}    {}    {}", item.player(), item.meta.level, date))
                            .pos(0.025, y + 0.083)
                            .size(0.29)
                            .color(semi_white(0.7))
                            .max_width(w - 0.05)
                            .draw();
                        let score = item
                            .record
                            .as_ref()
                            .map(|r| format!("{:07}    {:.2}%", r.score, r.accuracy * 100.))
                            .unwrap_or("成绩未记录".into());
                        ui.text(score).pos(0.025, y + 0.135).size(0.34).draw();
                        ui.text(format!("{}    {}", item.mode, item.speed.map(|s| format!("{s:.2}×")).unwrap_or("速度未记录".into())))
                            .pos(w - 0.025, y + 0.151)
                            .anchor(1., 0.5)
                            .size(0.29)
                            .color(semi_white(0.65))
                            .draw();
                        self.rows[i].set(ui, Rect::new(0., y, w, RH));
                        if expanded {
                            let keep = item.binding.as_ref().is_some_and(|b| b.keep);
                            for (row, actions) in [
                                [
                                    (7, "播放回放"),
                                    (8, "导出回放"),
                                    (9, "删除回放"),
                                    (19, if keep { "取消重要" } else { "标为重要" }),
                                ],
                                [(10, "关联谱面"), (11, "下载谱面"), (12, "录制版本"), (usize::MAX, "")],
                            ]
                            .iter()
                            .enumerate()
                            {
                                for (col, (index, label)) in actions.iter().enumerate() {
                                    if *index != usize::MAX {
                                        self.buttons[*index].render_text(
                                            ui,
                                            Rect::new(col as f32 * (bw + GAP), y + RH + row as f32 * (BH + GAP), bw, BH),
                                            t,
                                            *label,
                                            0.35,
                                            *index == 19 && keep,
                                        );
                                    }
                                }
                            }
                        }
                        y += h + GAP;
                    }
                    if items.is_empty() {
                        ui.text("暂无回放，请导入文件或调整搜索条件")
                            .pos(0.025, 0.035)
                            .size(0.37)
                            .max_width(w - 0.05)
                            .draw();
                        y = 0.16;
                    }
                }
                (w, y + 0.02)
            });
        });
        if self.task.is_some() || self.play_task.is_some() || self.download_task.is_some() || self.install_task.is_some() {
            ui.full_loading_simple(t);
        }
        if let Some(archive) = &mut self.archive {
            archive.render(ui, t);
        }
    }
}
