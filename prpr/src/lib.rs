pub mod audio;
pub mod bin;
pub mod config;
pub mod core;
pub mod dir;
pub mod ext;
pub mod fs;
pub mod info;
#[cfg(target_os = "ios")]
pub mod frame_pacing;

/// 把宽高比格式化成人类可读的比例形式，例如 `16:9`、`4:3`、`21:9`。
///
/// 官方显示的是 `1.77778` 这种小数，这里改成比例：先查常见比例表，命中就直接输出；
/// 否则用「分母不超过 32 的最佳有理逼近」；再不行才退回小数。
pub fn format_aspect_ratio(ratio: f32) -> String {
    const COMMON: &[(u32, u32)] = &[
        (32, 9),
        (21, 9),
        (2, 1),
        (16, 9),
        (16, 10),
        (3, 2),
        (4, 3),
        (5, 4),
        (1, 1),
        (3, 4),
        (9, 16),
        (9, 21),
        (9, 32),
    ];
    if !ratio.is_finite() || ratio <= 0. {
        return format!("{ratio}");
    }
    for (w, h) in COMMON {
        let target = *w as f32 / *h as f32;
        if (ratio - target).abs() <= target * 1e-4 {
            return format!("{w}:{h}");
        }
    }
    // 分母 <= 32 的最佳有理逼近；同误差时保留更简单的那个（即更小的分母）。
    let x = ratio as f64;
    let (mut best_n, mut best_d) = (1u32, 1u32);
    let mut best_err = f64::INFINITY;
    for d in 1..=32u32 {
        let n = (x * d as f64).round() as u32;
        if n == 0 {
            continue;
        }
        let err = (x - n as f64 / d as f64).abs();
        if err < best_err {
            best_err = err;
            best_n = n;
            best_d = d;
        }
    }
    if best_err <= x * 1e-3 {
        format!("{best_n}:{best_d}")
    } else {
        format!("{ratio:.4}")
    }
}

/// 解析宽高比输入：既接受 `16:9` / `16：9`，也接受 `1.7778` 这样的小数。
pub fn parse_aspect_ratio(text: &str) -> Option<f32> {
    let text = text.trim();
    let value = if let Some((w, h)) = text.split_once([':', '：']) {
        let w: f32 = w.trim().parse().ok()?;
        let h: f32 = h.trim().parse().ok()?;
        if h == 0. {
            return None;
        }
        w / h
    } else {
        text.parse::<f32>().ok()?
    };
    (value.is_finite() && value > 0.).then_some(value)
}
pub mod judge;
pub mod parse;
pub mod particle;
pub mod scene;
pub mod task;
pub mod time;
pub mod ui;

#[cfg(feature = "log")]
pub mod log;

pub use scene::Main;

pub fn build_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "Phira".to_string(),
        window_width: 973,
        window_height: 608,

        ..Default::default()
    }
}
