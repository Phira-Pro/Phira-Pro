//! A time sweep keeps large charts from visiting every past/future block twice
//! per frame. IDs remain in source order, including additive R8 blend ordering.
use super::BlockArea;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct BlockTimeline {
    count: Option<usize>,
    events: Vec<(f64, usize, bool)>,
    cursor: usize,
    time: Option<f64>,
    visible: BTreeSet<usize>,
    ids: Vec<usize>,
}

impl BlockTimeline {
    pub fn at(&mut self, areas: &[BlockArea], time: f64) -> &[usize] {
        if self.count != Some(areas.len()) {
            *self = Self::default();
            self.count = Some(areas.len());
            for (id, area) in areas.iter().enumerate() {
                if area.appear_time < area.disappear_time {
                    self.events.push((area.appear_time, id, true));
                    self.events.push((area.disappear_time, id, false));
                }
            }
            self.events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        }
        let end = self.events.partition_point(|event| event.0 <= time);
        let mut changed = false;
        if self.time.is_some_and(|previous| time < previous) {
            self.visible.clear();
            self.cursor = 0;
            changed = true;
        }
        while self.cursor < end {
            let (_, id, appears) = self.events[self.cursor];
            if appears {
                self.visible.insert(id);
            } else {
                self.visible.remove(&id);
            }
            self.cursor += 1;
            changed = true;
        }
        if changed {
            self.ids.clear();
            self.ids.extend(self.visible.iter().copied());
        }
        self.time = Some(time);
        &self.ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BlockPhase, Vector};

    #[test]
    fn sweep_matches_visibility_at_boundaries_and_after_seeks() {
        let areas: Vec<_> = (0..1000)
            .map(|id| BlockArea {
                rpe_canvas: false,
                top_right: Vector::new(1., 1.),
                bottom_left: Vector::zeros(),
                appear_time: (id % 53) as f64 / 4. - 3.,
                disappear_time: (id % 53) as f64 / 4. - 3. + (id % 7) as f64 / 4.,
                enable_time: 0.,
                disable_time: 100.,
                is_subtract: id % 2 == 0,
                rotate_events: vec![],
                move_events: vec![],
                scale_events: vec![],
            })
            .collect();
        let mut sweep = BlockTimeline::default();
        for t in (-20..80).map(|i| i as f64 / 4.).chain([0., 10., -10., 3.25, 3., 3.25]) {
            let expected: Vec<_> = areas
                .iter()
                .enumerate()
                .filter(|(_, a)| a.phase(t) != BlockPhase::Hidden)
                .map(|(id, _)| id)
                .collect();
            assert_eq!(sweep.at(&areas, t), expected, "time={t}");
        }
        assert!(sweep.at(&[], 0.).is_empty());
    }
}
