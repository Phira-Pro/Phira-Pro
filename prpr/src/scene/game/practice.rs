use super::*;

impl GameScene {
    pub(super) fn practice_range_changed(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.practice_loops = 0;
        self.practice_progress = Default::default();
        self.practice_last_events.clear();
        self.practice_tool_touches.clear();
        self.practice_last_touches.clear();
        self.practice_partial = false;
        self.practice_statistics.clear();
        self.last_practice_accuracy = None;
        self.judge.reset();
        self.chart.reset();
        self.bad_notes.clear();
        self.block_audio.reset();
        self.res.config.speed = self.practice_speed();
        self.res.config.practice_multiplier = self.res.config.practice.multiplier(self.res.config.speed);
        self.res.windows = self.res.config.judge_windows();
        tm.speed = self.res.config.speed as f64;
        self.music.try_set_playback_rate(tm.speed);
        self.music.configure_practice(
            self.res.config.practice.preserve_pitch,
            self.res.config.practice.correct_sound || self.res.config.has_mod(Mods::PERFECT_SOUND),
        )?;
        let start = self.exercise_range.start - self.offset() as f64;
        self.judge.advance_to(&mut self.chart, start);
        self.res.judge_line_color = self.res.res_pack.info.color_perfect();
        self.music.pause()?;
        self.music.seek_to(self.exercise_range.start.max(0.))?;
        tm.seek_to(self.exercise_range.start);
        self.res.time = self.exercise_range.start - self.offset() as f64;
        self.state = if self.exercise_range.start < 0. {
            State::BeforeMusic
        } else {
            State::Playing
        };
        self.pause_rewind = None;
        Ok(())
    }

    pub(super) fn practice_speed(&self) -> f32 {
        if self.res.config.practice.adaptive.enabled {
            practice::clamp_speed(self.res.config.practice_speed_start + self.res.config.practice_speed_step * self.practice_progress.level as f32)
                .min(self.practice_target_speed)
        } else {
            practice::round_speed(&self.res.config, self.practice_target_speed, self.practice_loops)
        }
    }

    pub(super) fn practice_action(&mut self, tm: &mut TimeManager, action: PracticeAction) -> Result<()> {
        match action {
            PracticeAction::External(tool) => {
                let notes = self
                    .chart
                    .lines
                    .iter()
                    .enumerate()
                    .flat_map(|(line, l)| {
                        l.notes
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| !n.fake)
                            .map(move |(i, n)| practice::tools::Note {
                                line: line as u32,
                                index: i as u32,
                                time: n.time,
                                end: if let NoteKind::Hold { end_time, .. } = n.kind {
                                    end_time
                                } else {
                                    n.time
                                },
                                kind: match n.kind {
                                    NoteKind::Click => 0,
                                    NoteKind::Hold { .. } => 1,
                                    NoteKind::Drag => 2,
                                    NoteKind::Flick => 3,
                                },
                            })
                    })
                    .collect();
                let events = self.practice_events();
                let request = practice::tools::Request {
                    tool,
                    local_path: self.res.config.practice_local_path.clone(),
                    info: self.res.info.clone(),
                    key: self.practice_key.clone(),
                    now: self.res.time,
                    duration: self.res.track_length,
                    notes,
                    events: if events.is_empty() { self.practice_last_events.clone() } else { events },
                    touches: if self.practice_tool_touches.is_empty() {
                        self.practice_last_touches.clone()
                    } else {
                        self.practice_tool_touches.clone()
                    },
                };
                if let Some(scene) = practice::tools::open(request) {
                    self.practice_panel.suspend(tm.real_time() as f32);
                    self.practice_tools_return = true;
                    self.practice_tools_position = tm.now();
                    self.music.pause()?;
                    tm.pause();
                    self.next_scene = Some(NextScene::Overlay(scene));
                }
            }
            PracticeAction::Input(id, text) => request_input(&id, InputBox::new().default_text(text)),
            PracticeAction::Value(id, value) => {
                let value = if matches!(id.as_str(), "practice_scale" | "practice_multiplier" | "practice_range_alpha") {
                    value * 100.
                } else {
                    value
                };
                self.practice_input(tm, &id, &value.to_string())?;
            }
            PracticeAction::Toggle(id) => {
                let cfg = &mut self.res.config;
                let p = &mut cfg.practice;
                let mut restart = true;
                match id.as_str() {
                    "practice_adaptive" => {
                        p.adaptive.enabled ^= true;
                        cfg.practice_ramp = false;
                    }
                    "practice_goal_fc" => {
                        p.adaptive.full_combo ^= true;
                    }
                    "practice_goal_drop" => {
                        p.adaptive.lower_on_failure ^= true;
                    }
                    "practice_loop" => {
                        self.practice_auto ^= true;
                        restart = false;
                    }
                    "practice_pitch" => p.preserve_pitch ^= true,
                    "practice_correct" => p.correct_sound ^= true,
                    "practice_ramp" => cfg.practice_ramp ^= true,
                    "practice_inverse" => p.inverse_speed ^= true,
                    "practice_view_reset" => {
                        p.view = Default::default();
                        restart = false;
                    }
                    "practice_ranges" => {
                        p.ranges ^= true;
                        restart = false;
                    }
                    "practice_range_anchor" => {
                        p.range_anchor = match p.range_anchor {
                            practice::RangeAnchor::JudgeLine => practice::RangeAnchor::Note,
                            practice::RangeAnchor::Note => practice::RangeAnchor::Both,
                            practice::RangeAnchor::Both => practice::RangeAnchor::JudgeLine,
                        };
                        restart = false;
                    }
                    _ if id.starts_with("practice_tap_range_") => {
                        if let Ok(i) = id[19..].parse::<usize>() {
                            if let Some(value) = p.range_tap.get_mut(i) {
                                *value ^= true;
                            }
                        }
                        restart = false;
                    }
                    _ if id.starts_with("practice_hold_range_") => {
                        if let Ok(i) = id[20..].parse::<usize>() {
                            if let Some(value) = p.range_hold.get_mut(i) {
                                *value ^= true;
                            }
                        }
                        restart = false;
                    }
                    "practice_debug" => {
                        cfg.chart_debug ^= true;
                        restart = false;
                    }
                    "practice_debug_line" => {
                        cfg.chart_debug_line ^= true;
                        restart = false;
                    }
                    "practice_debug_note" => {
                        cfg.chart_debug_note ^= true;
                        restart = false;
                    }
                    "practice_behavior" => {
                        p.behavior = p.behavior.next();
                        self.practice_originals.apply(&mut self.chart, p.behavior);
                        self.practice_schedule = if p.behavior == practice::NoteBehavior::Original {
                            self.practice_original_schedule.clone()
                        } else {
                            Self::correct_schedule(&self.chart, &self.res, self.offset() as f64)
                        };
                        self.music.schedule(self.practice_schedule.clone())?;
                        self.judge = Judge::new(&self.chart, self.res.config.hold_tail_judge);
                        self.judge.set_grading(self.res.config.judge_grading);
                        self.judge.set_hp_amount(self.res.config.hp_amount);
                        self.judge.set_hp_scale(self.res.config.hp_scale);
                    }
                    _ if id.starts_with("practice_kind_") => {
                        if let Ok(i) = id[14..].parse::<usize>() {
                            if let Some(kind) = p.range_kinds.get_mut(i) {
                                *kind ^= true;
                            }
                        }
                        restart = false;
                    }
                    _ => return Ok(()),
                }
                practice::save_preferences(&self.res.config);
                if restart {
                    self.practice_range_changed(tm)?;
                }
            }
            PracticeAction::Save => request_input(
                "practice_bookmark",
                InputBox::new().default_text(format!("{}–{}", fmt_time(self.exercise_range.start as f32), fmt_time(self.exercise_range.end as f32))),
            ),
            PracticeAction::Load(i) => {
                let Some(mut b) = self.practice_bookmarks.get(i).cloned() else {
                    return Ok(());
                };
                if !b.valid(self.res.track_length, self.offset().min(0.) as f64) {
                    show_message(tl!("ex-time-out-of-range")).error();
                    return Ok(());
                }
                b.options.sanitize();
                self.exercise_range = b.start..b.end;
                if let Some((first, last)) = b.combo {
                    let Some(range) = practice::combo_range(&self.practice_notes, first, last, self.offset() as f64, self.res.track_length) else {
                        show_message(tl!("ex-time-out-of-range")).error();
                        return Ok(());
                    };
                    self.exercise_range = range;
                }
                self.practice_combo = b.combo;
                self.practice_target_speed = b.speed;
                self.practice_auto = b.auto_loop;
                self.res.config.practice_ramp = b.ramp;
                self.res.config.practice_speed_start = b.ramp_start;
                self.res.config.practice_speed_step = b.ramp_step;
                self.res.config.practice = b.options;
                self.practice_originals.apply(&mut self.chart, self.res.config.practice.behavior);
                self.practice_schedule = if self.res.config.practice.behavior == practice::NoteBehavior::Original {
                    self.practice_original_schedule.clone()
                } else {
                    Self::correct_schedule(&self.chart, &self.res, self.offset() as f64)
                };
                self.music.schedule(self.practice_schedule.clone())?;
                self.judge = Judge::new(&self.chart, self.res.config.hold_tail_judge);
                self.judge.set_grading(self.res.config.judge_grading);
                self.judge.set_hp_amount(self.res.config.hp_amount);
                self.judge.set_hp_scale(self.res.config.hp_scale);
                self.practice_range_changed(tm)?;
                practice::save_preferences(&self.res.config);
            }
            PracticeAction::Delete(i) => {
                if i >= self.practice_bookmarks.len() {
                    return Ok(());
                }
                let mut list = self.practice_bookmarks.clone();
                list.remove(i);
                match practice::store_bookmarks(&self.practice_key, &list) {
                    Ok(()) => self.practice_bookmarks = list,
                    Err(e) => {
                        show_message(format!("{e:#}")).error();
                    }
                }
            }
            PracticeAction::Clear => self.practice_statistics.clear(),
            PracticeAction::Boundary(start, time) => {
                if !time.is_finite() {
                    return Ok(());
                }
                if start {
                    let low = self.offset().min(0.) as f64;
                    self.exercise_range.start = time.clamp(low, (self.exercise_range.end - 0.05).max(low));
                } else {
                    let high = self.res.track_length;
                    self.exercise_range.end = time.clamp((self.exercise_range.start + 0.05).min(high), high);
                }
                self.practice_combo = None;
                self.practice_range_changed(tm)?;
            }
            PracticeAction::Seek(time) => {
                let offset = self.offset() as f64;
                self.judge.reset();
                self.chart.reset();
                self.bad_notes.clear();
                self.block_audio.reset();
                self.judge.advance_to(&mut self.chart, time - offset);
                tm.seek_to(time);
                self.music.seek_to(time.max(0.))?;
                self.res.time = time - offset;
                self.res.judge_line_color = self.res.res_pack.info.color_perfect();
                self.practice_partial = true;
                self.state = if time < 0. { State::BeforeMusic } else { State::Playing };
            }
        }
        Ok(())
    }

    pub(super) fn practice_input(&mut self, tm: &mut TimeManager, id: &str, text: &str) -> Result<()> {
        if id == "practice_bookmark" {
            let name: String = text.trim().chars().filter(|c| !c.is_control()).take(48).collect();
            if name.is_empty() {
                show_message(tl!("ex-invalid-format")).error();
                return Ok(());
            }
            let cfg = &self.res.config;
            let b = Bookmark {
                name,
                start: self.exercise_range.start,
                end: self.exercise_range.end,
                combo: self.practice_combo,
                speed: self.practice_target_speed,
                auto_loop: self.practice_auto,
                ramp: cfg.practice_ramp,
                ramp_start: cfg.practice_speed_start,
                ramp_step: cfg.practice_speed_step,
                options: cfg.practice.clone(),
            };
            let mut list = self.practice_bookmarks.clone();
            if let Some(existing) = list.iter_mut().find(|v| v.name == b.name) {
                *existing = b;
            } else if list.len() < 100 {
                list.push(b);
            } else {
                show_message(tl!("practice-limit")).error();
                return Ok(());
            }
            match practice::store_bookmarks(&self.practice_key, &list) {
                Ok(()) => {
                    self.practice_bookmarks = list;
                    show_message(tl!("practice-saved")).ok();
                }
                Err(e) => {
                    show_message(format!("{e:#}")).error();
                }
            }
            return Ok(());
        }
        let Ok(value) = text.trim().parse::<f32>() else {
            show_message(tl!("ex-invalid-format")).error();
            return Ok(());
        };
        if !value.is_finite() {
            show_message(tl!("ex-invalid-format")).error();
            return Ok(());
        }
        let value = if matches!(id, "practice_scale" | "practice_multiplier" | "practice_range_alpha") {
            value / 100.
        } else {
            value
        };
        let cfg = &mut self.res.config;
        let mut restart = true;
        match id {
            "practice_speed" => {
                self.practice_target_speed = practice::clamp_speed(value);
                // A manual change selects the actual playback rate immediately.
                // The saved ramp can be enabled again from the drawer.
                cfg.practice_ramp = false;
                cfg.practice.adaptive.enabled = false;
            }
            "practice_target_accuracy" => cfg.practice.adaptive.accuracy = (value / 100.).clamp(0., 1.),
            "practice_consecutive" => cfg.practice.adaptive.consecutive = value.round().clamp(1., 99.) as u32,
            "practice_ramp_start" => cfg.practice_speed_start = value.clamp(0.1, 2.),
            "practice_ramp_step" => cfg.practice_speed_step = value.clamp(0.01, 0.5),
            "practice_retry" => {
                cfg.auto_retry = value.clamp(0., 10.).round();
                restart = false;
            }
            "practice_retry_lead" => {
                cfg.retry_lead = value.clamp(0., 10.);
                restart = false;
            }
            "practice_multiplier" => {
                cfg.practice.judgement_multiplier = value.clamp(0.1, 3.);
                cfg.practice.inverse_speed = false;
            }
            "practice_scale" => {
                cfg.practice.view.scale = value.clamp(0.1, 5.);
                restart = false;
            }
            "practice_x" => {
                cfg.practice.view.x = value.clamp(-4., 4.);
                restart = false;
            }
            "practice_y" => {
                cfg.practice.view.y = value.clamp(-4., 4.);
                restart = false;
            }
            "practice_range_alpha" => {
                cfg.practice.range_alpha = value.clamp(0.02, 0.6);
                restart = false;
            }
            "practice_horizon" => {
                cfg.practice.range_horizon = value.clamp(0.1, 10.);
                restart = false;
            }
            _ => return Ok(()),
        }
        practice::save_preferences(&self.res.config);
        if restart {
            self.practice_range_changed(tm)?;
        }
        Ok(())
    }

    #[cfg(record)]
    pub fn practice_control_point(&self, key: &str) -> Option<Vec2> {
        self.practice_panel
            .control_point(key)
            .or_else(|| self.practice_base.control_point(key))
            .map(|p| self.playfield_to_ui(p))
    }
    #[cfg(record)]
    pub fn practice_scroll_to(&mut self, offset: f32) {
        self.practice_panel.scroll.y_scroller.offset = offset;
    }
    #[cfg(record)]
    pub fn practice_round_count(&self) -> u32 {
        self.practice_statistics.total
    }
    #[cfg(record)]
    pub fn practice_touch_frame_for_qa(&mut self, tm: &mut TimeManager, ui: &mut Ui, touch: Touch) -> Result<()> {
        // Match Main::update_with_mutate -> Main::render, including consumed
        // scene input being removed before Ui sliders process the frame.
        let consumed = tm.with_event_time(tm.now(), |tm| Scene::touch(self, tm, &touch))?;
        Scene::update(self, tm)?;
        ui.set_touches(Some(if consumed { Vec::new() } else { vec![touch] }));
        Scene::render(self, tm, ui)
    }
    #[cfg(record)]
    pub fn practice_draw_ranges_for_qa(&self, ui: &mut Ui) {
        // QA replaces note data in-place; build fresh ordering for that fixture
        // instead of carrying the previous practice loop's consumed indices.
        let indices: Vec<_> = self
            .chart
            .lines
            .iter()
            .map(|line| {
                let mut ids: Vec<_> = (0..line.notes.len() as u32).collect();
                ids.sort_by(|a, b| line.notes[*a as usize].time.total_cmp(&line.notes[*b as usize].time));
                (ids, 0)
            })
            .collect();
        super::practice_ranges::draw(&self.res, &self.chart, ui, &indices);
    }
}

impl GameScene {
    #[cfg(record)]
    pub fn practice_open_analysis_for_qa(&mut self, tm: &mut TimeManager, tool: usize) -> Result<()> {
        self.practice_action(tm, PracticeAction::External(tool))
    }
    pub(super) fn practice_events(&self) -> Vec<practice::tools::Event> {
        self.judge
            .judgements
            .borrow()
            .iter()
            .map(|(t, line, index, grade, diff, head)| practice::tools::Event {
                time: *t,
                line: *line,
                index: *index,
                grade: match grade {
                    Ok(g) => *g as u8,
                    Err(p) => head.map_or(if *p { 0 } else { 1 }, |g| g as u8),
                },
                diff: *diff,
                head: grade.is_err(),
            })
            .collect()
    }
}
