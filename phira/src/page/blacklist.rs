prpr_l10n::tl_file!("blacklist");

// 黑名单管理页：查看 / 删除 / 手动输入 Phira 数字 ID 添加。
// （注：模块文档注释不能写成 //! —— tl_file! 展开后必须是文件开头）

use super::{Page, SharedState};
use crate::{blacklist, client::UserManager};
use anyhow::Result;
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    scene::{request_input, return_input, show_error, show_message, take_input},
    ui::{DRectButton, Scroll, Ui},
};
use std::borrow::Cow;

const ROW_H: f32 = 0.085;
const GAP: f32 = 0.012;

pub struct BlacklistPage {
    entries: Vec<blacklist::Entry>,
    scroll: Scroll,
    btn_add: DRectButton,
    /// 每行一个「解除」按钮（渲染时 set，触摸时用）。
    row_btns: Vec<DRectButton>,
}

impl BlacklistPage {
    pub fn new() -> Self {
        blacklist::ensure_loaded();
        let entries = blacklist::all();
        for e in &entries {
            UserManager::request(e.id);
        }
        Self {
            entries,
            scroll: Scroll::new(),
            btn_add: DRectButton::new().with_radius(0.008),
            row_btns: Vec::new(),
        }
    }

    fn reload(&mut self) {
        self.entries = blacklist::all();
        for e in &self.entries {
            UserManager::request(e.id);
        }
    }
}

impl Page for BlacklistPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if let Some((id, text)) = take_input() {
            if id == "blacklist-add" {
                match text.trim().parse::<i32>() {
                    Ok(v) if v > 0 => {
                        // 昵称尽量用在线解析结果；拿不到就留空，管理页会退回显示 ID。
                        let name = UserManager::name_and_color(v).map(|(n, _)| n).unwrap_or_default();
                        match blacklist::add(v, &name) {
                            Ok(true) => {
                                self.reload();
                                show_message(tl!("add-done")).ok();
                            }
                            Ok(false) => {
                                self.reload();
                                show_message(tl!("add-exists")).ok();
                            }
                            Err(err) => show_error(err.context(tl!("save-failed"))),
                        }
                    }
                    _ => show_error(anyhow::anyhow!(tl!("add-invalid").into_owned())),
                }
            } else {
                return_input(id, text);
            }
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.btn_add.touch(touch, t) {
            request_input("blacklist-add", InputBox::new().title(tl!("add-title")).prompt(tl!("add-hint")));
            return Ok(true);
        }
        // 顺序要和渲染时一致
        let ids: Vec<i32> = self.entries.iter().map(|it| it.id).collect();
        for (i, btn) in self.row_btns.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                if let Some(id) = ids.get(i) {
                    if let Err(err) = blacklist::remove(*id) {
                        show_error(err.context(tl!("save-failed")));
                    } else {
                        self.reload();
                        show_message(tl!("removed")).ok();
                    }
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let mut cr = ui.content_rect();
        cr.x += 0.025;
        cr.w -= 0.025;

        s.render_fader(ui, |ui| {
            // HUD 自定义：整页作为一个「列表大框」，可整体移动/缩放；行高可在编辑模式里调。
            let key = crate::hud::cur_page().key();
            let outer = crate::hud::slot_or(ui, key, "list", crate::hud::Cap(true, true, true), cr.feather(-0.005));
            let row_h = crate::hud::param("blacklist", "row_h", ROW_H).clamp(0.05, 0.2);
            self.scroll.size((outer.w, outer.h));
            ui.dx(outer.x);
            ui.dy(outer.y);
            self.scroll.render(ui, |ui| {
                let w = outer.w;
                let mut y = 0.;

                // ---------------- 顶部：添加 ----------------
                self.btn_add.render_text(ui, Rect::new(0., y, w, 0.085), t, tl!("add"), 0.5, false);
                y += 0.085 + GAP;

                // ---------------- 标题 ----------------
                ui.text(tl!("label")).pos(0.004, y).anchor(0., 0.).size(0.115).color(WHITE).draw();
                let n = self.entries.len();
                ui.text(tl!("count", "count" => n.to_string()))
                    .pos(w - 0.004, y + 0.025)
                    .anchor(1., 0.)
                    .size(0.06)
                    .color(semi_white(0.55))
                    .draw();
                y += 0.13;

                if n == 0 {
                    ui.text(tl!("empty"))
                        .pos(w / 2., y + 0.04)
                        .anchor(0.5, 0.)
                        .size(0.08)
                        .color(semi_white(0.5))
                        .draw();
                    y += 0.12;
                    return (w, y + GAP);
                }

                while self.row_btns.len() < n {
                    self.row_btns.push(DRectButton::new().with_radius(0.008));
                }

                let remove_label = tl!("remove").into_owned();
                for (i, entry) in self.entries.iter().enumerate() {
                    let r = Rect::new(0., y, w, row_h);
                    ui.fill_path(&r.rounded(0.010), semi_black(0.30));
                    ui.fill_path(&Rect::new(r.x + 0.006, r.y + 0.013, 0.007, r.h - 0.026).rounded(0.003), semi_white(0.5));
                    let cy = r.center().y;
                    ui.text(format!("#{}", entry.id))
                        .pos(r.x + 0.026, cy)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.08)
                        .draw();
                    let name = UserManager::name_and_color(entry.id)
                        .map(|(n, _)| n)
                        .filter(|it| !it.is_empty())
                        .unwrap_or_else(|| entry.name.clone());
                    ui.text(if name.is_empty() { "—".to_owned() } else { name })
                        .pos(r.x + 0.30, cy)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .max_width(r.w - 0.30 - 0.26)
                        .size(0.075)
                        .color(semi_white(0.9))
                        .draw();
                    let br = Rect::new(r.right() - 0.235, cy - 0.032, 0.22, 0.064);
                    self.row_btns[i].render_text_color(ui, br, t, remove_label.as_str(), 0.4, false, RED);
                    y += row_h + 0.008;
                }
                (w, y + GAP)
            });
        });
        Ok(())
    }
}
