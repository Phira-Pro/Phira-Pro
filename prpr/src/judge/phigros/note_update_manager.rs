use super::controls::{Control, JudgeContext, NoteState};
use super::{Event, Finger, Kind, Spec};
use std::collections::BTreeSet;

pub(super) struct NoteUpdateManager {
    pub(super) controls: Vec<Control>,
    pub(super) active: BTreeSet<usize>,
    entered: usize,
}

impl NoteUpdateManager {
    pub fn new(specs: &[Spec]) -> Self {
        Self {
            controls: specs.iter().map(|n| Control::new(n.kind)).collect(),
            active: BTreeSet::new(),
            entered: 0,
        }
    }

    pub fn advance_to(&mut self, specs: &[Spec], states: &mut [NoteState], time: f64) {
        for (i, note) in specs.iter().enumerate() {
            if note.time < time {
                states[i].done = true;
                if let Control::Hold(control) = &mut self.controls[i] {
                    control.hold_scored = false;
                }
            }
        }
        self.active.retain(|i| !states[*i].done);
    }

    pub fn update(
        &mut self,
        specs: &[Spec],
        states: &mut [NoteState],
        ctx: &JudgeContext,
        fingers: &[Finger],
        geometry: &mut impl FnMut((usize, u32), usize) -> Option<(f64, f64)>,
        events: &mut Vec<Event>,
    ) {
        let [drag_early, _] = ctx.rules.drag_sides().map(|v| v / 1000.);
        let [flick_early, _] = ctx.rules.flick_sides();
        let lookahead = ctx.limits.early[3].max(drag_early).max(ctx.limits.early[1] * flick_early);
        let end = specs.partition_point(|n| n.time <= ctx.now + lookahead * ctx.cfg.speed as f64);
        while self.entered < end {
            if !states[self.entered].done {
                self.active.insert(self.entered);
            }
            self.entered += 1;
        }
        for kind in [Kind::Tap, Kind::Drag, Kind::Hold, Kind::Flick] {
            for &i in &self.active {
                if specs[i].kind == kind {
                    self.controls[i].judge(&specs[i], &mut states[i], ctx, fingers, geometry, events);
                }
            }
        }
        self.active.retain(|i| self.controls[*i].is_active(&states[*i]));
    }
}
