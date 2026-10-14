use super::{Action, Config, Event, Finger, JudgeWindows, Judgement, Kind, PhigrosRules, Spec};
use super::super::{judge_distance, judgement_at_distance, judgement_of_offset, UP_TOLERANCE};

#[derive(Clone, Default)]
pub(super) struct NoteState {
    pub click: bool,
    pub special: bool,
    pub done: bool,
}

impl NoteState {
    fn score(&mut self, note: &Spec, judgement: Judgement, offset: f64, events: &mut Vec<Event>) {
        self.done = true;
        self.click = true;
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
}

#[derive(Clone, Default)]
pub(super) struct ClickControl {
    is_judged: bool,
}

impl ClickControl {
    fn judge(&mut self, note: &Spec, state: &mut NoteState, ctx: &JudgeContext, events: &mut Vec<Event>) {
        let dt = (note.time - ctx.now) / ctx.cfg.speed as f64;
        self.is_judged |= state.click;
        if self.is_judged {
            let side = ctx.limits.for_offset(-dt);
            let abs = judge_distance(-dt, ctx.cfg.late_leniency());
            state.score(note, judgement_at_distance(abs, &side, true), -dt, events);
        } else if dt < -ctx.limits.late_acceptance() - ctx.cfg.late_leniency() {
            state.score(note, Judgement::Miss, 0., events);
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct HoldControl {
    pub is_judged: bool,
    pub head: Option<(Judgement, f64)>,
    pub safe: i32,
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
        let dt = (note.time - ctx.now) / speed;
        let side = ctx.limits.for_offset(-dt);
        let abs = judge_distance(-dt, ctx.cfg.late_leniency());
        let mut score = None;
        if self.head.is_none() {
            self.is_judged |= state.click;
            if !self.is_judged && dt < -ctx.limits.late_acceptance() - ctx.cfg.late_leniency() {
                score = Some((Judgement::Miss, 0.));
            }
            if self.is_judged && abs < side.acceptance() {
                let judgement = judgement_at_distance(abs, &side, true);
                self.head = Some((judgement, -dt));
                self.safe = ctx.rules.hold_safe_frames;
                events.push(Event {
                    target: note.target,
                    action: Action::Head { judgement, offset: -dt },
                });
            } else if self.is_judged {
                state.click = false;
            }
        }
        if let Some(head) = self.head {
            let on_note = fingers
                .iter()
                .enumerate()
                .any(|(finger, _)| geometry(note.target, finger).is_some_and(|(dx, _)| dx < ctx.rules.tap_width));
            if ctx.cfg.hold_tail_judge {
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
                    let off = self.released.map_or(0., |up| (up - note.end) / speed);
                    score = Some((judgement_of_offset(off, ctx.limits), off));
                }
            } else {
                if on_note {
                    self.safe = ctx.rules.hold_safe_frames;
                } else if self.safe < 0 {
                    score = Some((Judgement::Miss, 0.));
                } else {
                    self.safe -= 1;
                }
                if score.is_none() && ctx.now > note.end - ctx.rules.hold_tail_ms / 1000. * speed {
                    score = Some(head);
                }
            }
        } else if ctx.now > note.end + ctx.rules.hold_delayed_miss_ms / 1000. * speed {
            score = Some((Judgement::Miss, 0.));
        }
        if let Some((judgement, offset)) = score {
            if ctx.cfg.hold_tail_judge && self.head.is_none() {
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
        if judge_distance(-dt, ctx.cfg.late_leniency()) <= if dt >= 0. { early } else { late } && !self.is_judged {
            self.is_judged = fingers
                .iter()
                .enumerate()
                .any(|(finger, _)| geometry(note.target, finger).is_some_and(|(dx, _)| dx < ctx.rules.special_width));
        }
        if !self.is_judged && dt < -late - ctx.cfg.late_leniency() {
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
        self.is_judged |= state.special;
        if !self.is_judged && dt < -ctx.limits.late[1] * late - ctx.cfg.late_leniency() {
            state.score(note, Judgement::Miss, 0., events);
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
