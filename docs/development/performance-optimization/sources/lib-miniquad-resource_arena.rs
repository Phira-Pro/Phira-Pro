//! Reusable resource slots with non-wrapping identities. A stale or foreign
//! handle must never silently identify a different shader/pipeline.
use std::ops::{Index, IndexMut};
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(1);
pub(super) fn unique_id() -> usize {
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        value.checked_add(1)
    })
    .expect("graphics resource identity exhausted")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Handle {
    pub slot: usize,
    serial: usize,
}
struct Slot<T> {
    serial: usize,
    value: Option<T>,
}
pub(super) struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
    live: usize,
}
impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }
}
impl<T> Arena<T> {
    pub fn insert(&mut self, value: T) -> Handle {
        let serial = unique_id();
        let slot = if let Some(slot) = self.free.pop() {
            self.slots[slot] = Slot {
                serial,
                value: Some(value),
            };
            slot
        } else {
            let slot = self.slots.len();
            self.slots.push(Slot {
                serial,
                value: Some(value),
            });
            slot
        };
        self.live += 1;
        Handle { slot, serial }
    }
    pub fn get(&self, handle: Handle) -> Option<&T> {
        let slot = self.slots.get(handle.slot)?;
        (slot.serial == handle.serial)
            .then_some(slot.value.as_ref())
            .flatten()
    }
    fn get_mut(&mut self, handle: Handle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.slot)?;
        if slot.serial == handle.serial {
            slot.value.as_mut()
        } else {
            None
        }
    }
    pub fn remove(&mut self, handle: Handle) -> Option<T> {
        let slot = self.slots.get_mut(handle.slot)?;
        if slot.serial != handle.serial {
            return None;
        }
        let value = slot.value.take()?;
        self.live -= 1;
        self.free.push(handle.slot);
        Some(value)
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|slot| slot.value.as_ref())
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|slot| slot.value.as_mut())
    }
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn slots_len(&self) -> usize {
        self.slots.len()
    }
}
impl<T> Index<Handle> for Arena<T> {
    type Output = T;
    fn index(&self, handle: Handle) -> &T {
        self.get(handle)
            .expect("stale or foreign graphics resource")
    }
}
impl<T> IndexMut<Handle> for Arena<T> {
    fn index_mut(&mut self, handle: Handle) -> &mut T {
        self.get_mut(handle)
            .expect("stale or foreign graphics resource")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    struct Owner(Rc<Cell<usize>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    #[test]
    fn stale_handles_do_not_alias_reused_slots_or_another_context() {
        let mut a = Arena::default();
        let mut b = Arena::default();
        let old = a.insert(10);
        let foreign = b.insert(20);
        assert_eq!(old.slot, foreign.slot);
        assert!(a.get(foreign).is_none());
        assert!(a.remove(foreign).is_none());
        assert_eq!(a.remove(old), Some(10));
        assert!(a.remove(old).is_none());
        let new = a.insert(30);
        assert_eq!(new.slot, old.slot);
        assert_ne!(new, old);
        assert!(a.get(old).is_none());
        assert!(a.remove(old).is_none());
        assert_eq!(a[new], 30);
        assert_eq!(a.len(), 1);
        assert_eq!(a.slots_len(), 1);
    }
    #[test]
    fn thousands_of_reentries_keep_capacity_bounded_and_release_owners_once() {
        let dropped = Rc::new(Cell::new(0));
        let mut arena = Arena::default();
        let persistent = arena.insert(Owner(dropped.clone()));
        for round in 0..2000 {
            let handles = (0..7)
                .map(|_| arena.insert(Owner(dropped.clone())))
                .collect::<Vec<_>>();
            assert_eq!(arena.len(), 8);
            for handle in handles.into_iter().rev() {
                drop(arena.remove(handle).unwrap());
                assert!(arena.remove(handle).is_none());
            }
            assert_eq!(dropped.get(), (round + 1) * 7);
            assert_eq!(arena.len(), 1);
            assert_eq!(arena.slots_len(), 8);
        }
        drop(arena.remove(persistent));
        assert_eq!(dropped.get(), 14001);
        assert_eq!(arena.len(), 0);
    }
    #[test]
    fn shader_retirement_waits_for_all_live_pipeline_references() {
        let mut shaders = Arena::default();
        let shader = shaders.insert(99);
        let mut pipelines = Arena::default();
        let first = pipelines.insert(shader);
        let second = pipelines.insert(shader);
        assert!(pipelines.iter().any(|&s| s == shader));
        pipelines.remove(first);
        assert!(pipelines.iter().any(|&s| s == shader));
        pipelines.remove(second);
        assert!(!pipelines.iter().any(|&s| s == shader));
        assert_eq!(shaders.remove(shader), Some(99));
        let fresh = shaders.insert(100);
        assert_eq!(shader.slot, fresh.slot);
        assert_ne!(shader, fresh);
    }
}
