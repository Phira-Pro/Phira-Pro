//! Per-player incremental stream state. No graphics, socket operations or scores.
use super::protocol::{JudgeEvent, TouchFrame};
use std::collections::{BTreeMap, HashSet, VecDeque};

const TOUCH_BUFFER: usize = 4096;
const DELAY: f64 = 0.12;

pub struct PlayerStream {
    pub name: String,
    pub invalid: u32,
    pub incomplete: bool,
    judges: Vec<JudgeEvent>,
    heads: HashSet<(u32, u32)>,
    scored: HashSet<(u32, u32)>,
    touches: VecDeque<TouchFrame>,
    contacts: BTreeMap<i8, [f32; 2]>,
    released: Vec<(i8, [f32; 2], f64)>,
    latest: Option<f64>,
    received_at: f64,
    last_touch: f64,
    last_judge: f64,
    current: f64,
    event_limit: usize,
}

impl PlayerStream {
    pub fn new(name: String, note_count: usize) -> Self {
        Self {
            name, invalid: 0, incomplete: false, judges: Vec::new(),
            heads: HashSet::new(), scored: HashSet::new(), touches: VecDeque::new(),
            contacts: BTreeMap::new(), released: Vec::new(), latest: None,
            received_at: 0., last_touch: f64::NEG_INFINITY, last_judge: f64::NEG_INFINITY,
            current: 0., event_limit: note_count.saturating_mul(2).min(400_000),
        }
    }
    fn observe_time(&mut self, time: f64, now: f64) {
        if self.latest.is_none_or(|last| time > last) {
            self.latest = Some(time);
            self.received_at = now;
        }
    }
    pub fn touch(&mut self, frame: TouchFrame, now: f64, end: f64) {
        let t = frame.time as f64;
        if !t.is_finite() || t < self.last_touch || t < -30. || t > end + 30.
            || frame.points.iter().any(|p| !half::f16::from_bits(p.x).is_finite() || !half::f16::from_bits(p.y).is_finite()) {
            self.invalid = self.invalid.saturating_add(1);
            return;
        }
        self.last_touch = t;
        self.observe_time(t, now);
        if self.touches.len() == TOUCH_BUFFER {
            self.touches.clear();
            self.contacts.clear();
            self.released.clear();
            self.incomplete = true;
        }
        self.touches.push_back(frame);
    }
    pub fn judge(&mut self, event: JudgeEvent, now: f64, end: f64, notes: &[usize]) {
        let time = event.time as f64;
        if !time.is_finite() || time < self.last_judge || time < -30. || time > end + 30.
            || event.judgement > 5 || notes.get(event.line_id as usize).is_none_or(|count| event.note_id as usize >= *count) {
            self.invalid = self.invalid.saturating_add(1);
            return;
        }
        let key = (event.line_id, event.note_id);
        let fresh = if event.judgement >= 4 {
            !self.scored.contains(&key) && self.heads.insert(key)
        } else {
            self.scored.insert(key)
        };
        if !fresh { return; }
        if self.judges.len() >= self.event_limit {
            self.incomplete = true;
            return;
        }
        self.last_judge = time;
        self.observe_time(time, now);
        self.judges.push(event);
    }
    pub fn advance(&mut self, now: f64) {
        if let Some(latest) = self.latest {
            // Brief interpolation delay absorbs normal network batching; a
            // stalled transport never lets the chart run freely to its end.
            self.current = self.current.max(latest - DELAY + (now - self.received_at).clamp(0., 0.25)).max(0.);
        }
        while self.touches.front().is_some_and(|frame| frame.time as f64 <= self.current) {
            let frame = self.touches.pop_front().unwrap();
            for point in frame.points {
                let pos = [half::f16::from_bits(point.x).to_f32(), half::f16::from_bits(point.y).to_f32()];
                if point.id < 0 {
                    self.contacts.remove(&!point.id);
                    self.released.push((!point.id, pos, frame.time as f64));
                } else {
                    self.contacts.insert(point.id, pos);
                }
            }
        }
        self.released.retain(|(_, _, at)| self.current - *at < 0.08);
    }
    pub fn time(&self) -> f64 { self.current }
    pub fn started(&self) -> bool { self.latest.is_some() }
    pub fn stalled(&self, now: f64) -> bool { self.latest.is_some() && now - self.received_at > 0.8 }
    pub fn judges(&self) -> &[JudgeEvent] { &self.judges }
    pub fn contacts(&self) -> Vec<(u64, [f32; 2])> {
        self.contacts.iter().map(|(&id, &pos)| (id as u64, pos)).collect()
    }
    pub fn markers(&self) -> Vec<(u64, [f32; 2], f32)> {
        let mut markers = self.released.iter().map(|(id, pos, at)| (*id as u64, *pos, (1. - (self.current - at) / 0.08).clamp(0., 1.) as f32)).collect::<Vec<_>>();
        markers.extend(self.contacts.iter().map(|(&id, &pos)| (id as u64, pos, 1.)));
        markers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp::protocol::TouchPoint;
    fn point(id: i8) -> TouchPoint { TouchPoint { id, x: half::f16::from_f32(0.3).to_bits(), y: 0 } }
    #[test]
    fn stationary_contacts_persist_empty_frames_advance_and_end_releases() {
        let mut p = PlayerStream::new("player".into(), 1);
        p.touch(TouchFrame { time: 1., points: vec![point(0)] }, 0., 10.);
        p.advance(0.2);
        assert_eq!(p.contacts().len(), 1);
        p.touch(TouchFrame { time: 2., points: vec![] }, 1., 10.);
        p.advance(1.2);
        assert_eq!(p.contacts().len(), 1);
        p.touch(TouchFrame { time: 2.1, points: vec![point(!0)] }, 1.2, 10.);
        p.advance(1.4);
        assert!(p.contacts().is_empty());
        let t = p.time(); p.advance(100.); assert!(p.time() - t < 0.1);
    }
    #[test]
    fn players_are_independent_and_duplicate_or_invalid_judges_are_ignored() {
        let mut a = PlayerStream::new("a".into(), 1);
        let b = PlayerStream::new("b".into(), 1);
        let mut e = JudgeEvent { time: 1., line_id: 0, note_id: 0, judgement: 4 };
        a.judge(e, 0., 10., &[1]); a.judge(e, 0., 10., &[1]);
        e.time = 2.; e.judgement = 0; a.judge(e, 1., 10., &[1]);
        a.judge(e, 1., 10., &[1]);
        e.note_id = 10; a.judge(e, 1., 10., &[1]);
        assert_eq!(a.judges().len(), 2); assert_eq!(a.invalid, 1);
        assert!(b.judges().is_empty());
    }
}
