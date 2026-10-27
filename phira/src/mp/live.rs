//! Bounded inbound live-data mailbox for monitor/render consumers.
use super::protocol::LiveBatch;
use std::collections::VecDeque;
use std::mem::size_of;

const MAX_BATCHES: usize = 128;
const MAX_BUFFER_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveScope {
    pub room_generation: u64,
    pub chart_generation: u64,
    pub round: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiveEvent {
    pub scope: LiveScope,
    pub batch: LiveBatch,
    /// Approximate retained heap size, used only for mailbox bounds.
    bytes: usize,
}

#[derive(Default)]
pub struct LiveBuffer {
    scope: Option<LiveScope>,
    events: VecDeque<LiveEvent>,
    bytes: usize,
    dropped_batches: u64,
}

impl LiveBuffer {
    pub fn set_scope(&mut self, scope: Option<LiveScope>) {
        if self.scope != scope {
            self.events.clear();
            self.bytes = 0;
            self.dropped_batches = 0;
            self.scope = scope;
        }
    }
    pub fn push(&mut self, scope: LiveScope, batch: LiveBatch) {
        self.set_scope(Some(scope));
        let bytes = match &batch {
            LiveBatch::Touches { frames, .. } => {
                frames.len() * size_of::<super::protocol::TouchFrame>()
                    + frames
                        .iter()
                        .map(|frame| frame.points.len() * size_of::<super::protocol::TouchPoint>())
                        .sum::<usize>()
            }
            LiveBatch::Judges { events, .. } => events.len() * size_of::<super::protocol::JudgeEvent>(),
        };
        // Oversized bursts are deliberately discarded as whole batches: partial
        // touch/judgement streams would produce misleading monitor state.
        if bytes > MAX_BUFFER_BYTES {
            self.dropped_batches = self.dropped_batches.saturating_add(1);
            return;
        }
        while self.events.len() >= MAX_BATCHES || self.bytes + bytes > MAX_BUFFER_BYTES {
            let Some(old) = self.events.pop_front() else { break };
            self.bytes = self.bytes.saturating_sub(old.bytes);
            self.dropped_batches = self.dropped_batches.saturating_add(1);
        }
        self.bytes += bytes;
        self.events.push_back(LiveEvent { scope, batch, bytes });
    }

    /// Transfer pending events to the consumer without cloning their payloads.
    pub fn drain(&mut self, scope: LiveScope) -> Vec<LiveEvent> {
        if self.scope != Some(scope) {
            return Vec::new();
        }
        self.bytes = 0;
        self.events.drain(..).collect()
    }

    pub fn dropped_batches(&self) -> u64 {
        self.dropped_batches
    }
}
