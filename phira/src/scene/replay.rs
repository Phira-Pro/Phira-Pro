prpr_l10n::tl_file!("replay");

// 回放播放场景：在 Phira Pro 内直接查看 `.phirar` 回放。
//
// 与官方联机监视器同一套思路：谱面用真正的 `GameScene`（View 模式）渲染，
// 回放时把录制好的判定事件按时间戳重放到谱面上（分数 / 连击 / 音符判定位与原
// 局完全一致），触摸帧用于画出手指触点。

use crate::{replay::{self, ChartRef, Replay}, scene::fs_from_path};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    config::{Config, Mods},
    core::{BadNote, Tweenable, Vector},
    ext::{semi_black, semi_white, BLACK_TEXTURE, RectExt, SafeTexture, ScaleType},
    judge::{JudgeStatus, Judgement as TJ},
    scene::{GameMode, GameScene, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, RectButton, Ui},
};
use std::{collections::HashMap, path::PathBuf};

pub struct ReplayScene {
    game_scene: GameScene,
    replay: Replay,

    // 回放时钟
    current: f64,
    playing: bool,
    speed: f32,

    // 判定 / 触点应用进度
    judge_idx: usize,
    touch_idx: usize,
    touch_points: Vec<(f32, f32)>,
    finishing_holds: HashMap<(u32, u32), f64>,

    // 触点插值用（同 phira-monitor 的 PlayerView）
    current_touches: HashMap<i8, Vec2>,
    last_t: f64,

    // UI
    exit: bool,
    btn_back: DRectButton,
    btn_play: DRectButton,
    btn_speed: DRectButton,
    seek_bar: RectButton,
}

impl ReplayScene {
    pub async fn new(path: PathBuf) -> Result<Self> {
        let replay = replay::load(&path).context("读取回放失败")?;
        let local_path = match &replay.chart {
            Some(ChartRef::Local(p)) => p.clone(),
            Some(ChartRef::Id(id)) => format!("download/{id}"),
            None => anyhow::bail!("回放缺少谱面引用"),
        };
        let mut fs = fs_from_path(&local_path)?;
        let mut info = prpr::fs::load_info(fs.as_mut()).await?;

        // 版本匹配：在线谱面若已更新（本地 chart_updated 与录制时不一致），
        // 用录制时记下的内容寻址 URL 重新下载对应版本的谱面，保证回放对得上。
        if let (Some(ChartRef::Id(_)), Some(url), Some(rec_cu)) = (&replay.chart, &replay.chart_file, replay.chart_updated) {
            let local_cu = info.chart_updated.map(|t| t.timestamp_millis());
            if local_cu != Some(rec_cu) {
                match Self::fetch_archived_chart(url).await {
                    Ok((new_fs, new_info)) => {
                        fs = new_fs;
                        info = new_info;
                    }
                    Err(err) => {
                        tracing::warn!(?err, "failed to fetch archived chart version, falling back to local");
                    }
                }
            }
        }

        info.id = match &replay.chart {
            Some(ChartRef::Id(id)) => Some(*id),
            _ => info.id,
        };
        let mut config = Config::default();
        config.sample_count = 4;
        config.player_name = "REPLAY".to_owned();
        config.offset = replay.offset;
        config.judge_grading = replay.grading;
        config.speed = if replay.speed > 0. { replay.speed } else { 1. };
        config.mods = Mods::from_bits(replay.mods as i32).unwrap_or_default();
        config.interactive = false;

        let (background, illustration) = match LoadingScene::load(fs.as_mut(), &info.illustration).await {
            Ok((ill, bg, _)) => (bg, ill),
            Err(_) => (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
        };

        let mut game_scene = GameScene::new(GameMode::View, info, config, fs, None, background, illustration, None, None, None).await?;

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

        Ok(Self {
            game_scene,
            replay,
            current: 0.,
            playing: true,
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
            btn_speed: DRectButton::new(),
            seek_bar: RectButton::new(),
        })
    }

    /// 下载并解压某个历史版本的谱面到临时目录，返回可用的文件系统与其 info。
    async fn fetch_archived_chart(url: &str) -> Result<(Box<dyn prpr::fs::FileSystem + Send + Sync>, prpr::info::ChartInfo)> {
        let bytes = reqwest::get(url).await?.error_for_status()?.bytes().await?;
        let tmp = std::env::temp_dir().join(format!("phira-replay-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp)?;
        {
            let dir = prpr::dir::Dir::new(&tmp)?;
            prpr::ext::unzip_into(std::io::Cursor::new(bytes.to_vec()), &dir, true)?;
        }
        let mut fs = prpr::fs::fs_from_file(&tmp)?;
        let info = prpr::fs::load_info(fs.as_mut()).await?;
        Ok((fs, info))
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
            scene.res.judge_line_color = scene.res.res_pack.info.color_perfect();
        }
        self.judge_idx = 0;
        self.touch_idx = 0;
        self.current_touches.clear();
        self.touch_points.clear();
        self.finishing_holds.clear();
        self.game_scene.res.time = t;
        self.apply_judges(t, true);
        self.apply_touches(t);
        self.game_scene.touch_points = self.touch_points.clone();
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
                    if !hold_active { note.judge = JudgeStatus::Judged; }
                    judge.commit(ev.t, match ev.kind { 7 => TJ::Great, 8 => TJ::Ok, 9 => TJ::Meh, _ => TJ::Good }, ev.line, ev.note, ev.diff as f64);
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
                        bad_notes.push(BadNote { time: ev.t, kind, matrix: mat });
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
        // Only ongoing, already scored holds are visited. Ending their visual
        // lifetime adds no judgement event and no second score; seek clears this map.
        let chart = &mut self.game_scene.chart;
        self.finishing_holds.retain(|&(line, id), tail| {
            let note = &mut chart.lines[line as usize].notes[id as usize];
            if t >= *tail { note.judge = JudgeStatus::Judged; }
            !matches!(note.judge, JudgeStatus::Judged)
        });
    }

    /// 应用 `t` 时刻的触点（用于画手指圆点，做帧间插值）。
    fn apply_touches(&mut self, t: f64) {
        let touches = &self.replay.touches;
        let mut changed = false;
        while self.touch_idx < touches.len() && touches[self.touch_idx].t as f64 <= t {
            let ev = touches[self.touch_idx];
            self.touch_idx += 1;
            if ev.phase == replay::PHASE_DOWN || ev.phase == replay::PHASE_MOVE {
                self.current_touches.insert(ev.id, Vec2::new(ev.x, ev.y));
            } else {
                self.current_touches.remove(&ev.id);
            }
            changed = true;
        }
        if changed {
            let mut pts = Vec::new();
            match touches.get(self.touch_idx) {
                Some(next) => {
                    let nt = next.t as f64;
                    for (id, old) in self.current_touches.iter() {
                        // 找该触点最近一次更新的时间作为插值起点
                        let st = touches[..self.touch_idx].iter().rev().find(|it| it.id == *id).map_or(nt, |it| it.t as f64);
                        let p = if (nt - st).abs() < 1e-6 { 1. } else { ((t - st) / (nt - st)).clamp(0., 1.) as f32 };
                        pts.push(Vec2::tween(old, &Vec2::new(next.x, next.y), p));
                    }
                }
                None => {
                    pts.extend(self.current_touches.values().cloned());
                }
            }
            self.touch_points = pts.into_iter().map(|it| (it.x, it.y)).collect();
        }
    }
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
        self.playing = false;
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.playing = true;
        tm.resume();
        self.last_t = tm.real_time();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if self.btn_back.touch(touch, t) {
            self.exit = true;
            return Ok(true);
        }
        if self.btn_play.touch(touch, t) {
            if !self.playing && self.current >= (self.end() - 0.01).max(0.) {
                self.restart();
            }
            self.playing = !self.playing;
            return Ok(true);
        }
        if self.btn_speed.touch(touch, t) {
            self.speed = if (self.speed - 1.).abs() < 1e-3 { 1.5 } else if (self.speed - 1.5).abs() < 1e-3 { 2. } else { 1. };
            return Ok(true);
        }
        if self.seek_bar.touch(touch) {
            // 复用游玩界面顶部自带的进度条热区：按横向位置跳转（x ∈ [-1, 1]）
            let p = ((touch.position.x + 1.) / 2.).clamp(0., 1.);
            let end = self.end();
            let t = (end * p as f64).min((end - 0.01).max(0.));
            self.seek(t);
            self.playing = true;
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        // 卡顿 / 失焦恢复后可能产生很大的间隔，钳一下避免进度突跳。
        let dt = (tm.real_time() - self.last_t).clamp(0., 0.25);
        self.last_t = tm.real_time();
        if self.playing {
            self.current += dt * self.speed as f64;
            if self.current >= self.end() {
                self.current = self.end();
                self.playing = false;
            }
        }
        let current = self.current.min(self.end());

        let _ = self.game_scene.res.audio.recover_if_needed();

        // 同步谱面时间与不透明度
        self.game_scene.res.time = current;
        self.game_scene.res.alpha = 1.;

        self.apply_judges(current, false);
        self.apply_touches(current);
        self.game_scene.touch_points = self.touch_points.clone();

        // 关键：状态机之外的刷新（谱面 line 缓存 / 事件 / 特效 / 判定线颜色）。
        // 少了这一步，谱面就只是静态贴图，音符不会按事件移动。
        self.game_scene.tick_replay();

        self.sync_music();
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let mut cam = ui.camera();
        cam.render_target = self.game_scene.res.camera.render_target;
        set_camera(&cam);

        self.game_scene.render(tm, ui)?;

        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let top = 1. / asp;
        cam.render_target = None;
        set_camera(&cam);

        let t = tm.real_time() as f32;

        // 不再单画进度条：直接复用游玩界面顶部自带的谱面进度条作为拖拽热区。
        self.seek_bar.set(ui, Rect::new(-1., -top - 0.01, 2., 0.06));

        let icon_back = self.game_scene.res.icon_back.clone();
        let icon_resume = self.game_scene.res.icon_resume.clone();

        // 底部居中的小巧图标控制条：返回 / 播放·暂停 / 倍速
        let s = 0.045;
        let cy = top - 0.13;
        let step = 0.15;
        let c = semi_white(0.92);

        let back_r = Rect::new(-step - s, cy - s, s * 2., s * 2.);
        let play_r = Rect::new(-s, cy - s, s * 2., s * 2.);
        let speed_r = Rect::new(step - 0.065, cy - 0.045, 0.13, 0.09);

        Self::icon_button(&mut self.btn_back, ui, back_r, t, icon_back, c);

        let playing = self.playing;
        self.btn_play.build(ui, t, play_r, |ui, path| {
            ui.fill_path(&path, semi_black(0.4));
            let ir = play_r.feather(-play_r.w * 0.26);
            if playing {
                // 暂停：两根竖条
                let bw = ir.w * 0.24;
                let bh = ir.h * 0.9;
                let y = ir.y + (ir.h - bh) / 2.;
                ui.fill_rect(Rect::new(ir.x + ir.w * 0.3 - bw / 2., y, bw, bh), c);
                ui.fill_rect(Rect::new(ir.x + ir.w * 0.7 - bw / 2., y, bw, bh), c);
            } else {
                ui.fill_rect(ir, (*icon_resume, ir.feather(0.01), ScaleType::Fit, c));
            }
        });

        self.btn_speed
            .render_text(ui, speed_r, t, format!("{:.1}x", self.speed), 0.45, false);

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
        self.game_scene.res.track_length
    }

    /// 让音乐跟随回放时钟：播放 / 暂停、倍速，并在拖动进度后重新对齐。
    ///
    /// `res.time` 与音乐位置差一个谱面偏移量（`GameScene::offset`），所以目标位置要补上它。
    fn sync_music(&mut self) {
        let target = (self.current + self.game_scene.offset() as f64).max(0.);
        let music = &mut self.game_scene.music;
        if self.playing {
            music.try_set_playback_rate(self.speed.max(0.05) as f64);
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

    /// 画一个圆角底 + 居中图标的小按钮（比整块文字按钮紧凑）。
    fn icon_button(btn: &mut DRectButton, ui: &mut Ui, r: Rect, t: f32, tex: SafeTexture, c: Color) {
        let ir = r.feather(-r.w * 0.26);
        btn.build(ui, t, r, |ui, path| {
            ui.fill_path(&path, semi_black(0.4));
            ui.fill_rect(ir, (*tex, ir.feather(0.01), ScaleType::Fit, c));
        });
    }
}
