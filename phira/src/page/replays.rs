//! Replay management shared by Settings and the per-chart local board.
mod layout;
#[cfg(all(test, target_os = "windows"))]
mod tests;
use super::{Page, SharedState};
use crate::replay::{
    self,
    library::{self, Item, Match},
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, LocalTask},
    scene::{request_file, return_file, show_error, show_message, take_file, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, RectButton, Scroll, Ui},
};
use std::{borrow::Cow, path::PathBuf};

pub struct ReplayManager {
    standalone: bool,
    key: Option<String>,
    items: Vec<Item>,
    library_revision: u64,
    filter: usize,
    query: String,
    search_clear: RectButton,
    search_icons: Option<(prpr::ext::SafeTexture,prpr::ext::SafeTexture)>,
    sort: usize,
    multi: bool,
    chosen: std::collections::HashSet<PathBuf>,
    selected: Option<PathBuf>,
    rows: Vec<RectButton>,
    scroll: Scroll,
    buttons: Vec<DRectButton>,
    status: String,
    choosing_chart: bool,
    import_waiting: bool,
    task: LocalTask<Result<(PathBuf, Match)>>,
    play_task: LocalTask<Result<crate::scene::replay::ReplayScene>>,
    download_task: LocalTask<Result<crate::scene::SongScene>>,
    waiting_download: bool,
    archive: Option<crate::deeplink::DeepLinkDownload>,
    install_task: LocalTask<Result<crate::data::LocalChart>>,
    next: Option<NextScene>,
    exit: bool,
}
impl ReplayManager {
    pub fn new(key: Option<String>, standalone: bool) -> Self {
        let mut manager = Self {
            standalone,
            key,
            items: Vec::new(),
            library_revision: library::revision(),
            filter: 0,
            query: String::new(),
            search_clear: RectButton::new(),
            search_icons: None,
            sort: 0,
            multi: false,
            chosen: Default::default(),
            selected: None,
            rows: Vec::new(),
            scroll: Scroll::new(),
            buttons: (0..21).map(|_| DRectButton::new()).collect(),
            status: String::new(),
            choosing_chart: false,
            import_waiting: false,
            task: None,
            play_task: None,
            download_task: None,
            waiting_download: false,
            archive: None,
            install_task: None,
            next: None,
            exit: false,
        };
        manager.reload();
        manager
    }
    fn reload(&mut self) {
        match library::all() {
            Ok(items) => self.items = items,
            Err(err) => show_error(err),
        }
        self.rows.clear();
        self.library_revision = library::revision();
        if self
            .selected
            .as_ref()
            .is_some_and(|path| !self.items.iter().any(|item| &item.path == path))
        {
            self.selected = None;
            self.choosing_chart = false;
        }
    }
    fn visible(&self) -> Vec<Item> {
        let mut out: Vec<_> = self
            .items
            .iter()
            .filter(|item| {
                let q = self.query.to_lowercase();
                format!(
                    "{} {} {} {} {}",
                    item.meta.name,
                    item.player(),
                    item.mode,
                    item.record.as_ref().map(|r| r.time_text()).unwrap_or_default(),
                    item.speed.map(|s| format!("{s:.2}x")).unwrap_or_default()
                )
                .to_lowercase()
                .contains(&q)
            })
            .filter(|item| match self.filter {
                1 => !item.imported(),
                2 => item.imported(),
                3 => item.imported() && item.binding.as_ref().is_none_or(|b| b.local_path.is_none()),
                _ => true,
            })
            .filter(|item| {
                self.key.as_ref().is_none_or(|key| {
                    item.record.as_ref().is_some_and(|r| &r.key == key)
                        || item.binding.as_ref().and_then(|b| b.local_path.as_ref()).is_some_and(|p| {
                            crate::replay::key_for(crate::get_data().charts.iter().find(|c| &c.local_path == p).and_then(|c| c.info.id), Some(p))
                                == *key
                        })
                        || self.selected.as_ref() == Some(&item.path)
                })
            })
            .cloned()
            .collect();
        match self.sort {
            0 => out.sort_by_key(|i| std::cmp::Reverse(i.meta.recorded_at.unwrap_or(0))),
            1 => out.sort_by_key(|i| i.meta.name.clone()),
            2 => out.sort_by_key(|i| std::cmp::Reverse(std::fs::metadata(&i.path).map(|m| m.len()).unwrap_or(0))),
            _ => out.sort_by_key(|i| !i.binding.as_ref().is_some_and(|b| b.keep)),
        }
        out
    }
    fn selected_item(&self) -> Option<Item> {
        self.items.iter().find(|i| Some(&i.path) == self.selected.as_ref()).cloned()
    }
    fn reenter(&mut self) {
        self.reload();
        if self.waiting_download {
            self.waiting_download = false;
            if let Some(item) = self.selected_item() {
                self.find_chart(item.path, item.binding.and_then(|b| b.local_path));
            }
        }
    }
    fn find_chart(&mut self, path: PathBuf, preferred: Option<String>) {
        let candidates = crate::get_data().charts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>();
        self.status = "正在校验谱面…".into();
        self.task = Some(Box::pin(async move {
            let replay = replay::load(&path)?;
            let result = library::match_chart(&replay, preferred.as_deref(), &candidates).await?;
            Ok((path, result))
        }));
    }
    fn update_inner(&mut self, t: f32) {
        if self.library_revision != library::revision() {
            self.reload();
        }
        self.scroll.update(t);
        if let Some((id, text)) = prpr::scene::take_input() {
            if id == "replay_search" {
                self.query = text;
                self.scroll.y_scroller.reset();
            } else {
                prpr::scene::return_input(id, text);
            }
        }
        self.chosen.retain(|p| self.items.iter().any(|i| &i.path == p));
        replay::poll_export();
        if self.import_waiting {
            if let Some((id, file)) = take_file() {
                if id == "replay_import" {
                    self.import_waiting = false;
                    if !file.is_empty() {
                        match library::import_package(std::path::Path::new(&file)) {
                            Ok(result) => {
                                let added = result.iter().any(|(_, a)| *a);
                                let path = result[0].0.clone();
                                self.selected = Some(path.clone());
                                self.reload();
                                if !added {
                                    show_message("此回放已在管理列表中");
                                }
                                self.find_chart(path, None);
                            }
                            Err(err) => show_error(err.context("导入回放失败")),
                        }
                    }
                } else {
                    return_file(id, file);
                }
            }
        }
        if let Some(task) = &mut self.task {
            if let Some(result) = poll_future(task.as_mut()) {
                self.task = None;
                match result {
                    Ok((path, Match::Found(local, verified))) => {
                        if self.items.iter().any(|i| i.path == path) {
                            if let Err(err) = library::bind(&path, &local, verified) {
                                show_error(err);
                                return;
                            }
                        }
                        self.status = if verified {
                            "谱面内容校验一致"
                        } else {
                            "旧回放未记录内容指纹；关联未校验"
                        }
                        .into();
                        self.reload();
                        self.play_task = Some(Box::pin(async move { crate::scene::replay::ReplayScene::new_bound(path, local, verified).await }));
                    }
                    Ok((_, Match::Missing)) => self.status = "缺少所需谱面。回放已暂存；请选择本地谱面，或前往下载在线谱面。".into(),
                    Ok((_, Match::WrongVersion)) => self.status = "谱面版本不一致。请导入录制时的谱面版本后重新关联。".into(),
                    Ok((_, Match::WrongAudio)) => self.status = "谱面内容一致，但音乐版本不同。请使用录制时的音乐文件。".into(),
                    Ok((_, Match::WrongServer)) => self.status = "回放来自其他谱面服务器。请切换至对应服务器后重新关联。".into(),
                    Err(err) => {
                        self.status = "打开回放失败".into();
                        show_error(err);
                    }
                }
            }
        }
        if let Some(task) = &mut self.play_task {
            if let Some(result) = poll_future(task.as_mut()) {
                self.play_task = None;
                match result {
                    Ok(scene) => self.next = Some(NextScene::Overlay(Box::new(scene))),
                    Err(err) => show_error(err),
                }
            }
        }
        if let Some(task) = &mut self.download_task {
            if let Some(result) = poll_future(task.as_mut()) {
                self.download_task = None;
                match result {
                    Ok(scene) => {
                        self.waiting_download = true;
                        self.next = Some(NextScene::Overlay(Box::new(scene)));
                    }
                    Err(err) => show_error(err.context("打开在线谱面失败")),
                }
            }
        }
        if let Some(result) = self.archive.as_mut().and_then(|a| a.take_result()) {
            self.archive = None;
            match result {
                Ok(file) => {
                    if let Some(path) = self.selected.clone() {
                        self.install_task = Some(Box::pin(async move {
                            let replay = replay::load(&path)?;
                            library::install_archived(&replay, file).await
                        }));
                    }
                }
                Err(err) => show_error(err.context("下载录制版本失败")),
            }
        }
        if let Some(task) = &mut self.install_task {
            if let Some(result) = poll_future(task.as_mut()) {
                self.install_task = None;
                match result {
                    Ok(chart) => {
                        let local = chart.local_path.clone();
                        crate::get_data_mut().charts.push(chart);
                        if let Err(err) = crate::save_data() {
                            crate::get_data_mut().charts.pop();
                            show_error(err);
                            return;
                        }
                        crate::charts_view::NEED_UPDATE.store(true, std::sync::atomic::Ordering::Relaxed);
                        if let Some(path) = self.selected.clone() {
                            self.find_chart(path, Some(local));
                        }
                    }
                    Err(err) => show_error(err.context("录制版本校验失败")),
                }
            }
        }
    }
    fn touch_inner(&mut self, touch: &Touch, t: f32) -> Result<bool> {
        if let Some(archive) = &mut self.archive {
            if archive.touch(touch, t) {
                self.archive = None;
            }
            return Ok(true);
        }
        if self.task.is_some() || self.play_task.is_some() || self.download_task.is_some() || self.install_task.is_some() {
            return Ok(true);
        }
        if !self.query.is_empty() && self.search_clear.touch(touch) {self.query.clear();self.scroll.y_scroller.reset();return Ok(true);}
        let fixed_control=[0,1,2,3,4,5,6,13,14,15,16,17,18,20].iter().any(|&i|self.buttons[i].inner.contains(touch.position));
        if !fixed_control && self.scroll.touch(touch, t) {
            for row in &mut self.rows {
                row.cancel();
            }
            for button in &mut self.buttons {
                button.cancel(t);
            }
            return Ok(true);
        }
        if !self.scroll.contains(touch) && !fixed_control {
            return Ok(false);
        }
        if self.buttons[20].touch(touch, t) {
            self.next = Some(NextScene::Overlay(Box::new(super::replay_analysis::ReplayAnalysisScene::new(self.selected.clone())?)));
            return Ok(true);
        }
        if self.buttons[13].touch(touch, t) {
            prpr::scene::request_input("replay_search", inputbox::InputBox::new().default_text(&self.query));
            return Ok(true);
        }
        if self.buttons[14].touch(touch, t) {
            self.sort = (self.sort + 1) % 4;
            self.scroll.y_scroller.reset();
            return Ok(true);
        }
        if self.buttons[15].touch(touch, t) {
            self.multi ^= true;
            self.chosen.clear();
            self.selected = None;
            self.choosing_chart = false;
            return Ok(true);
        }
        if self.buttons[16].touch(touch, t) {
            self.chosen = self.visible().iter().map(|i| i.path.clone()).collect();
            return Ok(true);
        }
        if self.buttons[17].touch(touch, t) {
            let paths: Vec<_> = self
                .items
                .iter()
                .filter(|i| self.chosen.contains(&i.path) && !i.binding.as_ref().is_some_and(|b| b.keep))
                .map(|i| i.path.clone())
                .collect();
            prpr::ui::Dialog::simple(format!("删除选中的 {} 个回放？重要回放会保留，成绩历史不受影响。", paths.len()))
                .buttons(vec!["取消".into(), "删除".into()])
                .listener(move |_, pos| {
                    if pos == 1 {
                        for p in &paths {
                            if let Err(e) = library::delete(p) {
                                show_error(e);
                                break;
                            }
                        }
                    }
                    false
                })
                .show();
            return Ok(true);
        }
        if self.buttons[18].touch(touch, t) {
            let paths = self.chosen.iter().cloned().collect::<Vec<_>>();
            if !paths.is_empty() {
                replay::export_batch(&paths)?;
            }
            return Ok(true);
        }
        if self.buttons[19].touch(touch, t) {
            if let Some(item) = self.selected_item() {
                library::set_keep(&item.path, !item.binding.as_ref().is_some_and(|b| b.keep))?;
                self.reload();
            }
            return Ok(true);
        }
        if self.buttons[0].touch(touch, t) {
            self.exit = true;
            return Ok(true);
        }
        if self.buttons[1].touch(touch, t) {
            self.import_waiting = true;
            request_file("replay_import");
            return Ok(true);
        }
        if self.buttons[2].touch(touch, t) {
            self.reload();
            return Ok(true);
        }
        for i in 0..4 {
            if self.buttons[i + 3].touch(touch, t) {
                self.filter = i;
                self.choosing_chart = false;
                self.scroll.y_scroller.reset();
                return Ok(true);
            }
        }
        if let Some(item) = self.selected_item() {
            if self.buttons[12].touch(touch, t) {
                let replay = replay::load(&item.path)?;
                if let Some(url) = replay.chart_file {
                    let url: reqwest::Url = url.parse()?;
                    anyhow::ensure!(matches!(url.scheme(), "http" | "https"), "录制版本下载地址无效");
                    self.archive = Some(crate::deeplink::start_deeplink_download(crate::deeplink::DeepLinkTarget { url, official: false })?);
                } else {
                    show_message("此回放没有记录可下载的历史版本地址").warn();
                }
                return Ok(true);
            }
            if self.buttons[7].touch(touch, t) {
                self.find_chart(item.path, item.binding.and_then(|b| b.local_path));
                return Ok(true);
            }
            if self.buttons[8].touch(touch, t) {
                replay::request_export(&item.path)?;
                return Ok(true);
            }
            if self.buttons[9].touch(touch, t) {
                let path = item.path.clone();
                // Use the existing confirmation dialog; delete only this tape.
                prpr::ui::Dialog::simple("删除此回放文件？成绩历史将保留。")
                    .buttons(vec!["取消".into(), "删除".into()])
                    .listener(move |_, pos| {
                        if pos == 1 {
                            if let Err(err) = library::delete(&path) {
                                show_error(err);
                            }
                        }
                        false
                    })
                    .show();
                return Ok(true);
            }
            if self.buttons[10].touch(touch, t) {
                self.choosing_chart = !self.choosing_chart;
                self.rows.clear();
                self.scroll.y_scroller.reset();
                return Ok(true);
            }
            if self.buttons[11].touch(touch, t) {
                let replay = replay::load(&item.path)?;
                if let Some(replay::ChartRef::Id(id)) = replay.chart {
                    if replay
                        .meta
                        .server
                        .as_ref()
                        .is_some_and(|s| s.trim_end_matches('/') != crate::client::api_url().trim_end_matches('/'))
                    {
                        show_message("请先在设置中切换到回放所属的谱面服务器").warn();
                    } else {
                        self.download_task = Some(Box::pin(async move {
                            let chart = crate::client::Ptr::<crate::client::Chart>::new(id).fetch().await?;
                            let icons = std::sync::Arc::new(crate::icons::Icons::new().await?);
                            let ranks = prpr::core::Resource::load_icons().await?;
                            let local = crate::get_data()
                                .charts
                                .iter()
                                .find(|c| c.info.id == Some(id))
                                .map(|c| c.local_path.clone());
                            Ok(crate::scene::SongScene::new(super::ChartItem::from_remote(&chart), local, icons, ranks, Default::default()))
                        }));
                    }
                } else {
                    show_message("这是本地谱面回放，请导入对应谱面后关联").warn();
                }
                return Ok(true);
            }
        }
        let charts = crate::get_data().charts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>();
        let items = self.visible();
        for (i, row) in self.rows.iter_mut().enumerate() {
            if row.touch(touch) {
                if self.choosing_chart {
                    if let (Some(path), Some(local)) = (self.selected.clone(), charts.get(i)) {
                        let preferred = local.clone();
                        self.choosing_chart = false;
                        self.find_chart(path, Some(preferred));
                    }
                } else if let Some(item) = items.get(i) {
                    if self.multi {
                        if !self.chosen.remove(&item.path) {
                            self.chosen.insert(item.path.clone());
                        }
                        self.selected = Some(item.path.clone());
                    } else if self.selected.as_ref() == Some(&item.path) {
                        self.selected = None;
                        self.choosing_chart = false;
                    } else {
                        self.selected = Some(item.path.clone());
                        self.scroll.y_scroller.goto = Some(layout::entry_top(i, items.len(), self.scroll.rect().h));
                    }
                    self.status.clear();
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn take_next(&mut self) -> NextScene {
        self.next
            .take()
            .unwrap_or_else(|| if self.exit { NextScene::Pop } else { NextScene::None })
    }
}
impl Page for ReplayManager {
    fn label(&self) -> Cow<'static, str> {
        "回放文件".into()
    }
    fn enter(&mut self, _: &mut SharedState) -> Result<()> {
        self.reenter();
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        self.reenter();
        Ok(())
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.update_inner(s.t);
        Ok(())
    }
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        self.touch_inner(touch, s.t)
    }
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui| self.render_inner(ui, t));
        Ok(())
    }
    fn next_scene(&mut self, _: &mut SharedState) -> NextScene {
        self.take_next()
    }
}
impl Scene for ReplayManager {
    fn enter(&mut self, _: &mut TimeManager, _: Option<RenderTarget>) -> Result<()> {
        self.reenter();
        Ok(())
    }
    fn resume(&mut self, _: &mut TimeManager) -> Result<()> {
        self.reenter();
        Ok(())
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.update_inner(tm.real_time() as f32);
        Ok(())
    }
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        self.touch_inner(touch, tm.real_time() as f32)
    }
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        clear_background(Color::new(0.10, 0.13, 0.18, 1.));
        self.render_inner(ui, tm.real_time() as f32);
        Ok(())
    }
    fn next_scene(&mut self, _: &mut TimeManager) -> NextScene {
        self.take_next()
    }
}

#[cfg(all(test, target_os = "windows"))]
pub(crate) fn render_regression(painter: &mut prpr::ui::TextPainter, path: &std::path::Path, state: &mut SharedState) {
    let mut manager = ReplayManager::new(None, true);
    for (w, h, label) in [(960, 720, "4x3"), (1280, 720, "16x9"), (1280, 548, "21x9")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        set_camera(&ui.camera());
        clear_background(Color::new(0.10, 0.13, 0.18, 1.));
        manager.selected = None;
        manager.render_inner(&mut ui, 2.);
        for i in [13, 14, 15] {
            assert!(
                (-100..=100).any(|x| (-75..=75).any(|y| manager.buttons[i].inner.contains(vec2(x as f32 / 100., y as f32 / 100.)))),
                "list-level button {i} must work before selecting a record"
            );
        }
        manager.selected = Some(path.to_path_buf());
        manager.render_inner(&mut ui, 2.);
        assert!(manager.selected_item().is_some());
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; (w * h * 4) as usize];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        Image {
            width: w as u16,
            height: h as u16,
            bytes,
        }
        .export_png(&format!("target/replay-qa/manager-{label}.png"));
    }
    manager.standalone = false;
    for (time, name) in [(2., "settings-open"), (2.35, "settings-leaving"), (2.71, "settings-closed")] {
        state.fader.back(2.);
        state.fader.sub = true;
        state.fader.reset();
        state.t = time;
        let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
        set_camera(&ui.camera());
        clear_background(BLACK);
        Page::render(&mut manager, &mut ui, state).unwrap();
        unsafe { get_internal_gl() }.flush();
        let mut bytes = vec![0; 1280 * 720 * 4];
        unsafe {
            use miniquad::gl::*;
            glReadPixels(0, 0, 1280, 720, GL_RGBA, GL_UNSIGNED_BYTE, bytes.as_mut_ptr() as _);
            assert_eq!(glGetError(), 0);
        }
        if name == "settings-closed" {
            assert!(bytes.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]), "replay buttons remain after page fade-out");
        }
        Image {
            width: 1280,
            height: 720,
            bytes,
        }
        .export_png(&format!("target/replay-qa/manager-{name}.png"));
    }
    state.fader = super::Fader::new();
    let mut copied = replay::load(path).unwrap();
    copied.meta.id.push_str("-delete-check");
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("delete-check.phirar");
    replay::save(&copied, &source).unwrap();
    let (deletable, _) = library::import(&source).unwrap();
    manager.reload();
    manager.selected = Some(deletable.clone());
    library::delete(&deletable).unwrap();
    manager.update_inner(3.);
    assert!(manager.selected.is_none());
    assert!(!manager.items.iter().any(|item| item.path == deletable));
}
