//! 回放播放场景。
//!
//! 谱面本体由 `prpr::scene::GameScene`（View 模式）绘制，本场景只负责：
//! 1. 按回放时钟推进 `res.time`；
//! 2. 把录制好的判定事件提交给 `Judge`，恢复分数 / 连击 / 长条状态；
//! 3. 插值并绘制触点；
//! 4. 提供播放、拖动、倍速等本地控制台。
//!
//! 这套流程与 Phira Pro 的 `ReplayScene` 一致，但不依赖应用内的谱面库、
//! 账号或网络服务。

use super::replay::{self, Replay, PHASE_UP};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    chart_video::VisualOptions,
    config::{Config, Mods},
    core::{BadNote, NoteKind, Vector},
    ext::{semi_white, RectExt, SafeTexture},
    fs,
    info::ChartInfo,
    judge::{JudgeStatus, Judgement as TJ, PlayResult},
    scene::{GameMode, GameScene, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, Ui},
};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf};

/// 观看倍率，与 Phira Pro 回放控制台保持同一组档位。
const RATES: [f32; 8] = [0.1, 0.25, 0.5, 0.75, 1., 1.5, 2., 3.];

/// 底部控制台高度占窗口高度的比例。
const CONSOLE_HEIGHT: f32 = 0.25;

pub struct ReplayScene {
    game_scene: GameScene,
    replay: Replay,
    #[allow(dead_code)]
    chart_path: PathBuf,
    title: String,
    warning: Option<String>,

    // 回放时钟
    current: f64,
    playing: bool,
    speed: f32,
    particle_delta: f32,
    last_t: f64,

    // 判定 / 触点应用进度
    judge_idx: usize,
    touch_idx: usize,
    block_frame_idx: usize,
    finishing_holds: HashMap<(u32, u32), f64>,
    current_touches: HashMap<u64, (Vec2, usize)>,
    next_touch: Vec<Option<usize>>,
    finger_numbers: Vec<usize>,
    errors: Vec<f64>,
    /// 视频导出时混音由 `video` 模块负责，场景内的实时打击音需要关闭。
    play_hitsounds: bool,

    // UI
    exit: bool,
    controls_visible: bool,
    help: bool,
    fingers: bool,
    finger_ids: bool,
    rate_index: usize,
    buttons: Vec<DRectButton>,
    hide_button: DRectButton,
    timeline: Rect,
    drag: Option<u64>,
    drag_playing: bool,
    pending_seek: Option<f64>,
    selected_error: Option<usize>,
    message: Option<(String, f64)>,
}

impl ReplayScene {
    pub async fn new(replay_path: PathBuf, chart_path: PathBuf) -> Result<Self> {
        let replay = replay::load(&replay_path)?;
        replay.ensure_playable()?;

        verbose(&format!(
            "loaded replay: {} judges, {} touches",
            replay.judges.len(),
            replay.touches.len()
        ));
        let mut fs = fs::fs_from_file(&chart_path)
            .with_context(|| format!("无法打开谱面 {}", chart_path.display()))?;
        verbose("opened chart filesystem");
        let mut info = fs::load_info(fs.as_mut()).await?;
        verbose(&format!("loaded chart info: {}", info.name));
        info.id = replay.chart_id().or(info.id);

        let mut warning = None;
        if let Some(expected) = &replay.meta.fingerprint {
            let chart = GameScene::load_chart_bytes(fs.as_mut(), &info).await?;
            verbose(&format!("loaded chart bytes: {}", chart.len()));
            let extra = fs.load_file("extra.json").await.ok();
            let mut hash = Sha256::new();
            hash.update(b"PhiraReplayChart1\0");
            hash.update((chart.len() as u64).to_le_bytes());
            hash.update(&chart);
            if let Some(extra) = extra {
                hash.update((extra.len() as u64).to_le_bytes());
                hash.update(extra);
            }
            hash.update(info.offset.to_le_bytes());
            if format!("{:x}", hash.finalize()) != *expected {
                warning = Some("谱面内容与回放记录不一致，播放结果可能与原成绩不同".into());
            }
        }
        if let Some(expected) = &replay.meta.audio_fingerprint {
            match fs.load_file(&info.music).await {
                Ok(bytes) if format!("{:x}", Sha256::digest(&bytes)) != *expected => {
                    warning = Some("音乐版本与回放记录不一致".into());
                }
                Err(_) => warning = Some("找不到回放所需的音乐文件".into()),
                _ => {}
            }
        }
        verbose("verified fingerprints");

        let mut config = Config::default();
        if let Some(settings) = &replay.settings {
            settings.apply(&mut config);
        }
        if let Some(gameplay) = &replay.gameplay {
            gameplay.apply(&mut config);
        }
        if !replay
            .gameplay
            .as_ref()
            .is_some_and(|it| it.input_time_corrected)
        {
            config.input_latency_compensation_ms = 0.;
        }
        config.sample_count = 4;
        config.player_name = replay
            .meta
            .player
            .clone()
            .unwrap_or_else(|| "REPLAY".into());
        config.theoretical_score = replay.theoretical_score;
        config.offset = replay.offset;
        config.judge_grading = replay.grading;
        config.speed = if replay.speed > 0. { replay.speed } else { 1. };
        config.mods = Mods::from_bits(replay.mods as i32).unwrap_or_default();
        let nightcore = config.mods.contains(Mods::NIGHTCORE);
        // v5 保存的是 Mod 生效后的实际速度；旧录像保存基础速度。
        if replay.settings.is_none() && nightcore {
            config.speed *= 1.5;
        }
        config.mods.remove(Mods::NIGHTCORE);
        config.interactive = false;
        config.touch_debug = false;
        config.offset_indicator = replay.has_diffs;
        config.adjust_time = false;
        if let Some(aspect) = replay.aspect_ratio {
            config.aspect_ratio = Some(aspect);
            info.force_aspect_ratio = true;
        }

        let (background, illustration) =
            match LoadingScene::load(fs.as_mut(), &info.illustration).await {
                Ok((ill, bg, _)) => (bg, ill),
                Err(err) => {
                    warning = Some(format!("曲绘加载失败：{err}"));
                    let texture: prpr::ext::SafeTexture =
                        Texture2D::from_rgba8(1, 1, &[24, 28, 36, 255]).into();
                    (texture.clone(), texture)
                }
            };
        verbose("loaded illustration");

        let mut game_scene = GameScene::new(
            GameMode::View,
            info,
            config,
            fs,
            None,
            background,
            illustration,
            None,
            None,
            None,
        )
        .await?;
        verbose("created game scene");
        game_scene.res.config.mods.set(Mods::NIGHTCORE, nightcore);

        anyhow::ensure!(
            replay.judges.iter().all(|e| game_scene
                .chart
                .lines
                .get(e.line as usize)
                .is_some_and(|l| (e.note as usize) < l.notes.len())),
            "回放的音符索引与谱面不一致，请确认选择的是录制时的同一版本"
        );
        anyhow::ensure!(
            replay
                .touches
                .iter()
                .all(|e| e.x.is_finite() && e.y.is_finite()),
            "回放的触点坐标无效"
        );

        // 回放不跑正常状态机时钟；手动推过 Starting / BeforeMusic 到 Playing，
        // 否则 HUD 会停留在开场进度并整体错位。
        let mut tm = TimeManager::from_config(&game_scene.res.config);
        game_scene.enter(&mut tm, None)?;
        verbose("entered game scene");
        tm.seek_to(GameScene::BEFORE_TIME + 0.1);
        game_scene.update(&mut tm)?;
        tm.seek_to(0.1);
        game_scene.update(&mut tm)?;
        verbose("advanced game scene to playing");
        let _ = game_scene.music.pause();
        let _ = game_scene.music.seek_to(game_scene.offset() as f64);

        let finger_numbers = replay::contact_numbers(&replay.touches);
        let mut next_touch = vec![None; replay.touches.len()];
        let mut next = HashMap::new();
        for (i, event) in replay.touches.iter().enumerate().rev() {
            next_touch[i] = next.insert(event.id, i);
        }
        let mut errors = replay
            .judges
            .iter()
            .filter(|e| matches!(e.kind, 2 | 3))
            .map(|e| e.t)
            .collect::<Vec<_>>();
        errors.dedup_by(|a, b| (*a - *b).abs() < 1e-7);

        let title = format!(
            "{} · {}",
            replay.meta.player.as_deref().unwrap_or("未记录玩家"),
            replay.meta.name
        );

        Ok(Self {
            game_scene,
            replay,
            chart_path,
            title,
            warning,
            current: 0.,
            playing: false,
            speed: 1.,
            particle_delta: 0.,
            last_t: 0.,
            judge_idx: 0,
            touch_idx: 0,
            block_frame_idx: 0,
            finishing_holds: HashMap::new(),
            current_touches: HashMap::new(),
            next_touch,
            finger_numbers,
            errors,
            play_hitsounds: true,
            exit: false,
            controls_visible: true,
            help: false,
            fingers: true,
            finger_ids: true,
            rate_index: 4,
            buttons: (0..10).map(|_| DRectButton::new()).collect(),
            hide_button: DRectButton::new(),
            timeline: Rect::default(),
            drag: None,
            drag_playing: false,
            pending_seek: None,
            selected_error: None,
            message: None,
        })
    }

    fn end(&self) -> f64 {
        self.game_scene
            .res
            .track_length
            .max(self.replay.judges.last().map_or(0., |e| e.t + 0.001))
    }

    /// 视频导出的谱面时间范围（秒）。
    pub fn video_end(&self) -> f64 {
        self.end()
    }

    /// 回放录制时的实际播放速率。视频按这个速率推进，观感与游玩时一致。
    pub fn video_speed(&self) -> f64 {
        self.game_scene.res.config.speed.max(0.01) as f64
    }

    /// 谱面时间与音乐时间之间的偏移。
    pub fn video_offset(&self) -> f64 {
        self.game_scene.offset() as f64
    }

    pub fn video_title(&self) -> &str {
        &self.title
    }

    #[allow(dead_code)]
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn video_music(&self) -> sasa::AudioClip {
        self.game_scene.res.music.clone()
    }

    /// Phira Pro 谱面渲染使用的曲绘、背景、头像与判定图标。
    pub fn video_background(&self) -> SafeTexture {
        self.game_scene.res.background.clone()
    }

    pub fn video_illustration(&self) -> SafeTexture {
        self.game_scene.res.illustration.clone()
    }

    pub fn video_avatar(&self) -> SafeTexture {
        self.game_scene.res.player.clone()
    }

    pub fn video_icons(&self) -> [SafeTexture; 8] {
        self.game_scene.res.icons.clone()
    }

    pub fn video_ending_audio(&self) -> sasa::AudioClip {
        self.game_scene.res.res_pack.ending.clone()
    }

    /// 展示用谱面信息；演出文本由调用方覆盖，不影响游戏内 HUD。
    pub fn video_info(&self) -> ChartInfo {
        self.game_scene.res.info.clone()
    }

    /// 回放记录中的玩家名，仅用于 Phira Pro 结算画面。
    pub fn video_player(&self) -> Option<&str> {
        self.replay.meta.player.as_deref()
    }

    /// 当前成绩，供 Phira Pro 结算画面显示。
    ///
    /// 回放文件记录了完整成绩时优先使用它，这样导出局部片段时结算面板仍然
    /// 显示这次游玩的总分，而不是片段内已提交判定的分数。
    pub fn video_result(&self) -> PlayResult {
        let no_combo = self
            .game_scene
            .res
            .config
            .mods
            .contains(Mods::NO_COMBO_SCORE);
        let mut result = self.game_scene.judge.result(no_combo);
        if let Some(record) = &self.replay.meta.result {
            result.score = record.score;
            result.accuracy = record.accuracy;
            result.max_combo = record.max_combo;
            result.num_of_notes = record.num_of_notes;
            result.counts[..4].copy_from_slice(&record.counts);
            // Phira Pro 的结算把 PerfectPlus 与 Perfect 合并显示。
            result.counts[4] = 0;
        }
        // 结算面板的 Early / Late 来自判定器的 Good 偏差记录。导出区间可能
        // 从中间开始，所以这里按整份回放重新统计，避免显示成 0 / 0。
        if self.replay.has_diffs {
            let (mut early, mut late) = (0, 0);
            for ev in &self.replay.judges {
                // `apply_judges` 只有 Good（kind 1）会写入 Judge::diffs。
                if ev.kind == 1 {
                    if ev.diff < 0. {
                        early += 1;
                    } else {
                        late += 1;
                    }
                }
            }
            result.early = early;
            result.late = late;
        }
        result
    }

    /// 应用 Phira Pro 谱面渲染的视觉选项。字段均在 `prpr` 中公开。
    pub fn apply_video_visuals(&mut self, visuals: &VisualOptions, show_hud: bool) {
        let config = &mut self.game_scene.res.config;
        config.offset_indicator = visuals.offset_visible(show_hud);
        config.ap_fc_indicator = visuals.ap_fc_indicator;
        config.double_hint = visuals.double_hint;
        config.particle = visuals.particle;
        config.show_acc = visuals.show_acc;
        config.show_judgement_details = visuals.show_judgement_details;
        config.show_perfect_combo = visuals.show_perfect_combo;
        config.show_early_late = visuals.show_early_late;
    }

    /// 准备离线渲染：冻结实时播放、关闭场景内音效，并从 `start` 稍早处重建状态。
    pub fn prepare_video(&mut self, start: f64, show_hud: bool) {
        self.play_hitsounds = false;
        self.playing = false;
        self.game_scene.hide_ui = !show_hud;
        self.game_scene.res.config.volume_music = 0.;
        self.game_scene.res.config.volume_sfx = 0.;
        self.game_scene.res.config.chart_debug = false;
        self.game_scene.res.config.chart_debug_note = false;
        self.rewind_to(start - 1e-6);
        self.current = start;
        let _ = self.game_scene.music.pause();
    }

    /// 推进到谱面时间 `t`。`frame_delta` 为输出帧的真实间隔，用于粒子动画。
    pub fn video_frame(&mut self, t: f64, frame_delta: f32) {
        self.current = t;
        self.game_scene.set_video_frame(t, 1., frame_delta);
        self.game_scene.touch_points.clear();
        self.apply_judges(t, false);
        self.apply_touches(t);
        self.game_scene.tick_replay();
    }

    pub fn set_video_target(&mut self, target: RenderTarget) {
        self.game_scene.res.camera.render_target = Some(target);
    }

    /// 只绘制谱面与可选触点，不绘制播放器控制台和警告条。
    pub fn render_video(
        &mut self,
        tm: &mut TimeManager,
        ui: &mut Ui,
        target: RenderTarget,
        show_touches: bool,
    ) -> Result<()> {
        self.game_scene.render(tm, ui)?;
        if !show_touches || self.replay.touches.is_empty() {
            return Ok(());
        }
        let vp = ui.viewport;
        let fitted = self.game_scene.res.camera.viewport.unwrap_or(vp);
        let mut touch_ui = Ui::new(ui.text_painter, Some(fitted));
        set_camera(&Camera2D {
            render_target: Some(target),
            ..touch_ui.camera()
        });
        let config = &self.game_scene.res.config;
        for (id, pos, fade) in self.visible_touches() {
            let alpha = config.touch_point_alpha * fade;
            touch_ui.fill_circle(
                pos.x,
                pos.y,
                config.touch_point_size,
                Color { a: alpha, ..BLUE },
            );
            if self.finger_ids {
                touch_ui
                    .text(format!("F{id}"))
                    .pos(pos.x, pos.y - config.touch_point_size - 0.008)
                    .anchor(0.5, 1.)
                    .no_baseline()
                    .size(0.32 * vp.2 as f32 / fitted.2 as f32)
                    .color(semi_white(alpha))
                    .draw();
            }
        }
        Ok(())
    }

    /// 回放中的打击音。视频音轨单独混音，而不是依赖实时音频设备。
    pub fn video_hitsounds(&self, start: f64, end: f64) -> Vec<(f64, sasa::AudioClip)> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for ev in &self.replay.judges {
            if !seen.insert((ev.line, ev.note)) || ev.t < start || ev.t >= end {
                continue;
            }
            let Some(note) = self
                .game_scene
                .chart
                .lines
                .get(ev.line as usize)
                .and_then(|line| line.notes.get(ev.note as usize))
            else {
                continue;
            };
            if !(matches!(ev.kind, 5 | 6)
                || (matches!(ev.kind, 0 | 1 | 4 | 7 | 8 | 9)
                    && !matches!(note.kind, NoteKind::Hold { .. })))
            {
                continue;
            }
            let res = &self.game_scene.res;
            let clip = match &note.hitsound {
                prpr::judge::HitSound::None => continue,
                prpr::judge::HitSound::Click => &res.res_pack.sfx_click,
                prpr::judge::HitSound::Drag => &res.res_pack.sfx_drag,
                prpr::judge::HitSound::Flick => &res.res_pack.sfx_flick,
                prpr::judge::HitSound::Custom(name) => self
                    .game_scene
                    .chart
                    .hitsounds
                    .get(name)
                    .unwrap_or(&res.res_pack.sfx_click),
            };
            result.push((ev.t, clip.clone()));
        }
        result
    }

    /// 视频音轨用：返回 `[start, end)` 内触点在噪域中的谱面时间区间。
    ///
    /// Phira Pro 在触点进入噪域时给音乐加 1500 Hz 低通，离开后扫回原声。
    /// 导出时没有实时音频设备，因此先把遮挡状态采样成区间，再在离线混音
    /// 时驱动同一个滤波器。没有噪域或简化噪域时直接返回空。
    pub fn video_block_ranges(&mut self, start: f64, end: f64) -> Vec<(f64, f64)> {
        if end <= start {
            return Vec::new();
        }
        let chart = &self.game_scene.chart;
        if !chart.has_block_areas()
            || (self.game_scene.res.config.block_area_simple && chart.next_block_areas.is_empty())
        {
            return Vec::new();
        }

        let start = start.max(0.);
        if end <= start {
            return Vec::new();
        }
        // 从回放开头重建到起点，保证跨起点的长按和 sticky 遮挡状态正确。
        self.rewind_to(start);
        self.current = start;

        // 游戏帧率通常是 60 Hz；用 120 Hz 采样以更准确地捕捉快速划入划出。
        const STEP: f64 = 1. / 120.;
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        let mut from: Option<f64> = None;
        let mut t = start;
        loop {
            self.current = t;
            self.game_scene.res.time = t;
            self.apply_touches(t);
            let blocked = !self.game_scene.chart.blocked_touches.is_empty();
            match (blocked, from) {
                (true, None) => from = Some(t),
                (false, Some(begin)) => {
                    if t > begin {
                        ranges.push((begin, t));
                    }
                    from = None;
                }
                _ => {}
            }
            if t >= end {
                break;
            }
            t = (t + STEP).min(end);
        }
        if let Some(begin) = from {
            if end > begin {
                ranges.push((begin, end));
            }
        }
        let total = ranges.iter().map(|&(begin, end)| end - begin).sum::<f64>();
        verbose(&format!(
            "block audio: {} ranges, {total:.2}s in {start:.2}s - {end:.2}s",
            ranges.len()
        ));
        if std::env::var_os("PPR_RENDER_VERBOSE").is_some() {
            for &(begin, end) in ranges.iter().take(16) {
                eprintln!("[ppr-render]   {begin:.3}s - {end:.3}s");
            }
        }
        ranges
    }

    fn clock(time: f64) -> String {
        let time = time.max(0.);
        format!("{}:{:05.2}", time as u64 / 60, time % 60.)
    }

    fn restart(&mut self) {
        self.rewind_to(0.);
        self.current = 0.;
        self.playing = true;
        self.message = None;
    }

    /// 跳转到 `t`：重置谱面与判定状态后静默快进，避免倒拖后旧音符消失。
    fn seek(&mut self, t: f64) {
        let t = t.clamp(0., self.end());
        if (t - self.current).abs() < 1e-9 {
            return;
        }
        self.rewind_to(t);
        self.current = t;
    }

    fn rewind_to(&mut self, t: f64) {
        self.game_scene.sync_replay_block_audio(false);
        {
            let scene = &mut self.game_scene;
            scene.judge.reset();
            scene.reset_life_display();
            scene.chart.reset();
            scene.bad_notes.clear();
            scene.res.emitter.emitter.reset();
            scene.res.emitter.emitter_square.reset();
            scene.res.judge_line_color = scene.res.res_pack.info.color_perfect();
        }
        self.judge_idx = 0;
        self.touch_idx = 0;
        self.block_frame_idx = 0;
        self.current_touches.clear();
        self.finishing_holds.clear();
        self.game_scene.res.time = t;
        self.game_scene.res.alpha = 1.;
        self.particle_delta = 0.;
        self.apply_judges(t, true);
        self.apply_touches(t);
        let target = (t + self.game_scene.offset() as f64).max(0.);
        let _ = self.game_scene.music.seek_to(target);
    }

    /// 把 `t` 之前的判定事件全部提交到引擎。
    ///
    /// `silent = true` 时不产生打击特效和音效，用于拖动跳转。
    fn apply_judges(&mut self, t: f64, silent: bool) {
        let play_hitsounds = self.play_hitsounds;
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
            } = &mut self.game_scene;
            let Some(line) = chart.lines.get_mut(ev.line as usize) else {
                continue;
            };
            let Some(note) = line.notes.get_mut(ev.note as usize) else {
                continue;
            };

            if self.replay.has_diffs && matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. })
            {
                let head = matches!(ev.kind, 5 | 6);
                let hold = matches!(note.kind, NoteKind::Hold { .. });
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
                && play_hitsounds
                && (matches!(ev.kind, 5 | 6)
                    || (matches!(ev.kind, 0 | 1 | 4 | 7 | 8 | 9)
                        && !matches!(note.kind, NoteKind::Hold { .. })))
            {
                note.hitsound.play(res);
            }

            let hold_success = matches!(ev.kind, 0 | 1 | 4 | 7 | 8 | 9)
                && matches!(note.kind, NoteKind::Hold { .. });
            let hold_active = if hold_success {
                let NoteKind::Hold { end_time, .. } = note.kind else {
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
                    let tj = if ev.kind == 4 {
                        TJ::PerfectPlus
                    } else {
                        TJ::Perfect
                    };
                    judge.commit(ev.t, tj, ev.line, ev.note, ev.diff as f64);
                    if !silent && !hold_success {
                        let fx = res
                            .config
                            .judge_grading
                            .color(tj as usize, res.res_pack.info.fx_perfect());
                        let (line_tr, obj, rot) = {
                            let line = &chart.lines[ev.line as usize];
                            (
                                line.now_transform(res, &chart.lines),
                                line.notes[ev.note as usize].object.now(res),
                                line.notes[ev.note as usize].rotation(line),
                            )
                        };
                        res.with_model(line_tr * obj, |res| res.emit_at_origin(rot, fx));
                    }
                }
                1 | 7 | 8 | 9 => {
                    if !hold_active {
                        note.judge = JudgeStatus::Judged;
                    }
                    let grade = match ev.kind {
                        7 => TJ::Great,
                        8 => TJ::Ok,
                        9 => TJ::Meh,
                        _ => TJ::Good,
                    };
                    judge.commit(ev.t, grade, ev.line, ev.note, ev.diff as f64);
                    if !silent && !hold_success {
                        let fx = res
                            .config
                            .judge_grading
                            .color(grade as usize, res.res_pack.info.fx_good());
                        let (line_tr, obj, rot) = {
                            let line = &chart.lines[ev.line as usize];
                            (
                                line.now_transform(res, &chart.lines),
                                line.notes[ev.note as usize].object.now(res),
                                line.notes[ev.note as usize].rotation(line),
                            )
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
                            let incline_sin = line
                                .incline
                                .now_opt()
                                .map(|it| it.to_radians().sin())
                                .unwrap_or_default();
                            let h = ((note.height - line.height.now() as f64)
                                / res.aspect_ratio as f64
                                * note.speed) as f32;
                            let nm = note.now_transform(
                                res,
                                &line.ctrl_obj.borrow_mut(),
                                h,
                                incline_sin,
                            );
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

        judge_queue_clear(&self.game_scene.judge);
        // 已开始计分的长条只结束视觉生命周期，不再次计分。
        let chart = &mut self.game_scene.chart;
        self.finishing_holds.retain(|&(line, id), tail| {
            let note = &mut chart.lines[line as usize].notes[id as usize];
            if t >= *tail {
                note.judge = JudgeStatus::Judged;
            }
            !matches!(note.judge, JudgeStatus::Judged)
        });
    }

    fn apply_touches(&mut self, t: f64) {
        let noise = self.game_scene.chart.has_block_areas();
        loop {
            let event_time = self
                .replay
                .touches
                .get(self.touch_idx)
                .map_or(f64::INFINITY, |e| e.t as f64);
            let frame_time = if noise {
                self.replay
                    .frames
                    .get(self.block_frame_idx)
                    .copied()
                    .unwrap_or(f64::INFINITY)
            } else {
                f64::INFINITY
            };
            let at = event_time.min(frame_time);
            if at > t || !at.is_finite() {
                break;
            }
            if frame_time < event_time {
                self.block_frame_idx += 1;
                self.update_block_touches(at);
                continue;
            }
            let index = self.touch_idx;
            let event = self.replay.touches[index];
            self.touch_idx += 1;
            if event.phase == PHASE_UP {
                self.current_touches.remove(&event.id);
            } else {
                if event.phase == replay::PHASE_DOWN {
                    self.game_scene.judge.blocked_finger_ids.remove(&event.id);
                }
                self.current_touches
                    .insert(event.id, (vec2(event.x, event.y), index));
            }
            if noise {
                self.update_block_touches(at);
            }
        }
        self.update_block_touches(t);
    }

    fn update_block_touches(&mut self, t: f64) {
        let touches = self
            .current_touches
            .iter()
            .map(|(&id, &(p, i))| (id, self.touch_position(p, i, t)))
            .collect::<Vec<_>>();
        self.game_scene.res.time = t;
        self.game_scene.update_replay_block_touches(&touches);
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

    /// 抬起后保留 80ms 的视觉淡出，短促点击不会被同一帧更新吞掉。
    fn visible_touches(&self) -> Vec<(u64, Vec2, f32)> {
        let mut points = HashMap::new();
        for (index, event) in self.replay.touches[..self.touch_idx]
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, e)| self.current - e.t as f64 <= 0.08)
        {
            if event.phase == PHASE_UP {
                points.entry(self.finger_numbers[index] as u64).or_insert((
                    vec2(event.x, event.y),
                    (1. - (self.current - event.t as f64) / 0.08) as f32,
                ));
            }
        }
        for &(pos, index) in self.current_touches.values() {
            points.insert(
                self.finger_numbers[index] as u64,
                (self.touch_position(pos, index, self.current), 1.),
            );
        }
        let mut points = points
            .into_iter()
            .map(|(id, (pos, alpha))| (id, pos, alpha))
            .collect::<Vec<_>>();
        points.sort_by_key(|p| p.0);
        points
    }

    /// 让音乐跟随回放时钟；`res.time` 与音乐位置差一个谱面偏移。
    fn sync_music(&mut self) {
        let target = (self.current + self.game_scene.offset() as f64).max(0.);
        let music = &mut self.game_scene.music;
        if self.playing {
            music.try_set_playback_rate(
                (self.speed * self.game_scene.res.config.speed).max(0.05) as f64
            );
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

    fn queue_seek(&mut self, t: f64) {
        self.pending_seek = Some(t.clamp(0., self.end()));
        self.selected_error = None;
    }

    fn set_rate(&mut self, index: usize) {
        self.rate_index = index.min(RATES.len() - 1);
        self.speed = RATES[self.rate_index];
    }

    fn show_message(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), self.game_scene.res.time));
    }

    fn control(&mut self, i: usize) {
        match i {
            0 => self.restart(),
            1 => self.queue_seek((self.pending_seek.unwrap_or(self.current) - 5.).max(0.)),
            2 => {
                if !self.playing && self.current >= self.end() - 1e-3 {
                    self.restart();
                    return;
                }
                self.playing = !self.playing;
            }
            3 => self.queue_seek((self.pending_seek.unwrap_or(self.current) + 5.).min(self.end())),
            4 => self.set_rate((self.rate_index + 1) % RATES.len()),
            5 => {
                if self.replay.touches.is_empty() {
                    self.show_message("该回放未记录触点");
                } else {
                    self.fingers = !self.fingers;
                }
            }
            6 => self.help = !self.help,
            7 => self.step_error(false),
            8 => self.step_error(true),
            9 => self.exit = true,
            _ => {}
        }
    }

    fn step_error(&mut self, forward: bool) {
        if self.errors.is_empty() {
            self.show_message("回放没有 BAD / MISS");
            return;
        }
        let current = self.pending_seek.unwrap_or(self.current);
        let found = if forward {
            self.errors.iter().position(|t| *t > current + 0.001)
        } else {
            self.errors.iter().rposition(|t| *t < current - 0.001)
        };
        if let Some(index) = found {
            self.selected_error = Some(index);
            self.queue_seek((self.errors[index] - 1.).max(0.));
            self.playing = false;
        } else {
            self.show_message(if forward {
                "已经是最后一处失误"
            } else {
                "没有更早的失误"
            });
        }
    }

    fn last_diff(&self) -> String {
        if !self.replay.has_diffs {
            return "旧回放：误差未记录".into();
        }
        self.replay.judges[..self.judge_idx]
            .last()
            .map(|e| {
                if e.kind == 3 {
                    "MISS".into()
                } else {
                    format!(
                        "{} {:+.2}ms",
                        kind_name(e.head_grade.unwrap_or(e.kind)),
                        e.diff * 1000.
                    )
                }
            })
            .unwrap_or_default()
    }

    fn render_console(&mut self, ui: &mut Ui, t: f32) -> Result<()> {
        let position = self.pending_seek.unwrap_or(self.current);
        let height = if self.controls_visible {
            CONSOLE_HEIGHT
        } else {
            0.
        };
        let y = ui.top - height;

        // 隐藏 / 显示按钮始终留在右上角，避免隐藏后无法用鼠标恢复。
        self.hide_button.render_text(
            ui,
            Rect::new(0.66, -ui.top + 0.02, 0.28, 0.055),
            t,
            if self.controls_visible {
                "隐藏控制"
            } else {
                "显示控制"
            },
            0.28,
            false,
        );

        if !self.controls_visible {
            self.timeline = Rect::default();
            if self.help {
                self.render_help(ui, t);
            }
            return Ok(());
        }

        ui.fill_rect(
            Rect::new(-1., y, 2., height),
            Color::new(0.08, 0.10, 0.13, 0.98),
        );
        let timeline = Rect::new(-0.94, y + 0.025, 1.88, 0.012);
        self.timeline = Rect::new(timeline.x, timeline.y - 0.022, timeline.w, 0.056);
        ui.fill_path(&timeline.rounded(0.006), semi_white(0.2));
        let progress = (position / self.end().max(0.001)) as f32;
        ui.fill_path(
            &Rect::new(timeline.x, timeline.y, timeline.w * progress, timeline.h).rounded(0.006),
            WHITE,
        );
        for error in &self.errors {
            ui.fill_rect(
                Rect::new(
                    timeline.x + timeline.w * (*error / self.end()) as f32,
                    timeline.y - 0.004,
                    0.002,
                    0.02,
                ),
                RED,
            );
        }

        let cy = y + 0.052;
        let bh = 0.060;
        let labels = [
            "重播".to_owned(),
            "−5s".to_owned(),
            if self.playing {
                "暂停".to_owned()
            } else {
                "播放".to_owned()
            },
            "+5s".to_owned(),
            format!("{:.2}×", self.speed),
            if self.replay.touches.is_empty() {
                "无触点".to_owned()
            } else {
                format!("触点 {}", if self.fingers { "开" } else { "关" })
            },
            if self.help {
                "关闭帮助".to_owned()
            } else {
                "帮助".to_owned()
            },
        ];
        for (i, label) in labels.iter().enumerate() {
            let chosen = matches!(i, 2 if self.playing)
                | matches!(i, 5 if self.fingers)
                | matches!(i, 6 if self.help);
            self.buttons[i].render_text(
                ui,
                panel_cell(i, 1, cy, bh),
                t,
                label.clone(),
                0.31,
                chosen,
            );
        }

        let row2 = cy + 0.070;
        self.buttons[7].render_text(
            ui,
            panel_cell(0, 1, row2, 0.052),
            t,
            "上个失误",
            0.27,
            false,
        );
        self.buttons[8].render_text(
            ui,
            panel_cell(1, 1, row2, 0.052),
            t,
            "下个失误",
            0.27,
            false,
        );
        self.buttons[9].render_text(
            ui,
            panel_cell(5, 2, row2, 0.052),
            t,
            "返回主界面",
            0.27,
            false,
        );
        ui.text(format!(
            "{} / {}",
            Self::clock(position),
            Self::clock(self.end())
        ))
        .pos(0.94, row2 + 0.026)
        .anchor(1., 0.5)
        .no_baseline()
        .size(0.29)
        .color(semi_white(0.85))
        .draw();

        let info = format!(
            "{} · 录制 {:.2}× · 观看 {:.2}× · {}",
            self.title,
            self.game_scene.res.config.speed,
            self.speed,
            self.last_diff()
        );
        ui.text(info)
            .pos(-0.94, row2 + 0.075)
            .anchor(0., 0.5)
            .no_baseline()
            .size(0.27)
            .max_width(1.86)
            .color(semi_white(0.7))
            .draw();

        if let Some((text, _)) = &self.message {
            ui.text(text)
                .pos(0., y - 0.035)
                .anchor(0.5, 1.)
                .size(0.32)
                .color(Color::new(1., 0.85, 0.4, 1.))
                .draw();
        }

        if self.help {
            self.render_help(ui, t);
        }
        Ok(())
    }

    fn render_help(&mut self, ui: &mut Ui, _t: f32) {
        ui.fill_rect(
            Rect::new(-0.75, -0.33, 1.5, 0.66),
            Color::new(0.03, 0.05, 0.08, 0.94),
        );
        #[cfg(target_os = "android")]
        let title = "PPR RENDER · 触屏操作";
        #[cfg(not(target_os = "android"))]
        let title = "PPR RENDER · 键盘快捷键";
        ui.text(title)
            .pos(0., -0.27)
            .anchor(0.5, 0.)
            .size(0.48)
            .color(WHITE)
            .draw();
        #[cfg(target_os = "android")]
        let lines = [
            "底部控制台          播放、暂停、重播",
            "拖动时间轴          定位回放进度",
            "−5s / +5s           后退、前进 5 秒",
            "倍速按钮            切换观看倍率",
            "触点 / 帮助         开关触点或本帮助",
            "上个 / 下个失误     跳到 BAD 或 MISS",
            "返回主界面          回到选择页",
        ];
        #[cfg(not(target_os = "android"))]
        let lines = [
            "空格                 播放、暂停",
            "R                    回到开头并播放",
            "← / →                后退、前进 5 秒",
            "↑ / ↓                降低、提高观看倍率",
            "T                    显示或隐藏触点",
            "H                    隐藏或显示控制台",
            "J / K                上一个、下一个失误",
            "F1                   打开或关闭本帮助",
            "Q / Esc              返回主界面",
        ];
        for (i, line) in lines.iter().enumerate() {
            ui.text(*line)
                .pos(-0.66, -0.19 + i as f32 * 0.045)
                .size(0.30)
                .color(semi_white(0.9))
                .draw();
        }
    }

    fn update_keys(&mut self) {
        if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Q) {
            self.exit = true;
        }
        if is_key_pressed(KeyCode::Space) {
            if !self.playing && self.current >= self.end() - 1e-3 {
                self.restart();
            } else {
                self.playing = !self.playing;
            }
        }
        if is_key_pressed(KeyCode::R) {
            self.restart();
        }
        if is_key_pressed(KeyCode::Left) {
            self.queue_seek((self.pending_seek.unwrap_or(self.current) - 5.).max(0.));
        }
        if is_key_pressed(KeyCode::Right) {
            self.queue_seek((self.pending_seek.unwrap_or(self.current) + 5.).min(self.end()));
        }
        if is_key_pressed(KeyCode::Up) {
            self.set_rate((self.rate_index + 1).min(RATES.len() - 1));
        }
        if is_key_pressed(KeyCode::Down) {
            self.set_rate(self.rate_index.saturating_sub(1));
        }
        if is_key_pressed(KeyCode::T) && !self.replay.touches.is_empty() {
            self.fingers = !self.fingers;
        }
        if is_key_pressed(KeyCode::H) {
            self.controls_visible = !self.controls_visible;
        }
        if is_key_pressed(KeyCode::F1) {
            self.help = !self.help;
        }
        if is_key_pressed(KeyCode::J) {
            self.step_error(false);
        }
        if is_key_pressed(KeyCode::K) {
            self.step_error(true);
        }
    }
}

/// 排查加载耗时：仅在 `PPR_RENDER_VERBOSE=1` 时输出。
fn verbose(message: &str) {
    if std::env::var_os("PPR_RENDER_VERBOSE").is_some() {
        eprintln!("[ppr-render] {message}");
    }
}

fn judge_queue_clear(judge: &prpr::judge::Judge) {
    judge.judgements.borrow_mut().clear();
}

fn panel_cell(column: usize, span: usize, y: f32, height: f32) -> Rect {
    const GAP: f32 = 0.02;
    const WIDTH: f32 = (1.88 - GAP * 7.) / 8.;
    Rect::new(
        -0.94 + column as f32 * (WIDTH + GAP),
        y,
        WIDTH * span as f32 + GAP * (span - 1) as f32,
        height,
    )
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

impl Scene for ReplayScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        tm.reset();
        self.last_t = tm.real_time();
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.playing = false;
        let _ = self.game_scene.music.pause();
        self.game_scene.sync_replay_block_audio(false);
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.resume();
        self.last_t = tm.real_time();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.real_time() as f32;
        if self.hide_button.touch(touch, t) {
            self.controls_visible = !self.controls_visible;
            return Ok(true);
        }
        if !self.controls_visible {
            return Ok(false);
        }
        if matches!(touch.phase, TouchPhase::Started) && self.timeline.contains(touch.position) {
            self.drag = Some(touch.id);
            self.drag_playing = self.playing;
            self.playing = false;
            self.queue_seek(
                self.end()
                    * ((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.) as f64,
            );
            return Ok(true);
        }
        if self.drag == Some(touch.id) {
            let p = ((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.);
            if !matches!(touch.phase, TouchPhase::Cancelled) {
                self.queue_seek(self.end() * p as f64);
            }
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.drag = None;
                self.playing = self.drag_playing;
            }
            return Ok(true);
        }
        // 按钮数量由初始化决定，不能写死循环上限，否则会越界 panic。
        let hit = self
            .buttons
            .iter_mut()
            .position(|button| button.touch(touch, t));
        if let Some(i) = hit {
            self.control(i);
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.update_keys();
        let dt = (tm.real_time() - self.last_t).clamp(0., 0.25);
        self.last_t = tm.real_time();

        if let Some(target) = self.pending_seek.take() {
            self.seek(target);
        }
        if self.playing {
            self.particle_delta =
                (dt * self.speed as f64 * self.game_scene.res.config.speed as f64).max(0.) as f32;
            let target =
                self.current + dt * self.speed as f64 * self.game_scene.res.config.speed as f64;
            self.current = target.min(self.end());
            if target >= self.end() {
                self.playing = false;
            }
        } else {
            self.particle_delta = 0.;
        }
        if let Some((_, since)) = &self.message {
            if self.current - since > 2.5 {
                self.message = None;
            }
        }

        let _ = self.game_scene.res.audio.recover_if_needed();
        self.game_scene.res.time = self.current;
        self.game_scene.res.alpha = 1.;
        self.game_scene.res.config.chart_debug = false;
        self.game_scene.res.config.chart_debug_note = false;
        self.game_scene.touch_points.clear();
        self.apply_judges(self.current, false);
        self.apply_touches(self.current);
        self.game_scene.tick_replay();
        self.sync_music();
        self.game_scene.sync_replay_block_audio(self.playing);
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        clear_background(Color::new(0.06, 0.08, 0.10, 1.));
        let t = tm.real_time() as f32;

        let height = if self.controls_visible {
            CONSOLE_HEIGHT
        } else {
            0.
        };
        let vp = ui.viewport;
        let console_pixels = (height * vp.2 as f32 / 2.).round() as i32;
        let chart_vp = (
            vp.0,
            vp.1 + console_pixels,
            vp.2,
            (vp.3 - console_pixels).max(1),
        );
        {
            let mut chart_ui = Ui::new(ui.text_painter, Some(chart_vp));
            self.game_scene.set_replay_frame_delta(self.particle_delta);
            self.particle_delta = 0.;
            self.game_scene.render(tm, &mut chart_ui)?;
            if self.fingers && !self.replay.touches.is_empty() {
                let fitted = self.game_scene.res.camera.viewport.unwrap_or(chart_vp);
                let mut touch_ui = Ui::new(chart_ui.text_painter, Some(fitted));
                set_camera(&touch_ui.camera());
                let config = &self.game_scene.res.config;
                for (id, pos, fade) in self.visible_touches() {
                    let alpha = config.touch_point_alpha * fade;
                    touch_ui.fill_circle(
                        pos.x,
                        pos.y,
                        config.touch_point_size,
                        Color { a: alpha, ..BLUE },
                    );
                    if self.finger_ids {
                        touch_ui
                            .text(format!("F{id}"))
                            .pos(pos.x, pos.y - config.touch_point_size - 0.008)
                            .anchor(0.5, 1.)
                            .no_baseline()
                            .size(0.32 * vp.2 as f32 / fitted.2 as f32)
                            .color(semi_white(alpha))
                            .draw();
                    }
                }
            }
        }

        set_camera(&ui.camera());
        self.render_console(ui, t)?;
        if let Some(warning) = &self.warning {
            let top = ui.top;
            ui.text(warning)
                .pos(0., -top + 0.075)
                .anchor(0.5, 0.)
                .size(0.28)
                .color(Color::new(1., 0.78, 0.35, 0.9))
                .draw();
        }
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if self.exit {
            NextScene::Exit
        } else {
            NextScene::None
        }
    }
}
