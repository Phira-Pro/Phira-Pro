//! Read-only gameplay view reconstructed from phira-mp live data.
use crate::{
    get_data,
    mp::{live::LiveScope, monitor_stream::PlayerStream, protocol::{JudgeEvent, LiveBatch}, session::Session},
    popup::ChooseButton, scene::fs_from_path,
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    config::Mods, core::{BadNote, NoteKind, Vector}, ext::{semi_white, BLACK_TEXTURE},
    judge::{JudgeStatus, Judgement}, scene::{GameMode, GameScene, LoadingScene, LocalSceneTask, NextScene, Scene},
    time::TimeManager, ui::{DRectButton, Ui},
};
use std::collections::HashMap;

pub fn launch(chart: i32, path: String, session: Session) -> LocalSceneTask {
    Some(Box::pin(async move { Ok(NextScene::Overlay(Box::new(MonitorScene::load(chart, path, session).await?))) }))
}

pub struct MonitorScene {
    game: GameScene,
    session: Session,
    scope: LiveScope,
    players: Vec<(i32, PlayerStream)>,
    picker: ChooseButton,
    selected: usize,
    applied: usize,
    note_counts: Vec<usize>,
    holds: HashMap<(u32, u32), f64>,
    current: f64,
    last_real: f64,
    dropped: u64,
    paused: bool,
    back: DRectButton,
    exit: bool,
}

impl MonitorScene {
    async fn load(chart_id: i32, path: String, session: Session) -> Result<Self> {
        let initial = session.snapshot();
        let scope = initial.monitor_scope().context("房间已变化，无法开始观战")?;
        let room = initial.room.as_ref().context("尚未加入房间")?;
        anyhow::ensure!(room.chart == Some(chart_id), "观战谱面与房间选择不一致");
        let mut fs = fs_from_path(&path)?;
        let mut info = prpr::fs::load_info(fs.as_mut()).await?;
        anyhow::ensure!(info.id.is_none_or(|id| id == chart_id), "观战谱面与房间选择不一致");
        info.id = Some(chart_id);
        let (background, illustration) = match LoadingScene::load(fs.as_mut(), &info.illustration).await {
            Ok((illustration, background, _)) => (background, illustration),
            Err(_) => (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
        };
        let mut config = get_data().config.clone();
        config.res_pack_path = get_data().selected_respack_path()?;
        config.force_official_play(&mut Mods::empty());
        config.mods = Mods::empty();
        config.interactive = false;
        config.touch_debug = false;
        config.offset_indicator = false; // v1 live packets carry no timing offset.
        config.speed = 1.;
        let mut game = GameScene::new(GameMode::View, info, config, fs, None, background, illustration, None, None, None).await?;
        let mut clock = TimeManager::from_config(&game.res.config);
        game.enter(&mut clock, None)?;
        clock.seek_to(GameScene::BEFORE_TIME + 0.1); game.update(&mut clock)?;
        clock.seek_to(0.1); game.update(&mut clock)?;
        let _ = game.music.pause();
        anyhow::ensure!(session.snapshot().monitor_scope() == Some(scope), "观战加载期间房间或回合已改变");
        let note_counts = game.chart.lines.iter().map(|line| line.notes.len()).collect::<Vec<_>>();
        let total = note_counts.iter().sum();
        let players = room.users.values().filter(|user| !user.monitor).take(32)
            .map(|user| (user.id, PlayerStream::new(user.name.clone(), total))).collect::<Vec<_>>();
        anyhow::ensure!(!players.is_empty(), "房间中没有可观看的玩家");
        let picker = ChooseButton::new().with_options(players.iter().map(|(_, player)| player.name.clone()).collect()).with_compact_popup();
        game.res.config.player_name = players[0].1.name.clone();
        Ok(Self { game, session, scope, players, picker, selected: 0, applied: 0, note_counts,
            holds: HashMap::new(), current: 0., last_real: 0., dropped: 0, paused: false, back: DRectButton::new(), exit: false })
    }

    fn reset_player(&mut self) {
        self.game.sync_replay_block_audio(false);
        self.game.judge.reset();
        self.game.chart.reset();
        self.game.bad_notes.clear();
        self.game.res.emitter.emitter.reset();
        self.game.res.emitter.emitter_square.reset();
        self.game.res.config.player_name = self.players[self.selected].1.name.clone();
        self.holds.clear();
        self.applied = 0;
        self.current = self.players[self.selected].1.time();
    }

    fn apply_judgement(&mut self, event: JudgeEvent, silent: bool) {
        let time = event.time as f64;
        let GameScene { chart, judge, res, bad_notes, .. } = &mut self.game;
        let Some(line) = chart.lines.get_mut(event.line_id as usize) else { return };
        let Some(note) = line.notes.get_mut(event.note_id as usize) else { return };
        let perfect = matches!(event.judgement, 0 | 4);
        let grade = match event.judgement { 0 | 4 => Judgement::Perfect, 1 | 5 => Judgement::Good, 2 => Judgement::Bad, _ => Judgement::Miss };
        let hold = matches!(note.kind, NoteKind::Hold { .. });
        if event.judgement >= 4 {
            // HoldPerfect/HoldGood are head contacts; only the subsequent
            // Perfect/Good/Bad/Miss event commits this note's score.
            if !hold { return; }
            note.judge = JudgeStatus::Hold(perfect, time, 0., false, f64::INFINITY, grade);
        } else {
            let active = if hold && event.judgement <= 1 {
                let NoteKind::Hold { end_time, .. } = note.kind else { unreachable!() };
                let active = note.judge.finish_hold_score(time, end_time);
                if active { self.holds.insert((event.line_id, event.note_id), end_time); }
                active
            } else { false };
            if !active { note.judge = JudgeStatus::Judged; }
            judge.commit(time, grade, event.line_id, event.note_id, 0.);
        }
        if !silent && (event.judgement >= 4 || (!hold && event.judgement <= 1)) {
            note.hitsound.play(res);
            let line = &chart.lines[event.line_id as usize];
            let note = &line.notes[event.note_id as usize];
            let matrix = line.now_transform(res, &chart.lines) * note.object.now(res);
            let rotation = note.rotation(line);
            let color = res.config.judge_grading.color(grade as usize, if perfect { res.res_pack.info.fx_perfect() } else { res.res_pack.info.fx_good() });
            res.with_model(matrix, |res| res.emit_at_origin(rotation, color));
        } else if !silent && event.judgement == 2 {
            let line = &chart.lines[event.line_id as usize];
            let note = &line.notes[event.note_id as usize];
            let mut matrix = line.now_transform(res, &chart.lines);
            if !note.above { matrix.append_nonuniform_scaling_mut(&Vector::new(1., -1.)); }
            let incline = line.incline.now_opt().map(|v| v.to_radians().sin()).unwrap_or_default();
            let h = ((note.height - line.height.now() as f64) / res.aspect_ratio as f64 * note.speed) as f32;
            matrix *= note.now_transform(res, &line.ctrl_obj.borrow_mut(), h, incline);
            bad_notes.push(BadNote { time, kind: note.kind.clone(), matrix });
        }
        judge.judgements.borrow_mut().clear();
    }

    fn stop_audio(&mut self) {
        let _ = self.game.music.pause();
        self.game.sync_replay_block_audio(false);
    }
}

impl Scene for MonitorScene {
    fn enter(&mut self, tm: &mut TimeManager, _: Option<RenderTarget>) -> Result<()> { self.last_real = tm.real_time(); Ok(()) }
    fn pause(&mut self, _: &mut TimeManager) -> Result<()> { self.paused = true; self.stop_audio(); Ok(()) }
    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> { self.paused = false; self.last_real = tm.real_time(); Ok(()) }
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if self.picker.top_touch(touch, t) { return Ok(true); }
        if self.back.touch(touch, t) { self.exit = true; self.stop_audio(); return Ok(true); }
        Ok(self.picker.touch(touch, t))
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if self.exit { return Ok(()); }
        let snapshot = self.session.snapshot();
        if snapshot.monitor_scope() != Some(self.scope) {
            self.exit = true;
            self.stop_audio();
            if !snapshot.connected { prpr::scene::show_message("连接中断，观战已结束；本回合没有历史流可补取").warn(); }
            return Ok(());
        }
        let now = tm.real_time();
        self.picker.update(now as f32);
        let switched = self.picker.selected() != self.selected;
        if switched { self.selected = self.picker.selected(); self.reset_player(); }
        for event in self.session.take_live_events(self.scope) {
            let id = match &event.batch { LiveBatch::Touches {player, ..} | LiveBatch::Judges {player, ..} => *player };
            let Some((_, player)) = self.players.iter_mut().find(|(player, _)| *player == id) else { continue };
            match event.batch {
                LiveBatch::Touches { frames, .. } => for frame in frames { player.touch(frame, now, self.game.res.track_length); },
                LiveBatch::Judges { events, .. } => for event in events { player.judge(event, now, self.game.res.track_length, &self.note_counts); },
            }
        }
        let dropped = self.session.dropped_live_batches();
        if dropped != self.dropped { for (_, player) in &mut self.players { player.incomplete = true; } self.dropped = dropped; }
        for (_, player) in &mut self.players { player.advance(now); }
        self.current = self.players[self.selected].1.time();
        self.game.res.time = self.current;
        self.game.tick_replay();
        while let Some(event) = self.players[self.selected].1.judges().get(self.applied).filter(|e| e.time as f64 <= self.current).copied() {
            self.applied += 1;
            self.apply_judgement(event, switched || self.current - event.time as f64 > 0.3);
        }
        self.holds.retain(|&(line, note), tail| {
            if self.current >= *tail { self.game.chart.lines[line as usize].notes[note as usize].judge = JudgeStatus::Judged; false } else { true }
        });
        let contacts = self.players[self.selected].1.contacts().into_iter().map(|(id,p)| (id, vec2(p[0],p[1]))).collect::<Vec<_>>();
        self.game.update_replay_block_touches(&contacts);
        let running = !self.paused && self.players[self.selected].1.started() && !self.players[self.selected].1.stalled(now);
        self.game.sync_replay_block_audio(running);
        let target = (self.current + self.game.offset() as f64).max(0.);
        if running {
            if (self.game.music.position() - target).abs() > 0.15 { let _ = self.game.music.seek_to(target); }
            if self.game.music.paused() { let _ = self.game.music.play(); }
        } else { let _ = self.game.music.pause(); }
        self.game.set_replay_frame_delta((now - self.last_real).clamp(0., 0.1) as f32);
        self.last_real = now;
        Ok(())
    }
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let real = tm.real_time();
        let mut clock = TimeManager::manual(Box::new(move || real));
        clock.seek_to(self.current);
        self.game.render(&mut clock, ui)?;
        let target = self.game.res.camera.render_target;
        let fitted = self.game.res.camera.viewport.unwrap_or(ui.viewport);
        {
            let mut fingers = Ui::new(ui.text_painter, Some(fitted));
            set_camera(&Camera2D { render_target: target, ..fingers.camera() });
            for (id, p, fade) in self.players[self.selected].1.markers() {
                let alpha = self.game.res.config.touch_point_alpha * fade;
                fingers.fill_circle(p[0], p[1], self.game.res.config.touch_point_size, Color { a: alpha, ..Color::from_hex_rgb(0x72d5ff) });
                fingers.text(format!("F{}", id + 1)).pos(p[0], p[1] - self.game.res.config.touch_point_size).anchor(0.5,1.).size(0.24).color(semi_white(alpha)).draw();
            }
        }
        set_camera(&Camera2D { render_target: target, ..ui.camera() });
        let t = tm.real_time() as f32;
        let y = -ui.top + 0.025;
        self.back.render_text(ui, Rect::new(-0.94,y,0.21,0.07), t, "返回房间", 0.3, false);
        self.picker.render(ui, Rect::new(-0.70,y,0.50,0.07), t);
        let player = &self.players[self.selected].1;
        let status = if player.incomplete { "实时数据不完整" } else if !player.started() { "等待玩家实时数据…" } else if player.stalled(tm.real_time()) { "等待网络数据…" } else { "正在观战" };
        ui.text(status).pos(-0.17,y+0.035).anchor(0.,0.5).no_baseline().size(0.28).color(semi_white(0.85)).draw();
        if player.invalid > 0 { ui.text(format!("已隔离 {} 条无效事件",player.invalid)).pos(-0.17,y+0.085).size(0.22).color(semi_white(0.7)).draw(); }
        self.picker.render_top(ui,t,1.);
        Ok(())
    }
    fn next_scene(&mut self, _: &mut TimeManager) -> NextScene { if self.exit { self.stop_audio(); NextScene::Pop } else { NextScene::None } }
}

