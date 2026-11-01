prpr_l10n::tl_file!("collection");
mod management;
#[cfg(all(test, record, target_os = "windows"))]
mod qa;

use super::{Illustration, NextPage, Page, SharedState};
use crate::{
    charts_view::{ChartDisplayItem, ChartsView, NEED_UPDATE},
    get_data, get_data_mut,
    icons::Icons,
    save_data,
    songpacks::{self, SongPack},
};
use anyhow::Result;
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt, ScaleType},
    scene::{request_file, request_input, return_file, return_input, show_error, show_message, take_file, take_input, NextScene},
    task::Task,
    ui::{DRectButton, Dialog, LongTouchState, RectButton, Scroll, Ui},
};
use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};

struct Card {
    pack: SongPack,
    illu: Illustration,
    btn: RectButton,
    long_touch: LongTouchState,
}
pub struct CollectionPage {
    icons: Arc<Icons>,
    cards: Vec<Card>,
    scroll: Scroll,
    plus: RectButton,
    action: Arc<AtomicU8>,
    managing: Option<SongPack>,
    editing: Option<uuid::Uuid>,
    renaming: Option<uuid::Uuid>,
    choose: Option<ChartsView>,
    create: DRectButton,
    cancel: DRectButton,
    pending_charts: Vec<String>,
    import_waiting: bool,
    import_task: Option<Task<Result<songpacks::ImportResult>>>,
    import_warnings: Vec<String>,
    official_cover: Option<prpr::ext::SafeTexture>,
    official_task: prpr::ext::LocalTask<Result<NextScene>>,
    next_scene: Option<NextScene>,
}
impl CollectionPage {
    pub async fn new(icons: Arc<Icons>) -> Result<Self> {
        let mut page = Self {
            icons,
            cards: Vec::new(),
            scroll: Scroll::new().horizontal(),
            plus: RectButton::new(),
            action: Arc::default(),
            managing: None,
            editing: None,
            renaming: None,
            choose: None,
            create: DRectButton::new(),
            cancel: DRectButton::new(),
            pending_charts: Vec::new(),
            import_waiting: false,
            import_task: None,
            import_warnings: Vec::new(),
            official_cover: if cfg!(closed) {
                Some(crate::load_res_tex("res/chap/c1/cover").await)
            } else {
                None
            },
            official_task: None,
            next_scene: None,
        };
        page.reload()?;
        Ok(page)
    }
    fn reload(&mut self) -> Result<()> {
        self.cards = songpacks::load()?
            .into_iter()
            .map(|pack| {
                let illu = pack
                    .charts
                    .iter()
                    .find(|p| get_data().charts.iter().any(|c| &c.local_path == *p))
                    .map(|p| super::local_illustration(p.clone(), self.icons.r#abstract.clone(), false))
                    .unwrap_or_else(|| Illustration::from_done(self.icons.r#abstract.clone()));
                illu.notify();
                Card {
                    pack,
                    illu,
                    btn: RectButton::new(),
                    long_touch: LongTouchState::default(),
                }
            })
            .collect();
        if let Some(cover) = &self.official_cover {
            let name = {
                use crate::resource::L10N_LOCAL;
                crate::resource::rtl!("chap-c1").into_owned()
            };
            self.cards.insert(
                0,
                Card {
                    pack: SongPack {
                        id: uuid::Uuid::nil(),
                        name,
                        charts: Vec::new(),
                    },
                    illu: Illustration::from_done(cover.clone()),
                    btn: RectButton::new(),
                    long_touch: LongTouchState::default(),
                },
            );
        }
        Ok(())
    }
    fn finish(&mut self, name: String) -> Result<()> {
        songpacks::create(name, self.pending_charts.clone())?;
        self.pending_charts.clear();
        self.choose = None;
        self.reload()?;
        show_message("曲包已创建").ok();
        if !self.import_warnings.is_empty() {
            Dialog::plain("警告", std::mem::take(&mut self.import_warnings).join("\n")).show();
        }
        Ok(())
    }
}
impl Page for CollectionPage {
    fn label(&self) -> Cow<'static, str> {
        if self.editing.is_some() {
            "编辑曲包曲目".into()
        } else if self.choose.is_some() {
            "选择曲包谱面".into()
        } else {
            tl!("label")
        }
    }
    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        self.cancel_card_touches();
        self.reload()
    }
    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        if self.import_task.is_some() || self.official_task.is_some() {
            return true;
        }
        if self.choose.is_some() {
            self.cancel_selection();
            true
        } else {
            self.cancel_card_touches();
            false
        }
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.import_task.is_some() || self.official_task.is_some() {
            return Ok(true);
        }
        if let Some(view) = &mut self.choose {
            if self.cancel.touch(touch, s.t) {
                self.cancel_selection();
                return Ok(true);
            }
            if self.create.touch(touch, s.t) {
                self.pending_charts = songpacks::paths_for_refs(view.multi_select.as_ref().unwrap())?;
                if self.pending_charts.is_empty() {
                    show_message("请先选择谱面").warn();
                } else if let Some(id) = self.editing {
                    match songpacks::set_charts(id, self.pending_charts.clone()) {
                        Ok(()) => {
                            self.cancel_selection();
                            self.reload()?;
                            show_message("曲包曲目已保存，谱面文件保持不变").ok();
                        }
                        Err(err) => show_error(err),
                    }
                } else {
                    request_input("songpack_name", InputBox::new().default_text("新曲包"));
                }
                return Ok(true);
            }
            return view.touch(touch, s.t, s.rt);
        }
        if self.scroll.touch(touch, s.rt) {
            self.cancel_card_touches();
            return Ok(true);
        }
        if !self.scroll.contains(touch) {
            self.cancel_card_touches();
            return Ok(false);
        }
        if self.plus.touch(touch) {
            let action = self.action.clone();
            Dialog::plain("添加曲包", "从设备导入 ZIP，或选择本地谱面组成曲包")
                .buttons(vec!["取消".into(), "导入 ZIP".into(), "选择谱面".into()])
                .listener(move |_, index| {
                    if index > 0 {
                        action.store(index as u8, Ordering::Relaxed);
                    }
                    false
                })
                .show();
            return Ok(true);
        }
        for index in 0..self.cards.len() {
            let card = &mut self.cards[index];
            // A release can arrive before the next update crosses the hold threshold.
            if !card.pack.id.is_nil()
                && touch.phase == TouchPhase::Ended
                && card.btn.contains(touch.position)
                && card.btn.update_long_touch(s.rt, &mut card.long_touch)
            {
                self.open_management(index, s.rt);
                return Ok(true);
            }
            if card.btn.touch(touch) {
                card.long_touch.reset();
                if card.pack.id.is_nil() {
                    let icons = self.icons.clone();
                    let ranks = s.icons.clone();
                    let cover = card.illu.texture.1.clone();
                    self.official_task = Some(Box::pin(async move {
                        Ok(NextScene::Overlay(Box::new(crate::scene::ChapterScene::new("c1".into(), icons, ranks, cover).await?)))
                    }));
                    return Ok(true);
                }
                match crate::scene::SongScene::from_pack(&card.pack, self.icons.clone(), s.icons.clone()) {
                    Ok(scene) => self.next_scene = Some(NextScene::Overlay(Box::new(scene))),
                    Err(err) => show_error(err),
                }
                return Ok(true);
            }
            if !card.pack.id.is_nil() && card.btn.long_touch(touch, s.rt, &mut card.long_touch) {
                self.open_management(index, s.rt);
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        if prpr::scene::INPUT_CANCELLED.lock().unwrap().take_if(|id| id == "songpack_name").is_some() {
            self.pending_charts.clear();
            if !self.import_warnings.is_empty() {
                Dialog::plain("警告", std::mem::take(&mut self.import_warnings).join("\n")).show();
            }
        }
        if prpr::scene::INPUT_CANCELLED
            .lock()
            .unwrap()
            .take_if(|id| id == "songpack_rename")
            .is_some()
        {
            self.renaming = None;
        }
        if let Some(result) = self.official_task.as_mut().and_then(|f| prpr::ext::poll_future(f.as_mut())) {
            self.official_task = None;
            match result {
                Ok(scene) => self.next_scene = Some(scene),
                Err(err) => show_error(err),
            }
        }
        self.scroll.update(s.rt);
        for card in &mut self.cards {
            card.illu.settle(s.t);
        }
        if let Some(index) = self
            .cards
            .iter_mut()
            .position(|card| !card.pack.id.is_nil() && card.btn.update_long_touch(s.rt, &mut card.long_touch))
        {
            self.open_management(index, s.rt);
        }
        match self.action.swap(0, Ordering::Relaxed) {
            1 => {
                self.import_waiting = true;
                request_file("songpack_zip");
            }
            2 => {
                self.start_selection(None, s);
            }
            action @ 3..=6 => self.manage_action(action, s)?,
            _ => {}
        }
        if let Some(view) = &mut self.choose {
            view.update(s.t)?;
        }
        if let Some((id, text)) = take_input() {
            if id == "songpack_name" {
                if let Err(err) = self.finish(text) {
                    show_error(err);
                }
            } else if id == "songpack_rename" {
                if let Some(id) = self.renaming.take() {
                    match songpacks::rename(id, text) {
                        Ok(()) => {
                            self.reload()?;
                            show_message("曲包名称已更新").ok();
                        }
                        Err(err) => show_error(err),
                    }
                }
            } else {
                return_input(id, text);
            }
        }
        if self.import_waiting {
            if let Some((id, path)) = take_file() {
                if id == "songpack_zip" {
                    self.import_waiting = false;
                    if !path.is_empty() {
                        self.import_task = Some(Task::new(songpacks::import(path.into())));
                    }
                } else {
                    return_file(id, path);
                }
            }
        }
        let result = self.import_task.as_mut().and_then(Task::take);
        if let Some(result) = result {
            self.import_task = None;
            match result {
                Err(err) => show_error(err),
                Ok(songpacks::ImportResult { name, charts, warnings }) => {
                    self.import_warnings = warnings;
                    self.pending_charts = charts.iter().map(|c| c.local_path.clone()).collect();
                    get_data_mut().charts.extend(charts);
                    save_data()?;
                    NEED_UPDATE.store(true, Ordering::Relaxed);
                    request_input("songpack_name", InputBox::new().default_text(name));
                }
            }
        }
        Ok(())
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui| {
            let top = ui.top;
            if let Some(view) = &mut self.choose {
                view.render(ui, Rect::new(-0.95, -top + 0.18, 1.9, top * 2. - 0.37), t);
                self.cancel
                    .render_text(ui, Rect::new(0.35, top - 0.14, 0.26, 0.09), t, "取消", 0.45, false);
                let n = view.multi_select.as_ref().map_or(0, Vec::len);
                self.create.render_text(
                    ui,
                    Rect::new(0.64, top - 0.14, 0.31, 0.09),
                    t,
                    format!("{} ({n})", if self.editing.is_some() { "保存曲目" } else { "创建曲包" }),
                    0.45,
                    false,
                );
                return;
            }
            let area = Rect::new(-0.94, -top + 0.20, 1.88, top * 2. - 0.26);
            let height = (area.h - 0.08).min(0.72);
            let width = height * 0.72;
            let step = width + 0.055;
            ui.scope(|ui| {
                ui.dx(area.x);
                ui.dy(area.y);
                self.scroll.size((area.w, area.h));
                self.scroll.render(ui, |ui| {
                    let y = (area.h - height) / 2.;
                    for (index, card) in self.cards.iter_mut().enumerate() {
                        let r = Rect::new(index as f32 * step, y, width, height);
                        card.btn.set(ui, r);
                        ui.fill_path(&r.rounded(0.008), card.illu.shading(r, t));
                        ui.fill_path(&r.rounded(0.008), (semi_black(0.05), (r.x, r.y), semi_black(0.65), (r.x, r.bottom())));
                        ui.text(&card.pack.name)
                            .pos(r.x + 0.025, r.y + 0.025)
                            .size(0.7)
                            .max_width(r.w - 0.05)
                            .draw_using(&prpr::core::BOLD_FONT);
                        ui.text(if card.pack.id.is_nil() {
                            "官方合集".into()
                        } else {
                            format!("{} 首谱面", card.pack.charts.len())
                        })
                        .pos(r.x + 0.025, r.bottom() - 0.03)
                        .anchor(0., 1.)
                        .size(0.4)
                        .color(semi_white(0.8))
                        .draw();
                    }
                    let r = Rect::new(self.cards.len() as f32 * step, y, width, height);
                    self.plus.set(ui, r);
                    ui.fill_path(&r.rounded(0.008), semi_white(0.22));
                    let icon = Rect::new(r.center().x - 0.055, r.center().y - 0.055, 0.11, 0.11);
                    ui.fill_rect(icon, (*self.icons.plus, icon, ScaleType::Fit));
                    ((self.cards.len() + 1) as f32 * step - 0.055, area.h)
                });
            });
        });
        Ok(())
    }
    fn render_top(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        if self.import_task.is_some() {
            ui.full_loading("正在导入曲包", s.t);
        }
        Ok(())
    }
    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        self.next_scene.take().unwrap_or_default()
    }
    fn next_page(&mut self) -> NextPage {
        NextPage::None
    }
}
