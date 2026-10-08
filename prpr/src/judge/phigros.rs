//! Frame-based engine following the user's 2026-10-07 judgement article.
//! Geometry is supplied by the renderer; all time/matching/control logic is testable without a GPU.
use super::Judgement;
use crate::config::{Config, JudgeAlgorithm, JudgeWindows, Mods, PhigrosRules};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Tap,
    Hold,
    Drag,
    Flick,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub target: (usize, u32),
    pub time: f64,
    pub end: f64,
    pub kind: Kind,
    pub skipped: bool,
}

#[derive(Clone, Debug)]
pub struct Finger {
    pub id: u64,
    pub started: bool,
    pub world: [f64; 2],
    pub keyboard: bool,
}

#[derive(Debug)]
pub enum Action {
    Head {
        judgement: Judgement,
        offset: f64,
    },
    Score {
        judgement: Judgement,
        offset: f64,
    },
    /// Successful holds score before their visual end.
    EndHold,
}
#[derive(Debug)]
pub struct Event {
    pub target: (usize, u32),
    pub action: Action,
}

#[derive(Clone, Default)]
struct Control {
    click: bool,
    special: bool,
    head: Option<(Judgement, f64)>,
    safe: i32,
    released: Option<f64>,
    done: bool,
    hold_scored: bool,
}

#[derive(Default)]
struct Flick {
    point: [f64; 2],
    delta: [f64; 2],
    stopped: bool,
    new_flick: bool,
}

impl Flick {
    fn push(&mut self, point: [f64; 2], seconds: f64, dpi: f64, rules: &PhigrosRules) {
        let d = [point[0] - self.point[0], point[1] - self.point[1]];
        self.point = point;
        if seconds <= 0. || !seconds.is_finite() {
            return;
        }
        let previous = self.delta[0].hypot(self.delta[1]);
        let relative = if previous > rules.flick_projection_min {
            (self.delta[0] * d[0] + self.delta[1] * d[1]) / previous / rules.flick_sample_hz / seconds
        } else {
            0.
        };
        let threshold = rules.flick_speed / rules.flick_dpi * dpi;
        if relative < threshold || self.stopped {
            self.new_flick = d[0].hypot(d[1]) / rules.flick_sample_hz / seconds >= threshold * rules.flick_multiplier;
            self.stopped = !self.new_flick;
        }
        self.delta = d;
    }
}

pub struct Engine {
    specs: Vec<Spec>,
    controls: Vec<Control>,
    active: BTreeSet<usize>,
    entered: usize,
    flicks: HashMap<u64, Flick>,
    frame_times: [f64; 10],
    frames: usize,
}

#[cfg(test)]
mod tests;

impl Engine {
    pub fn new(mut specs: Vec<Spec>) -> Self {
        // Stable tie order: chart line order then original note order.
        specs.sort_by(|a, b| a.time.total_cmp(&b.time));
        let controls = specs
            .iter()
            .map(|n| Control {
                done: n.skipped,
                ..Default::default()
            })
            .collect();
        Self {
            specs,
            controls,
            active: BTreeSet::new(),
            entered: 0,
            flicks: HashMap::new(),
            frame_times: [0.; 10],
            frames: 0,
        }
    }

    pub fn advance_to(&mut self, time: f64) {
        for (note, state) in self.specs.iter().zip(&mut self.controls) {
            if note.time < time {
                state.done = true;
                state.hold_scored = false;
            }
        }
        self.active.retain(|i| !self.controls[*i].done);
        self.flicks.clear();
        self.frames = 0;
    }

    pub fn clear_input(&mut self) {
        self.flicks.clear();
        self.frames = 0;
    }

    fn windows(&mut self, cfg: &Config, frame: f64) -> JudgeWindows {
        let mut windows = cfg.judge_windows();
        if frame.is_finite() && frame > 0. {
            self.frame_times[self.frames % 10] = frame;
            self.frames += 1;
        }
        if cfg.mods.contains(Mods::STRICT_JUDGE) && cfg.phigros_rules.frame_compensation && self.frames != 0 {
            let count = self.frames.min(10);
            let pad = self.frame_times[..count].iter().sum::<f64>() / count as f64 * 0.5;
            windows.add_frame_pad(pad);
        }
        windows
    }

    // The article's range search deliberately retains the last preceding note
    // when the normal interval is empty. Matching runs before miss processing.
    fn range(&self, now: f64, speed: f64, early: f64, late: f64, overrun: bool) -> std::ops::Range<usize> {
        let end = self.specs.partition_point(|n| n.time < now + early * speed);
        if end == 0 {
            return 0..0;
        }
        let lower = self.specs[..end].partition_point(|n| n.time <= now - late * speed);
        (if overrun { lower.min(end - 1) } else { lower })..end
    }

    fn select(
        &self,
        finger: usize,
        flick: bool,
        now: f64,
        speed: f64,
        limits: &JudgeWindows,
        cfg: &Config,
        rules: &PhigrosRules,
        geometry: &mut impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
    ) -> Option<usize> {
        let [early_ratio, late_ratio] = rules.flick_sides();
        let flick_early = limits.early[1] * early_ratio;
        let flick_late = limits.late[1] * late_ratio;
        let range = self.range(
            now,
            speed,
            if flick { flick_early } else { limits.early[3] },
            if flick { flick_late + cfg.late_leniency() } else { limits.late_acceptance() + cfg.late_leniency() },
            rules.late_overrun,
        );
        let mut best: Option<(usize, f64, f64)> = None;
        for i in range {
            let n = &self.specs[i];
            let state = &self.controls[i];
            if state.done || if flick { n.kind != Kind::Flick || state.special } else { state.click } {
                continue;
            }
            if !flick && ((n.kind == Kind::Drag && !cfg.drag_protect) || (n.kind == Kind::Flick && !cfg.flick_protect)) {
                continue;
            }
            let dt = (n.time - now) / speed;
            // Only absorb subtraction round-off at the documented 10ms boundary.
            if best.is_some_and(|(_, abs, _)| dt + 1e-12 >= abs + rules.protection_ms / 1000.) {
                continue;
            }
            let Some((dx, y)) = geometry(n.target, finger) else {
                continue;
            };
            if dx >= if flick { rules.special_width } else { rules.tap_width } {
                continue;
            }
            if !flick && dt > limits.early[3] - (dx - rules.bad_edge).max(0.) * limits.early[1] * rules.bad_shrink_factor {
                continue;
            }
            let metric = dx + (y / rules.metric_divisor).abs();
            if let Some((j, _, previous_metric)) = best {
                if flick || matches!(self.specs[j].kind, Kind::Tap | Kind::Hold) {
                    if !flick && !matches!(n.kind, Kind::Tap | Kind::Hold) {
                        continue;
                    }
                    if ((n.time - self.specs[j].time) / speed).abs() > rules.protection_ms / 1000. + 1e-12 || metric >= previous_metric {
                        continue;
                    }
                }
            }
            best = Some((i, dt.abs(), metric));
        }
        best.map(|b| b.0)
    }

    pub fn step(
        &mut self,
        cfg: &Config,
        now: f64,
        frame_seconds: f64,
        dpi: f64,
        fingers: &[Finger],
        mut geometry: impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
    ) -> (JudgeWindows, Vec<Event>) {
        debug_assert_eq!(cfg.judge_algorithm, JudgeAlgorithm::Phigros);
        let speed = cfg.speed as f64;
        let limits = self.windows(cfg, frame_seconds);
        let mut rules = cfg.phigros_rules.clone();
        rules.sanitize();
        let rules = &rules;
        let dpi = if rules.device_dpi > 0. { rules.device_dpi } else { dpi };
        let mut events = Vec::new();
        self.flicks.retain(|id, _| fingers.iter().any(|f| f.id == *id));
        for f in fingers {
            if f.started || !self.flicks.contains_key(&f.id) {
                self.flicks.insert(
                    f.id,
                    Flick {
                        point: f.world,
                        stopped: true,
                        ..Default::default()
                    },
                );
            } else {
                self.flicks.get_mut(&f.id).unwrap().push(f.world, frame_seconds, dpi, rules);
            }
        }
        // All click matches precede all flick matches; finger order is provided by the input stream.
        for (finger, f) in fingers.iter().enumerate().filter(|(_, f)| f.started) {
            let _ = f;
            if let Some(i) = self.select(finger, false, now, speed, &limits, cfg, rules, &mut geometry) {
                if self.specs[i].kind != Kind::Flick {
                    self.controls[i].click = true;
                }
            }
        }
        for (finger, f) in fingers.iter().enumerate() {
            if !self.flicks.get(&f.id).is_some_and(|t| t.new_flick) && !f.keyboard {
                continue;
            }
            if let Some(i) = self.select(finger, true, now, speed, &limits, cfg, rules, &mut geometry) {
                self.controls[i].special = true;
                self.flicks.get_mut(&f.id).unwrap().new_flick = false;
            }
        }
        let [drag_early, drag_late] = rules.drag_sides().map(|v| v / 1000.);
        let [flick_early, flick_late] = rules.flick_sides();
        let lookahead = limits.early[3].max(drag_early).max(limits.early[1] * flick_early);
        let end = self.specs.partition_point(|n| n.time <= now + lookahead * speed);
        while self.entered < end {
            if !self.controls[self.entered].done {
                self.active.insert(self.entered);
            }
            self.entered += 1;
        }
        for &i in &self.active {
            let n = &self.specs[i];
            let state = &mut self.controls[i];
            if state.done {
                if state.hold_scored && now >= n.end {
                    state.hold_scored = false;
                    events.push(Event {
                        target: n.target,
                        action: Action::EndHold,
                    });
                }
                continue;
            }
            let dt = (n.time - now) / speed;
            let side = limits.for_offset(-dt);
            let abs = super::judge_distance(-dt, cfg.late_leniency());
            let mut score = None;
            match n.kind {
                Kind::Tap => {
                    if state.click {
                        score = Some((
                            super::judgement_at_distance(abs, &side, true),
                            -dt,
                        ));
                    } else if dt < -limits.late_acceptance() - cfg.late_leniency() {
                        score = Some((Judgement::Miss, 0.));
                    }
                }
                Kind::Hold => {
                    if state.head.is_none() {
                        if !state.click && dt < -limits.late_acceptance() - cfg.late_leniency() {
                            score = Some((Judgement::Miss, 0.));
                        }
                        if state.click && abs < side.acceptance() {
                            let judgement = super::judgement_at_distance(abs, &side, true);
                            state.head = Some((judgement, -dt));
                            state.safe = rules.hold_safe_frames;
                            events.push(Event {
                                target: n.target,
                                action: Action::Head { judgement, offset: -dt },
                            });
                        }
                    }
                    if let Some(head) = state.head {
                        let on_note = fingers
                            .iter()
                            .enumerate()
                            .any(|(finger, _)| geometry(n.target, finger).is_some_and(|(dx, _)| dx < rules.tap_width));
                        if cfg.hold_tail_judge {
                            // Explicit custom extension; disabled in the built-in Phigros preset.
                            if on_note {
                                state.released = None;
                            } else {
                                state.released.get_or_insert(now);
                            }
                            if state.released.is_some_and(|up| now > up + super::UP_TOLERANCE * speed) && (n.end - now) / speed > limits.early[3] {
                                score = Some((Judgement::Miss, 0.));
                            }
                            if score.is_none() && now >= n.end {
                                let off = state.released.map_or(0., |up| (up - n.end) / speed);
                                score = Some((super::judgement_of_offset(off, &limits), off));
                            }
                        } else {
                            if on_note {
                                state.safe = rules.hold_safe_frames;
                            } else if state.safe < 0 {
                                score = Some((Judgement::Miss, 0.));
                            } else {
                                state.safe -= 1;
                            }
                            if score.is_none() && now > n.end - rules.hold_tail_ms / 1000. * speed {
                                score = Some(head);
                            }
                        }
                    } else if !state.done && now > n.end + rules.hold_delayed_miss_ms / 1000. * speed {
                        score = Some((Judgement::Miss, 0.));
                    }
                }
                Kind::Drag => {
                    if super::judge_distance(-dt, cfg.late_leniency()) <= if dt >= 0. { drag_early } else { drag_late } && !state.special {
                        state.special = fingers
                            .iter()
                            .enumerate()
                            .any(|(finger, _)| geometry(n.target, finger).is_some_and(|(dx, _)| dx < rules.special_width));
                    }
                    if !state.special && dt < -drag_late - cfg.late_leniency() {
                        score = Some((Judgement::Miss, 0.));
                    }
                    if state.special && dt < rules.special_early_ms / 1000. {
                        score = Some((Judgement::Perfect, 0.));
                    }
                }
                Kind::Flick => {
                    if !state.special && dt < -limits.late[1] * flick_late - cfg.late_leniency() {
                        score = Some((Judgement::Miss, 0.));
                    }
                    if state.special && dt < rules.special_early_ms / 1000. {
                        score = Some((Judgement::Perfect, 0.));
                    }
                }
            }
            if let Some((judgement, offset)) = score {
                if cfg.hold_tail_judge && n.kind == Kind::Hold && state.head.is_none() {
                    events.push(Event {
                        target: n.target,
                        action: Action::Score { judgement, offset },
                    });
                }
                state.done = true;
                state.hold_scored = n.kind == Kind::Hold && !matches!(judgement, Judgement::Miss) && now < n.end;
                state.click = true;
                events.push(Event {
                    target: n.target,
                    action: Action::Score { judgement, offset },
                });
                if n.kind == Kind::Hold && !matches!(judgement, Judgement::Miss) && now >= n.end {
                    events.push(Event {
                        target: n.target,
                        action: Action::EndHold,
                    });
                }
            }
        }
        self.active.retain(|i| !self.controls[*i].done || self.controls[*i].hold_scored);
        (limits, events)
    }
}
