//! Fold ordered events into the native per-frame touch snapshot. A released
//! same-frame click is eligible once, but must never become a cached held touch.
use super::*;

pub(super) fn merge_event(touches: &mut HashMap<u64, Touch>, infected: &mut HashSet<u64>, mut event: Touch) {
    let id = event.id;
    match event.phase {
        TouchPhase::Started => {
            // Android can reuse an ID immediately, even within this frame.
            // The previous contact's noise infection must not cross lifetimes.
            infected.remove(&id);
            touches.insert(id, event);
        }
        TouchPhase::Moved | TouchPhase::Stationary => {
            if touches.get(&id).is_some_and(|t| t.phase == TouchPhase::Started) {
                event.phase = TouchPhase::Started;
            }
            touches.insert(id, event);
        }
        TouchPhase::Ended => {
            infected.remove(&id);
            if !touches.get(&id).is_some_and(|t| t.phase == TouchPhase::Started) {
                touches.remove(&id);
            }
        }
        TouchPhase::Cancelled => {
            infected.remove(&id);
            touches.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: u64, phase: TouchPhase) -> Touch {
        Touch {
            id,
            phase,
            position: vec2(0., 0.),
            time: 2.,
        }
    }
    #[test]
    fn quick_release_remains_a_click_but_cancel_does_not() {
        for ending in [TouchPhase::Ended, TouchPhase::Cancelled] {
            let mut frame = HashMap::new();
            let mut infected = HashSet::new();
            for phase in [TouchPhase::Started, TouchPhase::Moved, ending] {
                merge_event(&mut frame, &mut infected, event(7, phase));
            }
            assert_eq!(frame.get(&7).map(|t| t.phase), if ending == TouchPhase::Ended { Some(TouchPhase::Started) } else { None });
        }
    }
    #[test]
    fn reused_id_does_not_inherit_infection_and_other_fingers_are_unchanged() {
        let mut frame = HashMap::from([(7, event(7, TouchPhase::Started)), (8, event(8, TouchPhase::Stationary))]);
        let mut infected = HashSet::from([7, 8]);
        merge_event(&mut frame, &mut infected, event(7, TouchPhase::Started));
        assert!(!infected.contains(&7));
        assert!(infected.contains(&8));
        assert_eq!(frame[&8].phase, TouchPhase::Stationary);
        merge_event(&mut frame, &mut infected, event(7, TouchPhase::Ended));
        assert_eq!(frame[&7].phase, TouchPhase::Started);
        merge_event(&mut frame, &mut infected, event(7, TouchPhase::Cancelled));
        assert!(!frame.contains_key(&7));
        assert!(frame.contains_key(&8));
    }
    #[test]
    fn normal_release_removes_a_held_contact() {
        let mut frame = HashMap::from([(7, event(7, TouchPhase::Stationary))]);
        let mut infected = HashSet::from([7]);
        merge_event(&mut frame, &mut infected, event(7, TouchPhase::Ended));
        assert!(frame.is_empty());
        assert!(infected.is_empty());
    }
}
