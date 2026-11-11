//! Bounded conservative managed texture-read markers. No GL queries or waits.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

const LIMIT: usize = 256;
static OWNERS: AtomicU64 = AtomicU64::new(1);

/// Identity of one explicitly registered texture in one QuadGl/context.
/// Private generations reject stale handles and recycled native names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureReadToken {
    context: usize,
    owner: u64,
    generation: u64,
    texture: u32,
}

/// Managed possible-read state, not GPU completion or elapsed time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureReadStatus {
    pub queued: bool,
    /// Conservative serial recorded after a positive-index managed draw
    /// attempt. Context.draw returns void and may reject an invalid request.
    pub last_submitted: u64,
}

struct Entry {
    generation: u64,
    last: u64,
}
pub(crate) struct Tracker {
    context: usize,
    owner: u64,
    generation: u64,
    serial: u64,
    entries: BTreeMap<u32, Entry>,
    known: bool,
    failed: bool,
}
impl Tracker {
    pub(crate) fn new(context: usize) -> Self {
        let owner = OWNERS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                old.checked_add(1)
            })
            .ok();
        Self {
            context,
            owner: owner.unwrap_or(0),
            generation: 0,
            serial: 0,
            entries: BTreeMap::new(),
            known: owner.is_some(),
            failed: owner.is_none(),
        }
    }
    /// A changed managed context permanently invalidates this owner. Equal
    /// IDs only describe managed Context identity, never native EGL lifetime.
    pub(crate) fn check_context(&mut self, current: usize) -> bool {
        if current != self.context {
            self.fail();
            return false;
        }
        true
    }
    pub(crate) fn active(&self) -> bool {
        !self.entries.is_empty()
    }
    pub(crate) fn invalidate(&mut self) {
        self.known = false;
    }
    fn fail(&mut self) {
        self.failed = true;
        self.known = false;
    }
    pub(crate) fn register(&mut self, texture: u32, queue_empty: bool) -> Option<TextureReadToken> {
        if self.failed
            || texture == 0
            || self.entries.len() >= LIMIT
            || self.entries.contains_key(&texture)
        {
            return None;
        }
        if !self.known {
            if !self.entries.is_empty() || !queue_empty {
                return None;
            }
            self.known = true;
        }
        let generation = match self.generation.checked_add(1) {
            Some(next) => next,
            None => {
                self.fail();
                return None;
            }
        };
        self.generation = generation;
        self.entries.insert(
            texture,
            Entry {
                generation,
                last: 0,
            },
        );
        Some(TextureReadToken {
            context: self.context,
            owner: self.owner,
            generation,
            texture,
        })
    }
    pub(crate) fn texture(&self, token: TextureReadToken) -> Option<u32> {
        let entry = self.entries.get(&token.texture)?;
        (token.context == self.context
            && token.owner == self.owner
            && token.generation == entry.generation)
            .then_some(token.texture)
    }
    pub(crate) fn status(
        &self,
        token: TextureReadToken,
        queued: bool,
    ) -> Option<TextureReadStatus> {
        if self.failed || !self.known {
            return None;
        }
        self.texture(token)?;
        Some(TextureReadStatus {
            queued,
            last_submitted: self.entries.get(&token.texture)?.last,
        })
    }
    /// Cleanup remains possible in an unknown epoch, but only after the owner
    /// has consumed every possibly referring pending command.
    pub(crate) fn unregister(&mut self, token: TextureReadToken, pending: Option<bool>) -> bool {
        if self.texture(token).is_none() || pending != Some(false) {
            return false;
        }
        self.entries.remove(&token.texture);
        true
    }
    pub(crate) fn submitted(&mut self, indices: usize, images: impl IntoIterator<Item = u32>) {
        if !self.active() || self.failed || indices == 0 {
            return;
        }
        self.serial = match self.serial.checked_add(1) {
            Some(next) => next,
            None => {
                self.fail();
                return;
            }
        };
        for image in images {
            if let Some(entry) = self.entries.get_mut(&image) {
                entry.last = self.serial;
            }
        }
    }
}

/// Conservative semantics gate for normal queued commands. Named textures are
/// late-bound, and custom shaders may consume flush-time Projection/_Time.
/// Their queued state is deliberately unknown, even if all names are assigned.
pub(crate) fn pending_normal(
    target: u32,
    primary: u32,
    indices: usize,
    capture: bool,
    pipeline_present: bool,
    custom: bool,
    screen: bool,
    named: impl IntoIterator<Item = Option<u32>>,
) -> Option<bool> {
    if capture || !pipeline_present || custom || screen {
        return None;
    }
    if named.into_iter().next().is_some() {
        return None;
    }
    Some(indices > 0 && primary == target)
}
pub(crate) fn pending_resident(
    target: u32,
    indices: usize,
    capture: bool,
    images: impl IntoIterator<Item = u32>,
) -> Option<bool> {
    if capture {
        return None;
    }
    Some(indices > 0 && images.into_iter().any(|image| image == target))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn submitted_images_include_primary_screen_named_and_resident_bindings() {
        let mut tracker = Tracker::new(7);
        let tokens: Vec<_> = (11..16)
            .map(|id| tracker.register(id, true).unwrap())
            .collect();
        let mut expected = [0u64; 5];
        for step in 1..=1000u64 {
            let count = if step % 7 == 0 { 0 } else { 6 };
            let images = [
                11 + (step % 5) as u32,
                11 + ((step + 1) % 5) as u32,
                11 + ((step + 3) % 5) as u32,
            ];
            tracker.submitted(count, images);
            if count > 0 {
                let serial = step - step / 7;
                for id in images {
                    expected[(id - 11) as usize] = serial;
                }
            }
            for (index, &token) in tokens.iter().enumerate() {
                assert_eq!(
                    tracker.status(token, false).unwrap().last_submitted,
                    expected[index]
                );
            }
        }
    }
    #[test]
    fn queued_primary_and_resident_reads_and_opaque_paths_are_distinct() {
        assert_eq!(
            pending_normal(9, 9, 6, false, true, false, false, []),
            Some(true)
        );
        assert_eq!(
            pending_normal(9, 8, 6, false, true, false, false, []),
            Some(false)
        );
        assert_eq!(
            pending_normal(9, 9, 0, false, true, false, false, []),
            Some(false)
        );
        assert_eq!(
            pending_normal(9, 9, 6, false, false, false, false, []),
            None
        );
        assert_eq!(pending_normal(9, 9, 6, true, true, false, false, []), None);
        assert_eq!(pending_normal(9, 9, 6, false, true, false, true, []), None);
        assert_eq!(pending_normal(9, 8, 6, false, true, true, false, []), None);
        assert_eq!(
            pending_normal(9, 8, 6, false, true, false, false, [Some(9)]),
            None
        );
        assert_eq!(
            pending_normal(9, 8, 6, false, true, false, false, [None]),
            None
        );
        assert_eq!(pending_resident(9, 6, false, [1, 2, 9, 4]), Some(true));
        assert_eq!(pending_resident(9, 6, false, [1, 2, 3, 4]), Some(false));
        assert_eq!(pending_resident(9, 0, false, [9]), Some(false));
        assert_eq!(pending_resident(9, 0, true, [9]), None);
    }
    #[test]
    fn token_owners_contexts_pending_cleanup_and_recycled_ids_do_not_alias() {
        let mut a = Tracker::new(1);
        let b = Tracker::new(1);
        let c = Tracker::new(2);
        let old = a.register(42, true).unwrap();
        assert!(b.status(old, false).is_none());
        assert!(c.status(old, false).is_none());
        assert!(!a.unregister(old, Some(true)));
        assert!(!a.unregister(old, None));
        a.submitted(6, [42]);
        assert_eq!(a.status(old, false).unwrap().last_submitted, 1);
        assert!(a.unregister(old, Some(false)));
        let fresh = a.register(42, true).unwrap();
        assert_ne!(old, fresh);
        assert!(a.status(old, false).is_none());
        assert_eq!(a.status(fresh, false).unwrap().last_submitted, 0);
        assert!(!a.unregister(old, Some(false)));
    }
    #[test]
    fn unknown_epoch_requires_all_old_tokens_removed_and_empty_queue() {
        let mut tracker = Tracker::new(3);
        let a = tracker.register(8, true).unwrap();
        let b = tracker.register(9, true).unwrap();
        tracker.invalidate();
        assert!(tracker.status(a, false).is_none());
        assert!(tracker.register(10, true).is_none());
        assert!(tracker.unregister(a, Some(false)));
        assert!(tracker.unregister(b, Some(false)));
        assert!(tracker.register(8, false).is_none());
        let fresh = tracker.register(8, true).unwrap();
        assert!(tracker.status(a, false).is_none());
        assert!(tracker.status(fresh, false).is_some());
    }
    #[test]
    fn capacity_duplicate_and_generation_overflow_are_bounded() {
        let mut tracker = Tracker::new(4);
        assert!(tracker.register(0, true).is_none());
        let first = tracker.register(1, true).unwrap();
        assert!(tracker.register(1, true).is_none());
        for id in 2..=256 {
            assert!(tracker.register(id, true).is_some());
        }
        assert!(tracker.register(257, true).is_none());
        assert!(tracker.status(first, false).is_some());
        let mut tracker = Tracker::new(4);
        let token = tracker.register(1, true).unwrap();
        tracker.generation = u64::MAX;
        assert!(tracker.register(2, true).is_none());
        assert!(tracker.status(token, false).is_none());
        assert!(tracker.unregister(token, Some(false)));
        assert!(tracker.register(2, true).is_none());
    }
    #[test]
    fn same_tracker_changed_context_fails_closed_permanently() {
        let mut tracker = Tracker::new(7);
        let token = tracker.register(1, true).unwrap();
        assert!(tracker.check_context(7));
        assert!(tracker.status(token, false).is_some());
        assert!(!tracker.check_context(8));
        assert!(tracker.status(token, false).is_none());
        assert!(tracker.register(2, true).is_none());
        assert!(tracker.check_context(7));
        assert!(tracker.status(token, false).is_none());
        assert!(tracker.unregister(token, Some(false)));
        assert!(tracker.register(1, true).is_none());
    }
    #[test]
    fn serial_overflow_fails_closed_and_zero_indices_do_not_advance() {
        let mut tracker = Tracker::new(5);
        let token = tracker.register(1, true).unwrap();
        tracker.serial = u64::MAX;
        tracker.submitted(0, [1]);
        assert_eq!(tracker.status(token, false).unwrap().last_submitted, 0);
        tracker.submitted(6, [1]);
        assert!(tracker.status(token, false).is_none());
        assert!(tracker.unregister(token, Some(false)));
        assert!(tracker.register(2, true).is_none());
        assert!(Tracker::new(5).register(2, true).is_some());
    }
}
