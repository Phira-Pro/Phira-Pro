use super::*;

impl GameScene {
    pub(super) fn practice_range_changed(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.practice_loops = 0;
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

    fn practice_speed(&self) -> f32 {
        if self.res.config.practice_ramp {
            (self.res.config.practice_speed_start + self.res.config.practice_speed_step * self.practice_loops as f32)
                .clamp(0.1, self.practice_target_speed.max(0.1))
        } else {
            self.practice_target_speed
        }
    }

    pub(super) fn practice_action(&mut self, tm: &mut TimeManager, action: PracticeAction) -> Result<()> {
        match action {
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
            "practice_speed" => self.practice_target_speed = value.clamp(0.1, 2.),
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
        self.practice_panel.control_point(key).map(|p| p / self.touch_scale())
    }
    #[cfg(record)]
    pub fn practice_scroll_to(&mut self, offset: f32) {
        self.practice_panel.scroll.y_scroller.offset = offset;
    }
    #[cfg(record)]
    pub fn practice_round_count(&self) -> u32 {
        self.practice_statistics.total
    }
}
