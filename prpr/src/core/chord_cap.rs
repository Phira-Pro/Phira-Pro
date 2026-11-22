//! 三压以上自动降为双压。
//!
//! 判定里一次按下只会命中一个音符（`TouchMatcher` 每条触点只结算一个目标），而且按住
//! Hold 期间那根手指一直被占用。所以只要**任意时刻**有 3 个以上需要按的音符处于活跃状态，
//! 就必须 3 根手指：这里把同一时刻第 3 个及之后的音符改成 Drag 或删除，把谱面降成双压。
//!
//! 活跃区间：Click / Flick 按一次算一个点（前后各留 [`SIMULTANEOUS_WINDOW`] 的容差，
//! 让 2ms 内的和音仍算同一组）；Hold 从头部一直占到尾部，整条都算键。Drag 与 `fake`
//! 不占手指，不计入。
//!
//! 转 Drag 时还会把降级音符并到同一条判定线上最近的活跃音符处：Drag 只有在手指附近才会
//! 自动命中，不并位置的话第三根手指照样躲不掉。不同判定线的 x 不是同一个坐标系，不跨线对齐。
//!
//! 只在加载期跑一次，判定、渲染、练习和成绩都看到同一份转换后的谱面；音符数变化后必须
//! 重建 [`JudgeLineCache`]，否则索引缓存会越界。

use super::{JudgeLine, JudgeLineCache, Note, NoteKind};
use crate::config::ChordCap;
use crate::judge::HitSound;

/// 视为「同一时刻」的时间窗口（秒）。
///
/// 正好 2ms：正常的和音是同一时刻的若干个音符，相邻的十六分音符在 300 BPM 下也有 50ms，
/// 不会被误合并。长条尾部同样多留这么一点，避免尾判瞬间的下一颗音符被漏算。
pub const SIMULTANEOUS_WINDOW: f64 = 0.002;

/// 一次转换的结果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// 被转成 Drag / 删除的音符数。
    pub changed: usize,
    /// 其中因为长条还在持续占用手指而被迫降级的个数。
    pub blocked_by_holds: usize,
}

impl Report {
    pub fn is_empty(self) -> bool {
        self.changed == 0
    }
}

/// 需要按下去的音符：Click / Flick / Hold 头。Drag 不用按，fake 不判定。
fn needs_press(note: &Note) -> bool {
    !note.fake && matches!(note.kind, NoteKind::Click | NoteKind::Flick | NoteKind::Hold { .. })
}

/// 音符在某时刻的横向位置。判定用的是 `translation.0`，所以这里也读同一个值。
fn note_x(note: &Note, time: f64) -> f32 {
    let mut x = note.object.translation.0.clone();
    x.set_time(time);
    x.now()
}

fn set_note_x(note: &mut Note, x: f32) {
    note.object.translation.0 = super::AnimFloat::fixed(x);
}

/// 保留优先级，数字越小越应该留在双压里。
///
/// Hold 和 Flick 转成 Drag 会丢掉尾判 / 划键语义，所以优先把蓝键降级。
fn keep_rank(kind: &NoteKind) -> u8 {
    match kind {
        NoteKind::Hold { .. } => 0,
        NoteKind::Flick => 1,
        NoteKind::Click => 2,
        NoteKind::Drag => 3,
    }
}

/// 一个音符占住一根手指的区间。
struct Span {
    start: f64,
    /// 最后一个需要在线的时刻；Click / Flick 与 `start` 相同。
    end: f64,
    line: usize,
    index: usize,
    rank: u8,
}

/// 已经被接受的、仍在占用手指的音符。
struct Active {
    end: f64,
    line: usize,
    index: usize,
}

/// 把 [`lines`] 里三压以上的组合降到双压，返回改动统计。
pub fn apply(lines: &mut [JudgeLine], mode: ChordCap) -> Report {
    if mode == ChordCap::Off || lines.is_empty() {
        return Report::default();
    }

    let mut spans: Vec<Span> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        for (ni, note) in line.notes.iter().enumerate() {
            if !needs_press(note) {
                continue;
            }
            let end = match note.kind {
                NoteKind::Hold { end_time, .. } => end_time.max(note.time),
                _ => note.time,
            };
            spans.push(Span {
                start: note.time,
                end,
                line: li,
                index: ni,
                rank: keep_rank(&note.kind),
            });
        }
    }
    if spans.len() < 3 {
        return Report::default();
    }
    // 2ms 内的音符算同一时刻：把起始时刻归一到组内第一个，这样紧接着和弦起手的长条
    // 会和和弦一起排序，靠优先级先占位，而不是被晚 1ms 的和弦挤掉。
    spans.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut base = f64::NEG_INFINITY;
    for span in &mut spans {
        if span.start - base > SIMULTANEOUS_WINDOW {
            base = span.start;
        } else {
            span.start = base;
        }
    }
    // 同一时刻先处理 Hold / Flick：它们比蓝键更值得保留，后到的蓝键会直接降级。
    spans.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(a.rank.cmp(&b.rank))
            .then((a.line, a.index).cmp(&(b.line, b.index)))
    });

    let mut active: Vec<Active> = Vec::new();
    // (被降级的音符, 要对齐到的活跃音符)：只在 ToDrag 模式下收集。
    let mut demoted: Vec<(usize, usize)> = Vec::new();
    let mut snaps: Vec<((usize, usize), (usize, usize))> = Vec::new();
    let mut blocked_by_holds = 0;
    for span in &spans {
        let mut holds_busy = false;
        active.retain(|it| {
            if it.end + SIMULTANEOUS_WINDOW >= span.start {
                holds_busy |= matches!(lines[it.line].notes[it.index].kind, NoteKind::Hold { .. });
                true
            } else {
                false
            }
        });
        if active.len() >= 2 {
            demoted.push((span.line, span.index));
            if holds_busy {
                blocked_by_holds += 1;
            }
            if mode == ChordCap::ToDrag {
                // 并到同一条判定线上最近的活跃音符，双指即可命中。
                let time = lines[span.line].notes[span.index].time;
                let x = note_x(&lines[span.line].notes[span.index], time);
                let mut best: Option<((usize, usize), f32)> = None;
                for it in &active {
                    if it.line != span.line {
                        continue;
                    }
                    let d = (note_x(&lines[it.line].notes[it.index], time) - x).abs();
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some(((it.line, it.index), d));
                    }
                }
                if let Some((target, _)) = best {
                    snaps.push(((span.line, span.index), target));
                }
            }
        } else {
            active.push(Active {
                end: span.end,
                line: span.line,
                index: span.index,
            });
        }
    }
    if demoted.is_empty() {
        return Report::default();
    }

    let changed = demoted.len();
    let mut touched = vec![false; lines.len()];
    for &(li, _) in &demoted {
        touched[li] = true;
    }
    match mode {
        ChordCap::ToDrag => {
            for &(li, ni) in &demoted {
                let note = &mut lines[li].notes[ni];
                note.kind = NoteKind::Drag;
                note.hold_body_speed = None;
                note.hitsound = HitSound::default_from_kind(&note.kind);
            }
            for &((li, ni), (tl, tn)) in &snaps {
                let time = lines[li].notes[ni].time;
                let target = note_x(&lines[tl].notes[tn], time);
                set_note_x(&mut lines[li].notes[ni], target);
            }
        }
        ChordCap::Delete => {
            // 从后往前删，前面的下标就不会被前面的删除操作搞乱。
            demoted.sort_unstable_by(|a, b| b.cmp(a));
            for &(li, ni) in &demoted {
                if ni < lines[li].notes.len() {
                    lines[li].notes.remove(ni);
                }
            }
        }
        ChordCap::Off => unreachable!("handled above"),
    }
    // kind、位置和音符数量都可能变了：缓存里的 update_order / above_indices 必须按新列表重建。
    for li in (0..lines.len()).filter(|&li| touched[li]) {
        let line = &mut lines[li];
        line.cache = JudgeLineCache::new(&mut line.notes);
    }
    Report { changed, blocked_by_holds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AnimFloat, CtrlObject, JudgeLineKind, Object};
    use std::cell::RefCell;

    fn note(time: f64, kind: NoteKind) -> Note {
        Note {
            object: Object::default(),
            hitsound: HitSound::default_from_kind(&kind),
            kind,
            time,
            height: 0.,
            speed: 1.,
            color: Default::default(),
            fx_color: None,
            judge_area: 1.,
            hold_body_speed: None,
            above: true,
            multiple_hint: false,
            fake: false,
            judge: crate::judge::JudgeStatus::NotJudged,
        }
    }

    fn hold(start: f64, end: f64) -> Note {
        note(start, NoteKind::Hold { end_time: end, end_height: 0. })
    }

    fn line(notes: Vec<Note>) -> JudgeLine {
        let mut notes = notes;
        let cache = JudgeLineCache::new(&mut notes);
        JudgeLine {
            object: Object::default(),
            ctrl_obj: RefCell::new(CtrlObject::default()),
            kind: JudgeLineKind::Normal,
            height: AnimFloat::default(),
            incline: AnimFloat::default(),
            notes,
            color: Default::default(),
            parent: None,
            rot_with_parent: true,
            z_index: 0,
            show_below: false,
            attach_ui: None,
            cache,
        }
    }

    fn kinds(lines: &[JudgeLine]) -> Vec<Vec<NoteKind>> {
        lines.iter().map(|it| it.notes.iter().map(|n| n.kind.clone()).collect()).collect()
    }

    #[test]
    fn triple_becomes_two_clicks_and_one_drag() {
        let mut lines = vec![line(vec![]), line(vec![note(1., NoteKind::Click), note(1., NoteKind::Click), note(1., NoteKind::Click)])];
        let report = apply(&mut lines, ChordCap::ToDrag);
        assert_eq!(report, Report { changed: 1, blocked_by_holds: 0 });
        assert_eq!(lines[1].notes.len(), 3, "drag conversion keeps the note count");
        let drags = lines[1].notes.iter().filter(|it| matches!(it.kind, NoteKind::Drag)).count();
        assert_eq!(drags, 1);
    }

    #[test]
    fn delete_mode_removes_the_extra_notes_and_rebuilds_the_cache() {
        let mut lines = vec![line(vec![
            note(1., NoteKind::Click),
            note(1., NoteKind::Click),
            note(1., NoteKind::Click),
            note(1., NoteKind::Click),
            note(2., NoteKind::Click),
        ])];
        let report = apply(&mut lines, ChordCap::Delete);
        assert_eq!(report, Report { changed: 2, blocked_by_holds: 0 });
        assert_eq!(lines[0].notes.len(), 3);
        assert_eq!(lines[0].cache.tracked_len(), 3, "the cache must be rebuilt after deletion");
    }

    #[test]
    fn double_presses_and_spaced_notes_are_untouched() {
        let mut lines = vec![line(vec![note(1., NoteKind::Click), note(1., NoteKind::Click), note(1.05, NoteKind::Click)])];
        assert_eq!(apply(&mut lines, ChordCap::Delete), Report::default());
        assert_eq!(lines[0].notes.len(), 3);
    }

    #[test]
    fn notes_inside_the_window_count_as_the_same_chord() {
        let mut lines = vec![line(vec![
            note(1., NoteKind::Click),
            note(1.0005, NoteKind::Click),
            note(1.0018, NoteKind::Click),
        ])];
        assert_eq!(apply(&mut lines, ChordCap::Delete).changed, 1);
        assert_eq!(lines[0].notes.len(), 2);
    }

    #[test]
    fn drags_and_fakes_do_not_take_a_finger() {
        let mut lines = vec![line(vec![
            note(1., NoteKind::Click),
            note(1., NoteKind::Drag),
            note(1., NoteKind::Drag),
            note(1., NoteKind::Click),
        ])];
        assert_eq!(apply(&mut lines, ChordCap::Delete), Report::default());
        assert_eq!(lines[0].notes.len(), 4);
        let mut fake = note(2., NoteKind::Click);
        fake.fake = true;
        let mut lines = vec![line(vec![note(2., NoteKind::Click), note(2., NoteKind::Click), fake])];
        assert_eq!(apply(&mut lines, ChordCap::Delete), Report::default());
    }

    #[test]
    fn a_hold_occupies_a_finger_for_its_whole_body() {
        // 长条 0..4 一直占着一根手指，2.0 处的双押只能再留一个。
        let mut lines = vec![line(vec![hold(0., 4.), note(2., NoteKind::Click), note(2., NoteKind::Click)])];
        let report = apply(&mut lines, ChordCap::Delete);
        assert_eq!(report, Report { changed: 1, blocked_by_holds: 1 });
        assert_eq!(lines[0].notes.len(), 2);
    }

    #[test]
    fn a_hold_stops_occupying_a_finger_after_its_tail() {
        // 长条 0..1 结束后手指就空了，3.0 处的双押是正常双压。
        let mut lines = vec![line(vec![hold(0., 1.), note(3., NoteKind::Click), note(3., NoteKind::Click)])];
        assert_eq!(apply(&mut lines, ChordCap::Delete), Report::default());
        assert_eq!(lines[0].notes.len(), 3);
    }

    #[test]
    fn a_hold_tail_and_the_next_note_count_as_simultaneous() {
        // 尾判瞬间手指还在线上，此刻再落两个音符就是三压。
        let mut lines = vec![line(vec![hold(0., 2.), note(2., NoteKind::Click), note(2., NoteKind::Click)])];
        assert_eq!(apply(&mut lines, ChordCap::Delete).changed, 1);
    }

    #[test]
    fn two_long_holds_leave_no_room_for_a_chord() {
        let mut lines = vec![line(vec![
            hold(0., 4.),
            hold(0., 4.),
            note(1., NoteKind::Click),
            note(1., NoteKind::Click),
        ])];
        let report = apply(&mut lines, ChordCap::Delete);
        assert_eq!(report, Report { changed: 2, blocked_by_holds: 2 });
        assert_eq!(lines[0].notes.len(), 2);
    }

    #[test]
    fn a_demoted_hold_frees_its_finger_immediately() {
        // 三个长条只留两个：被降级的那个不再占手指，但仍在占用的是两条。
        let mut lines = vec![line(vec![hold(0., 4.), hold(0., 4.), hold(0., 4.)])];
        let report = apply(&mut lines, ChordCap::ToDrag);
        assert_eq!(report, Report { changed: 1, blocked_by_holds: 1 });
        let holds = lines[0].notes.iter().filter(|n| matches!(n.kind, NoteKind::Hold { .. })).count();
        assert_eq!(holds, 2);
    }

    #[test]
    fn holds_and_flicks_survive_before_clicks() {
        // `JudgeLineCache` 会把不 plain 的 Hold 排到前面，所以这里按种类计数，不按下标。
        let mut lines = vec![line(vec![
            note(1., NoteKind::Click),
            note(1., NoteKind::Flick),
            hold(1., 2.),
        ])];
        apply(&mut lines, ChordCap::ToDrag);
        let count = |f: fn(&NoteKind) -> bool| lines[0].notes.iter().filter(|n| f(&n.kind)).count();
        assert_eq!(count(|k| matches!(k, NoteKind::Hold { .. })), 1, "the hold keeps its body");
        assert_eq!(count(|k| matches!(k, NoteKind::Flick)), 1, "the flick is not touched");
        assert_eq!(count(|k| matches!(k, NoteKind::Click)), 0, "the click is the one demoted");
        assert_eq!(count(|k| matches!(k, NoteKind::Drag)), 1);
    }

    #[test]
    fn chords_across_lines_share_one_budget() {
        let mut lines = vec![
            line(vec![note(1., NoteKind::Click), note(1., NoteKind::Click)]),
            line(vec![note(1., NoteKind::Click)]),
        ];
        let report = apply(&mut lines, ChordCap::Delete);
        assert_eq!(report, Report { changed: 1, blocked_by_holds: 0 });
        assert_eq!(lines[0].notes.len() + lines[1].notes.len(), 2);
    }

    #[test]
    fn to_drag_merges_the_demoted_note_onto_the_nearest_kept_one_on_the_same_line() {
        let mut a = note(1., NoteKind::Click);
        a.object.translation.0 = AnimFloat::fixed(-0.4);
        let mut b = note(1., NoteKind::Click);
        b.object.translation.0 = AnimFloat::fixed(0.1);
        let mut c = note(1., NoteKind::Click);
        c.object.translation.0 = AnimFloat::fixed(0.5);
        let mut lines = vec![line(vec![a, b, c])];
        assert_eq!(apply(&mut lines, ChordCap::ToDrag).changed, 1);
        let note = &lines[0].notes[2];
        assert!(matches!(note.kind, NoteKind::Drag));
        // 最近的保留音符是 x = 0.1，降级音符被并到它上面，双指即可命中。
        assert!((note_x(note, 1.) - 0.1).abs() < 1e-6, "got {}", note_x(note, 1.));
    }

    #[test]
    fn to_drag_merges_onto_a_hold_that_is_still_being_held() {
        // 长条从 0 一直按到 4，两个点击落在 2.0：保留更近的那个，另一个并到长条上，
        // 因为按住长条的手指就在那里，这样双指即可命中。
        let mut h = hold(0., 4.);
        h.object.translation.0 = AnimFloat::fixed(0.2);
        let mut near = note(2., NoteKind::Click);
        near.object.translation.0 = AnimFloat::fixed(0.0);
        let mut far = note(2., NoteKind::Click);
        far.object.translation.0 = AnimFloat::fixed(0.15);
        let mut lines = vec![line(vec![h, near, far])];
        assert_eq!(apply(&mut lines, ChordCap::ToDrag).changed, 1);
        let merged = lines[0].notes.iter().find(|n| matches!(n.kind, NoteKind::Drag)).expect("one drag");
        assert!((note_x(merged, 2.) - 0.2).abs() < 1e-6, "merged onto the held note, got {}", note_x(merged, 2.));
    }

    #[test]
    fn to_drag_keeps_the_position_when_the_kept_notes_are_on_another_line() {
        let mut c = note(1., NoteKind::Click);
        c.object.translation.0 = AnimFloat::fixed(0.5);
        let mut lines = vec![
            line(vec![note(1., NoteKind::Click), note(1., NoteKind::Click)]),
            line(vec![c]),
        ];
        apply(&mut lines, ChordCap::ToDrag);
        assert!((note_x(&lines[1].notes[0], 1.) - 0.5).abs() < 1e-6, "cross-line x is not comparable");
    }

    #[test]
    fn a_hold_starting_right_after_a_chord_still_wins() {
        // 和弦在 1.000、长条在 1.0005：同一组内优先级更高的长条先占位，蓝键让路。
        let mut lines = vec![line(vec![
            note(1., NoteKind::Click),
            note(1., NoteKind::Click),
            hold(1.0005, 3.),
        ])];
        let report = apply(&mut lines, ChordCap::ToDrag);
        // 长条先占位，降级发生时它已经在占用手指，所以计入 blocked_by_holds。
        assert_eq!(report, Report { changed: 1, blocked_by_holds: 1 });
        let holds = lines[0].notes.iter().filter(|n| matches!(n.kind, NoteKind::Hold { .. })).count();
        assert_eq!(holds, 1, "the hold keeps its body");
    }

    #[test]
    fn off_mode_is_a_no_op() {
        let mut lines = vec![line(vec![note(1., NoteKind::Click), note(1., NoteKind::Click), note(1., NoteKind::Click)])];
        assert_eq!(apply(&mut lines, ChordCap::Off), Report::default());
        assert_eq!(kinds(&lines)[0].len(), 3);
    }
}
