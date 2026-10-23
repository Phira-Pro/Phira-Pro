//! One game's explicit context, separate from the global record ID and UI.
use super::{protocol, room::Launch, session::Session};
use prpr::{
    config::{Config, Mods},
    judge::{Judge, Judgement},
    scene::{SaveFn, UpdateFn},
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone)]
pub struct GameContext {
    pub session: Session,
    pub launch: Launch,
    completed: Arc<AtomicBool>,
}
impl GameContext {
    pub fn new(session: Session, launch: Launch) -> Self {
        Self {
            session,
            launch,
            completed: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn completed(&self) -> bool {
        self.completed.load(Ordering::Relaxed)
    }
    pub fn configure(&self, c: &mut Config, mods: &mut Mods) {
        // Baseline for v1 interoperability. Only this game's copy is touched.
        c.force_official_play(mods);
        c.speed = 1.;
        c.drag_protect = false;
        c.flick_protect = false;
        c.hold_tail_judge = false;
        c.hp_amount = 1.;
        c.hp_scale = 1.;
        c.mods = Mods::empty();
        *mods = Mods::empty();
        c.practice_ramp = false;
        c.auto_retry = 0.;
    }
    pub fn wrap_save(&self, save: Option<SaveFn>) -> Option<SaveFn> {
        let completed = Arc::clone(&self.completed);
        Some(Box::new(move |record| {
            if let Some(save) = &save {
                save(record)?;
            }
            completed.store(true, Ordering::Relaxed);
            Ok(())
        }))
    }
    pub fn wrap_update(&self, mut recorder: Option<UpdateFn>) -> Option<UpdateFn> {
        let session = self.session.clone();
        let launch = self.launch;
        let mut live = LiveEncoder::default();
        let mut enabled = session.snapshot().room.is_some_and(|r| r.live);
        Some(Box::new(move |t, res, judge| {
            if enabled && judge.recording_active() {
                if !session.live_context_valid(launch) {
                    enabled = false;
                } else {
                    let packets = live.update(t, res, judge);
                    for p in packets {
                        if !session.live(launch, p) {
                            enabled = false;
                            break;
                        }
                    }
                }
            }
            // Network observes judgement events first; recorder alone drains them.
            if let Some(recorder) = &mut recorder {
                recorder(t, res, judge);
            }
        }))
    }
}
#[derive(Default)]
struct LiveEncoder {
    fingers: HashMap<u64, i8>,
    last_touch: f64,
    last_judge: f64,
    judges: Vec<u8>,
    judge_count: usize,
}
impl LiveEncoder {
    fn update(&mut self, t: f64, res: &prpr::core::Resource, judge: &Judge) -> Vec<Vec<u8>> {
        use macroquad::prelude::TouchPhase;
        let mut out = Vec::new();
        let mut points = Vec::new();
        let mut count = 0;
        for touch in Judge::get_touches() {
            if touch.phase == TouchPhase::Stationary {
                continue;
            }
            let ending = matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled);
            let id = if let Some(id) = self.fingers.get(&touch.id) {
                *id
            } else {
                if ending {
                    continue;
                }
                let Some(id) = (0i8..=127).find(|id| !self.fingers.values().any(|v| v == id)) else {
                    continue;
                };
                self.fingers.insert(touch.id, id);
                id
            };
            if touch.phase == TouchPhase::Moved && t - self.last_touch < 1. / 30. {
                continue;
            }
            let (sx, sy, sw, sh) = prpr::ext::get_viewport();
            let (vx, vy, vw, vh) = res.camera.viewport.unwrap_or((sx, sy, sw, sh));
            let px = sx as f32 + (touch.position.x + 1.) * sw as f32 / 2.;
            let py = sy as f32 + sh as f32 / 2. - touch.position.y * sw as f32 / 2.;
            let x = (px - vx as f32) * 2. / vw as f32 - 1.;
            let y = (vy as f32 + vh as f32 / 2. - py) * 2. / vw as f32;
            points.push(if ending { !id } else { id } as u8);
            points.extend(half::f16::from_f32(x).to_bits().to_le_bytes());
            points.extend(half::f16::from_f32(y).to_bits().to_le_bytes());
            count += 1;
            if ending {
                self.fingers.remove(&touch.id);
            }
        }
        if count > 0 || t - self.last_touch >= 0.1 {
            let mut b = vec![3, 1];
            b.extend((t as f32).to_le_bytes());
            protocol::uleb(&mut b, count);
            b.extend(points);
            out.push(b);
            self.last_touch = t;
        }
        for (time, line, note, kind, _, _) in judge.judgements.borrow().iter() {
            let grade = match kind {
                Ok(Judgement::PerfectPlus | Judgement::Perfect) => 0,
                Ok(Judgement::Good) => 1,
                Ok(Judgement::Bad) => 2,
                Ok(Judgement::Miss) => 3,
                Err(true) => 4,
                Err(false) => 5,
                // Detailed grades are disabled by the per-game compatibility configuration.
                Ok(_) => continue,
            };
            self.judges.extend((*time as f32).to_le_bytes());
            self.judges.extend(line.to_le_bytes());
            self.judges.extend(note.to_le_bytes());
            self.judges.push(grade);
            self.judge_count += 1;
        }
        if self.judge_count > 0 && (self.judge_count >= 32 || t - self.last_judge >= 0.1) {
            let mut b = vec![4];
            protocol::uleb(&mut b, self.judge_count);
            b.append(&mut self.judges);
            self.judge_count = 0;
            self.last_judge = t;
            out.push(b);
        }
        out
    }
}
