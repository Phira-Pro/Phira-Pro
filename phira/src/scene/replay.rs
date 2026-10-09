// 回放播放场景：在 Phira Pro 内直接查看 `.phirar` 回放。
//
// 与官方联机监视器同一套思路：谱面用真正的 `GameScene`（View 模式）渲染，
// 回放时把录制好的判定事件按时间戳重放到谱面上（分数 / 连击 / 音符判定位与原
// 局完全一致），触摸帧用于画出手指触点。

use crate::{
    replay::{self, ChartRef, Replay},
    scene::fs_from_path,
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    config::{Config, Mods},
    core::{BadNote, Vector},
    ext::{semi_white, RectExt, BLACK_TEXTURE},
    judge::{JudgeStatus, Judgement as TJ},
    scene::{GameMode, GameScene, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, Scroll, Ui},
};
use std::{collections::HashMap, path::PathBuf};

pub struct ReplayScene {
    game_scene: GameScene,
    replay: Replay,

    // 回放时钟
    current: f64,
    playing: bool,
    particle_delta: f32,
    speed: f32,

    // 判定 / 触点应用进度
    judge_idx: usize,
    touch_idx: usize,
    touch_points: Vec<(f32, f32)>,
    finishing_holds: HashMap<(u32, u32), f64>,

    // 触点插值用（同 phira-monitor 的 PlayerView）
    current_touches: HashMap<u64, (Vec2, usize)>,
    last_t: f64,

    // UI
    exit: bool,
    btn_back: DRectButton,
    btn_play: DRectButton,
    rate: crate::popup::ChooseButton,
    controls: Vec<DRectButton>,
    expanded: bool,
    controls_visible: bool,
    info_mode: bool,
    info_scroll: Scroll,
    fingers: bool,
    finger_ids: bool,
    ranges: bool,
    saved_playing: bool,
    drag: Option<u64>,
    drag_playing: bool,
    pending_seek: Option<f64>,
    timeline: Rect,
    a: Option<f64>,
    b: Option<f64>,
    looping: bool,
    next_touch: Vec<Option<usize>>,
    finger_numbers: HashMap<u64, usize>,
    errors: Vec<f64>,
    checkpoints: Vec<Checkpoint>,
    checkpoint_step: f64,
}

impl ReplayScene {
    pub async fn new(path: PathBuf) -> Result<Self> {
        let replay = replay::load(&path).context("读取回放失败")?;
        let binding = replay::library::all()?
            .into_iter()
            .find(|i| i.path == path)
            .and_then(|i| i.binding)
            .and_then(|b| b.local_path);
        let candidates = crate::get_data().charts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>();
        match replay::library::match_chart(&replay, binding.as_deref(), &candidates).await? {
            replay::library::Match::Found(local, verified) => Self::new_bound(path, local, verified).await,
            replay::library::Match::WrongVersion => anyhow::bail!("谱面版本不一致；请到设置 → 谱面 → 回放文件管理关联录制时的谱面"),
            replay::library::Match::WrongAudio => anyhow::bail!("谱面内容一致，但音乐版本与回放不一致"),
            replay::library::Match::WrongServer => anyhow::bail!("回放所属谱面服务器与当前服务器不一致"),
            replay::library::Match::Missing => anyhow::bail!("缺少回放所需谱面；请到设置 → 谱面 → 回放文件管理下载或关联谱面"),
        }
    }
    pub async fn new_bound(path: PathBuf, local_path: String, verified: bool) -> Result<Self> {
        let replay = replay::load(&path).context("读取回放失败")?;
        let mut fs = fs_from_path(&local_path)?;
        let mut info = prpr::fs::load_info(fs.as_mut()).await?;
        if let Some(expected) = &replay.meta.fingerprint {
            anyhow::ensure!(replay::library::fingerprint(fs.as_mut(), &info).await? == *expected, "谱面内容与回放不一致");
        }
        if let Some(expected) = &replay.meta.audio_fingerprint {
            anyhow::ensure!(replay::library::audio_fingerprint(fs.as_mut(), &info).await? == *expected, "音乐版本与回放不一致");
        }
        if !verified {
            prpr::scene::show_message("旧回放未记录内容指纹，当前谱面关联未校验").warn();
        }

        info.id = match &replay.chart {
            Some(ChartRef::Id(id)) => Some(*id),
            _ => info.id,
        };
        let mut config = crate::get_data().config.clone();
        if let Some(settings) = &replay.settings {
            settings.apply(&mut config);
        } else {
            config = Config::default();
        }
        if let Some(gameplay) = &replay.gameplay {
            gameplay.apply(&mut config);
        }
        config.sample_count = 4;
        config.player_name = replay.meta.player.clone().unwrap_or_else(|| "REPLAY · 未记录玩家".into());
        config.theoretical_score = replay.theoretical_score;
        config.offset = replay.offset;
        config.judge_grading = replay.grading;
        config.speed = if replay.speed > 0. { replay.speed } else { 1. };
        config.mods = Mods::from_bits(replay.mods as i32).unwrap_or_default();
        let nightcore = config.mods.contains(Mods::NIGHTCORE);
        // v5 stores the effective speed after mods; old tapes stored the base speed.
        if replay.settings.is_none() && nightcore {
            config.speed *= 1.5;
        }
        config.mods.remove(Mods::NIGHTCORE);
        config.interactive = false;
        config.touch_debug = false;
        config.offset_indicator = replay.has_diffs;
        if let Some(aspect) = replay.aspect_ratio {
            config.aspect_ratio = Some(aspect);
            info.force_aspect_ratio = true;
        }

        let (background, illustration) = match LoadingScene::load(fs.as_mut(), &info.illustration).await {
            Ok((ill, bg, _)) => (bg, ill),
            Err(_) => (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
        };

        let mut game_scene = GameScene::new(GameMode::View, info, config, fs, None, background, illustration, None, None, None).await?;

        game_scene.res.config.mods.set(Mods::NIGHTCORE, nightcore);
        anyhow::ensure!(
            replay.judges.iter().all(|e| game_scene
                .chart
                .lines
                .get(e.line as usize)
                .is_some_and(|l| (e.note as usize) < l.notes.len())),
            "回放的音符索引与谱面不一致"
        );
        // 回放不跑正常时钟，必须手动把状态机推过 Starting / BeforeMusic 到 Playing，
        // 否则 `ui()` 会一直停留在开场进度（p≈0），HUD 整体错位、透明度也不对。
        // （不能靠循环调 update 推进：同一帧内 get_time() 不变，状态永远走不出去。）
        let mut tm = TimeManager::from_config(&game_scene.res.config);
        game_scene.enter(&mut tm, None)?;
        tm.seek_to(GameScene::BEFORE_TIME + 0.1);
        game_scene.update(&mut tm)?; // Starting -> BeforeMusic
        tm.seek_to(0.1);
        game_scene.update(&mut tm)?; // BeforeMusic -> Playing
        let _ = game_scene.music.pause();
        let _ = game_scene.music.seek_to(game_scene.offset() as f64);

        let mut next_touch = vec![None; replay.touches.len()];
        let mut finger_numbers = HashMap::new();
        for event in &replay.touches {
            let n = finger_numbers.len() + 1;
            finger_numbers.entry(event.id).or_insert(n);
        }
        let mut next = HashMap::new();
        for (i, e) in replay.touches.iter().enumerate().rev() {
            next_touch[i] = next.insert(e.id, i);
        }
        let errors = replay.judges.iter().filter(|e| matches!(e.kind, 2 | 3)).map(|e| e.t).collect();
        let note_count = game_scene.chart.lines.iter().map(|l| l.notes.len()).sum::<usize>().max(1);
        let checkpoint_count = ((16 << 20) / (note_count * 64)).min(32);
        let checkpoint_step = if checkpoint_count == 0 {
            f64::INFINITY
        } else {
            (game_scene.res.track_length / checkpoint_count as f64).max(5.)
        };
        Ok(Self {
            game_scene,
            replay,
            current: 0.,
            playing: false,
            particle_delta: 0.,
            speed: 1.,
            judge_idx: 0,
            touch_idx: 0,
            touch_points: Vec::new(),
            finishing_holds: HashMap::new(),
            current_touches: HashMap::new(),
            last_t: 0.,
            exit: false,
            btn_back: DRectButton::new(),
            btn_play: DRectButton::new(),
            rate: crate::popup::ChooseButton::new()
                .with_options(replay::transport::RATES.iter().map(|r| format!("{r}×")).collect())
                .with_selected(4)
                .with_compact_popup(),
            controls: (0..21).map(|_| DRectButton::new()).collect(),
            expanded: false,
            fingers: true,
            finger_ids: false,
            ranges: false,
            controls_visible: true,
            info_mode: false,
            info_scroll: Scroll::new(),
            saved_playing: false,
            drag: None,
            drag_playing: false,
            pending_seek: None,
            timeline: Rect::default(),
            a: None,
            b: None,
            looping: false,
            next_touch,
            finger_numbers,
            errors,
            checkpoints: Vec::new(),
            checkpoint_step,
        })
    }

    fn restart(&mut self) {
        self.rewind_to(0.);
        self.current = 0.;
    }

    /// 跳转到 `t`：先重置谱面/判定状态再静默快进到 `t`。
    ///
    /// 若不重置，`JudgeLineCache`（`update_order` / `above_indices` / `below_indices`）
    /// 与音符的 `judge` 状态都只进不退，往回拖进度条后之前那段音符会"消失"；
    /// 重置 + 重放才能实现真正的"再看一次"。
    fn seek(&mut self, t: f64) {
        let t = t.clamp(0., self.end());
        if (t - self.current).abs() < 1e-9 {
            return;
        }
        self.rewind_to(t);
        self.current = t;
    }

    /// 把谱面与判定状态重置，并静默快进到 `t`（不产生打击特效）。
    fn rewind_to(&mut self, t: f64) {
        {
            let scene = &mut self.game_scene;
            scene.judge.reset();
            scene.chart.reset();
            scene.bad_notes.clear();
            scene.res.emitter.emitter.reset();
            scene.res.emitter.emitter_square.reset();
            scene.res.judge_line_color = scene.res.res_pack.info.color_perfect();
        }
        self.judge_idx = 0;
        self.touch_idx = 0;
        self.current_touches.clear();
        self.touch_points.clear();
        self.finishing_holds.clear();
        if let Some(checkpoint) = self.checkpoints.iter().rev().find(|c| c.t <= t) {
            self.game_scene.judge.restore_replay_snapshot(&checkpoint.judge);
            for (line, states) in self.game_scene.chart.lines.iter_mut().zip(&checkpoint.notes) {
                for (note, status) in line.notes.iter_mut().zip(states) {
                    note.judge = status.clone();
                }
            }
            self.judge_idx = checkpoint.judge_idx;
            self.touch_idx = checkpoint.touch_idx;
            self.current_touches = checkpoint.touches.clone();
            self.finishing_holds = checkpoint.holds.clone();
        }
        self.game_scene.res.time = t;
        self.particle_delta = 0.;
        self.apply_judges(t, true);
        self.apply_touches(t);
        self.game_scene.touch_points = if self.fingers { self.touch_points.clone() } else { Vec::new() };
        let target = (t + self.game_scene.offset() as f64).max(0.);
        let _ = self.game_scene.music.seek_to(target);
    }

    /// 应用 `t` 时刻之前的所有判定事件到谱面（参照 phira-monitor 的 PlayerView）。
    /// `silent = true` 时不产生打击特效 / 爆炸，用于拖动跳转与回溯快进。
    fn apply_judges(&mut self, t: f64, silent: bool) {
        while self.judge_idx < self.replay.judges.len() {
            let ev = self.replay.judges[self.judge_idx];
            if ev.t > t {
                break;
            }
            self.judge_idx += 1;
            let GameScene { chart, judge, res, bad_notes, .. } = &mut self.game_scene;
            let Some(line) = chart.lines.get_mut(ev.line as usize) else { continue };
            let Some(note) = line.notes.get_mut(ev.note as usize) else { continue };
            if self.replay.has_diffs && matches!(note.kind, prpr::core::NoteKind::Click | prpr::core::NoteKind::Hold { .. }) {
                let head = matches!(ev.kind, 5 | 6);
                let hold = matches!(note.kind, prpr::core::NoteKind::Hold { .. });
                let timed = if hold {
                    if res.config.hold_tail_judge {
                        !head
                    } else {
                        head
                    }
                } else {
                    true
                };
                if timed && ev.kind != 3 {
                    let grade = match ev.head_grade.unwrap_or(ev.kind) {
                        0 | 5 => TJ::Perfect,
                        4 => TJ::PerfectPlus,
                        1 | 6 => TJ::Good,
                        2 => TJ::Bad,
                        7 => TJ::Great,
                        8 => TJ::Ok,
                        9 => TJ::Meh,
                        _ => TJ::Miss,
                    };
                    judge.record_replay_offset(ev.t, grade, ev.diff as f64);
                }
            }
            if !silent
                && (matches!(ev.kind, 5 | 6) || (matches!(ev.kind, 0 | 1 | 4 | 7 | 8 | 9) && !matches!(note.kind, prpr::core::NoteKind::Hold { .. })))
            {
                note.hitsound.play(res);
            }
            let hold_success = matches!(ev.kind, 0 | 1 | 4 | 7 | 8 | 9) && matches!(note.kind, prpr::core::NoteKind::Hold { .. });
            let hold_active = if hold_success {
                let prpr::core::NoteKind::Hold { end_time, .. } = note.kind else { unreachable!() };
                let active = note.judge.finish_hold_score(ev.t, end_time);
                if active { self.finishing_holds.insert((ev.line, ev.note), end_time); }
                active
            } else { false };
            match ev.kind {
                0 | 4 => {
                    if !hold_active { note.judge = JudgeStatus::Judged; }
                    let tj = if ev.kind == 4 { TJ::PerfectPlus } else { TJ::Perfect };
                    judge.commit(ev.t, tj, ev.line, ev.note, ev.diff as f64);
                    if !silent && !hold_success {
                        let fx = res.res_pack.info.fx_perfect();
                        let (line_tr, obj, rot) = {
                            let line = &chart.lines[ev.line as usize];
                            let tr = line.now_transform(res, &chart.lines);
                            let obj = line.notes[ev.note as usize].object.now(res);
                            (tr, obj, line.notes[ev.note as usize].rotation(line))
                        };
                        res.with_model(line_tr * obj, |res| res.emit_at_origin(rot, fx));
                    }
                }
                1 | 7 | 8 | 9 => {
                    if !hold_active {
                        note.judge = JudgeStatus::Judged;
                    }
                    judge.commit(
                        ev.t,
                        match ev.kind {
                            7 => TJ::Great,
                            8 => TJ::Ok,
                            9 => TJ::Meh,
                            _ => TJ::Good,
                        },
                        ev.line,
                        ev.note,
                        ev.diff as f64,
                    );
                    if !silent && !hold_success {
                        let fx = res.res_pack.info.fx_good();
                        let (line_tr, obj, rot) = {
                            let line = &chart.lines[ev.line as usize];
                            let tr = line.now_transform(res, &chart.lines);
                            let obj = line.notes[ev.note as usize].object.now(res);
                            (tr, obj, line.notes[ev.note as usize].rotation(line))
                        };
                        res.with_model(line_tr * obj, |res| res.emit_at_origin(rot, fx));
                    }
                }
                2 => {
                    note.judge = JudgeStatus::Judged;
                    judge.commit(ev.t, TJ::Bad, ev.line, ev.note, ev.diff as f64);
                    if !silent {
                        let (mat, kind) = {
                            let line = &chart.lines[ev.line as usize];
                            let note = &line.notes[ev.note as usize];
                            let mut mat = line.now_transform(res, &chart.lines);
                            if !note.above {
                                mat.append_nonuniform_scaling_mut(&Vector::new(1., -1.));
                            }
                            let incline_sin = line.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default();
                            let h = ((note.height - line.height.now() as f64) / res.aspect_ratio as f64 * note.speed) as f32;
                            let nm = note.now_transform(res, &line.ctrl_obj.borrow_mut(), h, incline_sin);
                            (mat * nm, note.kind.clone())
                        };
                        bad_notes.push(BadNote {
                            time: ev.t,
                            kind,
                            matrix: mat,
                        });
                    }
                }
                3 => {
                    note.judge = JudgeStatus::Judged;
                    judge.commit(ev.t, TJ::Miss, ev.line, ev.note, ev.diff as f64);
                }
                5 => {
                    note.judge = JudgeStatus::Hold(true, ev.t, 0., false, f64::INFINITY);
                }
                6 => {
                    note.judge = JudgeStatus::Hold(false, ev.t, 0., false, f64::INFINITY);
                }
                _ => {}
            }
        }
        judge_queue_clear(&self.game_scene.judge);
        // Only ongoing, already scored holds are visited. Ending their visual
        // lifetime adds no judgement event and no second score; seek clears this map.
        let chart = &mut self.game_scene.chart;
        self.finishing_holds.retain(|&(line, id), tail| {
            let note = &mut chart.lines[line as usize].notes[id as usize];
            if t >= *tail { note.judge = JudgeStatus::Judged; }
            !matches!(note.judge, JudgeStatus::Judged)
        });
    }

    fn apply_touches(&mut self, t: f64) {
        let touches = &self.replay.touches;
        while self.touch_idx < touches.len() && touches[self.touch_idx].t as f64 <= t {
            let index = self.touch_idx;
            let event = touches[index];
            self.touch_idx += 1;
            if event.phase == replay::PHASE_UP {
                self.current_touches.remove(&event.id);
            } else {
                self.current_touches.insert(event.id, (vec2(event.x, event.y), index));
            }
        }
        self.touch_points = self
            .current_touches
            .values()
            .map(|(pos, index)| {
                let pos = self.touch_position(*pos, *index, t);
                (pos.x, pos.y)
            })
            .collect();
    }
    fn touch_position(&self, mut pos: Vec2, index: usize, t: f64) -> Vec2 {
        if let Some(next) = self.next_touch[index] {
            let a = self.replay.touches[index];
            let b = self.replay.touches[next];
            if b.phase != replay::PHASE_DOWN && b.t > a.t {
                let p = ((t - a.t as f64) / (b.t - a.t) as f64).clamp(0., 1.) as f32;
                pos = pos.lerp(vec2(b.x, b.y), p);
            }
        }
        pos
    }
    fn checkpoint(&mut self) {
        if !self.checkpoint_step.is_finite() {
            return;
        }
        if self.checkpoints.last().is_some_and(|c| self.current < c.t + self.checkpoint_step) {
            return;
        }
        self.checkpoints.push(Checkpoint {
            t: self.current,
            judge_idx: self.judge_idx,
            touch_idx: self.touch_idx,
            judge: self.game_scene.judge.replay_snapshot(),
            notes: self
                .game_scene
                .chart
                .lines
                .iter()
                .map(|l| l.notes.iter().map(|n| n.judge.clone()).collect())
                .collect(),
            touches: self.current_touches.clone(),
            holds: self.finishing_holds.clone(),
        });
    }
    fn control(&mut self, i: usize) {
        use replay::transport::{error_target, frame_target};
        match i {
            0 => self.restart(),
            1 => self.pending_seek = Some((self.current - 5.).max(0.)),
            2 => self.pending_seek = Some((self.current + 5.).min(self.end())),
            3..=6 => {
                self.playing = false;
                self.pending_seek = Some(frame_target(&self.replay.frames, self.current, [-10, -1, 1, 10][i - 3], self.end()));
            }
            7 => {
                self.a = Some(self.current);
                if self.b.is_some_and(|b| b <= self.current + 0.05) {
                    self.b = None;
                    self.looping = false;
                }
            }
            8 => {
                if self.a.is_some_and(|a| self.current - a >= 0.05) {
                    self.b = Some(self.current);
                    self.looping = true;
                } else {
                    prpr::scene::show_message("先设置 A，再在至少 50ms 后设置 B").warn();
                }
            }
            9 => {
                if self.a.is_some() && self.b.is_some() {
                    self.looping = !self.looping;
                }
            }
            10 => self.fingers = !self.fingers,
            11 => self.expanded = !self.expanded,
            12 | 13 => self.pending_seek = error_target(&self.errors, self.current, i == 13),
            14 => {
                if self.replay.settings.is_some() {
                    self.ranges = !self.ranges;
                } else {
                    prpr::scene::show_message("旧回放未记录判定配置，无法显示原始判定范围").warn();
                }
            }
            15 => self.finger_ids = !self.finger_ids,
            16 => {
                self.game_scene.res.config.touch_point_size = if self.game_scene.res.config.touch_point_size > 0.035 {
                    0.012
                } else {
                    self.game_scene.res.config.touch_point_size + 0.008
                }
            }
            17 => {
                self.game_scene.res.config.touch_point_alpha = if self.game_scene.res.config.touch_point_alpha >= 0.9 {
                    0.25
                } else {
                    self.game_scene.res.config.touch_point_alpha + 0.25
                }
            }
            18 => {
                self.a = None;
                self.b = None;
                self.looping = false;
            }
            19 => self.controls_visible = !self.controls_visible,
            20 => {
                self.info_mode = !self.info_mode;
                self.info_scroll.y_scroller.reset();
            }
            _ => {}
        }
    }
}
struct Checkpoint {
    t: f64,
    judge_idx: usize,
    touch_idx: usize,
    judge: prpr::judge::ReplayJudgeState,
    notes: Vec<Vec<JudgeStatus>>,
    touches: HashMap<u64, (Vec2, usize)>,
    holds: HashMap<(u32, u32), f64>,
}
fn judge_queue_clear(judge: &prpr::judge::Judge) {
    judge.judgements.borrow_mut().clear();
}

impl Scene for ReplayScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        tm.reset();
        // `real_time()` 是「自 app 启动起」的绝对时间，必须在这里对齐，否则首帧 dt 巨大，
        // 回放会一帧直接跳到结尾。
        self.last_t = tm.real_time();
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.saved_playing = if self.drag.is_some() { self.drag_playing } else { self.playing };
        self.playing = false;
        self.drag = None;
        let _ = self.game_scene.music.pause();
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.playing = self.saved_playing;
        tm.resume();
        self.last_t = tm.real_time();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if self.controls[19].touch(touch, t) {
            self.control(19);
            return Ok(true);
        }
        if !self.controls_visible {
            return Ok(false);
        }
        if self.rate.top_touch(touch, t) {
            return Ok(true);
        }
        if matches!(touch.phase, TouchPhase::Started) && self.timeline.contains(touch.position) {
            self.drag = Some(touch.id);
            self.drag_playing = self.playing;
            self.playing = false;
        }
        if self.drag == Some(touch.id) {
            let p = ((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.);
            if !matches!(touch.phase, TouchPhase::Cancelled) {
                self.pending_seek = Some(self.end() * p as f64);
            }
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.drag = None;
                self.playing = self.drag_playing;
            }
            return Ok(true);
        }
        if self.btn_back.touch(touch, t) {
            self.exit = true;
            return Ok(true);
        }
        if self.btn_play.touch(touch, t) {
            if !self.playing && self.current >= self.end() - 0.01 {
                self.restart();
            }
            self.playing = !self.playing;
            return Ok(true);
        }
        if self.rate.touch(touch, t) {
            return Ok(true);
        }
        if self.controls[20].touch(touch, t) {
            self.control(20);
            return Ok(true);
        }
        if self.expanded && self.info_mode && self.info_scroll.touch(touch, t) {
            return Ok(true);
        }
        for i in 0..19 {
            if self.controls[i].touch(touch, t) {
                self.control(i);
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let dt = (tm.real_time() - self.last_t).clamp(0., 0.25);
        self.last_t = tm.real_time();
        self.rate.update(tm.real_time() as f32);
        self.info_scroll.update(tm.real_time() as f32);
        if self.rate.changed() {
            self.speed = replay::transport::RATES[self.rate.selected()];
        }
        if let Some(target) = self.pending_seek.take() {
            self.seek(target);
        }
        self.particle_delta = 0.;
        if self.playing {
            self.particle_delta = dt as f32 * self.speed;
            let target = self.current + dt * self.speed as f64 * self.game_scene.res.config.speed as f64;
            if let Some(target) = replay::transport::loop_target(target, self.a, self.b, self.looping) {
                self.seek(target);
            } else {
                self.current = target.min(self.end());
                if target >= self.end() {
                    self.playing = false;
                }
            }
        }
        let _ = self.game_scene.res.audio.recover_if_needed();
        self.game_scene.res.time = self.current;
        self.game_scene.res.alpha = 1.;
        self.apply_judges(self.current, false);
        self.apply_touches(self.current);
        self.game_scene.touch_points = if self.fingers { self.touch_points.clone() } else { Vec::new() };
        self.game_scene.res.config.chart_debug = self.ranges;
        self.game_scene.res.config.chart_debug_note = self.ranges;
        self.game_scene.tick_replay();
        self.checkpoint();
        self.sync_music();
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        clear_background(Color::new(0.06, 0.08, 0.10, 1.));
        let t = tm.real_time() as f32;
        let height = if !self.controls_visible {
            0.
        } else if self.expanded {
            0.53
        } else {
            0.23
        };
        let vp = ui.viewport;
        let console_pixels = (height * vp.2 as f32 / 2.).round() as i32;
        let chart_vp = (vp.0, vp.1 + console_pixels, vp.2, (vp.3 - console_pixels).max(1));
        {
            let mut chart_ui = Ui::new(ui.text_painter, Some(chart_vp));
            self.game_scene.set_replay_frame_delta(self.particle_delta);
            self.particle_delta = 0.;
            self.game_scene.render(tm, &mut chart_ui)?;
            if self.finger_ids && self.fingers {
                set_camera(&self.game_scene.res.camera);
                for (id, (pos, index)) in &self.current_touches {
                    let pos = self.touch_position(*pos, *index, self.current);
                    chart_ui
                        .text(format!("F{}", self.finger_numbers[id]))
                        .pos(pos.x, pos.y - 0.035)
                        .anchor(0.5, 1.)
                        .size(0.35)
                        .draw();
                }
            }
        }
        set_camera(&ui.camera());
        self.controls[19].render_text(
            ui,
            Rect::new(0.74, -ui.top + 0.065, 0.22, 0.055),
            t,
            if self.controls_visible { "隐藏控制" } else { "显示控制" },
            0.30,
            false,
        );
        if !self.controls_visible {
            self.timeline = Rect::default();
            return Ok(());
        }
        let y = ui.top - height;
        ui.fill_rect(Rect::new(-1., y, 2., height), Color::new(0.08, 0.10, 0.13, 1.));
        let timeline = Rect::new(-0.94, y + 0.025, 1.88, 0.012);
        self.timeline = Rect::new(timeline.x, timeline.y - 0.02, timeline.w, 0.055);
        ui.fill_path(&timeline.rounded(0.006), semi_white(0.2));
        let position = self.pending_seek.unwrap_or(self.current);
        ui.fill_path(&Rect::new(timeline.x, timeline.y, timeline.w * (position / self.end().max(0.001)) as f32, timeline.h).rounded(0.006), WHITE);
        for error in &self.errors {
            ui.fill_rect(Rect::new(timeline.x + timeline.w * (*error / self.end()) as f32, timeline.y - 0.004, 0.002, 0.02), RED);
        }
        for (marker, label) in [(self.a, "A"), (self.b, "B")] {
            if let Some(time) = marker {
                let x = timeline.x + timeline.w * (time / self.end()) as f32;
                ui.fill_rect(Rect::new(x, timeline.y - 0.008, 0.003, 0.028), SKYBLUE);
                ui.text(label).pos(x, y + 0.05).size(0.28).draw();
            }
        }
        let cy = y + 0.062;
        let bh = 0.067;
        self.btn_back.render_text(ui, Rect::new(-0.95, cy, 0.13, bh), t, "返回", 0.36, false);
        self.btn_play
            .render_text(ui, Rect::new(-0.80, cy, 0.15, bh), t, if self.playing { "暂停" } else { "播放" }, 0.36, false);
        for (i, label) in [(0, "重播"), (1, "−5s"), (2, "+5s")] {
            self.controls[i].render_text(ui, Rect::new(-0.63 + i as f32 * 0.15, cy, 0.13, bh), t, label, 0.36, false);
        }
        self.rate.render(ui, Rect::new(-0.16, cy, 0.16, bh), t);
        self.controls[12].render_text(ui, Rect::new(0.02, cy, 0.20, bh), t, "上个失误", 0.33, false);
        self.controls[13].render_text(ui, Rect::new(0.24, cy, 0.20, bh), t, "下个失误", 0.33, false);
        self.controls[11].render_text(ui, Rect::new(0.46, cy, 0.22, bh), t, if self.expanded { "收起分析" } else { "展开分析" }, 0.33, self.expanded);
        ui.text(format!("{} / {}", clock(position), clock(self.end())))
            .pos(0.96, cy + 0.027)
            .anchor(1., 0.5)
            .no_baseline()
            .size(0.34)
            .draw();
        let name = self.replay.meta.player.as_deref().unwrap_or("玩家未记录");
        let diff = if self.replay.has_diffs {
            self.replay.judges[..self.judge_idx]
                .last()
                .map(|e| {
                    if e.kind == 3 {
                        "MISS · 无点击误差".into()
                    } else if self
                        .game_scene
                        .chart
                        .lines
                        .get(e.line as usize)
                        .and_then(|l| l.notes.get(e.note as usize))
                        .is_some_and(|n| matches!(n.kind, prpr::core::NoteKind::Click | prpr::core::NoteKind::Hold { .. }))
                    {
                        format!("{} {:+.2}ms", kind_name(e.head_grade.unwrap_or(e.kind)), e.diff * 1000.)
                    } else {
                        format!("{} · 非计时判定", kind_name(e.kind))
                    }
                })
                .unwrap_or_default()
        } else {
            "旧回放：误差未记录".into()
        };
        let recorded_rate = if self.replay.has_speed {
            format!("录制 {:.2}×", self.game_scene.res.config.speed)
        } else {
            "录制速度未记录".into()
        };
        ui.text(format!("{name} · {recorded_rate} · 观看 {:.2}× · {diff}", self.speed))
            .pos(-0.94, cy + 0.085)
            .size(0.34)
            .max_width(1.88)
            .color(semi_white(0.7))
            .draw();
        if self.expanded {
            let y = cy + 0.14;
            if self.info_mode {
                self.controls[20].render_text(ui, Rect::new(0.66, y, 0.28, 0.06), t, "返回分析", 0.34, true);
                let lines = recording_info(&self.replay, &self.game_scene.res.config);
                self.info_scroll.size((1.56, 0.27));
                ui.scope(|ui| {
                    ui.dx(-0.94);
                    ui.dy(y);
                    self.info_scroll.render(ui, |ui| {
                        for (i, line) in lines.iter().enumerate() {
                            ui.text(line).pos(0., i as f32 * 0.052).size(0.34).max_width(1.54).draw();
                        }
                        (1.56, lines.len() as f32 * 0.052)
                    });
                });
                for index in [3, 4, 5, 6, 7, 8, 9, 10, 14, 15, 16, 17, 18] {
                    self.controls[index].invalidate();
                }
                self.rate.render_top(ui, t, 1.);
                return Ok(());
            }
            let labels = ["−10帧", "−1帧", "+1帧", "+10帧", "设置A", "设置B", "A–B循环", "触点"];
            for (n, label) in labels.iter().enumerate() {
                let index = n + 3;
                self.controls[index].render_text(
                    ui,
                    Rect::new(-0.95 + n as f32 * 0.24, y, 0.22, 0.065),
                    t,
                    *label,
                    0.34,
                    match index {
                        9 => self.looping,
                        10 => self.fingers,
                        _ => false,
                    },
                );
            }
            let y = y + 0.08;
            for (n, (index, label, active)) in [
                (
                    14,
                    if self.replay.settings.is_some() {
                        "判定范围"
                    } else {
                        "范围未记录"
                    },
                    self.ranges,
                ),
                (15, "触点ID", self.finger_ids),
                (16, "触点大小", false),
                (17, "触点透明度", false),
                (18, "清除A/B", false),
            ]
            .iter()
            .enumerate()
            {
                self.controls[*index].render_text(ui, Rect::new(-0.95 + n as f32 * 0.29, y, 0.27, 0.065), t, *label, 0.34, *active);
            }
            let frame = if self.replay.frames.is_empty() {
                "旧回放使用固定 60Hz 参考步长".into()
            } else {
                format!("录制帧 {}/{}", self.replay.frames.partition_point(|t| *t <= self.current), self.replay.frames.len())
            };
            self.controls[20].render_text(ui, Rect::new(0.52, y, 0.42, 0.065), t, "录制信息", 0.34, false);
            ui.text(frame)
                .pos(0.96, y + 0.085)
                .anchor(1., 0.)
                .size(0.29)
                .max_width(0.40)
                .color(semi_white(0.6))
                .draw();
            let info = format!(
                "{} · {} · {} · {}",
                self.replay.meta.name,
                self.replay.meta.level,
                self.replay
                    .meta
                    .recorded_at
                    .map(|time| crate::history::Record { time, ..Default::default() }.time_text())
                    .unwrap_or("时间未记录".into()),
                self.replay.meta.fingerprint.as_ref().map_or("内容指纹未记录", |_| "已校验谱面内容")
            );
            ui.text(info)
                .pos(-0.94, y + 0.08)
                .size(0.32)
                .max_width(1.42)
                .color(semi_white(0.6))
                .draw();
            if self.ranges {
                let windows = self.game_scene.res.config.judge_windows();
                ui.text(format!(
                    "录制窗口(ms)：PERFECT −{:.1}/+{:.1} · GOOD −{:.1}/+{:.1} · BAD −{:.1}/+{:.1}",
                    windows.early[1] * 1000.,
                    windows.late[1] * 1000.,
                    windows.early[2] * 1000.,
                    windows.late[2] * 1000.,
                    windows.early[3] * 1000.,
                    windows.late[3] * 1000.
                ))
                .pos(-0.94, y + 0.13)
                .size(0.30)
                .max_width(1.88)
                .draw();
            }
        } else {
            for index in [3, 4, 5, 6, 7, 8, 9, 10, 14, 15, 16, 17, 18, 20] {
                self.controls[index].invalidate();
            }
        }
        self.rate.render_top(ui, t, 1.);
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if self.exit {
            NextScene::Pop
        } else {
            NextScene::None
        }
    }
}

impl ReplayScene {
    fn end(&self) -> f64 {
        self.game_scene
            .res
            .track_length
            .max(self.replay.judges.last().map_or(0., |e| e.t + 0.001))
    }

    /// 让音乐跟随回放时钟：播放 / 暂停、倍速，并在拖动进度后重新对齐。
    ///
    /// `res.time` 与音乐位置差一个谱面偏移量（`GameScene::offset`），所以目标位置要补上它。
    fn sync_music(&mut self) {
        let target = (self.current + self.game_scene.offset() as f64).max(0.);
        let music = &mut self.game_scene.music;
        if self.playing {
            music.try_set_playback_rate((self.speed * self.game_scene.res.config.speed).max(0.05) as f64);
            if (music.position() - target).abs() > 0.12 {
                let _ = music.seek_to(target);
            }
            if music.paused() {
                let _ = music.play();
            }
        } else if !music.paused() {
            let _ = music.pause();
        } else if (music.position() - target).abs() > 0.12 {
            let _ = music.seek_to(target);
        }
    }
}

fn clock(t: f64) -> String {
    format!("{}:{:05.2}", t.max(0.) as u64 / 60, t.max(0.) % 60.)
}
fn kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "PERFECT",
        1 => "GOOD",
        2 => "BAD",
        3 => "MISS",
        4 => "PERFECT+",
        5 => "HOLD PERFECT",
        6 => "HOLD GOOD",
        7 => "GREAT",
        8 => "OK",
        9 => "MEH",
        _ => "?",
    }
}

fn recording_info(replay: &Replay, config: &Config) -> Vec<String> {
    let on = |v| if v { "开" } else { "关" };
    let mut lines = vec![
        format!(
            "玩家：{} · ID {}（文件记录）",
            replay.meta.player.as_deref().unwrap_or("未记录"),
            replay.meta.player_id.map_or("未记录".into(), |id| id.to_string())
        ),
        format!(
            "时间：{}",
            replay
                .meta
                .recorded_at
                .map_or("未记录".into(), |time| crate::history::Record { time, ..Default::default() }.time_text())
        ),
        format!("谱面：{} · {}", replay.meta.name, replay.meta.level),
        format!("谱面来源：{}", replay.meta.server.as_deref().unwrap_or("未记录 / 本地谱面")),
        format!(
            "谱面版本：{}",
            replay
                .chart_updated
                .map_or("更新时间未记录".into(), |time| crate::history::Record { time, ..Default::default() }.time_text())
        ),
        format!(
            "谱面内容：{} · 音乐：{}",
            if replay.meta.fingerprint.is_some() {
                "已校验"
            } else {
                "指纹未记录"
            },
            if replay.meta.audio_fingerprint.is_some() {
                "已校验"
            } else {
                "指纹未记录"
            }
        ),
        format!(
            "录制速度：{} · 观看速度单独设置",
            if replay.has_speed {
                format!("{:.2}×", config.speed)
            } else {
                "未记录".into()
            }
        ),
        format!("录制偏移：{:+.2}ms · Mods：{:?}", replay.offset * 1000., config.mods),
    ];
    if let Some(result) = &replay.meta.result {
        lines.push(format!("原成绩：{:07} · {:.2}% · MAX COMBO {}", result.score, result.accuracy * 100., result.max_combo));
        if let Some(counts) = result.grade_counts {
            for (id, label) in prpr::judge::visible_grades(result.grading.unwrap_or(replay.grading)) {
                lines.push(format!("{label}：{}", counts[id]));
            }
        } else {
            lines.push(format!("PERFECT(含 PERFECT+) / GOOD / BAD / MISS：{:?}", result.counts));
        }
    } else {
        lines.push("原成绩：未记录".into());
    }
    if let Some(s) = &replay.settings {
        lines.push(format!(
            "判定：{} · PERFECT+ {} · 细致判定 {}",
            if matches!(s.algorithm, prpr::config::JudgeAlgorithm::Phigros) {
                "Phigros"
            } else {
                "Phira Pro"
            },
            on(s.grading.perfect_plus),
            on(s.grading.detailed)
        ));
        let windows = config.judge_windows();
        for (i, label) in ["PERFECT+", "PERFECT", "GOOD", "BAD"].iter().enumerate() {
            lines.push(format!("{label}：−{:.2}ms / +{:.2}ms", windows.early[i] * 1000., windows.late[i] * 1000.));
        }
        if s.grading.detailed {
            for (i, label) in ["GREAT", "OK", "MEH"].iter().enumerate() {
                lines.push(format!("{label}：−{:.2}ms / +{:.2}ms", windows.extended_early[i] * 1000., windows.extended_late[i] * 1000.));
            }
        }
        lines.push(format!("严格判定 {} · 全屏判定 {} · 去连击分 {}", on(s.strict), on(s.fullscreen), on(s.no_combo_score)));
        lines.push(format!("黄键保护 {} · 红键保护 {} · 尾判 {}", on(s.drag_protect), on(s.flick_protect), on(s.hold_tail)));
        lines.push(format!("晚按补偿 {:.2}ms · 理论值计分 {}", s.late_ms, on(replay.theoretical_score)));
        if matches!(s.algorithm, prpr::config::JudgeAlgorithm::Phigros) {
            let p = &s.phigros;
            lines.push(format!(
                "黄键窗口 −{:.2}/+{:.2}ms · 红键倍率 −{:.2}/+{:.2}",
                p.drag_sides()[0],
                p.drag_sides()[1],
                p.flick_sides()[0],
                p.flick_sides()[1]
            ));
            lines.push(format!("点击横向范围 {:.2} · 特殊键范围 {:.2}", p.tap_width, p.special_width));
            lines.push(format!("BAD 边缘 {:.2} · 收缩倍率 {:.2} · 误差除数 {:.2}", p.bad_edge, p.bad_shrink_factor, p.metric_divisor));
            lines.push(format!("保护差值 {:.2}ms · Hold 保护 {}帧", p.protection_ms, p.hold_safe_frames));
            lines.push(format!("Hold 尾部 {:.2}ms · 延迟 Miss {:.2}ms", p.hold_tail_ms, p.hold_delayed_miss_ms));
            lines.push(format!("特殊键提前 {:.2}ms · 帧补偿 {} · 越界延迟 {}", p.special_early_ms, on(p.frame_compensation), on(p.late_overrun)));
            lines.push(format!("严格提前窗口 {:?} · 延后窗口 {:?} ms", p.strict_sides()[0], p.strict_sides()[1]));
            lines.push(format!("滑动速度 {:.3} · 参考 DPI {:.1} · 设备 DPI {:.1}", p.flick_speed, p.flick_dpi, p.device_dpi));
            lines.push(format!("采样 {:.1}Hz · 位移倍率 {:.2} · 最小投影 {:.3}", p.flick_sample_hz, p.flick_multiplier, p.flick_projection_min));
        }
    } else {
        lines.push("旧格式未记录完整判定配置，不能推断原始判定窗口。".into());
    }
    if let Some(g) = &replay.gameplay {
        lines.push(format!("血条 {} · 扣血 {:.2}× · 血量倍率 {:.2}× · 键盘 {}", on(g.hp_mode), g.hp_amount, g.hp_scale, on(g.use_keyboard)));
    }
    lines
}

#[cfg(all(test, target_os = "windows"))]
mod tests;
