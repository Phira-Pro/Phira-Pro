//! Descriptive statistics only; decision thresholds belong to the plugin.
use serde_json::{json, Value};

pub fn quantile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.;
    }
    let i = (sorted.len() - 1) as f64 * p.clamp(0., 1.);
    let k = i.floor() as usize;
    sorted[k] + (sorted[i.ceil() as usize] - sorted[k]) * i.fract()
}

pub fn describe(values: &[f64]) -> Value {
    let mut v: Vec<_> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if v.is_empty() {
        return json!({"n":0,"median":null,"mad":null,"p05":null,"p95":null,"std":null,"mode_ratio":null});
    }
    v.sort_by(f64::total_cmp);
    let median = quantile(&v, 0.5);
    let mut dev: Vec<_> = v.iter().map(|x| (x - median).abs()).collect();
    dev.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let std = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
    // 1 ms bins are explicitly a display statistic, never inferred hardware rate.
    let mut best = 0;
    let mut run = 0;
    let mut previous = f64::NAN;
    for x in &v {
        let x = x.round();
        if x == previous {
            run += 1;
        } else {
            run = 1;
            previous = x;
        }
        best = best.max(run);
    }
    json!({"n":v.len(),"median":median,"mad":quantile(&dev,0.5),"mean":mean,"std":std,"p05":quantile(&v,0.05),"p95":quantile(&v,0.95),"min":v[0],"max":v[v.len()-1],"mode_ratio":best as f64/v.len() as f64,"mode_bin_ms":1,"mode_lower95":wilson_lower(best,v.len()),"mode_count":best})
}

/// Wilson lower endpoint describes the uncertainty of the observed bin share.
/// It is not a probability of cheating and assumes no population baseline.
pub fn wilson_lower(successes: usize, count: usize) -> f64 {
    if count == 0 {
        return 0.;
    }
    let n = count as f64;
    let p = successes as f64 / n;
    let z = 1.959963984540054;
    ((p + z * z / (2. * n) - z * (p * (1. - p) / n + z * z / (4. * n * n)).sqrt()) / (1. + z * z / n)).max(0.)
}

pub fn correlation(v: &[f64], lag: usize) -> Option<f64> {
    if v.len() < lag + 20 {
        return None;
    }
    let a = &v[..v.len() - lag];
    let b = &v[lag..];
    let ma = a.iter().sum::<f64>() / a.len() as f64;
    let mb = b.iter().sum::<f64>() / b.len() as f64;
    let denom = (a.iter().map(|v| (v - ma).powi(2)).sum::<f64>() * b.iter().map(|v| (v - mb).powi(2)).sum::<f64>()).sqrt();
    (denom > 1e-12).then(|| a.iter().zip(b).map(|(a, b)| (a - ma) * (b - mb)).sum::<f64>() / denom)
}

pub fn histogram(v: &[f64], lo: f64, hi: f64, bins: usize) -> Vec<[f64; 2]> {
    let mut out: Vec<_> = (0..bins).map(|i| [lo + (i as f64 + 0.5) * (hi - lo) / bins as f64, 0.]).collect();
    for &v in v {
        if v.is_finite() && v >= lo && v <= hi {
            let i = (((v - lo) / (hi - lo) * bins as f64) as usize).min(bins - 1);
            out[i][1] += 1.;
        }
    }
    out
}

/// Retain each time bucket's min/max; never used for statistical calculations.
pub fn envelope(points: &[[f64; 2]], bins: usize) -> Vec<[f64; 2]> {
    if points.len() <= bins * 2 {
        return points.to_vec();
    }
    let chunk = points.len().div_ceil(bins);
    let mut out = Vec::new();
    for group in points.chunks(chunk) {
        let min = group.iter().enumerate().min_by(|a, b| a.1[1].total_cmp(&b.1[1])).unwrap();
        let max = group.iter().enumerate().max_by(|a, b| a.1[1].total_cmp(&b.1[1])).unwrap();
        if min.0 <= max.0 {
            out.push(*min.1);
            out.push(*max.1);
        } else {
            out.push(*max.1);
            out.push(*min.1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn robust_statistics_expose_empty_and_extreme_values() {
        assert!(describe(&[])["median"].is_null());
        let s = describe(&[1., 2., 2., 3., 1000.]);
        assert_eq!(s["median"], 2.);
        assert_eq!(s["mad"], 1.);
        assert_eq!(s["n"], 5);
        let points: Vec<_> = (0..10000).map(|i| [i as f64, if i == 999 { 999. } else { 0. }]).collect();
        assert!(envelope(&points, 100).iter().any(|p| p[1] == 999.));
    }
}
