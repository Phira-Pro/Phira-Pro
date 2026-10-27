//! Deterministic CPU raster report. No browser, external Python or GPU required.
use super::{Control, Dataset, Series};
use crate::plugins::replay_analysis::Analyzer;
use anyhow::Result;
use image::{Rgba, RgbaImage};
use prpr::ui::ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use serde_json::Value;

const INK: [u8; 3] = [26, 33, 54];
const MUTED: [u8; 3] = [107, 117, 146];
const ACCENT: [u8; 3] = [43, 85, 232];

struct Canvas {
    image: RgbaImage,
    font: FontArc,
}
impl Canvas {
    fn rounded(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, color: [u8; 3]) {
        for py in 0..h {
            for px in 0..w {
                let dx = if px < r { r - px } else { px.saturating_sub(w - r - 1) };
                let dy = if py < r { r - py } else { py.saturating_sub(h - r - 1) };
                if dx * dx + dy * dy <= r * r && x + px < self.image.width() && y + py < self.image.height() {
                    self.image.put_pixel(x + px, y + py, Rgba([color[0], color[1], color[2], 255]));
                }
            }
        }
    }
    fn card(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.rounded(x, y + 3, w, h, 28, [224, 230, 242]);
        self.rounded(x, y, w, h, 28, [230, 233, 242]);
        self.rounded(x + 2, y + 2, w - 4, h - 4, 27, [255, 255, 255]);
    }
    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: [u8; 3]) {
        for py in y..(y + h).min(self.image.height()) {
            for px in x..(x + w).min(self.image.width()) {
                self.image.put_pixel(px, py, Rgba([color[0], color[1], color[2], 255]));
            }
        }
    }
    fn dot(&mut self, x: i32, y: i32, r: i32, color: [u8; 3]) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r
                    && x + dx >= 0
                    && y + dy >= 0
                    && (x + dx) < self.image.width() as i32
                    && (y + dy) < self.image.height() as i32
                {
                    self.image
                        .put_pixel((x + dx) as u32, (y + dy) as u32, Rgba([color[0], color[1], color[2], 255]));
                }
            }
        }
    }
    fn line(&mut self, a: (i32, i32), b: (i32, i32), color: [u8; 3]) {
        let steps = (a.0 - b.0).abs().max((a.1 - b.1).abs()).max(1).min(10000);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            self.dot((a.0 as f64 + (b.0 - a.0) as f64 * t) as i32, (a.1 as f64 + (b.1 - a.1) as f64 * t) as i32, 1, color);
        }
    }
    fn text(&mut self, text: &str, x: u32, y: u32, size: f32, width: u32, max_lines: usize, color: [u8; 3]) -> u32 {
        let scale = PxScale::from(size);
        let font = self.font.as_scaled(scale);
        let ascent = font.ascent();
        let step = (size * 1.45).ceil() as u32;
        let mut px = x as f32;
        let mut row = 0usize;
        for c in text.chars() {
            let id = font.glyph_id(c);
            let advance = font.h_advance(id);
            if c == '\n' || px + advance > (x + width) as f32 {
                row += 1;
                px = x as f32;
                if row >= max_lines {
                    break;
                }
                if c == '\n' {
                    continue;
                }
            }
            let mut g = id.with_scale(scale);
            g.position = point(px, y as f32 + row as f32 * step as f32 + ascent);
            if let Some(outline) = font.outline_glyph(g) {
                let bounds = outline.px_bounds();
                let img = &mut self.image;
                outline.draw(|gx, gy, a| {
                    let ix = bounds.min.x as i32 + gx as i32;
                    let iy = bounds.min.y as i32 + gy as i32;
                    if ix >= 0 && iy >= 0 && ix < img.width() as i32 && iy < img.height() as i32 {
                        let p = img.get_pixel_mut(ix as u32, iy as u32);
                        for k in 0..3 {
                            p[k] = (p[k] as f32 * (1. - a) + color[k] as f32 * a).round() as u8;
                        }
                    }
                });
            }
            px += advance;
        }
        ((row + 1).min(max_lines) as u32) * step
    }
    fn plot(&mut self, s: &Series, x: u32, y: u32, w: u32, h: u32) {
        self.card(x, y, w, h);
        self.text(&s.title, x + 30, y + 20, 28., w - 60, 1, ACCENT);
        let note = format!("绘图 {} 点 / 原始 {} 点", s.points.len(), s.observed);
        self.text(&note, x + 20, y + 54, 18., w - 40, 1, MUTED);
        if s.points.is_empty() {
            self.text("数据不足或此项目不适用", x + 100, y + h / 2, 23., w - 130, 2, MUTED);
            return;
        }
        let minx = s.points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let maxx = s.points.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max).max(minx + 0.001);
        let mut miny = s.points.iter().map(|p| p[1]).fold(0., f64::min);
        let mut maxy = s.points.iter().map(|p| p[1]).fold(0., f64::max).max(miny + 0.001);
        if s.y_label == "偏差 / ms" {
            let span = miny.abs().max(maxy.abs()).max(5.);
            miny = -span;
            maxy = span;
        }
        let left = x + 76;
        let top = y + 90;
        let pw = w - 108;
        let ph = h - 166;
        for i in 0..=4 {
            let py = top + ph * i / 4;
            self.rect(left, py, pw, 1, [223, 228, 240]);
            let value = maxy - (maxy - miny) * i as f64 / 4.;
            self.text(&format!("{value:.1}"), x + 7, py.saturating_sub(10), 17., 66, 1, MUTED);
        }
        let map = |p: [f64; 2]| {
            ((left as f64 + (p[0] - minx) / (maxx - minx) * pw as f64) as i32, (top as f64 + (maxy - p[1]) / (maxy - miny) * ph as f64) as i32)
        };
        let mut prev = None;
        for p in &s.points {
            let pos = map(*p);
            if !s.scatter {
                if let Some(prev) = prev {
                    self.line(prev, pos, ACCENT);
                }
            }
            self.dot(pos.0, pos.1, if s.scatter { 2 } else { 1 }, ACCENT);
            prev = Some(pos);
        }
        self.text(&format!("{minx:.1}"), left, top + ph + 6, 17., 100, 1, MUTED);
        self.text(&format!("{maxx:.1}"), left + pw - 80, top + ph + 6, 17., 100, 1, MUTED);
        self.text(&format!("{}   |   {}", s.x_label, s.y_label), x + 25, y + h - 34, 18., w - 50, 1, MUTED);
    }
}

pub fn badge(status: &str) -> (&'static str, [u8; 3]) {
    match status {
        "clear" => ("正常", [22, 163, 74]),
        "cheat" => ("存在作弊", [229, 72, 77]),
        "review" => ("存疑", [251, 191, 36]),
        "conflict" => ("数据异常", [107, 117, 146]),
        _ => ("数据不足", [107, 117, 146]),
    }
}
pub fn render(data: &Dataset, result: &Value, analyzer: &Analyzer, control: &Control) -> Result<RgbaImage> {
    let font = FontArc::try_from_slice(include_bytes!("../../../../assets/harmonyos.ttf")).map_err(|_| anyhow::anyhow!("报告字体无效"))?;
    let status = result["status"].as_str().unwrap_or("limited");
    let (label, badge_color) = badge(status);
    let (start, end) = match status {
        "cheat" => ([253u8, 233, 236], [245u8, 194, 200]),
        "review" => ([255u8, 253, 242], [253u8, 245, 218]),
        _ => ([245u8, 248, 255], [236u8, 239, 249]),
    };
    let mut c = Canvas {
        image: RgbaImage::from_fn(1440, 9000, |x, y| {
            let t = ((y as f32 / 9000.) * 0.8 + x as f32 / 1440. * 0.2).clamp(0., 1.);
            Rgba([
                (start[0] as f32 * (1. - t) + end[0] as f32 * t) as u8,
                (start[1] as f32 * (1. - t) + end[1] as f32 * t) as u8,
                (start[2] as f32 * (1. - t) + end[2] as f32 * t) as u8,
                255,
            ])
        }),
        font,
    };
    c.text("PRCA 作弊分析", 48, 45, 51., 1030, 1, ACCENT);
    c.rounded(1160, 48, 232, 70, 35, badge_color);
    let text_width = label.chars().count() as u32 * 34;
    c.text(label, 1160 + (232 - text_width) / 2, 62, 34., 220, 1, [255, 255, 255]);
    let mut y = 132;
    let meta = &data.payload["metadata"];
    y += c.text(meta["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("未记录谱面名称"), 50, y, 30., 1338, 2, INK) + 12;
    let speed = meta["speed"].as_f64().map(|v| format!("{v:.2}×")).unwrap_or("未记录".into());
    let score = data
        .replay
        .meta
        .result
        .as_ref()
        .map(|r| format!("{:07}", r.score))
        .unwrap_or("未记录".into());
    let accuracy = data
        .replay
        .meta
        .result
        .as_ref()
        .map(|r| format!("{:.3}%", r.accuracy * 100.))
        .unwrap_or("未记录".into());
    let combo = data
        .replay
        .meta
        .result
        .as_ref()
        .map(|r| r.max_combo.to_string())
        .unwrap_or("未记录".into());
    let chips = [
        ("玩家", meta["player"].as_str().unwrap_or("未记录").to_owned()),
        ("谱面等级", meta["level"].as_str().unwrap_or("未记录").to_owned()),
        ("录制速度", speed),
        ("分数", score),
        ("准度", accuracy),
        ("最大连击", combo),
    ];
    for (i, (name, value)) in chips.iter().enumerate() {
        let x = 48 + (i % 3) as u32 * 456;
        let py = y + (i / 3) as u32 * 90;
        c.card(x, py, 432, 76);
        c.text(name, x + 22, py + 20, 24., 125, 1, MUTED);
        c.text(value, x + 147, py + 19, 28., 266, 1, ACCENT);
    }
    y += 194;
    if let Some(record) = &data.replay.meta.result {
        c.card(48, y, 1344, 248);
        c.text("判定统计", 78, y + 20, 29., 1200, 1, ACCENT);
        let maximum = record.counts.iter().copied().max().unwrap_or(1).max(1);
        for (i, (name, color)) in [
            ("PERFECT", [43, 85, 232]),
            ("GOOD", [48, 155, 228]),
            ("BAD", [225, 151, 50]),
            ("MISS", [229, 72, 77]),
        ]
        .iter()
        .enumerate()
        {
            let row = y + 75 + i as u32 * 39;
            c.text(name, 80, row, 22., 178, 1, *color);
            c.rounded(270, row + 5, 945, 15, 7, [238, 241, 248]);
            let width = (945. * record.counts[i] as f64 / maximum as f64) as u32;
            if width > 15 {
                c.rounded(270, row + 5, width, 15, 7, *color);
            }
            c.text(&record.counts[i].to_string(), 1240, row, 22., 120, 1, INK);
        }
        y += 272;
    }
    c.card(48, y, 1344, 148);
    c.text("判断说明", 78, y + 18, 28., 1200, 1, ACCENT);
    c.text(result["summary"].as_str().unwrap_or("分析完成"), 78, y + 64, 25., 1280, 2, INK);
    y += 174;
    let info = format!(
        "有效计时组 {}    完整接触 {}    输入事件 {}    回放版本 {}",
        data.payload["compact"]["independent_timing_samples"],
        data.payload["coverage"]["complete_contacts"],
        data.payload["coverage"]["input_events"],
        meta["source_version"]
    );
    y += c.text(&info, 52, y, 22., 1330, 2, MUTED) + 16;
    if let Some(checks) = result["checks"].as_array() {
        let checks = &checks[..checks.len().min(6)];
        let height = 66 + checks.len() as u32 * 68;
        c.card(48, y, 1344, height);
        c.text("检查覆盖", 78, y + 16, 28., 1280, 1, ACCENT);
        for (i, check) in checks.iter().enumerate() {
            let row = y + 65 + i as u32 * 68;
            let (label, color) = match check["status"].as_str().unwrap_or("") {
                "pass" => ("已检查", [22, 163, 74]),
                "review" => ("需复核", [217, 119, 6]),
                "observe" => ("已观测", ACCENT),
                "not_applicable" => ("不适用", MUTED),
                "insufficient" => ("样本少", MUTED),
                "data" => ("需核对", MUTED),
                _ => ("未启用", MUTED),
            };
            c.text(check["name"].as_str().unwrap_or("检查"), 78, row, 23., 300, 1, INK);
            c.text(label, 395, row, 23., 130, 1, color);
            let reason = check["reason"].as_str().unwrap_or("");
            c.text(reason, 540, row, 22., 790, 2, MUTED);
        }
        y += height + 24;
    }
    let graphs: Vec<_> = data.series.iter().filter(|s| s.observed > 0 && !s.points.is_empty()).collect();
    for (i, s) in graphs.iter().enumerate() {
        control.progress(i, graphs.len())?;
        c.plot(s, 48, y, 1344, 330);
        y += 352;
    }
    if graphs.is_empty() {
        c.card(48, y, 1344, 120);
        c.text("回放没有足够的操作数据，无法生成有效曲线。", 78, y + 36, 26., 1260, 2, MUTED);
        y += 144;
    }
    let evidence = result["evidence"].as_array().unwrap();
    let mut rows: Vec<(String, String)> = evidence
        .iter()
        .map(|e| {
            (
                format!("{:.2}–{:.2} 秒  {}", e["start"].as_f64().unwrap_or(0.), e["end"].as_f64().unwrap_or(0.), e["title"].as_str().unwrap_or("")),
                e["detail"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    if rows.is_empty() && status != "clear" {
        rows.push(("数据说明".into(), result["summary"].as_str().unwrap_or("数据不足").into()));
    }
    for (title, detail) in rows {
        control.check()?;
        c.card(48, y, 1344, 218);
        c.text(&title, 78, y + 22, 27., 1280, 2, if status == "cheat" { [229, 72, 77] } else { [217, 119, 6] });
        c.text(&detail, 78, y + 86, 23., 1280, 3, INK);
        y += 242;
    }
    let q = data
        .quality
        .iter()
        .filter(|s| !s.contains("第三方认证"))
        .take(6)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let quality_height = c.text(&q, 54, y + 14, 20., 1328, 12, MUTED);
    y += quality_height + 40;
    c.rect(48, y, 1344, 1, [220, 226, 238]);
    y += 22;
    y += c.text(&format!("PRCA {}    作者 Kenari_15", analyzer.version()), 54, y, 21., 1328, 1, MUTED) + 10;
    y += c.text("正常表示可用项目未发现异常。存疑需要具体证据；数据不足与数据异常不等于作弊嫌疑。", 54, y, 20., 1328, 2, MUTED) + 25;
    anyhow::ensure!(y < 8950, "报告超过布局上限，未保存截断图片");
    Ok(image::imageops::crop_imm(&c.image, 0, 0, 1440, y).to_image())
}
