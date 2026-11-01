use super::*;
use crate::replay::{self, Replay, TouchEvent, PHASE_DOWN, PHASE_MOVE, PHASE_UP};

fn tape(viewport: [i32; 4], screen_height: f32, source: &str) -> Replay {
    let mut capture = Capture {
        schema: 1,
        ..Default::default()
    };
    let [vx, vy, vw, vh] = viewport;
    let top = screen_height - (vy + vh) as f32;
    let mut touches = Vec::new();
    for (frame, (phase, fx, fy)) in [("down", 0.2, 0.8), ("move", 0.5, 0.5), ("up", 0.8, 0.2)].into_iter().enumerate() {
        let time = 1. + frame as f64 * 0.1;
        capture.anchors.push(Anchor {
            frame: frame as u64,
            uptime: time,
            chart_time: time,
            active: true,
            speed: 1.,
            viewport,
            screen_height,
        });
        capture.inputs.push(Input {
            seq: frame as u64,
            frame: frame as u64,
            active: true,
            event: prpr::input_capture::Input {
                source: source.into(),
                phase: phase.into(),
                id: 7,
                event_time: (source == "touch").then_some(time),
                observed_time: time,
                x: Some(vx as f32 + fx * vw as f32),
                y: Some(top + fy * vh as f32),
            },
        });
        touches.push(TouchEvent {
            phase: [PHASE_DOWN, PHASE_MOVE, PHASE_UP][frame],
            id: 0,
            x: 2. * fx - 1.,
            y: (2. * fy - 1.) * vh as f32 / vw as f32,
            t: time as f32,
        });
    }
    Replay {
        capture: Some(capture),
        touches,
        speed: 1.,
        ..Default::default()
    }
}

fn assert_positions(actual: &[TouchEvent], expected: &[TouchEvent]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert_eq!((a.phase, a.id, a.t), (e.phase, e.id, e.t));
        assert!((a.x - e.x).abs() < 1e-6, "x: {} != {}", a.x, e.x);
        assert!((a.y - e.y).abs() < 1e-6, "y: {} != {}", a.y, e.y);
    }
}

#[test]
fn playback_coordinates_match_downward_ui_axis_and_offset_viewports() {
    for (viewport, height) in [
        ([0, 0, 1920, 1080], 1080.),
        ([0, 0, 2048, 1536], 1536.),
        ([240, 0, 1920, 1080], 1080.),
        ([0, 240, 1920, 1080], 1560.),
        ([31, 107, 1279, 629], 960.),
    ] {
        for source in ["mouse", "touch"] {
            let replay = tape(viewport, height, source);
            let actual = replay.capture.as_ref().unwrap().playback_touches();
            assert_positions(&actual, &replay.touches);
            assert!(actual[0].y > 0., "lower-screen contact must stay below the center");
            assert!(actual[2].y < 0., "upper-screen contact must stay above the center");
        }
    }
}

#[test]
fn v6_load_rebuilds_existing_flipped_display_tape_from_unchanged_raw_input() {
    let mut original = tape([31, 107, 1279, 629], 960., "mouse");
    let expected = original.touches.clone();
    let raw = serde_json::to_value(&original.capture).unwrap();
    for touch in &mut original.touches {
        touch.y = -touch.y;
    }
    let loaded = replay::from_bytes(&replay::bytes(&original).unwrap()).unwrap();
    assert_eq!(loaded.source_version, 6);
    assert_positions(&loaded.touches, &expected);
    assert_eq!(serde_json::to_value(&loaded.capture).unwrap(), raw);
    let reloaded = replay::from_bytes(&replay::bytes(&loaded).unwrap()).unwrap();
    assert_positions(&reloaded.touches, &expected);
}

#[test]
fn dropped_capture_and_v5_keep_recorded_ui_coordinates() {
    let mut original = tape([31, 107, 1279, 629], 960., "touch");
    original.capture.as_mut().unwrap().dropped = 1;
    let loaded = replay::from_bytes(&replay::bytes(&original).unwrap()).unwrap();
    assert_eq!(loaded.source_version, 6);
    assert_positions(&loaded.touches, &original.touches);
    original.capture = None;
    let loaded = replay::from_bytes(&replay::bytes(&original).unwrap()).unwrap();
    assert_eq!(loaded.source_version, 5);
    assert_positions(&loaded.touches, &original.touches);
}
