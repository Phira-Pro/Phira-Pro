use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

pub const MAX_NODES: usize = 10;
const GAP: f64 = 1.;

#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize)]
pub struct Segment {
    pub attempts: u64,
    pub fc: u64,
    pub ap: u64,
    pub fc_rate: Option<f64>,
    pub ap_rate: Option<f64>,
}

impl Segment {
    fn count(&mut self, judgement: u8) {
        self.attempts += 1;
        self.fc += u64::from(judgement < 2);
        self.ap += u64::from(judgement == 0);
        let rate = |passed| (passed as f64 / self.attempts as f64 * 1000.).round() / 10.;
        self.fc_rate = Some(rate(self.fc));
        self.ap_rate = Some(rate(self.ap));
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PassRate {
    pub duration: f64,
    pub nodes: Vec<f64>,
    pub segments: Vec<Segment>,
}

impl PassRate {
    pub fn new(duration: f64, nodes: Vec<f64>) -> Self {
        Self {
            duration,
            segments: vec![Segment::default(); nodes.len() + 1],
            nodes,
        }
    }

    pub fn index(&self, time: f64) -> usize {
        self.nodes.partition_point(|node| *node <= time)
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        let reader = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let value: Self = serde_json::from_reader(std::io::BufReader::new(reader))?;
        ensure!(value.valid(), "Invalid PassRate data");
        Ok(Some(value))
    }

    fn valid(&self) -> bool {
        self.duration.is_finite()
            && self.duration > 0.
            && self.nodes.len() <= MAX_NODES
            && self.segments.len() == self.nodes.len() + 1
            && self
                .nodes
                .iter()
                .copied()
                .try_fold(0., |left, right| (right - left >= GAP - 1e-9).then_some(right))
                .is_some_and(|last| self.nodes.is_empty() || self.duration - last >= GAP - 1e-9)
            && self.segments.iter().all(|s| s.ap <= s.fc && s.fc <= s.attempts && s.attempts < u64::MAX)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        ensure!(self.valid(), "Invalid PassRate data");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temp, self)?;
        temp.write_all(b"\n")?;
        temp.persist(path).map_err(|err| err.error)?;
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Attempt {
    start: f64,
    furthest: f64,
    grades: [u8; MAX_NODES + 1],
}

impl Attempt {
    pub fn start_at(&mut self, time: f64) {
        self.start = time.max(0.);
    }

    pub fn advance(&mut self, time: f64) {
        self.furthest = self.furthest.max(time);
    }

    pub fn mark(&mut self, index: usize, grade: u8) {
        self.grades[index] = self.grades[index].max(grade);
    }

    pub fn finish(self, data: &mut PassRate) -> bool {
        if self.furthest <= self.start {
            return false;
        }
        // Stopping exactly on a marker does not enter the next segment.
        let last = data.nodes.partition_point(|node| *node < self.furthest);
        let first = data.index(self.start);
        for (segment, grade) in data.segments[first..=last].iter_mut().zip(&self.grades[first..=last]) {
            segment.count(*grade);
        }
        true
    }
}

pub(crate) fn add_node(nodes: &[f64], duration: f64) -> Option<(usize, f64)> {
    if nodes.len() >= MAX_NODES {
        return None;
    }
    let last = nodes.last().copied().unwrap_or(0.);
    if duration - last >= 2. * GAP {
        return Some((nodes.len(), last + GAP));
    }
    let mut previous = 0.;
    for (index, next) in nodes.iter().copied().chain(std::iter::once(duration)).enumerate() {
        if next - previous >= 2. * GAP {
            return Some((index, previous + GAP));
        }
        previous = next;
    }
    None
}

pub(crate) fn move_node(nodes: &[f64], duration: f64, index: usize, time: f64) -> f64 {
    let left = index.checked_sub(1).map_or(0., |i| nodes[i]) + GAP;
    let right = nodes.get(index + 1).copied().unwrap_or(duration) - GAP;
    time.clamp(left, right.max(left))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_runs_only_count_reached_segments_and_ap_also_counts_as_fc() {
        let mut data = PassRate::new(20., vec![5., 10.]);
        for (time, grade) in [(7., 0), (4., 1), (5., 0), (20., 2)] {
            let mut run = Attempt::default();
            run.advance(time);
            run.advance(1.); // pause rewind cannot reduce progress
            run.mark(0, grade);
            assert!(run.finish(&mut data));
        }
        assert_eq!(data.segments[0].fc_rate, Some(75.));
        assert_eq!(data.segments[0].ap_rate, Some(50.));
        assert_eq!(data.segments[1].attempts, 2);
        assert_eq!(data.segments[2].attempts, 1);
        assert!(!Attempt::default().finish(&mut data));
    }

    #[test]
    fn markers_cannot_cross_and_need_one_second_on_each_side() {
        let nodes = [1., 3., 6.];
        assert_eq!(add_node(&nodes, 10.), Some((3, 7.)));
        assert_eq!(add_node(&nodes, 7.), Some((1, 2.)));
        assert_eq!(move_node(&nodes, 10., 1, -10.), 2.);
        assert_eq!(move_node(&nodes, 10., 1, 10.), 5.);
        assert_eq!(move_node(&nodes, 10., 2, 20.), 9.);
        assert_eq!(add_node(&[1.; MAX_NODES], 60.), None);
    }
}
