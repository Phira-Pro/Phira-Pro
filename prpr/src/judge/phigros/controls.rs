use super::{Action, Config, Event, Finger, JudgeWindows, Judgement, Kind, PhigrosRules, Spec};
use super::super::{judgement_at_distance, judgement_of_offset, timing_error, UP_TOLERANCE};

#[derive(Clone, Default)]
pub(super) struct NoteState {
    pub is_judged: bool,
    pub is_judged_for_flick: bool,
    pub done: bool,
}

impl NoteState {
    fn score(&mut self, note: &Spec, judgement: Judgement, offset: f64, events: &mut Vec<Event>) {
        self.done = true;
        self.is_judged = true;
        events.push(Event {
            target: note.target,
            action: Action::Score { judgement, offset },
        });
    }
}

pub(super) struct JudgeContext<'a> {
    pub cfg: &'a Config,
    pub now: f64,
    pub limits: &'a JudgeWindows,
    pub rules: &'a PhigrosRules,
    /// Real frame time, used for cadence-independent hold release grace.
    pub frame_seconds: f64,
}

#[derive(Clone, Default)]
pub(super) struct ClickControl {
    is_judged: bool,
}

impl ClickControl {
    fn judge(&mut self, note: &Spec, state: &mut NoteState, ctx: &JudgeContext, events: &mut Vec<Event>) {
        let offset = timing_error(ctx.now, note.time, ctx.cfg.speed as f64);
        self.is_judged |= state.is_judged;
        if self.is_judged {
            let side = ctx.limits.for_offset(offset);
            state.score(note, judgement_at_distance(offset.abs(), &side, true), offset, events);
        } else if offset > ctx.limits.late_acceptance() {
            state.score(note, Judgement::Miss, 0., events);
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct HoldControl {
    pub is_judged: bool,
    pub head: Option<(Judgement, f64)>,
    pub safe: f64,
    released: Option<f64>,
    pub hold_scored: bool,
}

impl HoldControl {
    fn judge(
        &mut self,
        note: &Spec,
        state: &mut NoteState,
        ctx: &JudgeContext,
        fingers: &[Finger],
        geometry: &mut impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
        events: &mut Vec<Event>,
    ) {
        if state.done {
            if self.hold_scored && ctx.now >= note.end {
                self.hold_scored = false;
                events.push(Event { target: note.target, action: Action::EndHold });
            }
            return;
        }
        let speed = ctx.cfg.speed as f64;
        let offset = timing_error(ctx.now, note.time, speed);
        let side = ctx.limits.for_offset(offset);
        let mut score = None;
        if self.head.is_none() {
            self.is_judged |= state.is_judged;
            if !self.is_judged && offset > ctx.limits.late_acceptance() {
                score = Some((Judgement::Miss, 0.));
            }
            if self.is_judged && offset.abs() < side.acceptance() {
                let judgement = judgement_at_distance(offset.abs(), &side, true);
                self.head = Some((judgement, offset));
                self.safe = ctx.rules.hold_safe_seconds();
                events.push(Event {
                    target: note.target,
                    action: Action::Head { judgement, offset },
                });
            } else if self.is_judged {
                state.is_judged = false;
            }
        }
        let milthm = ctx.cfg.mods.contains(crate::config::Mods::MILTHM);
        let tail_judge = ctx.cfg.hold_tail_judge && !milthm;
        if let Some(head) = self.head {
            if milthm {
                if ctx.now >= note.end {
                    score = Some(head);
                } else if fingers.is_empty() {
                    score = Some((Judgement::Miss, 0.));
                }
            } else if tail_judge {
                let on_note = hold_contact(fingers, note.target, ctx.rules.tap_width, geometry);
                // Explicit custom extension; disabled in the built-in Phigros preset.
                if on_note {
                    self.released = None;
                } else {
                    self.released.get_or_insert(ctx.now);
                }
                if self.released.is_some_and(|up| ctx.now > up + UP_TOLERANCE * speed) && (note.end - ctx.now) / speed > ctx.limits.early[3] {
                    score = Some((Judgement::Miss, 0.));
                }
                if score.is_none() && ctx.now >= note.end {
                    let off = self.released.map_or(0., |up| timing_error(up, note.end, speed));
                    score = Some((judgement_of_offset(off, ctx.limits), off));
                }
            } else {
                if hold_contact(fingers, note.target, ctx.rules.tap_width, geometry) {
                    self.safe = ctx.rules.hold_safe_seconds();
                } else {
                    // Cap one frame's contribution so a GC/layout stall cannot
                    // consume the whole grace at once; low refresh rates keep
                    // at least the same number of tolerated frames.
                    self.safe -= ctx.frame_seconds.min(1. / 60.);
                    if self.safe < -1e-9 {
                        score = Some((Judgement::Miss, 0.));
                    }
                }
                if score.is_none() && ctx.now > note.end - ctx.rules.hold_tail_ms / 1000. * speed {
                    score = Some(head);
                }
            }
        } else if ctx.now > note.end + ctx.rules.hold_delayed_miss_ms / 1000. * speed {
            score = Some((Judgement::Miss, 0.));
        }
        if let Some((judgement, offset)) = score {
            if tail_judge && self.head.is_none() {
                events.push(Event { target: note.target, action: Action::Score { judgement, offset } });
            }
            state.score(note, judgement, offset, events);
            self.hold_scored = !matches!(judgement, Judgement::Miss) && ctx.now < note.end;
            if !matches!(judgement, Judgement::Miss) && ctx.now >= note.end {
                events.push(Event { target: note.target, action: Action::EndHold });
            }
        }
    }
}

fn hold_contact(
    fingers: &[Finger],
    target: (usize, u32),
    tap_width: f64,
    geometry: &mut impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
) -> bool {
    fingers.iter().enumerate().any(|(finger, _)| geometry(target, finger).is_some_and(|(dx, _)| dx < tap_width))
}

#[derive(Clone, Default)]
pub(super) struct DragControl {
    pub is_judged: bool,
}

impl DragControl {
    fn judge(
        &mut self,
        note: &Spec,
        state: &mut NoteState,
        ctx: &JudgeContext,
        fingers: &[Finger],
        geometry: &mut impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
        events: &mut Vec<Event>,
    ) {
        let dt = (note.time - ctx.now) / ctx.cfg.speed as f64;
        let [early, late] = ctx.rules.drag_sides().map(|v| v / 1000.);
        if dt.abs() <= if dt >= 0. { early } else { late } && !self.is_judged {
            self.is_judged = fingers
                .iter()
                .enumerate()
                .any(|(finger, _)| geometry(note.target, finger).is_some_and(|(dx, _)| dx < ctx.rules.special_width));
        }
        if !self.is_judged && dt < -late {
            state.score(note, Judgement::Miss, 0., events);
        } else if self.is_judged && dt < ctx.rules.special_early_ms / 1000. {
            state.score(note, Judgement::Perfect, 0., events);
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct FlickControl {
    is_judged: bool,
}

impl FlickControl {
    fn judge(&mut self, note: &Spec, state: &mut NoteState, ctx: &JudgeContext, events: &mut Vec<Event>) {
        let dt = (note.time - ctx.now) / ctx.cfg.speed as f64;
        let [_, late] = ctx.rules.flick_sides();
        self.is_judged |= state.is_judged_for_flick;
        if !self.is_judged && dt < -ctx.limits.late[1] * late {
            state.score(note, Judgement::Miss, 0., events);
            state.is_judged_for_flick = true;
        } else if self.is_judged && dt < ctx.rules.special_early_ms / 1000. {
            state.score(note, Judgement::Perfect, 0., events);
        }
    }
}

#[derive(Clone)]
pub(super) enum Control {
    Click(ClickControl),
    Hold(HoldControl),
    Drag(DragControl),
    Flick(FlickControl),
}

impl Control {
    pub fn new(kind: Kind) -> Self {
        match kind {
            Kind::Tap => Self::Click(ClickControl::default()),
            Kind::Hold => Self::Hold(HoldControl::default()),
            Kind::Drag => Self::Drag(DragControl::default()),
            Kind::Flick => Self::Flick(FlickControl::default()),
        }
    }

    pub fn is_active(&self, state: &NoteState) -> bool {
        !state.done || matches!(self, Self::Hold(control) if control.hold_scored)
    }

    pub fn judge(
        &mut self,
        note: &Spec,
        state: &mut NoteState,
        ctx: &JudgeContext,
        fingers: &[Finger],
        geometry: &mut impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
        events: &mut Vec<Event>,
    ) {
        if state.done && !matches!(self, Self::Hold(_)) {
            return;
        }
        match self {
            Self::Click(control) => control.judge(note, state, ctx, events),
            Self::Hold(control) => control.judge(note, state, ctx, fingers, geometry, events),
            Self::Drag(control) => control.judge(note, state, ctx, fingers, geometry, events),
            Self::Flick(control) => control.judge(note, state, ctx, events),
        }
    }
}
