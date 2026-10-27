//! Compact, non-overlapping observation blocks. Counts are effective operations,
//! not elapsed song time; simultaneous judgements do not inflate sample size.
use super::stats;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct Contact {
    pub time: f64,
    pub millis: f64,
    pub key: Option<u64>,
    pub segment: usize,
}

fn summary(values: &[f64], frame_ms: f64) -> Value {
    let mut out = stats::describe(values);
    // Subscriber events can be delivered on a frame lattice. A narrow peak on
    // that lattice is an acquisition property, not a physiological anomaly.
    let tolerance = (frame_ms * 0.04).max(0.2);
    let locked = if frame_ms >= 0.5 && frame_ms <= 100. && !values.is_empty() {
        values
            .iter()
            .filter(|&&v| {
                let nearest = (v / frame_ms).round() * frame_ms;
                (v - nearest).abs() <= tolerance
            })
            .count() as f64
            / values.len() as f64
    } else {
        0.
    };
    out["frame_lattice_ratio"] = json!(locked);
    out
}

fn blocks(points: &[[f64; 2]], frame_ms: f64, key: &str) -> Vec<Value> {
    // Fixed local blocks preserve short anomalies inside long or dense charts.
    // The plugin reads bounded pages, so no block widening or subsampling is needed.
    let size = 32;
    points
        .chunks(size)
        .map(|chunk| {
            let values: Vec<_> = chunk.iter().map(|p| p[1]).collect();
            let intervals: Vec<_> = chunk
                .windows(2)
                .filter_map(|w| {
                    let dt = (w[1][0] - w[0][0]) * 1000.;
                    (dt > 0.).then_some(dt)
                })
                .collect();
            json!({"key":key,"start":chunk[0][0],"end":chunk.last().unwrap()[0],
            "stats":summary(&values, frame_ms),"intervals":stats::describe(&intervals)})
        })
        .collect()
}

pub fn compact(timing: &[[f64; 2]], contacts: &[Contact], frame_ms: f64) -> Value {
    let mut simultaneous: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    for &[t, value] in timing {
        simultaneous.entry((t * 1_000_000.).round() as i64).or_default().push(value);
    }
    let independent: Vec<_> = simultaneous
        .into_iter()
        .map(|(time, mut values)| {
            values.sort_by(f64::total_cmp);
            [time as f64 / 1_000_000., stats::quantile(&values, 0.5)]
        })
        .collect();
    let mut channels: BTreeMap<(Option<u64>, usize), Vec<[f64; 2]>> = BTreeMap::new();
    for c in contacts {
        channels.entry((c.key, c.segment)).or_default().push([c.time, c.millis]);
    }
    let duration: Vec<_> = channels
        .into_iter()
        .flat_map(|((key, segment), mut points)| {
            points.sort_by(|a, b| a[0].total_cmp(&b[0]));
            let key = key.map(|k| k.to_string()).unwrap_or_else(|| "contact".into());
            let mut out = blocks(&points, frame_ms, &key);
            for w in &mut out {
                w["segment"] = json!(segment);
            }
            out
        })
        .collect();
    let timing = blocks(&independent, frame_ms, "timing");
    json!({"version":2,"block_target":32,"independent_timing_samples":independent.len(),
        "raw_timing_samples":timing.len(),"complete_contacts":contacts.len(),
        "timing_blocks":timing.len(),"duration_blocks":duration.len(),
        "timing":timing,"duration":duration})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chords_are_one_observation_and_sparse_songs_use_the_same_sample_budget() {
        let mut timing = Vec::new();
        for i in 0..64 {
            for _ in 0..8 {
                timing.push([i as f64 * 3., i as f64]);
            }
        }
        let c = compact(&timing, &[], 16.);
        assert_eq!(c["independent_timing_samples"], 64);
        assert_eq!(c["timing"].as_array().unwrap().len(), 2);
        assert_eq!(c["timing"][0]["stats"]["n"], 32);
        for p in &mut timing {
            p[0] /= 100.;
        }
        let short = compact(&timing, &[], 16.);
        assert_eq!(short["timing"][0]["stats"], c["timing"][0]["stats"]);
    }
    #[test]
    fn per_key_blocks_do_not_pool_keys_or_cross_pause_segments() {
        let contacts: Vec<_> = (0..64)
            .map(|i| Contact {
                time: i as f64,
                millis: 40.,
                key: Some(i % 2),
                segment: if i < 32 { 0 } else { 1 },
            })
            .collect();
        let c = compact(&[], &contacts, 10.);
        let blocks = c["duration"].as_array().unwrap();
        assert_eq!(blocks.len(), 4);
        assert!(blocks.iter().all(|w| w["stats"]["n"] == 16));
        assert!(blocks.iter().all(|w| w["stats"]["frame_lattice_ratio"] == 1.));
    }
}
