//! Three distinct local charts, one locked ruleset, one final result.
mod presentation;
pub(crate) mod records;
use crate::{get_data, scene::fs_from_path};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use presentation::{all_fc, badge_index, difficulty, fade, grade, score, Assets};
use prpr::{
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt, SafeTexture, ScaleType},
    info::ChartInfo,
    judge::PlayResult,
    scene::{GameMode, GameScene, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, IntoShading, Ui},
};
use std::any::Any;

/// Tell the parent menu to return to its home page after the award screen.
pub(crate) struct ReturnHome;
struct Stage {
    info: ChartInfo,
    illustration: SafeTexture,
    result: PlayResult,
}
#[derive(Clone, Copy)]
enum Destination {
    Award,
    Exit,
    Retry,
    Home,
}
pub struct CourseScene {
    paths: Vec<String>,
    stages: Vec<Stage>,
    task: LocalTask<Result<(GameScene, ChartInfo, SafeTexture, Option<Assets>)>>,
    next: Option<NextScene>,
    waiting: bool,
    aborted: bool,
    buttons: [DRectButton; 4],
    pending: Option<(ChartInfo, SafeTexture)>,
    background: Option<SafeTexture>,
    assets: Option<Assets>,
    shown_at: f64,
    award: bool,
    transition: Option<(f64, Destination)>,
    saved: bool,
    record_id: String,
    finished_at: i64,
}
impl CourseScene {
    const FADE: f64 = 0.6;
    pub(crate) fn new(paths: Vec<String>) -> Self {
        Self {
            paths,
            stages: Vec::new(),
            task: None,
            next: None,
            waiting: false,
            aborted: false,
            buttons: std::array::from_fn(|_| DRectButton::new()),
            pending: None,
            background: None,
            assets: None,
            shown_at: 0.,
            award: false,
            transition: None,
            saved: false,
            record_id: uuid::Uuid::new_v4().to_string(),
            finished_at: 0,
        }
    }
    fn load(&mut self) -> Result<()> {
        let path = self.paths[self.stages.len()].clone();
        let mut config = prpr::course::config(&get_data().config);
        config.res_pack_path = get_data().selected_respack_path()?;
        let paths = self.paths.clone();
        let validate = self.stages.is_empty();
        let need_assets = self.assets.is_none();
        self.task = Some(Box::pin(async move {
            if validate {
                let mut seen = std::collections::HashSet::new();
                for p in &paths {
                    let mut fs = fs_from_path(p)?;
                    let info = prpr::fs::load_info(fs.as_mut()).await?;
                    let identity = crate::replay::library::fingerprint(fs.as_mut(), &info).await?;
                    anyhow::ensure!(seen.insert(identity), "课题中包含重复谱面内容，请选择三张不同谱面");
                }
            }
            let mut fs = fs_from_path(&path)?;
            let info = prpr::fs::load_info(fs.as_mut()).await?;
            let (ill, bg, _) = LoadingScene::load(fs.as_mut(), &info.illustration).await?;
            let game = GameScene::new(GameMode::Course, info.clone(), config, fs, None, bg, ill.clone(), None, None, None).await?;
            let assets = if need_assets { Some(Assets::load().await?) } else { None };
            Ok((game, info, ill, assets))
        }));
        Ok(())
    }
    fn save_rating(&mut self) -> Result<()> {
        if self.saved {
            return Ok(());
        }
        let record = records::Record {
            id: self.record_id.clone(),
            time: self.finished_at,
            score: score(&self.stages),
            difficulty: difficulty(&self.stages),
            grade: grade(score(&self.stages)).into(),
            full_combo: all_fc(&self.stages),
            charts: self
                .stages
                .iter()
                .map(|s| records::ChartScore {
                    name: s.info.name.clone(),
                    level: s.info.level.clone(),
                    score: s.result.score,
                    accuracy: s.result.accuracy,
                    counts: s.result.counts,
                    early: s.result.early,
                    late: s.result.late,
                })
                .collect(),
        };
        records::save_at(std::path::Path::new(&crate::dir::root()?), &record)?;
        self.saved = true;
        Ok(())
    }
}
impl Scene for CourseScene {
    fn enter(&mut self, tm: &mut TimeManager, _: Option<RenderTarget>) -> Result<()> {
        tm.speed = 1.;
        tm.reset();
        self.shown_at = tm.real_time();
        if self.waiting {
            self.waiting = false;
            self.aborted = true;
            self.pending = None;
        }
        Ok(())
    }
    fn on_result(&mut self, _: &mut TimeManager, result: Box<dyn Any>) -> Result<()> {
        self.waiting = false;
        match result.downcast::<PlayResult>() {
            Ok(result) => {
                let (info, illustration) = self.pending.take().context("课题结果缺少谱面")?;
                self.stages.push(Stage {
                    info,
                    illustration,
                    result: *result,
                });
                if self.stages.len() == 3 {
                    self.finished_at = chrono::Utc::now().timestamp_millis();
                }
            }
            Err(result) => {
                self.aborted = true;
                if let Ok(e) = result.downcast::<anyhow::Error>() {
                    prpr::scene::show_error(*e);
                }
            }
        }
        Ok(())
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let now = tm.real_time();
        if let Some((start, destination)) = self.transition {
            if now - start >= Self::FADE {
                self.transition = None;
                self.shown_at = now;
                match destination {
                    Destination::Award => self.award = true,
                    Destination::Home => self.next = Some(NextScene::PopWithResult(Box::new(ReturnHome))),
                    Destination::Exit => self.next = Some(NextScene::Pop),
                    Destination::Retry => {
                        self.stages.clear();
                        self.aborted = false;
                        self.pending = None;
                        self.award = false;
                        self.saved = false;
                        self.record_id = uuid::Uuid::new_v4().to_string();
                        self.finished_at = 0;
                    }
                }
            }
        }
        if !self.aborted && self.stages.len() < 3 && self.task.is_none() && !self.waiting {
            self.load()?;
        }
        if let Some(result) = self.task.as_mut().and_then(|f| poll_future(f.as_mut())) {
            self.task = None;
            match result {
                Ok((game, info, ill, assets)) => {
                    if let Some(assets) = assets {
                        self.assets = Some(assets);
                    }
                    self.background = Some(game.res.background.clone());
                    self.pending = Some((info, ill));
                    self.waiting = true;
                    self.next = Some(NextScene::Overlay(Box::new(game)));
                }
                Err(e) => {
                    self.aborted = true;
                    self.shown_at = now;
                    prpr::scene::show_error(e.context("课题加载失败"));
                }
            }
        }
        Ok(())
    }
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let now = tm.real_time();
        let t = now as f32;
        if self.transition.is_some() || now - self.shown_at < Self::FADE {
            return Ok(true);
        }
        if self.award {
            if self.buttons[0].touch(touch, t) {
                self.transition = Some((now, Destination::Home));
            }
            if self.buttons[1].touch(touch, t) {
                match self.save_rating() {
                    Ok(()) => self.transition = Some((now, Destination::Home)),
                    Err(e) => prpr::scene::show_error(e.context("保存课题成绩失败")),
                }
            }
            return Ok(true);
        }
        if self.buttons[3].touch(touch, t) && self.stages.len() == 3 && !self.aborted {
            match self.save_rating() {
                Ok(()) => {
                    prpr::scene::show_message("已保存到：设置 → 谱面 → 成绩历史 → 课题成绩").ok();
                }
                Err(e) => prpr::scene::show_error(e.context("保存课题成绩失败")),
            }
            return Ok(true);
        }
        if self.buttons[0].touch(touch, t) {
            self.transition = Some((now, Destination::Exit));
            return Ok(true);
        }
        if self.buttons[2].touch(touch, t) && self.stages.len() == 3 && !self.aborted {
            self.transition = Some((
                now,
                if badge_index(score(&self.stages)).is_some() {
                    Destination::Award
                } else {
                    Destination::Exit
                },
            ));
            return Ok(true);
        }
        if self.buttons[1].touch(touch, t) {
            self.transition = Some((now, Destination::Retry));
            return Ok(true);
        }
        Ok(false)
    }
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        clear_background(BLACK);
        for b in &mut self.buttons {
            b.invalidate();
        }
        // The preceding game has already faded fully to black. Stay black while loading.
        if self.stages.len() < 3 && !self.aborted {
            return Ok(());
        }
        if let Some(bg) = &self.background {
            ui.fill_rect(ui.screen_rect(), (**bg, ui.screen_rect(), ScaleType::CropCenter));
            ui.fill_rect(ui.screen_rect(), semi_black(0.35));
        }
        if let Some(assets) = &mut self.assets {
            if self.award {
                presentation::award(ui, assets, &self.stages, &mut self.buttons, tm.real_time() as f32);
            } else {
                presentation::results(ui, assets, &self.stages, self.aborted, self.saved, &mut self.buttons, tm.real_time() as f32);
            }
        } else {
            self.buttons[0].render_text(ui, Rect::new(-0.9, -0.2, 0.3, 0.1), tm.real_time() as f32, "返回", 0.4, false);
            ui.text("课题加载失败").pos(0., 0.).anchor(0.5, 0.5).size(0.5).draw();
        }
        let black = if let Some((start, _)) = self.transition {
            fade((tm.real_time() - start) / Self::FADE)
        } else {
            1. - fade((tm.real_time() - self.shown_at) / Self::FADE)
        };
        ui.fill_rect(ui.screen_rect(), semi_black(black));
        Ok(())
    }
    fn next_scene(&mut self, _: &mut TimeManager) -> NextScene {
        self.next.take().unwrap_or_default()
    }
}
#[cfg(all(test, record, target_os = "windows"))]
pub(crate) async fn render_regression(painter: &mut prpr::ui::TextPainter) {
    async fn local<T>(future: impl std::future::Future<Output = T>) -> T {
        let mut f = Box::pin(future);
        loop {
            if let Some(v) = poll_future(f.as_mut()) {
                return v;
            }
            next_frame().await;
        }
    }
    let root = std::path::PathBuf::from(crate::dir::charts().unwrap());
    let source = root.join("renamed-chart");
    let mut paths = Vec::new();
    for i in 0..3 {
        let path = format!("course-qa-{i}");
        let dst = root.join(&path);
        std::fs::create_dir_all(&dst).unwrap();
        for file in ["chart.json", "info.yml", "song.wav", "background.png"] {
            std::fs::copy(source.join(file), dst.join(file)).unwrap();
        }
        let mut info: ChartInfo = serde_yaml::from_str(&std::fs::read_to_string(dst.join("info.yml")).unwrap()).unwrap();
        info.name = ["Exoplanetary Mirage · QA", "Entrance to the Chaos · QA", "Rrhar'il · QA"][i].into();
        info.level = "AT Lv.17".into();
        info.difficulty = 17.;
        std::fs::write(dst.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(dst.join("chart.json")).unwrap()).unwrap();
        json["judgeLineList"][0]["notesAbove"][0]["positionX"] = serde_json::json!(i as f64 * 0.5);
        std::fs::write(dst.join("chart.json"), serde_json::to_vec(&json).unwrap()).unwrap();
        paths.push(path);
    }
    let mut course = CourseScene::new(paths);
    let mut tm = TimeManager::default();
    course.enter(&mut tm, None).unwrap();
    for _ in 0..3 {
        course.load().unwrap();
        let (mut game, info, ill, assets) = local(course.task.take().unwrap()).await.unwrap();
        if let Some(assets) = assets {
            course.assets = Some(assets);
        }
        assert_eq!(game.res.config.judge_algorithm, prpr::config::JudgeAlgorithm::Phigros);
        assert!(game.res.config.mods.contains(prpr::config::Mods::STRICT_JUDGE));
        assert!(!game.res.config.autoplay());
        game.res.config.adjust_time = false;
        game.enter(&mut tm, None).unwrap();
        tm.seek_to(GameScene::BEFORE_TIME + 0.1);
        game.update(&mut tm).unwrap();
        tm.seek_to(0.1);
        game.update(&mut tm).unwrap();
        let ids: Vec<_> = game
            .chart
            .lines
            .iter()
            .enumerate()
            .flat_map(|(l, line)| {
                line.notes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| !n.fake)
                    .map(move |(i, _)| (l as u32, i as u32))
            })
            .collect();
        for (l, i) in ids {
            game.judge.commit(1., prpr::judge::Judgement::Perfect, l, i, 0.);
        }
        course.background = Some(game.res.background.clone());
        let end = game.res.track_length;
        game.judge.advance_to(&mut game.chart, end + 1.);
        tm.seek_to(end + 0.6);
        game.update(&mut tm).unwrap();
        tm.seek_to(end + 10.);
        game.update(&mut tm).unwrap();
        let NextScene::PopWithResult(result) = game.next_scene(&mut tm) else {
            panic!("course must return one result without a normal ending scene")
        };
        course.pending = Some((info, ill));
        course.waiting = true;
        course.on_result(&mut tm, result).unwrap();
        course.enter(&mut tm, None).unwrap();
        assert!(!course.aborted);
        assert_eq!(course.stages.last().unwrap().result.score, 1_000_000);
    }
    assert_eq!(course.stages.len(), 3);
    assert_eq!(course.stages.iter().map(|s| s.result.score).sum::<u32>(), 3_000_000);
    course.shown_at = tm.real_time();
    {
        let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
        course.render(&mut tm, &mut ui).unwrap();
        crate::practice_tools::qa::capture(&mut ui, "pro15-course-black-transition");
        let image = image::open("target/replay-qa/pro15-course-black-transition.png").unwrap().to_rgb8();
        assert!(image.pixels().all(|p| p.0.iter().all(|v| *v < 5)), "result entrance must start at full black");
    }
    course.shown_at = tm.real_time() - 1.;
    for (w, h, label) in [(1280, 548, "phone"), (1280, 720, "wide"), (960, 720, "tablet")] {
        let mut ui = Ui::new(painter, Some((0, 0, w, h)));
        course.render(&mut tm, &mut ui).unwrap();
        crate::practice_tools::qa::capture(&mut ui, &format!("pro15-course-{label}"));
    }
    course.transition = Some((tm.real_time() - CourseScene::FADE - 0.01, Destination::Award));
    course.update(&mut tm).unwrap();
    assert!(course.award && course.transition.is_none(), "continue must fade from results into award");
    course.shown_at = tm.real_time() - 1.;
    let backdrop = image::load_from_memory(&load_file("background.jpg").await.unwrap()).unwrap().blur(18.);
    crate::scene::TEX_BACKGROUND_BLUR.with(|slot| *slot.borrow_mut() = Some(backdrop.into()));
    for (total, label) in [
        (3_000_000, "rainbow"),
        (2_999_999, "gold"),
        (2_939_999, "red"),
        (2_849_999, "blue"),
        (2_699_999, "green"),
    ] {
        course.stages[0].result.score = total - 2_000_000;
        course.award = true;
        let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
        course.render(&mut tm, &mut ui).unwrap();
        crate::practice_tools::qa::capture(&mut ui, &format!("pro15-course-award-{label}"));
    }
    course.stages[0].result.score = 1_000_000;
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        course
            .touch(
                &mut tm,
                &Touch {
                    id: 111,
                    phase,
                    position: vec2(-0.18, 0.30),
                    time: 0.,
                },
            )
            .unwrap();
    }
    assert!(matches!(course.transition, Some((_, Destination::Home))));
    course.transition = Some((tm.real_time() - CourseScene::FADE - 0.01, Destination::Home));
    course.update(&mut tm).unwrap();
    assert!(!course.saved, "cancel must not save");
    assert!(
        matches!(course.next_scene(&mut tm), NextScene::PopWithResult(result) if result.is::<ReturnHome>()),
        "cancel must leave the course and return home"
    );
    course.award = true;
    course.shown_at = tm.real_time() - 1.;
    let mut ui = Ui::new(painter, Some((0, 0, 1280, 720)));
    course.render(&mut tm, &mut ui).unwrap();
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        course
            .touch(
                &mut tm,
                &Touch {
                    id: 112,
                    phase,
                    position: vec2(0.18, 0.30),
                    time: 0.,
                },
            )
            .unwrap();
    }
    assert!(course.saved && matches!(course.transition, Some((_, Destination::Home))));
    let records = records::load_at(std::path::Path::new(&crate::dir::root().unwrap())).unwrap();
    assert!(records
        .iter()
        .any(|r| r.id == course.record_id && r.charts.len() == 3 && r.score == 3_000_000));
    println!(
        "pro.15 course: three real chart loads, locked Phigros strict rules, no intermediate ending and 3,000,000-point vertical results passed"
    );
}
