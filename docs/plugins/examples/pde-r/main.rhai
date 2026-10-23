// Phira Pro Plugin API v2.
// Port of phigros_difficulty_estimator_re: parse the raw chart, rebuild the
// 106-feature vector of rating/features.py, then score it with the exported
// model of rating/model.py. Everything here mirrors the Python reference; the
// numbers must stay in sync with tools/check_features.py.

// ---------------------------------------------------------------------------
// small numeric helpers
// ---------------------------------------------------------------------------

fn num_or(list, key, fallback) {
    let v = list[key];
    if v == () { return fallback; }
    if type_of(v) == "i64" { return v * 1.0; }
    if type_of(v) == "f64" { return v; }
    fallback
}

fn get_num(map, key, fallback) {
    if !map.contains(key) { return fallback; }
    let v = map[key];
    if v == () { return fallback; }
    if type_of(v) == "i64" { return v * 1.0; }
    if type_of(v) == "f64" { return v; }
    fallback
}

fn clamp(v, low, high) {
    if v < low { low } else if v > high { high } else { v }
}

fn safe_div(a, b) {
    if b <= 1e-9 { 0.0 } else { a / b }
}

// Fixed-point text. Rhai's to_string() on a float prints the shortest form
// ("18.4", not "18.40"), and the overlay reads better with a stable width.
fn zeros(count) {
    let out = "";
    for _i in 0..count { out += "0"; }
    out
}

fn fmt_fixed(value, decimals) {
    let scale = 1.0;
    for _i in 0..decimals { scale *= 10.0; }
    let text = ((value * scale).round() / scale).to_string();
    if decimals == 0 { return text; }
    let dot = text.index_of(".");
    if dot < 0 { return text + "." + zeros(decimals); }
    let have = text.len() - dot - 1;
    if have >= decimals { return text; }
    text + zeros(decimals - have)
}

fn mean(list) {
    if list.len() == 0 { return 0.0; }
    let total = 0.0;
    for v in list { total += v; }
    total / list.len()
}

// numpy.quantile default (linear interpolation between order statistics).
//
// Rhai's sort runs in Rust and only calls back into the script per comparison,
// and the host counts a callback call as a handful of operations, so sorting a
// copy is far cheaper than a scripted selection loop (measured: 3000-element
// shuffle = 27k operations, a single quickselect pass = 100k).
fn quantile(values, q) {
    let n = values.len();
    if n == 0 { return 0.0; }
    let sorted = values;
    // Built-in sort: no comparator callback, so it is both far cheaper (Rhai
    // sorts natively, ~30 operations for 2,400 values) and immune to
    // "comparer does not implement a total order" panics that Rhai's
    // sort-by-closure raises on some large float arrays.
    sorted.sort();
    quantile_sorted(sorted, q)
}

// Kept for call sites that already know their array is ordered.
fn quantile_sorted(sorted, q) {
    let n = sorted.len();
    if n == 0 { return 0.0; }
    if n == 1 { return sorted[0]; }
    let pos = q * (n - 1);
    let lo = pos.floor();
    let hi = if lo + 1.0 > n - 1.0 { n - 1.0 } else { lo + 1.0 };
    let frac = pos - lo;
    sorted[lo.to_int()] + (sorted[hi.to_int()] - sorted[lo.to_int()]) * frac
}

fn median_of(values) { quantile(values, 0.5) }

// ---------------------------------------------------------------------------
// chart parsing (pgr2rpe -> rating/charts.py ChartData)
// ---------------------------------------------------------------------------

// pgr2rpe/spec.py: tick * 60 / (32 * bpm) - the exact expression matters,
// a mathematically equivalent regrouping rounds differently and moves notes
// across window boundaries.
fn tick_to_seconds(tick, bpm) { tick * 60.0 / (32.0 * bpm) }

fn build_bpm_axis(entries) {
    let beats = [];
    let bpms = [];
    let index = 0;
    for entry in entries {
        if type_of(entry) != "map" { index += 1; continue; }
        let bpm = get_num(entry, "bpm", 0.0);
        if bpm <= 0.0 { index += 1; continue; }
        let start = entry["startTime"];
        let beat = 0.0;
        if type_of(start) == "array" && start.len() == 3 {
            beat = (start[0] + start[1] * 1.0 / start[2]) * 1.0;
        } else if type_of(start) == "i64" {
            beat = start * 1.0;
        }
        beats.push(beat);
        bpms.push(bpm);
        index += 1;
    }
    if beats.len() == 0 {
        beats.push(0.0);
        bpms.push(120.0);
    }
    // sort by beat, keeping bpm paired
    let order = [];
    for i in 0..beats.len() { order.push(i); }
    order.sort(|a, b| if beats[a] < beats[b] { -1 } else if beats[a] > beats[b] { 1 } else { 0 });
    let sorted_beats = [];
    let sorted_bpms = [];
    for i in order {
        sorted_beats.push(beats[i]);
        sorted_bpms.push(bpms[i]);
    }
    #{"beats": sorted_beats, "bpms": sorted_bpms}
}

fn beat_to_time(axis, beat) {
    let beats = axis["beats"];
    let bpms = axis["bpms"];
    if beat <= beats[0] { return (beat - beats[0]) * 60.0 / bpms[0]; }
    let total = 0.0;
    for index in 0..beats.len() {
        if beat <= beats[index] { break; }
        let end = if index + 1 < beats.len() { beats[index + 1] } else { beat };
        let seg_end = if beat < end { beat } else { end };
        if seg_end > beats[index] {
            total += (seg_end - beats[index]) * 60.0 / bpms[index];
        }
        if beat <= end { break; }
    }
    total
}

// RPE lines store time as beats; official lines store ticks.
fn is_rpe(doc) { doc.contains("BPMList") || doc.contains("META") }

// RPE stores a beat as [beat, numerator, denominator].
fn triple_to_beat(value) {
    if type_of(value) == "array" && value.len() == 3 {
        return (value[0] + value[1] * 1.0 / value[2]) * 1.0;
    }
    0.0
}

// RPE numbers its note types differently from the official format
// (pgr2rpe/rpe_in.py: RPE_TYPE_TO_OFFICIAL = {1: 1, 2: 3, 3: 4, 4: 2}).
// Without this map a Hold is read as a Drag, which zeroes every hold feature
// and swaps frac_drag / frac_flick.
fn rpe_type_to_official(value) {
    if value == 1 { 1 } else if value == 2 { 3 } else if value == 3 { 4 } else if value == 4 { 2 } else { value }
}

fn note_from_rpe(raw_note, factor, axis) {
    let beat = triple_to_beat(raw_note["startTime"]);
    let end_beat = beat;
    if raw_note.contains("endTime") {
        end_beat = triple_to_beat(raw_note["endTime"]);
    }
    let type_id = rpe_type_to_official(get_num(raw_note, "type", 1).to_int());
    let above = get_num(raw_note, "above", 1) != 0.0;
    let fake = get_num(raw_note, "isFake", 0) != 0.0;
    // rpe_in.py: x is in RPE pixels, normalised by X_PX_PER_NORM = 675.
    let x_units = get_num(raw_note, "positionX", 0.0) / 675.0;

    let t = beat_to_time(axis, beat) * factor;
    let e = beat_to_time(axis, end_beat) * factor;
    // A Hold without positive duration is a Tap (same rule as the official reader).
    if type_id == 3 && e <= t {
        type_id = 1;
        e = t;
    }
    // The scroll speed comes from the line's speedEvents, not the note.
    [t, e, x_units, type_id, above, fake, 1.0]
}

// RPE lines animate their scroll speed with piecewise-linear events, one layer
// per eventLayers entry (layers stack additively). rating/charts.py takes every
// note's speed from line.speed.at(t) * OFFICIAL_Y_TO_NORM, so the whole model
// hinges on this curve: using the note's own `speed` field instead shifts every
// speed / throughput feature.
//
// Segments are stored flat as [t0, t1, v0, v1], ascending by (t0, t1).
fn build_speed_tracks(line, axis, factor) {
    let tracks = [];
    if !line.contains("eventLayers") { return tracks; }
    let layers = line["eventLayers"];
    if type_of(layers) != "array" { return tracks; }
    for layer in layers {
        if type_of(layer) != "map" { continue; }
        if !layer.contains("speedEvents") { continue; }
        let events = layer["speedEvents"];
        if type_of(events) != "array" { continue; }
        let segs = [];
        for event in events {
            if type_of(event) != "map" { continue; }
            if !event.contains("startTime") || !event.contains("endTime") { continue; }
            let t0 = beat_to_time(axis, triple_to_beat(event["startTime"])) * factor;
            let t1 = beat_to_time(axis, triple_to_beat(event["endTime"])) * factor;
            if t1 < t0 { continue; }
            // pgr2rpe/spec.py: OFFICIAL_SPEED_TO_RPE = 4.5
            segs.push([
                t0, t1,
                get_num(event, "start", 0.0) / 4.5,
                get_num(event, "end", 0.0) / 4.5
            ]);
        }
        if segs.len() > 0 {
            segs.sort(|a, b| if a[0] < b[0] { -1 } else if a[0] > b[0] { 1 } else if a[1] < b[1] { -1 } else if a[1] > b[1] { 1 } else { 0 });
            tracks.push(segs);
        }
    }
    tracks
}

// pgr2rpe/model.py Track.at: clamped linear interpolation, flat outside the domain.
fn track_at(segs, t) {
    let count = segs.len();
    if count == 0 { return 0.0; }
    if t <= segs[0][0] { return segs[0][2]; }
    if t >= segs[count - 1][1] { return segs[count - 1][3]; }
    let lo = 0;
    let hi = count - 1;
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if segs[mid][0] <= t { lo = mid; } else { hi = mid - 1; }
    }
    let seg = segs[lo];
    let span = seg[1] - seg[0];
    if span <= 0.0 { return seg[3]; }
    let u = clamp((t - seg[0]) / span, 0.0, 1.0);
    seg[2] + (seg[3] - seg[2]) * u
}

fn line_speed_at(tracks, t) {
    // A line without speed events plays at 1.0, i.e. 1.2 chart heights/second.
    if tracks.len() == 0 { return 1.2; }
    let total = 0.0;
    for segs in tracks {
        total += track_at(segs, t);
    }
    total * 1.2
}

// pgr2rpe's official reader hard-codes ``fake=False`` for the official format
// (official_in.py), so only RPE charts ever contribute fake notes. Mirroring
// that keeps frac_fake identical to the reference; "fixing" it here would make
// the plugin disagree with the trained model's inputs.
fn note_from_official(raw_note, bpm, above) {
    let type_id = get_num(raw_note, "type", 1).to_int();
    let time_ticks = get_num(raw_note, "time", 0.0);
    // pgr2rpe/spec.py: OFFICIAL_X_UNIT = 0.05625 and the model works in
    // RPE-normalised x (-1..1), i.e. 2 * OFFICIAL_X_UNIT = 0.1125 per unit.
    let x_units = get_num(raw_note, "positionX", 0.0) * 0.1125;
    // tick_to_seconds inlined: this runs once per note and the call overhead is
    // a real share of the operation budget on long charts.
    let t = time_ticks * 60.0 / (32.0 * bpm);
    let e = t;
    let real_type = type_id;
    if type_id == 3 {
        // Only a Hold has an end, and a zero-length Hold is a Tap.
        let hold_ticks = get_num(raw_note, "holdTime", 0.0);
        e = (time_ticks + hold_ticks) * 60.0 / (32.0 * bpm);
        if hold_ticks <= 0.0 { real_type = 1; }
    }
    // The note's own `speed` field is the hold-head value; the scroll speed is
    // resolved from the line's speedEvents by the caller.
    [t, e, x_units, real_type, above, false, 1.0]
}

// Judge-line scroll speed at a given time, in chart heights per second.
//
// rating/charts.py takes each note's speed from ``line.speed.at(note.t)`` (the
// line's own animation) times OFFICIAL_Y_TO_NORM = 1.2, not from the note's own
// ``speed`` field. Events are step functions: a value holds until the next
// event starts. ``bpm`` converts official ticks; RPE uses the beat axis instead,
// which callers resolve before calling this.
fn official_speed_at(events, time, bpm) {
    let chosen = 1.0;
    for event in events {
        if type_of(event) != "map" { continue; }
        if !event.contains("startTime") { continue; }
        let start = tick_to_seconds(get_num(event, "startTime", 0.0), bpm);
        if start > time { break; }
        // official_in.py picks speedEvents by their `value` field.
        chosen = get_num(event, "value", chosen);
    }
    chosen
}

fn parse_chart(doc) {
    let rpe = is_rpe(doc);
    let axis = if rpe {
        build_bpm_axis(if doc.contains("BPMList") { doc["BPMList"] } else { [] })
    } else {
        #{"beats": [0.0], "bpms": [120.0]}
    };

    let lines = if rpe {
        if doc.contains("judgeLineList") { doc["judgeLineList"] } else { [] }
    } else {
        if doc.contains("judgeLineList") { doc["judgeLineList"] } else { [] }
    };

    let out = [];
    for line in lines {
        if type_of(line) != "map" { continue; }
        let bpm = get_num(line, "bpm", 120.0);
        let speed_events = [];
        if line.contains("speedEvents") && type_of(line["speedEvents"]) == "array" {
            speed_events = line["speedEvents"];
        }
        // Almost every official chart has exactly one speed event covering the
        // whole chart. Resolving that per note is a full function call and an
        // event scan 2,000 times per chart, so hoist the common case.
        let single_speed = speed_events.len() == 1;
        let single_start = 0.0;
        let single_value = 1.0;
        if single_speed {
            let event = speed_events[0];
            if type_of(event) == "map" {
                if event.contains("startTime") {
                    single_start = tick_to_seconds(get_num(event, "startTime", 0.0), bpm);
                }
                single_value = get_num(event, "value", 1.0);
            } else {
                single_speed = false;
            }
        }
        // RPE: per-line time factor and scroll-speed curve.
        let factor = if rpe { get_num(line, "bpmfactor", 1.0) } else { 1.0 };
        let tracks = if rpe { build_speed_tracks(line, axis, factor) } else { [] };
        let groups = [];
        if line.contains("notes") { groups.push(line["notes"]); }
        if line.contains("notesAbove") { groups.push(line["notesAbove"]); }
        if line.contains("notesBelow") { groups.push(line["notesBelow"]); }
        for group in groups {
            if type_of(group) != "array" { continue; }
            let group_above = true;
            if line.contains("notesBelow") && group == line["notesBelow"] { group_above = false; }
            for raw_note in group {
                if type_of(raw_note) != "map" { continue; }
                let note = if rpe {
                    note_from_rpe(raw_note, factor, axis)
                } else {
                    note_from_official(raw_note, bpm, group_above)
                };
                if rpe {
                    note[6] = line_speed_at(tracks, note[0]);
                } else {
                    // Official charts animate the line; the note's own speed
                    // field is the hold-head value, not the scroll speed.
                    if single_speed {
                        let line_speed = if single_start <= note[0] { single_value } else { 1.0 };
                        note[6] = line_speed * 1.2;
                    } else {
                        note[6] = official_speed_at(speed_events, note[0], bpm) * 1.2;
                    }
                }
                out.push(note);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// feature extraction (rating/features.py)
// ---------------------------------------------------------------------------

// Box-kernel density on a uniform grid. The grid advances monotonically, so both
// window bounds do too: one sweep keeps this O(n + grid) instead of
// O(grid * log n) binary searches. That matters because the host enforces a
// configurable operation budget per chart callback.
// A chart with a stray far-future note (or a huge silent tail) would make the
// grid run for millions of points and blow the host's operation budget, so the
// step grows when the requested span is unreasonable. Real charts never hit it.
// Rhai has no module-level constants, so the cap lives here as a literal.
fn grid_step(end, half) {
    let step = clamp(half / 4.0, 0.02, 0.25);
    if end / step > 20000.0 { end / 20000.0 } else { step }
}

// Box-kernel density on a uniform grid, evaluated with a cumulative-count table.
//
// The direct form advances two window pointers per grid point, which costs O(n +
// grid) *indexing* steps in an interpreter - and the host enforces its
// configurable Rhai operation budget per callback. Counts come from one monotone sweep that is
// shared by every grid point, so each point is a single lookup.
fn density_curve(times, duration, half) {
    let step = grid_step(if duration > half { duration } else { half }, half);
    let end = if duration > step { duration } else { step };
    let ratio = (end + step) / step;
    let whole = ratio.floor().to_int();
    let points = if ratio > whole * 1.0 { whole + 1 } else { whole };
    let n = times.len();
    let width = 2.0 * half;
    let curve = [];
    let lo = 0;
    let hi = 0;
    for k in 0..points {
        let centre = k * 1.0 * step;
        let lo_t = centre - half;
        let hi_t = centre + half;
        while lo < n && (times[lo] - lo_t) < 0.0 { lo += 1; }
        if hi < lo { hi = lo; }
        while hi < n && (times[hi] - hi_t) <= 0.0 { hi += 1; }
        curve.push((hi - lo) * 1.0 / width);
    }
    curve
}

// Peak of the same box-density curve, starting the sweep at `from_time`.
//
// Used by the finale: the curve spans the whole chart while only the tail holds
// notes, so every grid point before `from_time - half` is exactly zero and can
// never be the peak. Skipping them saves most of the finale's cost.
fn density_peak(times, end, half, from_time) {
    let step = grid_step(end, half);
    let ratio = (end + step) / step;
    let whole = ratio.floor().to_int();
    let points = if ratio > whole * 1.0 { whole + 1 } else { whole };
    let n = times.len();
    let width = 2.0 * half;
    let first = ((from_time - half) / step).floor().to_int();
    let k0 = if first > 0 { if first < points { first } else { points } } else { 0 };
    let lo = lower_bound(times, k0 * 1.0 * step - half);
    let hi = upper_bound(times, k0 * 1.0 * step + half);
    let peak = 0.0;
    for k in k0..points {
        let centre = k * 1.0 * step;
        let lo_t = centre - half;
        let hi_t = centre + half;
        while lo < n && (times[lo] - lo_t) < 0.0 { lo += 1; }
        if hi < lo { hi = lo; }
        while hi < n && (times[hi] - hi_t) <= 0.0 { hi += 1; }
        let value = (hi - lo) * 1.0 / width;
        if value > peak { peak = value; }
    }
    peak
}

fn weighted_density(times, weights, duration, half) {
    let step = grid_step(if duration > half { duration } else { half }, half);
    let end = if duration > step { duration } else { step };
    let ratio = (end + step) / step;
    let whole = ratio.floor().to_int();
    let points = if ratio > whole * 1.0 { whole + 1 } else { whole };
    let n = times.len();
    let width = 2.0 * half;
    // The reference reads (cumsum[hi] - cumsum[lo]) / width, so the running sum
    // has to be a prefix sum: adding and subtracting weights on the fly rounds
    // differently and shifts the p95 of the hand-imbalance curve.
    let cumulative = [0.0];
    let running = 0.0;
    for w in weights {
        running += w;
        cumulative.push(running);
    }
    let curve = [];
    let lo = 0;
    let hi = 0;
    for k in 0..points {
        let grid = (k * 1.0) * step;
        let lo_t = grid - half;
        let hi_t = grid + half;
        while hi < n && (times[hi] - hi_t) <= 0.0 { hi += 1; }
        while lo < hi && (times[lo] - lo_t) < 0.0 { lo += 1; }
        curve.push((cumulative[hi] - cumulative[lo]) / width);
    }
    curve
}

fn lower_bound(sorted, value) {
    let lo = 0;
    let hi = sorted.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if sorted[mid] < value { lo = mid + 1; } else { hi = mid; }
    }
    lo
}

fn upper_bound(sorted, value) {
    let lo = 0;
    let hi = sorted.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if sorted[mid] <= value { lo = mid + 1; } else { hi = mid; }
    }
    lo
}

// Least-squares slope of ys against an evenly spaced 0..1 axis
// (numpy.polyfit(deg=1) on np.linspace(0, 1, n)). The x mean and variance have
// closed forms, so only one pass over ys is needed and no xs array is built.
fn polyfit_slope_uniform(ys) {
    let n = ys.len();
    if n < 2 { return 0.0; }
    let mx = 0.5;
    let span = (n - 1) * 1.0;
    let my = mean(ys);
    let num = 0.0;
    let den = 0.0;
    for i in 0..n {
        let dx = i * 1.0 / span - mx;
        num += dx * (ys[i] - my);
        den += dx * dx;
    }
    safe_div(num, den)
}

// Feature extraction works on flat column arrays.
//
// Rhai deep-copies nested arrays at every call boundary: handing one 1,500-note
// chart (an array of 7-element arrays) to a function cost 1.3M operations, a
// third of the whole budget. Flattening first makes the call almost free.
fn extract_features(times, ends, xs, types, above, speeds, frac_fake) {
    let f = #{"frac_fake": frac_fake};
    let n = times.len();
    if n == 0 { return f; }

    let duration = if times[n - 1] > 1e-9 { times[n - 1] } else { 1e-9 };
    f["duration"] = duration;
    f["n_notes"] = n * 1.0;
    f["nps"] = n * 1.0 / duration;
    let active = if n > 1 { times[n - 1] - times[0] } else { duration };
    f["nps_active"] = safe_div(n * 1.0, if active > 1e-9 { active } else { 1e-9 });
    f["notes_per_minute"] = f["nps"] * 60.0;
    if n <= 2 { return f; }

    // press notes = Tap + Hold; also the hand weights and the type histogram
    let press_times = [];
    let press_xs = [];
    let press_mask = [];
    let left_weight = [];
    let taps = 0;
    let drags = 0;
    let flicks = 0;
    let holds = 0;
    let below = 0;
    for i in 0..n {
        let kind = types[i];
        let x = xs[i];
        let is_press = kind == 1 || kind == 3;
        left_weight.push((1.0 - x) / 2.0);
        press_mask.push(is_press);
        if is_press {
            press_times.push(times[i]);
            press_xs.push(x);
        }
        if kind == 1 { taps += 1; } else if kind == 3 { holds += 1; }
        if kind == 2 { drags += 1; }
        if kind == 4 { flicks += 1; }
        if !above[i] { below += 1; }
    }
    if press_times.len() == 0 {
        for i in 0..n {
            press_times.push(times[i]);
            press_xs.push(xs[i]);
        }
    }

    // press notes = Tap + Hold
    if press_times.len() == 0 {
        for i in 0..n {
            press_times.push(times[i]);
            press_xs.push(xs[i]);
        }
    }

    // ---- density at six scales ----
    // Only the 1 s curve is needed downstream (trend features); the others
    // contribute statistics only, so keep one curve instead of six.
    let halves = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
    let tags = ["d0.25", "d0.5", "d1", "d2", "d4", "d8"];
    let curves = #{};
    for i in 0..halves.len() {
        let curve = density_curve(press_times, duration, halves[i]);
        // Peak and sum in the same pass: a second loop over a 3,000-point grid
        // is thousands of interpreted operations.
        let peak = 0.0;
        let sum = 0.0;
        for v in curve {
            if v > peak { peak = v; }
            sum += v;
        }
        let m = sum / curve.len();
        // Sort once and reuse for both percentiles.
        let ordered = curve;
        ordered.sort();
        f[tags[i] + "_max"] = peak;
        f[tags[i] + "_p90"] = quantile_sorted(ordered, 0.90);
        f[tags[i] + "_p99"] = quantile_sorted(ordered, 0.99);
        f[tags[i] + "_peak_ratio"] = peak / if m > 1e-9 { m } else { 1e-9 };
        // index 2 is the 1 s window, the only curve used downstream.
        if i == 2 { curves["d1"] = curve; }
    }
    f["d1_mean"] = press_times.len() * 1.0 / duration;
    f["burst_1s_over_mean"] = (f["d1_max"] + 0.1) / (f["d1_mean"] + 0.1);
    f["sustain_4s_over_1s"] = (f["d4_p90"] + 0.1) / (f["d1_p90"] + 0.1);
    f["peak_sharpness"] = (f["d0.5_max"] + 0.1) / (f["d8_p90"] + 0.1);

    let d1 = curves["d1"];
    let quarter = if d1.len() / 4 > 1 { d1.len() / 4 } else { 1 };
    // Early/late means without materialising two sub-arrays.
    let early_sum = 0.0;
    for i in 0..quarter { early_sum += d1[i]; }
    let late_sum = 0.0;
    for i in (d1.len() - quarter)..d1.len() { late_sum += d1[i]; }
    f["trend_late_early"] = (late_sum / quarter + 0.1) / (early_sum / quarter + 0.1);
    if d1.len() > 2 {
        f["trend_slope"] = polyfit_slope_uniform(d1);
    } else {
        f["trend_slope"] = 0.0;
    }

    // ---- simultaneity ----
    let radii = [0.02, 0.05, 0.1, 0.2];
    let ch_tags = ["ch0.02", "ch0.05", "ch0.1", "ch0.2"];
    for i in 0..radii.len() {
        // times is sorted, so both window bounds move monotonically: sweep
        // instead of binary-searching per note (this was the single largest
        // operation cost on long charts).
        let neighbours = [];
        let total = press_times.len();
        let lo = 0;
        let hi = 0;
        for t in press_times {
            let lo_t = t - radii[i];
            let hi_t = t + radii[i];
            while lo < total && (press_times[lo] - lo_t) < 0.0 { lo += 1; }
            if hi < lo { hi = lo; }
            while hi < total && (press_times[hi] - hi_t) <= 0.0 { hi += 1; }
            neighbours.push((hi - lo - 1) * 1.0);
        }
        // Separate pass for max/solo/sum: Rhai charges per statement inside a
        // loop, and a tighter sweep is measurably cheaper than a fused one.
        let mx = 0.0;
        let solo = 0;
        let sum = 0.0;
        for v in neighbours {
            if v > mx { mx = v; }
            if v == 0.0 { solo += 1; }
            sum += v;
        }
        let ordered = neighbours;
        ordered.sort();
        f[ch_tags[i] + "_mean"] = sum / neighbours.len();
        f[ch_tags[i] + "_p95"] = quantile_sorted(ordered, 0.95);
        f[ch_tags[i] + "_max"] = mx;
        f[ch_tags[i] + "_solo"] = solo * 1.0 / neighbours.len();
    }

    // ---- hands ----
    let left_mean = mean(left_weight);
    f["hand_left_frac"] = left_mean;
    f["hand_balance"] = abs(left_mean - 0.5) * 2.0;

    let spread50 = spread_curve(times, xs, 0.05);
    f["spread_50ms_mean"] = mean(spread50);
    f["spread_50ms_p95"] = quantile(spread50, 0.95);
    f["spread_150ms_mean"] = spread_mean(times, xs, 0.15);

    // ---- jumps ----
    let dt = [];
    let dx = [];
    for i in 1..n {
        dt.push(times[i] - times[i - 1]);
        dx.push(abs(xs[i] - xs[i - 1]));
    }
    let dt_safe = [];
    for v in dt { dt_safe.push(if v > 0.01 { v } else { 0.01 }); }
    let close = [];
    for v in dt { close.push(v > 1e-6); }
    let close_count = 0;
    for v in close { if v { close_count += 1; } }
    if close_count > 0 {
        let jd = [];
        let jc = [];
        let js = [];
        let alt_num = 0.0;
        let alt_den = 0.0;
        for i in 0..close.len() {
            if !close[i] { continue; }
            jd.push(dx[i]);
            if dt[i] < 0.2 { jc.push(dx[i]); }
            js.push(dx[i] / dt_safe[i]);
            let cross = (1.0 - xs[i] * xs[i + 1]) / 2.0;
            let w = exp(-clamp(dt[i], 0.0, 1e9) / 0.15);
            alt_num += cross * w;
            alt_den += w;
        }
        f["jump_mean"] = mean(jd);
        f["jump_close_mean"] = if jc.len() > 0 { mean(jc) } else { 0.0 };
        f["jump_speed_mean"] = mean(js);
        f["jump_speed_p95"] = quantile(js, 0.95);
        f["alternation"] = safe_div(alt_num, if alt_den > 1e-9 { alt_den } else { 1e-9 });
    } else {
        f["jump_mean"] = 0.0;
        f["jump_close_mean"] = 0.0;
        f["jump_speed_mean"] = 0.0;
        f["jump_speed_p95"] = 0.0;
        f["alternation"] = 0.0;
    }

    // ---- scroll speed ----
    f["speed_mean"] = mean(speeds);
    f["speed_median"] = median_of(speeds);
    f["speed_p90"] = quantile(speeds, 0.90);
    f["speed_log_p90"] = ln(1.0 + f["speed_p90"]);
    f["speed_p90_over_mean"] = (f["speed_p90"] + 0.5) / (f["speed_mean"] + 0.5);
    f["throughput"] = f["nps"] * f["speed_mean"];
    f["throughput_p90"] = f["d1_p90"] * f["speed_p90"];
    f["notes_per_height"] = f["nps"] / if f["speed_mean"] > 0.05 { f["speed_mean"] } else { 0.05 };

    // ---- holds ----
    let hold_idx = [];
    for i in 0..n { if types[i] == 3 { hold_idx.push(i); } }
    f["hold_frac"] = hold_idx.len() * 1.0 / n;
    f["hold_coverage"] = hold_coverage(times, ends, hold_idx, duration);
    f["hold_max_concurrent"] = max_concurrent(times, ends, hold_idx) * 1.0;
    if hold_idx.len() > 0 {
        let lengths = [];
        for i in hold_idx { lengths.push(ends[i] - times[i]); }
        let mx = 0.0;
        for v in lengths { if v > mx { mx = v; } }
        f["hold_len_mean"] = mean(lengths);
        f["hold_len_p90"] = quantile(lengths, 0.90);
        f["hold_len_max"] = mx;
    } else {
        f["hold_len_mean"] = 0.0;
        f["hold_len_p90"] = 0.0;
        f["hold_len_max"] = 0.0;
    }

    // ---- finale ----
    let tail_start = times[n - 1] - (if 0.25 * duration > 20.0 { 0.25 * duration } else { 20.0 });
    let tail_times = [];
    for t in press_times { if t >= tail_start { tail_times.push(t); } }
    if tail_times.len() >= 8 {
        let span = if times[n - 1] - tail_start > 1.0 { times[n - 1] - tail_start } else { 1.0 };
        // Python formats these with {:g}, so 1.0 -> "1" not "1.0"; the names
        // have to match the model's column list exactly.
        let finale_tags = ["finale_d0.25_max", "finale_d0.5_max", "finale_d1_max"];
        let finale_halves = [0.25, 0.5, 1.0];
        for i in 0..finale_halves.len() {
            // The curve spans the whole chart but only the tail holds notes, so
            // every grid point before the tail is a guaranteed zero; start the
            // sweep there instead of walking the empty lead-in.
            f[finale_tags[i]] = density_peak(tail_times, times[n - 1], finale_halves[i], tail_times[0]);
        }
        // Chord size inside the tail: monotone bounds instead of two binary
        // searches per note.
        let chord_max = 0.0;
        let clo = 0;
        let chi = 0;
        for t in tail_times {
            while clo < tail_times.len() && (tail_times[clo] - (t - 0.02)) < 0.0 { clo += 1; }
            if chi < clo { chi = clo; }
            while chi < tail_times.len() && (tail_times[chi] - (t + 0.02)) <= 0.0 { chi += 1; }
            let count = (chi - clo - 1) * 1.0;
            if count > chord_max { chord_max = count; }
        }
        f["finale_chord_max"] = chord_max;
        f["finale_density"] = tail_times.len() * 1.0 / span;
        f["finale_press_nps"] = tail_times.len() * 1.0 / span;
    } else {
        f["finale_d0.25_max"] = 0.0;
        f["finale_d0.5_max"] = 0.0;
        f["finale_d1_max"] = 0.0;
        f["finale_chord_max"] = 0.0;
        f["finale_density"] = 0.0;
        f["finale_press_nps"] = 0.0;
    }

    // ---- jacks ----
    if close_count > 0 {
        let jack = [];
        for i in 0..close.len() {
            if !close[i] { continue; }
            let same_place = exp(-dx[i] / 0.06);
            let quick = exp(-dt_safe[i] / 0.12);
            jack.push(same_place * quick);
        }
        f["jackness"] = mean(jack);
        f["jackness_p95"] = quantile(jack, 0.95);
    } else {
        f["jackness"] = 0.0;
        f["jackness_p95"] = 0.0;
    }

    // ---- type mix ----
    f["frac_tap"] = taps * 1.0 / n;
    f["frac_drag"] = drags * 1.0 / n;
    f["frac_flick"] = flicks * 1.0 / n;
    f["frac_below"] = below * 1.0 / n;
    f["press_nps"] = (taps + holds) * 1.0 / duration;
    f["tap_nps"] = taps * 1.0 / duration;
    f["drag_nps"] = drags * 1.0 / duration;
    f["hold_nps"] = holds * 1.0 / duration;
    f["flick_nps"] = flicks * 1.0 / duration;
    f["tap_notes"] = taps * 1.0;

    // ---- per hand over time ----
    let press_left = [];
    for i in 0..n { if press_mask[i] { press_left.push(left_weight[i]); } }
    if press_left.len() == 0 { press_left = left_weight; }
    let left_curve = weighted_density(press_times, press_left, duration, 1.0);
    let right_weights = [];
    for w in press_left { right_weights.push(1.0 - w); }
    let right_curve = weighted_density(press_times, right_weights, duration, 1.0);
    let imbalance = [];
    let lpeak = 0.0;
    let rpeak = 0.0;
    for i in 0..left_curve.len() {
        let total = left_curve[i] + right_curve[i];
        imbalance.push(abs(left_curve[i] - right_curve[i]) / if total > 1e-9 { total } else { 1e-9 });
        if left_curve[i] > lpeak { lpeak = left_curve[i]; }
        if right_curve[i] > rpeak { rpeak = right_curve[i]; }
    }
    f["hand_imbalance_p95"] = quantile(imbalance, 0.95);
    f["hand_left_peak"] = lpeak;
    f["hand_right_peak"] = rpeak;

    // ---- rhythm ----
    let positive = [];
    for v in dt { if v > 1e-6 { positive.push(v); } }
    if positive.len() > 4 {
        let med = median_of(positive);
        let scale = if med > 1e-9 { med } else { 1e-9 };
        let ratio = [];
        for v in positive { ratio.push(v / scale); }
        let avg = mean(positive);
        let variance = 0.0;
        for v in positive { variance += (v - avg) * (v - avg); }
        let std = sqrt(variance / positive.len());
        f["interval_cv"] = std / if avg > 1e-9 { avg } else { 1e-9 };
        if ratio.len() > 4 {
            let diffs = [];
            for i in 1..ratio.len() { diffs.push(abs(ln(ratio[i]) - ln(ratio[i - 1]))); }
            f["rhythm_regularity"] = median_of(diffs);
        } else {
            f["rhythm_regularity"] = 0.0;
        }
        let on_grid = 0;
        for v in ratio { if abs(v - 1.0) < 0.1 { on_grid += 1; } }
        f["grid_concentration"] = on_grid * 1.0 / ratio.len();
        f["dt_p10"] = quantile(positive, 0.10);
        f["dt_p25"] = quantile(positive, 0.25);
        f["dt_median"] = med;
        let fast = [];
        for v in positive { fast.push(exp(-v / 0.08)); }
        f["fast_frac"] = mean(fast);
        if positive.len() > 2 {
            // rating/features.py builds ``second = x[2:] - 2x[1:-1] + x[:-2]``
            // (length n-2) and divides element-wise by ``dt_safe[:-1]`` whose
            // element k corresponds to note k -> k+1. Second-difference entry k
            // is centred on note k+1, so the divisor is dt_safe[k] = dt[k].
            let jag = [];
            for k in 0..(n - 2) {
                let second = xs[k + 2] - 2.0 * xs[k + 1] + xs[k];
                jag.push(abs(second) / dt_safe[k]);
            }
            f["x_jagged_mean"] = mean(jag);
            f["x_jagged_p95"] = quantile(jag, 0.95);
        } else {
            f["x_jagged_mean"] = 0.0;
            f["x_jagged_p95"] = 0.0;
        }
    } else {
        f["interval_cv"] = 0.0;
        f["rhythm_regularity"] = 0.0;
        f["grid_concentration"] = 0.0;
        f["dt_p10"] = 0.0;
        f["dt_p25"] = 0.0;
        f["dt_median"] = 0.0;
        f["fast_frac"] = 0.0;
        f["x_jagged_mean"] = 0.0;
        f["x_jagged_p95"] = 0.0;
    }
    f
}

// Spread of x over each note's time window: window.max() - window.min().
//
// `times` is sorted, so both window bounds only move forward. On real charts a
// window holds one or two notes, and scanning it is cheaper in the interpreter
// than maintaining two monotone index deques per note.
fn spread_curve(times, xs, radius) {
    let n = times.len();
    let out = [];
    let lo = 0;
    let hi = 0;
    for i in 0..n {
        let hi_t = times[i] + radius;
        while hi < n && (times[hi] - hi_t) <= 0.0 { hi += 1; }
        let lo_t = times[i] - radius;
        while lo < hi && (times[lo] - lo_t) < 0.0 { lo += 1; }
        if hi - lo > 1 {
            let mx = xs[lo];
            let mn = mx;
            for j in (lo + 1)..hi {
                let v = xs[j];
                if v > mx { mx = v; }
                if v < mn { mn = v; }
            }
            out.push(mx - mn);
        } else {
            out.push(0.0);
        }
    }
    out
}

// The 150 ms spread only feeds a mean, so skip materialising the array.
fn spread_mean(times, xs, radius) {
    let n = times.len();
    if n == 0 { return 0.0; }
    let total = 0.0;
    let lo = 0;
    let hi = 0;
    for i in 0..n {
        let hi_t = times[i] + radius;
        while hi < n && (times[hi] - hi_t) <= 0.0 { hi += 1; }
        let lo_t = times[i] - radius;
        while lo < hi && (times[lo] - lo_t) < 0.0 { lo += 1; }
        if hi - lo > 1 {
            let mx = xs[lo];
            let mn = mx;
            for j in (lo + 1)..hi {
                let v = xs[j];
                if v > mx { mx = v; }
                if v < mn { mn = v; }
            }
            total += mx - mn;
        }
    }
    total / n
}

fn hold_coverage(times, ends, idx, duration) {
    if idx.len() == 0 || duration <= 0.0 { return 0.0; }
    let spans = [];
    for i in idx { spans.push([times[i], ends[i]]); }
    spans.sort(|a, b| if a[0] < b[0] { -1 } else if a[0] > b[0] { 1 } else { 0 });
    let covered = 0.0;
    let cursor = 0.0;
    for span in spans {
        let start = if span[0] > cursor { span[0] } else { cursor };
        if span[1] > start {
            covered += span[1] - start;
            cursor = span[1];
        }
    }
    covered / if duration > 1e-9 { duration } else { 1e-9 }
}

fn max_concurrent(times, ends, idx) {
    if idx.len() == 0 { return 0; }
    let events = [];
    for i in idx {
        events.push([times[i], 1]);
        events.push([ends[i], -1]);
    }
    // (time asc, +1 before -1)
    events.sort(|a, b| if a[0] < b[0] { -1 } else if a[0] > b[0] { 1 } else if a[1] > b[1] { -1 } else if a[1] < b[1] { 1 } else { 0 });
    let live = 0;
    let best = 0;
    for e in events {
        live += e[1];
        if live > best { best = live; }
    }
    best
}

// ---------------------------------------------------------------------------
// model scoring (rating/model.py + rating/predictor.py)
// ---------------------------------------------------------------------------

fn curve_at(curve, value) {
    let xs = curve["x"];
    let ys = curve["y"];
    let ds = curve["d"];
    let n = xs.len();
    if value <= xs[0] { return ys[0]; }
    if value >= xs[n - 1] { return ys[n - 1]; }
    let lo = 0;
    let hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xs[mid] <= value { lo = mid; } else { hi = mid; }
    }
    let h = xs[hi] - xs[lo];
    let t = (value - xs[lo]) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    (2.0 * t3 - 3.0 * t2 + 1.0) * ys[lo]
        + (t3 - 2.0 * t2 + t) * h * ds[lo]
        + (-2.0 * t3 + 3.0 * t2) * ys[hi]
        + (t3 - t2) * h * ds[hi]
}

fn tree_value(tree, x) {
    let features = tree[0];
    let thresholds = tree[1];
    let left = tree[2];
    let right = tree[3];
    let leaves = tree[4];
    let base = features.len();
    let node = 0;
    while node < base {
        node = if x[features[node]] <= thresholds[node] { left[node] } else { right[node] };
    }
    leaves[node - base]
}

fn ensemble(model, x) {
    let total = 0.0;
    for tree in model["trees"] {
        total += tree_value(tree, x);
    }
    let value = model["init"] + model["learning_rate"] * total;
    let ridge = model["ridge"];
    let linear = ridge["intercept"];
    for i in 0..x.len() {
        linear += ridge["coef"][i] * (x[i] - ridge["mean"][i]) / ridge["scale"][i];
    }
    let clipped = if linear > ridge["clip"] { ridge["clip"] } else if linear < -ridge["clip"] { -ridge["clip"] } else { linear };
    value + clipped
}

fn calibration(calib, raw) {
    let low = calib["low"];
    let high = calib["high"];
    let value = curve_at(calib["curve"], clamp(raw, low, high));
    if raw < low { value += calib["slope_low"] * (raw - low); }
    if raw > high { value += calib["slope_high"] * (raw - high); }
    value
}

fn official_to_community(scale, value) {
    let xs = scale["official"];
    let ys = scale["community"];
    if value < xs[0] { return ys[0] + (value - xs[0]) * scale["first_slope"]; }
    if value > xs[xs.len() - 1] { return ys[ys.len() - 1] + (value - xs[xs.len() - 1]) * scale["last_slope"]; }
    curve_at(scale["curve"], value)
}

fn community_to_official(scale, value) {
    let xs = scale["official"];
    let ys = scale["community"];
    if value > ys[ys.len() - 1] {
        return xs[xs.len() - 1] + (value - ys[ys.len() - 1]) / scale["last_slope"];
    }
    let low = xs[0] - 6.0;
    let high = xs[xs.len() - 1];
    for step in 0..80 {
        let mid = 0.5 * (low + high);
        if official_to_community(scale, mid) < value { low = mid; } else { high = mid; }
    }
    0.5 * (low + high)
}

fn tail_composite(tail, x) {
    let total = 0.0;
    for i in 0..tail["columns"].len() {
        let span = tail["high"][i] - tail["low"][i];
        let span_safe = if span > 1e-6 { span } else { 1e-6 };
        let raw = (x[tail["columns"][i]] - tail["low"][i]) / span_safe;
        total += if raw > 0.0 { raw } else { 0.0 };
    }
    total / tail["columns"].len()
}

fn score(payload, x) {
    let base = calibration(payload["calib_full"], ensemble(payload["full"], x));
    let blend_low = payload["blend"][0];
    let blend_high = payload["blend"][1];
    let weight = clamp((base - blend_low) / (blend_high - blend_low), 0.0, 1.0);

    let tail = payload["tail"];
    let composite = tail_composite(tail, x);
    let excess = if composite > tail["c_max"] { composite - tail["c_max"] } else { 0.0 };
    let value = base;

    // The second ("top") ensemble only enters through the blend weight and the
    // tail boost. Below the blend window and without tail excess the weight is
    // exactly 0, so evaluating it changes nothing - and it is half the model
    // (700 more trees). Skipping it here is exact, not an approximation.
    if weight > 0.0 || excess > 0.0 {
        let raw_top = calibration(payload["calib_top"], ensemble(payload["top"], x));
        let boost = (tail["ceiling"] - raw_top) * (1.0 - exp(-tail["rate"] * excess));
        let boosted = if boost > 0.0 { raw_top + boost } else { raw_top };
        let blend = weight;
        let tail_weight = clamp(excess / 0.35, 0.0, 1.0);
        if tail_weight > blend { blend = tail_weight; }
        value = (1.0 - blend) * base + blend * community_to_official(payload["scale"], boosted);
    }
    let bonus = excess * tail["slope"];
    value += if bonus > tail["cap"] { tail["cap"] } else { bonus };

    let bias = payload["bias_correction"];
    let clipped = clamp(curve_at(bias["curve"], value), -bias["limit"], bias["limit"]);
    let u = (value - bias["center"]) / bias["width"];
    let window = exp(-(u * u * u * u));
    value -= bias["damping"] * clipped * window;

    let high = payload["high_lift"];
    let community = official_to_community(payload["scale"], value);
    let above = if community > high["start"] { community - high["start"] } else { 0.0 };
    let q = high["k"] * above * above;
    community += high["cap"] * (1.0 - exp(-q / high["cap"]));
    community_to_official(payload["scale"], community)
}

// ---------------------------------------------------------------------------
// breakdown (rating/compose.py)
// ---------------------------------------------------------------------------

// A few headline features, shown under the two difficulty numbers. The source
// model has no decomposed "difficulty parts" - it is a single regressor - so
// these are reported as the descriptive chart statistics they actually are
// rather than invented sub-scores.
fn chart_summary(f) {
    let rows = [
        #{"key": "nps", "label": "每秒音符", "value": f["nps"], "digits": 2},
        #{"key": "burst", "label": "1s峰值/均值", "value": f["burst_1s_over_mean"], "digits": 2},
        #{"key": "chord", "label": "多押均值", "value": f["ch0.05_mean"], "digits": 2},
        #{"key": "jump", "label": "跳距均值", "value": f["jump_mean"], "digits": 2},
        #{"key": "speed", "label": "平均流速", "value": f["speed_mean"], "digits": 2},
        #{"key": "hold", "label": "长条占比", "value": f["hold_frac"], "digits": 2},
        #{"key": "jack", "label": "纵连强度", "value": f["jackness"], "digits": 2},
        #{"key": "dur", "label": "时长", "value": f["duration"], "digits": 2}
    ];
    rows
}

// rating/compose.py: five player-facing categories, measured by occlusion.
//
// Each group's features are replaced by the training median ("an unremarkable
// chart"), the model is run again, and the drop is that group's contribution.
// The result is what the host's difficulty panel draws as 1..10 bars, so the
// bars match the number above them instead of being decorative.
fn decompose(payload, x, base) {
    let columns = payload["columns"];
    let baseline = payload["baseline"];
    let position = #{};
    for i in 0..columns.len() { position[columns[i]] = i; }

    let groups = [
        ["n_notes", "duration", "nps", "nps_active", "notes_per_minute", "d1_mean", "press_nps", "tap_nps", "drag_nps", "hold_nps", "flick_nps", "tap_notes", "frac_tap", "frac_drag", "frac_flick", "frac_below"],
        ["d0.25_max", "d0.25_p90", "d0.25_p99", "d0.25_peak_ratio", "d0.5_max", "d0.5_p90", "d0.5_p99", "d0.5_peak_ratio", "d1_max", "d1_p90", "d1_p99", "d1_peak_ratio", "burst_1s_over_mean", "peak_sharpness", "finale_d0.25_max", "finale_d0.5_max", "finale_d1_max", "finale_chord_max", "finale_density", "finale_press_nps"],
        ["ch0.02_mean", "ch0.02_p95", "ch0.02_max", "ch0.02_solo", "ch0.05_mean", "ch0.05_p95", "ch0.05_max", "ch0.05_solo", "ch0.1_mean", "ch0.1_p95", "ch0.1_max", "ch0.1_solo", "ch0.2_mean", "ch0.2_p95", "ch0.2_max", "ch0.2_solo", "spread_50ms_mean", "spread_50ms_p95", "spread_150ms_mean", "hand_left_frac", "hand_balance", "hand_imbalance_p95", "hand_left_peak", "hand_right_peak", "hold_max_concurrent"],
        ["speed_mean", "speed_median", "speed_p90", "speed_log_p90", "speed_p90_over_mean", "throughput", "throughput_p90", "notes_per_height", "jump_mean", "jump_close_mean", "jump_speed_mean", "jump_speed_p95", "x_jagged_mean", "x_jagged_p95"],
        ["d2_max", "d2_p90", "d2_p99", "d2_peak_ratio", "d4_max", "d4_p90", "d4_p99", "d4_peak_ratio", "d8_max", "d8_p90", "d8_p99", "d8_peak_ratio", "sustain_4s_over_1s", "trend_late_early", "trend_slope", "hold_frac", "hold_coverage", "hold_len_mean", "hold_len_p90", "hold_len_max", "interval_cv", "rhythm_regularity", "grid_concentration", "dt_p10", "dt_p25", "dt_median", "fast_frac", "alternation", "jackness", "jackness_p95", "frac_fake"]
    ];
    let keys = ["volume", "burst", "chords", "movement", "stamina"];
    let labels = ["物量密度", "爆发与尾杀", "多押与双手", "位移与读谱", "体力与节奏"];

    let contrib = [];
    let total = 0.0;
    let biggest = 0.0;
    for group in groups {
        let probe = [];
        for v in x { probe.push(v); }
        for name in group {
            if position.contains(name) && baseline.contains(name) {
                probe[position[name]] = baseline[name];
            }
        }
        let delta = base - score(payload, probe);
        contrib.push(delta);
        total += abs(delta);
        if abs(delta) > biggest { biggest = abs(delta); }
    }
    if total <= 1e-9 { total = 1.0; }
    let parts = [];
    for i in 0..contrib.len() {
        let share = 100.0 * abs(contrib[i]) / total;
        parts.push(#{
            "key": keys[i],
            // The host draws `label` on the left and a bar of score/10; the
            // percentage goes in the label so the exact number is readable.
            "label": labels[i] + " " + fmt_fixed(share, 0) + "%",
            "score": clamp(share / 10.0, 1.0, 10.0),
            "share": share,
            "value": contrib[i]
        });
    }
    parts
}

// Tap / Hold / Drag / Flick 占比（服务器页面上是饼图，插件面板只能画条形，
// 所以用同样的四行条形呈现）。
fn type_mix_parts(features) {
    let rows = [
        #{"key": "tap", "label": "Tap", "value": features["frac_tap"]},
        #{"key": "hold", "label": "Hold", "value": features["hold_frac"]},
        #{"key": "drag", "label": "Drag", "value": features["frac_drag"]},
        #{"key": "flick", "label": "Flick", "value": features["frac_flick"]}
    ];
    let parts = [];
    for row in rows {
        let share = row["value"] * 100.0;
        parts.push(#{
            "key": row["key"],
            "label": row["label"] + " " + fmt_fixed(share, 0) + "%",
            "score": clamp(share / 10.0, 1.0, 10.0),
            "share": share
        });
    }
    parts
}

// ---------------------------------------------------------------------------
// callbacks
// ---------------------------------------------------------------------------

// Every failure path has to return a report instead of an error.
//
// The host sets `error` on the plugin the moment a callback returns an error or
// exceeds the operation budget, logs "plugin suspended", and from then on skips
// the plugin in every overlay pass. A suspended plugin draws *nothing*, which
// looks exactly like "the plugin does not work"; a report carrying an
// explanatory hud line at least tells the player what happened.
fn refusal(message) {
    #{
        algorithm: "phigros_difficulty_estimator_re",
        failed: true,
        text: message,
        hud: [
            #{"text": "难度预测不可用", "x": -0.62, "y": 0.86, "color": 0xFFD166},
            #{"text": message, "x": -0.62, "y": 0.79, "color": 0xFFFFFF}
        ]
    }
}

fn analyze_chart(ctx) {
    let raw = if ctx.contains("chart_data") { ctx["chart_data"] } else { ctx["raw"] };
    if raw == () {
        return refusal("此谱面没有可解析的原始文本（可能是旧版 pbc 格式）");
    }
    if type_of(raw) != "map" {
        return refusal("谱面原始数据不是 JSON 对象（仅支持 RPE / 官方格式）");
    }

    // The host hands over the chart file verbatim; parse it ourselves so the
    // 106 features match rating/features.py instead of a different extractor.
    let notes = [];
    try {
        notes = parse_chart(raw);
    } catch (err) {
        return refusal("谱面解析失败：" + err);
    }
    if notes.len() < 8 {
        return refusal("解析到的音符太少（" + notes.len() + " 个），无法预测");
    }

    // The host permanently suspends a plugin that overruns its configured
    // operation budget, with nothing rendered.
    //
    // Cost model fitted on real measurements (rhai 1.26.1, same limits as the
    // host): ~1,370 operations per note, ~4,000 per second of chart length
    // (the six density grids are sampled per second), plus the model pass.
    // A twelve-minute chart of 1,700 notes costs as much as a three-minute one
    // of 2,700, so duration has to be part of the guard.
    let span = 0.0;
    for note in notes {
        if note[0] > span { span = note[0]; }
    }
    // Fitted on measured runs: an RPE chart costs ~500 operations per note just
    // to parse (triples, per-line speed tracks), an official one ~250, and the
    // six density grids are sampled per second of chart length.
    //   RPE 1,513 notes / 173 s = 3.85M, RPE 2,025 notes = 4.58M,
    //   official 1,936 notes / 142 s = 3.47M.
    let rpe_chart = is_rpe(raw);
    let estimate = if rpe_chart {
        1050000 + notes.len() * 1400 + span * 4200
    } else {
        600000 + notes.len() * 1200 + span * 4200
    };
    let operation_limit = if ctx.contains("operation_limit") {
        ctx["operation_limit"].to_int()
    } else {
        20000000
    };
    let estimate_limit = operation_limit - operation_limit / 20;
    if estimate > estimate_limit {
        return refusal("谱面过大（" + notes.len() + " 音符 / " + fmt_fixed(span, 0) + " 秒），超出插件计算预算");
    }

    let features = #{};
    try {
        // (time to 1us, x to 1us, type) so a chord comes out format-independently.
        notes.sort(|a, b| {
            let ta = (a[0] * 1000000.0).round();
            let tb = (b[0] * 1000000.0).round();
            if ta < tb { return -1; }
            if ta > tb { return 1; }
            let xa = (a[2] * 1000000.0).round();
            let xb = (b[2] * 1000000.0).round();
            if xa < xb { return -1; }
            if xa > xb { return 1; }
            if a[3] < b[3] { return -1; }
            if a[3] > b[3] { return 1; }
            0
        });
    
        let all = notes.len();
        let kept = [];
        let fake_count = 0;
        for note in notes {
            if note[5] { fake_count += 1; } else { kept.push(note); }
        }
    
        // A sentinel note parked far in the future would stretch duration to
        // millions of seconds; charts.py drops anything past 1.2x the 95th
        // percentile once the last note is 3x beyond it.
        if kept.len() >= 20 {
            let all_times = [];
            for note in kept { all_times.push(note[0]); }
            let horizon = quantile_sorted(all_times, 0.95);
            if kept[kept.len() - 1][0] > horizon * 3.0 {
                let filtered = [];
                for note in kept {
                    if note[0] <= horizon * 1.2 { filtered.push(note); }
                }
                kept = filtered;
            }
        }
    
        let times = [];
        let ends = [];
        let xs = [];
        let types = [];
        let above = [];
        let speeds = [];
        for note in kept {
            times.push(note[0]);
            ends.push(note[1]);
            xs.push(clamp(note[2], -1.0, 1.0));
            types.push(note[3]);
            above.push(note[4]);
            speeds.push(clamp(note[6], 0.0, 50.0));
        }
        features = extract_features(times, ends, xs, types, above, speeds, safe_div(fake_count, all));
    } catch (err) {
        return refusal("特征提取失败：" + err);
    }

    let payload = ctx["assets"]["model"];
    let x = [];
    let official = 0.0;
    let community = 0.0;
    try {
        for name in payload["columns"] {
            if features.contains(name) { x.push(features[name]); } else { x.push(0.0); }
        }
        official = score(payload, x);
        community = official_to_community(payload["scale"], official);
    } catch (err) {
        return refusal("模型计算失败：" + err);
    }

    // The decomposition costs five extra model passes (~600k operations), so it
    // only runs when the chart still leaves room under the configured budget.
    let parts = [];
    if estimate + 800000 <= estimate_limit {
        try {
            parts = decompose(payload, x, official);
        } catch (err) {
            parts = [];
        }
    }
    // 键型占比不依赖细分模型，即使预算不够也照样给出。
    for row in type_mix_parts(features) {
        parts.push(row);
    }

    // Everything is published through the host's difficulty panel
    // (`difficulty` + `parts`, phira/src/plugins.rs::render_ppsr_panel) so the
    // plugin looks like part of the UI instead of loose text in a corner. The
    // `hud` channel is left empty on success; it is only used to explain a
    // failure, because a silent plugin is indistinguishable from a broken one.
    #{
        algorithm: "phigros_difficulty_estimator_re",
        // 面板标题：宿主目前把这个位置写死成 "PPSR"（phira/src/plugins.rs:398），
        // 只有它愿意读这个字段时才会显示成我们的名字；留着不占地方。
        title: "pde_r by fwmtnk",
        difficulty: official,
        community: community,
        parts: parts,
        note_count: notes.len(),
        features: x.len(),
        // Kept for tools/check_features.py, which compares these against
        // rating/features.py; harmless in the overlay.
        feature_map: features,
        hud: [],
        text: "预测定数 " + fmt_fixed(official, 2) + "（社区 " + fmt_fixed(community, 2) + "）"
    }
}

fn on_chart(ctx, settings) {
    analyze_chart(ctx)
}

fn on_chart_selected(ctx, settings) {
    analyze_chart(ctx)
}
