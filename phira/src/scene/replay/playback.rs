//! Shared playback state for the interactive viewer and offline video.
use crate::replay::{self, ChartRef, Replay};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    chart_video::ReplayVideoSource,
    config::{Config, Mods},
    core::{BadNote, NoteKind, Vector},
    fs::FileSystem,
    info::ChartInfo,
    judge::{JudgeStatus, Judgement as TJ},
    scene::GameScene,
    ui::Ui,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub(super) struct Playback {
    pub(super) replay: Replay,
    pub(super) judge_idx: usize,
    pub(super) touch_idx: usize,
    pub(super) block_frame_idx: usize,
    pub(super) current_touches: HashMap<u64, (Vec2, usize)>,
    pub(super) finishing_holds: HashMap<(u32, u32), f64>,
    next_touch: Vec<Option<usize>>,
    pub(super) finger_numbers: Vec<usize>,
}

pub(super) async fn verify_content(replay: &Replay, fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<()> {
    if let Some(expected) = &replay.meta.fingerprint {
        anyhow::ensure!(replay::library::fingerprint(fs, info).await? == *expected, "谱面内容与回放不一致");
    }
    if let Some(expected) = &replay.meta.audio_fingerprint {
        anyhow::ensure!(replay::library::audio_fingerprint(fs, info).await? == *expected, "音乐版本与回放不一致");
    }
    Ok(())
}

pub(super) fn configure(replay: &Replay, config: &mut Config, info: &mut ChartInfo) -> bool {
    if let Some(settings) = &replay.settings {
        settings.apply(config);
    } else {
        *config = Config::default();
    }
    if let Some(gameplay) = &replay.gameplay {
        gameplay.apply(config);
    } else {
        config.hp_mode = false;
    }
    if !replay.gameplay.as_ref().is_some_and(|settings| settings.input_time_corrected) {
        config.input_latency_compensation_ms = 0.;
    }
    // Judgement/playback settings belong to the replay; note textures and
    // sounds come from the viewer's current resource pack, as in gameplay.
    config.player_name = replay.meta.player.clone().unwrap_or_else(|| "未记录玩家".into());
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
    nightcore
}

impl Playback {
    pub(super) fn new(replay: Replay) -> Self {
        let mut next_touch = vec![None; replay.touches.len()];
        let finger_numbers = replay::contact_numbers(&replay.touches);
        let mut next = HashMap::new();
        for (i, e) in replay.touches.iter().enumerate().rev() {
            next_touch[i] = next.insert(e.id, i);
        }
        Self {
            replay,
            judge_idx: 0,
            touch_idx: 0,
            block_frame_idx: 0,
            current_touches: HashMap::new(),
            finishing_holds: HashMap::new(),
            next_touch,
            finger_numbers,
        }
    }

    pub(super) fn validate_notes(&self, game: &GameScene) -> Result<()> {
        anyhow::ensure!(
            self.replay
                .judges
                .iter()
                .all(|e| game.chart.lines.get(e.line as usize).is_some_and(|l| (e.note as usize) < l.notes.len())),
            "回放的音符索引与谱面不一致"
        );
        Ok(())
    }

    pub(super) fn apply_judges(&mut self, game: &mut GameScene, t: f64, silent: bool, audio: bool) {
        while self.judge_idx < self.replay.judges.len() {
            let ev = self.replay.judges[self.judge_idx];
            if ev.t > t {
                break;
            }
            self.judge_idx += 1;
            let GameScene {
                chart,
                judge,
                res,
                bad_notes,
                ..
            } = game;
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
            if audio && hit_sound(ev.kind, &note.kind) {
                note.hitsound.play(res);
            }
            let hold_success = matches!(ev.kind, 0 | 1 | 4 | 7 | 8 | 9) && matches!(note.kind, prpr::core::NoteKind::Hold { .. });
            let hold_active = if hold_success {
                let prpr::core::NoteKind::Hold { end_time, .. } = note.kind else {
                    unreachable!()
                };
                let active = note.judge.finish_hold_score(ev.t, end_time);
                if active {
                    self.finishing_holds.insert((ev.line, ev.note), end_time);
                }
                active
            } else {
                false
            };
            match ev.kind {
                0 | 4 => {
                    if !hold_active {
                        note.judge = JudgeStatus::Judged;
                    }
                    let tj = if ev.kind == 4 { TJ::PerfectPlus } else { TJ::Perfect };
                    judge.commit(ev.t, tj, ev.line, ev.note, ev.diff as f64);
                    if !silent && !hold_success {
                        let fx = res.config.judge_grading.color(tj as usize, res.res_pack.info.fx_perfect());
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
                        let grade = match ev.kind {
                            7 => TJ::Great,
                            8 => TJ::Ok,
                            9 => TJ::Meh,
                            _ => TJ::Good,
                        };
                        let fx = res.config.judge_grading.color(grade as usize, res.res_pack.info.fx_good());
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
                    note.judge = JudgeStatus::Hold(
                        true,
                        ev.t,
                        0.,
                        false,
                        f64::INFINITY,
                        match ev.head_grade {
                            Some(4) => TJ::PerfectPlus,
                            _ => TJ::Perfect,
                        },
                    );
                }
                6 => {
                    note.judge = JudgeStatus::Hold(
                        false,
                        ev.t,
                        0.,
                        false,
                        f64::INFINITY,
                        match ev.head_grade {
                            Some(7) => TJ::Great,
                            Some(8) => TJ::Ok,
                            Some(9) => TJ::Meh,
                            _ => TJ::Good,
                        },
                    );
                }
                _ => {}
            }
        }
        game.judge.judgements.borrow_mut().clear();
        // Only ongoing, already scored holds are visited. Ending their visual
        // lifetime adds no judgement event and no second score; seek clears this map.
        let chart = &mut game.chart;
        self.finishing_holds.retain(|&(line, id), tail| {
            let note = &mut chart.lines[line as usize].notes[id as usize];
            if t >= *tail {
                note.judge = JudgeStatus::Judged;
            }
            !matches!(note.judge, JudgeStatus::Judged)
        });
    }

    pub(super) fn apply_touches(&mut self, game: &mut GameScene, t: f64) {
        let noise = game.chart.has_block_areas();
        // Replay recorded frame times too: a stationary finger can become
        // blocked when an animated zone moves underneath it between events.
        loop {
            let event_time = self.replay.touches.get(self.touch_idx).map_or(f64::INFINITY, |e| e.t as f64);
            let frame_time = if noise {
                self.replay.frames.get(self.block_frame_idx).copied().unwrap_or(f64::INFINITY)
            } else {
                f64::INFINITY
            };
            let at = event_time.min(frame_time);
            if at > t || !at.is_finite() {
                break;
            }
            if frame_time < event_time {
                self.block_frame_idx += 1;
                self.update_block_touches(game, at);
                continue;
            }
            let index = self.touch_idx;
            let event = self.replay.touches[index];
            self.touch_idx += 1;
            if event.phase == replay::PHASE_UP {
                self.current_touches.remove(&event.id);
            } else {
                if event.phase == replay::PHASE_DOWN {
                    game.judge.blocked_finger_ids.remove(&event.id);
                }
                self.current_touches.insert(event.id, (vec2(event.x, event.y), index));
            }
            if noise {
                self.update_block_touches(game, at);
            }
        }
        self.update_block_touches(game, t);
    }
    fn update_block_touches(&mut self, game: &mut GameScene, t: f64) {
        let touches = self
            .current_touches
            .iter()
            .map(|(&id, &(p, i))| (id, self.touch_position(p, i, t)))
            .collect::<Vec<_>>();
        game.res.time = t;
        game.update_replay_block_touches(&touches);
    }

    pub(super) fn touch_position(&self, mut pos: Vec2, index: usize, t: f64) -> Vec2 {
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
    /// Keep a released tap visible briefly, including DOWN/UP processed in one
    /// playback update. This is a visual tail, never an active gameplay touch.
    pub(super) fn visible_touches(&self, time: f64) -> Vec<(u64, Vec2, f32)> {
        let mut points = HashMap::new();
        for (index, event) in self.replay.touches[..self.touch_idx]
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, e)| time - e.t as f64 <= 0.08)
        {
            if event.phase == replay::PHASE_UP {
                points
                    .entry(self.finger_numbers[index] as u64)
                    .or_insert((vec2(event.x, event.y), (1. - (time - event.t as f64) / 0.08) as f32));
            }
        }
        for &(pos, index) in self.current_touches.values() {
            points.insert(self.finger_numbers[index] as u64, (self.touch_position(pos, index, time), 1.));
        }
        let mut points = points.into_iter().map(|(id, (pos, alpha))| (id, pos, alpha)).collect::<Vec<_>>();
        points.sort_by_key(|p| p.0);
        points
    }
}

fn hit_sound(kind: u8, note: &NoteKind) -> bool {
    matches!(kind, 5 | 6) || (matches!(kind, 0 | 1 | 4 | 7 | 8 | 9) && !matches!(note, NoteKind::Hold { .. }))
}

pub(crate) struct VideoReplay(Playback);

#[cfg(test)]
mod video_tests {
    use super::*;

    #[test]
    fn video_replay_restores_the_life_rules_from_the_ppr() {
        let mut tape = Replay {
            speed: 1.,
            settings: Some(Default::default()),
            gameplay: Some(replay::Gameplay {
                hp_mode: true,
                hp_disable_healing: true,
                hp_scale: 100. / 12.,
                hp_amount: 2.5,
                points_damage: true,
                input_time_corrected: true,
                use_keyboard: false,
                note_scale: 1.,
                flow_speed: 1.,
                fade_strength: 0.,
            }),
            ..Default::default()
        };
        let mut config = Config::default();
        let mut info = ChartInfo::default();
        let decoded = replay::from_bytes(&replay::bytes(&tape).unwrap()).unwrap();
        VideoReplay(Playback::new(decoded)).configure(&mut config, &mut info);
        assert!(config.hp_mode && config.hp_disable_healing && !config.legacy_hp_damage);
        assert_eq!(config.hp_capacity(), 12);
        assert_eq!(config.hp_damage(), 2.5);

        let gameplay = tape.gameplay.as_mut().unwrap();
        gameplay.points_damage = false;
        gameplay.hp_disable_healing = false;
        let decoded = replay::from_bytes(&replay::bytes(&tape).unwrap()).unwrap();
        VideoReplay(Playback::new(decoded)).configure(&mut config, &mut info);
        assert!(config.hp_mode && config.legacy_hp_damage && !config.hp_disable_healing);

        tape.gameplay = None;
        let decoded = replay::from_bytes(&replay::bytes(&tape).unwrap()).unwrap();
        VideoReplay(Playback::new(decoded)).configure(&mut config, &mut info);
        assert!(!config.hp_mode);
    }
}

impl VideoReplay {
    pub(crate) async fn load(path: PathBuf, chart: &Path, chart_id: Option<i32>) -> Result<Self> {
        let replay = tokio::task::spawn_blocking(move || replay::load(&path)).await?.context("读取回放失败")?;
        replay.ensure_playable()?;
        let mut fs = prpr::fs::fs_from_file(chart).context("无法打开回放对应的谱面")?;
        let info = prpr::fs::load_info(fs.as_mut()).await?;
        if let (Some(ChartRef::Id(expected)), Some(id)) = (&replay.chart, chart_id.or(info.id)) {
            anyhow::ensure!(*expected == id, "所选谱面与回放记录的谱面编号不一致");
        }
        verify_content(&replay, fs.as_mut(), &info).await?;
        if replay.meta.fingerprint.is_none() {
            prpr::scene::show_message("此回放未记录谱面指纹，请确认所选谱面为录制版本").warn();
        }
        Ok(Self(Playback::new(replay)))
    }
}

impl ReplayVideoSource for VideoReplay {
    fn configure(&self, config: &mut Config, info: &mut ChartInfo) {
        configure(&self.0.replay, config, info);
    }

    fn validate(&self, scene: &GameScene) -> Result<()> {
        self.0.validate_notes(scene)
    }

    fn advance(&mut self, scene: &mut GameScene, chart_time: f64) {
        // Judge effects use transforms at the recorded hit time, independent of export FPS.
        while let Some(event) = self.0.replay.judges.get(self.0.judge_idx).filter(|event| event.t <= chart_time) {
            let time = event.t;
            scene.res.time = time;
            scene.tick_replay();
            self.0.apply_judges(scene, time, false, false);
        }
        self.0.apply_judges(scene, chart_time, false, false);
        scene.res.time = chart_time;
        self.0.apply_touches(scene, chart_time);
    }

    fn draw_touches(&self, scene: &GameScene, ui: &mut Ui) {
        let fitted = scene.res.camera.viewport.unwrap_or(ui.viewport);
        let mut touch_ui = Ui::new(ui.text_painter, Some(fitted));
        set_camera(&Camera2D {
            render_target: scene.res.camera.render_target,
            ..touch_ui.camera()
        });
        let config = &scene.res.config;
        for (_, pos, fade) in self.0.visible_touches(scene.res.time) {
            touch_ui.fill_circle(
                pos.x,
                pos.y,
                config.touch_point_size,
                Color {
                    a: config.touch_point_alpha * fade,
                    ..BLUE
                },
            );
        }
    }

    fn hits(&self, scene: &GameScene) -> Vec<(f64, u32, u32)> {
        let mut seen = HashSet::new();
        self.0
            .replay
            .judges
            .iter()
            .filter_map(|event| {
                let note = &scene.chart.lines[event.line as usize].notes[event.note as usize];
                (hit_sound(event.kind, &note.kind) && seen.insert((event.line, event.note))).then_some((event.t, event.line, event.note))
            })
            .collect()
    }
}
