//! Read-only practice analysis and two independent replay renderers on one chart clock.
use crate::{
    replay::{self, library::Item},
    scene::{fs_from_path, replay::ReplayScene},
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt, ScaleType},
    practice::tools::{Event, Note, Request, Selection},
    scene::{NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, RectButton, Scroll, Ui},
};
#[cfg(all(test, record, target_os = "windows"))]
pub(crate) mod qa;

pub fn open(request: Request) -> Box<dyn Scene> {
    Box::new(AnalysisScene::new(request))
}
pub struct AnalysisScene {
    request: Request,
    buttons: Vec<DRectButton>,
    rows: Vec<RectButton>,
    scroll: Scroll,
    next: Option<NextScene>,
    library: Vec<Item>,
    local: Option<String>,
    status: String,
    library_task: LocalTask<Result<(String, Vec<Item>)>>,
    load_task: LocalTask<Result<(usize, ReplayScene, String)>>,
    panes: [Option<ReplayScene>; 2],
    names: [String; 2],
    targets: [Option<RenderTarget>; 2],
    choosing: Option<usize>,
    current: f64,
    playing: bool,
    last: f64,
    rate: usize,
    fingers: bool,
    source: usize,
    note: usize,
    graph: Rect,
    range: (f64, f64),
    drag: Option<(u64, f64)>,
    timeline: Rect,
    seek_drag: Option<u64>,
}
impl AnalysisScene {
    fn replay_time(item: &Item) -> String {
        item.meta
            .recorded_at
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| item.path.file_name().unwrap_or_default().to_string_lossy().into_owned())
    }
    fn new(mut request: Request) -> Self {
        request
            .notes
            .sort_by(|a, b| a.time.total_cmp(&b.time).then(a.line.cmp(&b.line)).then(a.index.cmp(&b.index)));
        let current = request.now.max(0.);
        let duration = request.duration.max(0.1);
        let local = request.local_path.clone();
        let key = request.key.clone();
        let id = request.info.id;
        let candidates: Vec<_> = crate::get_data().charts.iter().map(|c| c.local_path.clone()).collect();
        let library_task = Some(Box::pin(async move {
            use sha2::{Digest, Sha256};
            let paths = local.into_iter().chain(candidates.into_iter()).collect::<Vec<_>>();
            let mut found = None;
            for path in paths {
                let mut fs = match fs_from_path(&path) {
                    Ok(fs) => fs,
                    Err(_) => continue,
                };
                let info = match prpr::fs::load_info(fs.as_mut()).await {
                    Ok(i) => i,
                    Err(_) => continue,
                };
                let bytes = GameBytes::read(fs.as_mut(), &info).await?;
                if format!("{:x}", Sha256::digest(&bytes)) == key {
                    found = Some((path, fs, info));
                    break;
                }
            }
            let (path, mut fs, info) = found.context("未找到当前版本的练习谱面，请重新导入后进入练习")?;
            let fingerprint = replay::library::fingerprint(fs.as_mut(), &info).await?;
            let audio = replay::library::audio_fingerprint(fs.as_mut(), &info).await?;
            let record_key = replay::key_for(id, Some(&path));
            let items = replay::library::all()?
                .into_iter()
                .filter(|i| {
                    if let Some(f) = &i.meta.fingerprint {
                        f == &fingerprint && i.meta.audio_fingerprint.as_ref().is_none_or(|a| a == &audio)
                    } else {
                        i.binding.as_ref().and_then(|b| b.local_path.as_ref()) == Some(&path)
                            || i.record.as_ref().is_some_and(|r| r.key == record_key)
                    }
                })
                .collect();
            Ok((path, items))
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(String, Vec<Item>)>>>>);
        Self {
            request,
            buttons: (0..14).map(|_| DRectButton::new()).collect(),
            rows: Vec::new(),
            scroll: Scroll::new(),
            next: None,
            library: Vec::new(),
            local: None,
            status: String::new(),
            library_task,
            load_task: None,
            panes: std::array::from_fn(|_| None),
            names: ["选择左侧回放".into(), "选择右侧回放".into()],
            targets: [None, None],
            choosing: None,
            current,
            playing: false,
            last: 0.,
            rate: 1,
            fingers: true,
            source: 0,
            note: 0,
            graph: Rect::default(),
            range: (0., duration),
            drag: None,
            timeline: Rect::default(),
            seek_drag: None,
        }
    }
    fn events(&self) -> Vec<Event> {
        if self.source == 0 {
            self.request.events.clone()
        } else {
            self.panes[self.source - 1].as_ref().map(|p| p.analysis_events()).unwrap_or_default()
        }
    }
    fn select_replay(&mut self, index: usize) -> Result<()> {
        let Some(side) = self.choosing.take() else {
            return Ok(());
        };
        let item = self.library.get(index).context("回放已被移除")?.clone();
        let local = self.local.clone().context("谱面尚未校验完成")?;
        self.playing = false;
        let title = format!(
            "{} · {} · {}",
            item.player(),
            item.record
                .as_ref()
                .map(|r| format!("{} / {:.2}%", r.score, r.accuracy * 100.))
                .unwrap_or_else(|| "成绩未记录".into()),
            Self::replay_time(&item)
        );
        self.load_task = Some(Box::pin(async move {
            let scene = ReplayScene::new_bound(item.path, local, item.meta.fingerprint.is_some()).await?;
            Ok((side, scene, title))
        }));
        Ok(())
    }
    fn seek(&mut self, time: f64) {
        self.current = time.clamp(0., self.request.duration);
    }
    fn selected_event(&self) -> Option<Event> {
        let n = self.request.notes.get(self.note)?;
        self.events().into_iter().rev().find(|e| e.line == n.line && e.index == n.index)
    }
    fn density(&self) -> Vec<[u32; 4]> {
        let count = (self.request.duration.ceil() as usize).clamp(1, 240);
        let mut bins = vec![[0; 4]; count];
        for n in &self.request.notes {
            let i = ((n.time.max(0.) / self.request.duration.max(0.1)) * count as f64) as usize;
            bins[i.min(count - 1)][n.kind.min(3) as usize] += 1;
        }
        bins
    }
}
struct GameBytes;
impl GameBytes {
    async fn read(fs: &mut dyn prpr::fs::FileSystem, info: &prpr::info::ChartInfo) -> Result<Vec<u8>> {
        prpr::scene::GameScene::load_chart_bytes(fs, info).await
    }
}
impl Scene for AnalysisScene {
    fn enter(&mut self, tm: &mut TimeManager, _: Option<RenderTarget>) -> Result<()> {
        self.last = tm.real_time();
        Ok(())
    }
    fn pause(&mut self, _: &mut TimeManager) -> Result<()> {
        for p in self.panes.iter_mut().flatten() {
            p.analysis_pause();
        }
        Ok(())
    }
    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.last = tm.real_time();
        Ok(())
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(result) = self.library_task.as_mut().and_then(|t| poll_future(t.as_mut())) {
            self.library_task = None;
            match result {
                Ok((path, items)) => {
                    self.local = Some(path);
                    self.library = items;
                }
                Err(e) => self.status = format!("{e:#}"),
            }
        }
        if let Some(result) = self.load_task.as_mut().and_then(|t| poll_future(t.as_mut())) {
            self.load_task = None;
            match result {
                Ok((side, scene, title)) => {
                    self.panes[side] = Some(scene);
                    self.names[side] = title;
                    self.status.clear();
                }
                Err(e) => self.status = format!("{e:#}"),
            }
        }
        let dt = (tm.real_time() - self.last).clamp(0., 0.25);
        self.last = tm.real_time();
        self.scroll.update(self.last as f32);
        if self.playing {
            self.seek(self.current + dt * [0.5, 1., 1.5, 2.][self.rate]);
            if self.current >= self.request.duration {
                self.playing = false;
            }
        }
        let left = self.panes[0].is_some();
        for (side, pane) in self.panes.iter_mut().enumerate() {
            if let Some(p) = pane {
                p.analysis_tick(self.current, self.playing, [0.5, 1., 1.5, 2.][self.rate] as f32, self.fingers, side == 0 || !left)?;
            }
        }
        Ok(())
    }
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if self.seek_drag == Some(touch.id) {
            self.seek(((touch.position.x - self.timeline.x) / self.timeline.w.max(0.001)).clamp(0., 1.) as f64 * self.request.duration);
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.seek_drag = None;
            }
            return Ok(true);
        }
        if let Some((id, start)) = self.drag {
            if touch.id == id {
                let now = ((touch.position.x - self.graph.x) / self.graph.w).clamp(0., 1.) as f64 * self.request.duration;
                self.range = (start.min(now), start.max(now));
                if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    self.drag = None;
                }
                return Ok(true);
            }
        }
        for (i, b) in self.buttons.iter_mut().enumerate() {
            if b.touch(touch, t) {
                match i {
                    0 => self.next = Some(NextScene::Pop),
                    1 => self.playing ^= true,
                    2 => self.seek(self.current - 5.),
                    3 => self.seek(self.current + 5.),
                    4 => self.rate = (self.rate + 1) % 4,
                    5 => self.fingers ^= true,
                    6 | 7 => {
                        self.choosing = Some(i - 6);
                        self.playing = false;
                        self.scroll.y_scroller.reset();
                    }
                    8 => {
                        self.source = (self.source + 1) % 3;
                    }
                    9 => {
                        let start = self.range.0;
                        let mut end = self.range.1.max(start + 0.05);
                        for n in &self.request.notes {
                            if n.time >= start && n.time <= end {
                                end = end.max(n.end);
                            }
                        }
                        self.next = Some(NextScene::PopWithResult(Box::new(Selection { start, end })));
                    }
                    10 => self.note = self.note.saturating_sub(1),
                    11 => self.note = (self.note + 1).min(self.request.notes.len().saturating_sub(1)),
                    12 => {
                        self.choosing = None;
                    }
                    13 => {
                        if let Some(n) = self.request.notes.get(self.note) {
                            self.seek((n.time - 1.).max(0.));
                        }
                    }
                    _ => {}
                }
                return Ok(true);
            }
        }
        if touch.phase == TouchPhase::Started && self.timeline.contains(touch.position) {
            self.playing = false;
            self.seek_drag = Some(touch.id);
            self.seek(((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.) as f64 * self.request.duration);
            return Ok(true);
        }
        if self.request.tool == 0 && touch.phase == TouchPhase::Started && self.graph.contains(touch.position) {
            let start = ((touch.position.x - self.graph.x) / self.graph.w).clamp(0., 1.) as f64 * self.request.duration;
            self.drag = Some((touch.id, start));
            self.range = (start, start);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            for b in &mut self.rows {
                b.cancel();
            }
            return Ok(true);
        }
        if self.scroll.contains(touch) {
            for (i, b) in self.rows.iter_mut().enumerate() {
                if b.touch(touch) {
                    if self.choosing.is_some() {
                        self.select_replay(i)?;
                    } else if self.request.tool == 2 {
                        self.note = i;
                        if let Some(n) = self.request.notes.get(i) {
                            self.seek((n.time - 0.05).max(0.));
                        }
                    }
                    return Ok(true);
                }
            }
        }
        Ok(true)
    }
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        clear_background(Color::from_hex_rgb(0x192632));
        let t = tm.real_time() as f32;
        let r = ui.screen_rect().feather(-0.035);
        for b in &mut self.buttons {
            b.invalidate();
        }
        self.graph = Rect::default();
        self.timeline = Rect::default();
        self.buttons[0].render_text(ui, Rect::new(r.x, r.y, 0.18, 0.075), t, "返回", 0.38, false);
        ui.text(["密度与失误时间轴", "双回放对照", "单音符判定详情"][self.request.tool])
            .pos(r.x + 0.21, r.y + 0.035)
            .anchor(0., 0.5)
            .size(0.48)
            .draw();
        if let Some(side) = self.choosing {
            self.buttons[12].render_text(ui, Rect::new(r.right() - 0.25, r.y, 0.25, 0.075), t, "取消选择", 0.37, false);
            ui.text(format!("{}侧回放 · 只显示此谱面的回放", if side == 0 { "左" } else { "右" }))
                .pos(r.x, r.y + 0.10)
                .size(0.4)
                .draw();
            let h = r.h - 0.17;
            self.scroll.size((r.w, h));
            self.rows.resize_with(self.library.len(), RectButton::new);
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y + 0.16);
                self.scroll.render(ui, |ui| {
                    if self.library.is_empty() {
                        ui.text(if self.library_task.is_some() {
                            "正在校验谱面…"
                        } else {
                            "没有可用的同谱回放，请先录制或导入回放。"
                        })
                        .pos(0.01, 0.025)
                        .size(0.4)
                        .max_width(r.w)
                        .draw();
                    }
                    for (i, item) in self.library.iter().enumerate() {
                        let row = Rect::new(0., i as f32 * 0.105, r.w, 0.09);
                        ui.fill_path(&row.rounded(0.008), semi_black(0.3));
                        self.rows[i].set(ui, row);
                        ui.text(format!(
                            "{} · {} · {} · {}",
                            item.player(),
                            item.meta.name,
                            item.record
                                .as_ref()
                                .map(|v| format!("{:07} {:.2}%", v.score, v.accuracy * 100.))
                                .unwrap_or("成绩未记录".into()),
                            Self::replay_time(item)
                        ))
                        .pos(0.015, row.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.40)
                        .max_width(r.w - 0.03)
                        .draw();
                    }
                    (r.w, (self.library.len() as f32 * 0.105).max(h))
                });
            });
            return Ok(());
        }
        if !self.status.is_empty() {
            ui.text(&self.status)
                .pos(r.x, r.bottom() - 0.035)
                .anchor(0., 1.)
                .size(0.3)
                .max_width(r.w)
                .draw();
        }
        let y = r.y + 0.105;
        if self.request.tool == 1 {
            let pane_w = (r.w - 0.02) / 2.;
            let pane_h = (r.h * 0.52).min(pane_w * 0.65).min((r.h - 0.42).max(0.10));
            let mut views = Vec::new();
            for side in 0..2 {
                let x = r.x + side as f32 * (pane_w + 0.02);
                let rect = Rect::new(x, y + 0.08, pane_w, pane_h);
                let w = (pane_w * ui.viewport.2 as f32 / 2.).round().max(16.) as u32;
                let h = (pane_h * ui.viewport.2 as f32 / 2.).round().max(16.) as u32;
                if self.targets[side].is_none_or(|rt| rt.texture.width() != w as f32 || rt.texture.height() != h as f32) {
                    if let Some(old) = self.targets[side].take() {
                        old.delete();
                    }
                    self.targets[side] = Some(render_target(w, h));
                }
                let target = self.targets[side].unwrap();
                if let Some(p) = &mut self.panes[side] {
                    let mut pane_ui = Ui::new(ui.text_painter, Some((0, 0, w as i32, h as i32)));
                    p.analysis_render(tm, &mut pane_ui, target)?;
                }
                views.push((rect, target, self.panes[side].is_some()));
            }
            set_camera(&ui.camera());
            for side in 0..2 {
                self.buttons[6 + side].render_text(
                    ui,
                    Rect::new(r.x + side as f32 * (pane_w + 0.02), y, pane_w, 0.065),
                    t,
                    &self.names[side],
                    0.33,
                    false,
                );
            }
            for (rect, target, loaded) in views {
                ui.fill_path(&rect.rounded(0.008), semi_black(0.5));
                if loaded {
                    {
                        let shading = prpr::ui::IntoShading::into_shading((target.texture, rect, ScaleType::Fit)).flip_y();
                        ui.fill_rect(rect, shading);
                    }
                } else {
                    ui.text("点击上方选择回放")
                        .pos(rect.center().x, rect.center().y)
                        .anchor(0.5, 0.5)
                        .size(0.4)
                        .draw();
                }
            }
            let cy = y + 0.10 + pane_h;
            let width = (r.w - 4. * 0.01) / 5.;
            for (i, label) in [
                if self.playing { "暂停" } else { "播放" },
                "−5s",
                "+5s",
                ["0.50×", "1.00×", "1.50×", "2.00×"][self.rate],
                if self.fingers { "触点：开" } else { "触点：关" },
            ]
            .iter()
            .enumerate()
            {
                self.buttons[i + 1].render_text(ui, Rect::new(r.x + i as f32 * (width + 0.01), cy, width, 0.07), t, *label, 0.34, false);
            }
            self.timeline = Rect::new(r.x, cy + 0.09, r.w, 0.06);
            let line = Rect::new(r.x, cy + 0.115, r.w, 0.012);
            ui.fill_path(&line.rounded(0.006), semi_white(0.2));
            ui.fill_path(&Rect::new(line.x, line.y, line.w * (self.current / self.request.duration.max(0.01)) as f32, line.h).rounded(0.006), WHITE);
            ui.text(format!(
                "{} / {} · 左侧音频 · 同一谱面时间",
                prpr::practice::format_time(self.current),
                prpr::practice::format_time(self.request.duration)
            ))
            .pos(r.x, cy + 0.17)
            .size(0.34)
            .max_width(r.w)
            .draw();
        } else if self.request.tool == 0 {
            let bins = self.density();
            let peak = bins.iter().map(|v| v.iter().sum::<u32>()).max().unwrap_or(1).max(1) as f32;
            let graph = Rect::new(r.x, y, r.w, (r.h * 0.40).max(0.14));
            self.graph = graph;
            ui.fill_path(&graph.rounded(0.01), semi_black(0.35));
            let width = graph.w / bins.len() as f32;
            for (i, bin) in bins.iter().enumerate() {
                let mut bottom = graph.bottom();
                for (k, count) in bin.iter().enumerate() {
                    let h = *count as f32 / peak * graph.h;
                    ui.fill_rect(Rect::new(graph.x + i as f32 * width, bottom - h, width, h), [SKYBLUE, GREEN, YELLOW, RED][k]);
                    bottom -= h;
                }
            }
            for e in self.events() {
                if e.grade == 2 || e.grade == 3 {
                    let x = graph.x + (e.time / self.request.duration.max(0.1)) as f32 * graph.w;
                    ui.fill_rect(Rect::new(x, graph.y, 0.003, graph.h), RED);
                }
            }
            let a = graph.x + (self.range.0 / self.request.duration) as f32 * graph.w;
            let b = graph.x + (self.range.1 / self.request.duration) as f32 * graph.w;
            ui.fill_rect(Rect::new(a, graph.y, (b - a).max(0.002), graph.h), semi_white(0.18));
            let cy = graph.bottom() + 0.035;
            self.buttons[8].render_text(
                ui,
                Rect::new(r.x, cy, r.w * 0.45, 0.075),
                t,
                format!("失误来源：{}", ["本轮练习", "左侧回放", "右侧回放"][self.source]),
                0.37,
                false,
            );
            self.buttons[9].render_text(ui, Rect::new(r.x + r.w * 0.47, cy, r.w * 0.53, 0.075), t, "将选中区间用于练习", 0.37, true);
            ui.text(format!(
                "拖选区间 {} — {} · Tap / Hold / Drag / Flick",
                prpr::practice::format_time(self.range.0),
                prpr::practice::format_time(self.range.1)
            ))
            .pos(r.x, cy + 0.10)
            .size(0.34)
            .max_width(r.w)
            .draw();
            ui.text(format!(
                "共 {} 个音符 · {} 个已记录判定 · 每格 {:.2} 秒",
                self.request.notes.len(),
                self.events().len(),
                self.request.duration / bins.len() as f64
            ))
            .pos(r.x, cy + 0.16)
            .size(0.36)
            .max_width(r.w)
            .draw();
            for (side, label) in [(0, "选择左回放作为来源"), (1, "选择右回放作为来源")] {
                self.buttons[6 + side].render_text(
                    ui,
                    Rect::new(r.x + side as f32 * r.w / 2., cy + 0.23, r.w / 2. - 0.01, 0.075),
                    t,
                    label,
                    0.35,
                    false,
                );
            }
        } else {
            self.buttons[8].render_text(
                ui,
                Rect::new(r.x, y, r.w * 0.45, 0.075),
                t,
                format!("判定来源：{}", ["本轮练习", "左侧回放", "右侧回放"][self.source]),
                0.38,
                false,
            );
            for (side, label) in [(0, "选择左回放"), (1, "选择右回放")] {
                self.buttons[6 + side].render_text(
                    ui,
                    Rect::new(r.x + r.w * (0.47 + side as f32 * 0.27), y, r.w * 0.25, 0.075),
                    t,
                    label,
                    0.34,
                    false,
                );
            }
            if let Some(n) = self.request.notes.get(self.note) {
                let event = self.selected_event();
                let text = match event {
                    Some(e) if e.grade == 3 => "MISS · 无点击误差".into(),
                    Some(e) if n.kind >= 2 => format!("{} · 非计时判定", grade(e.grade)),
                    Some(e) => format!("{} · {:+.2}ms", grade(e.grade), e.diff * 1000.),
                    None => "尚未判定 / 此来源未记录".into(),
                };
                ui.text(format!(
                    "#{} · 线 {} / 音符 {} · {} · {}\n{}",
                    self.note + 1,
                    n.line,
                    n.index,
                    ["Tap", "Hold", "Drag", "Flick"][n.kind as usize],
                    prpr::practice::format_time(n.time),
                    text
                ))
                .pos(r.x, y + 0.105)
                .size(0.44)
                .max_width(r.w)
                .multiline()
                .draw();
            }
            if let Some(n) = self.request.notes.get(self.note) {
                let events = self.events();
                let recorded = events
                    .iter()
                    .filter(|e| e.line == n.line && e.index == n.index)
                    .map(|e| {
                        format!(
                            "{} {} {}",
                            if e.head { "Hold 头部" } else { "计分" },
                            grade(e.grade),
                            if e.grade == 3 || n.kind >= 2 {
                                "无点击误差".into()
                            } else {
                                format!("{:+.2}ms", e.diff * 1000.)
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                ui.text(if recorded.is_empty() { "此来源未记录该音符" } else { &recorded })
                    .pos(r.x, y + 0.24)
                    .size(0.32)
                    .max_width(r.w)
                    .draw();
            }
            if let Some(n) = self.request.notes.get(self.note) {
                let time = self.selected_event().map_or(n.time, |e| e.time);
                let points = if self.source == 0 {
                    let mut live = std::collections::BTreeMap::new();
                    for touch in self.request.touches.iter().take_while(|t| t.time <= time + 1e-6) {
                        if touch.phase >= 2 {
                            live.remove(&touch.id);
                        } else {
                            live.insert(touch.id, (touch.x, touch.y));
                        }
                    }
                    live.into_iter().map(|(id, (x, y))| format!("ID{id} ({x:.2},{y:.2})")).collect::<Vec<_>>()
                } else {
                    self.panes[self.source - 1]
                        .as_ref()
                        .map(|p| p.analysis_contacts(time))
                        .unwrap_or_default()
                };
                ui.text(if points.is_empty() {
                    "该时刻没有已记录的按住触点".into()
                } else {
                    format!("该时刻触点（未记录与音符的直接绑定）：{}", points.join(" · "))
                })
                .pos(r.x, y + 0.29)
                .size(0.29)
                .max_width(r.w)
                .draw();
            }
            let list_y = y + 0.35;
            let h = (r.bottom() - list_y - 0.025).max(0.1);
            self.scroll.size((r.w, h));
            self.rows.resize_with(self.request.notes.len(), RectButton::new);
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(list_y);
                self.scroll.render(ui, |ui| {
                    for (i, n) in self.request.notes.iter().enumerate() {
                        let rect = Rect::new(0., i as f32 * 0.078, r.w, 0.068);
                        self.rows[i].set(ui, rect);
                        ui.fill_path(&rect.rounded(0.006), if i == self.note { semi_white(0.14) } else { semi_black(0.18) });
                        ui.text(format!(
                            "#{}  {} · {} · 线 {} / 音符 {}",
                            i + 1,
                            prpr::practice::format_time(n.time),
                            ["Tap", "Hold", "Drag", "Flick"][n.kind as usize],
                            n.line,
                            n.index
                        ))
                        .pos(0.015, rect.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.36)
                        .max_width(r.w - 0.03)
                        .draw();
                    }
                    (r.w, self.request.notes.len() as f32 * 0.078)
                });
            });
        }
        if self.load_task.is_some() {
            ui.full_loading_simple(t);
        }
        Ok(())
    }
    fn next_scene(&mut self, _: &mut TimeManager) -> NextScene {
        self.next.take().unwrap_or_default()
    }
}
fn grade(g: u8) -> &'static str {
    ["PERFECT", "GOOD", "BAD", "MISS", "PERFECT+", "GREAT", "OK", "MEH"]
        .get(g as usize)
        .copied()
        .unwrap_or("未记录")
}
impl Drop for AnalysisScene {
    fn drop(&mut self) {
        for target in self.targets.iter_mut().filter_map(Option::take) {
            target.delete();
        }
    }
}
