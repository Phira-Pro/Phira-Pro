//! Course-specific assets and presentation; session control stays in course.rs.
use super::*;
use prpr::ui::{parse_font, TextPainter};

pub(super) struct Assets {
    font: TextPainter,
    badges: [SafeTexture; 5],
    ranks: [SafeTexture; 8],
    cancel: SafeTexture,
    confirm: SafeTexture,
}
impl Assets {
    pub(super) async fn load() -> Result<Self> {
        let font = parse_font(load_file("course/Saira-Regular.otf").await?)?;
        let fallback = parse_font(load_file("harmonyos.ttf").await?)?;
        Ok(Self {
            font: TextPainter::new(font, Some(fallback)),
            badges: [
                load_texture("course/rainbow.png").await?.into(),
                load_texture("course/gold.png").await?.into(),
                load_texture("course/red.png").await?.into(),
                load_texture("course/blue.png").await?.into(),
                load_texture("course/green.png").await?.into(),
            ],
            ranks: prpr::core::Resource::load_icons().await?,
            cancel: load_texture("close.png").await?.into(),
            confirm: load_texture("ok.png").await?.into(),
        })
    }
}
/// A badge is awarded independently of the ordinary rank icon.
pub(super) fn badge_index(total: u32) -> Option<usize> {
    match total {
        3_000_000 => Some(0),
        2_940_000..=2_999_999 => Some(1),
        2_850_000..=2_939_999 => Some(2),
        2_700_000..=2_849_999 => Some(3),
        2_460_000..=2_699_999 => Some(4),
        _ => None,
    }
}
pub(super) fn rank_index(total: u32, full_combo: bool) -> usize {
    match badge_index(total) {
        Some(0) => 7,
        Some(1) => {
            if full_combo {
                6
            } else {
                5
            }
        }
        Some(2) => 4,
        Some(3) => 3,
        Some(4) => 2,
        _ => 0,
    }
}
pub(super) fn grade(total: u32) -> &'static str {
    match badge_index(total) {
        Some(0) => "彩色",
        Some(1) => "金色",
        Some(2) => "红色",
        Some(3) => "蓝色",
        Some(4) => "绿色",
        _ => "未达标",
    }
}
pub(super) fn score(stages: &[Stage]) -> u32 {
    stages.iter().map(|s| s.result.score).sum()
}
pub(super) fn all_fc(stages: &[Stage]) -> bool {
    stages.len() == 3
        && stages
            .iter()
            .all(|s| s.result.num_of_notes > 0 && s.result.max_combo == s.result.num_of_notes)
}
pub(super) fn difficulty(stages: &[Stage]) -> u32 {
    stages.iter().map(|s| s.info.difficulty as f64).sum::<f64>().floor() as u32
}
pub(super) fn fade(p: f64) -> f32 {
    let p = p.clamp(0., 1.) as f32;
    p * p * (3. - 2. * p)
}
fn quad(ui: &mut Ui, r: Rect, skew: f32, shading: impl IntoShading) {
    let mut b = ui.builder(shading);
    b.add(r.x + skew, r.y);
    b.add(r.right() + skew, r.y);
    b.add(r.right(), r.bottom());
    b.add(r.x, r.bottom());
    b.triangle(0, 1, 2);
    b.triangle(0, 2, 3);
    b.commit();
}
pub(super) fn results(ui: &mut Ui, assets: &mut Assets, stages: &[Stage], aborted: bool, saved: bool, buttons: &mut [DRectButton; 4], t: f32) {
    std::mem::swap(ui.text_painter, &mut assets.font);
    let area = ui.screen_rect().feather(-0.06);
    buttons[0].render_text(ui, Rect::new(area.x, area.y, 0.22, 0.075), t, "返回", 0.4, false);
    ui.text(if aborted {
        "课题已中断".into()
    } else {
        get_data()
            .me
            .as_ref()
            .map(|me| format!("{} · 课题结算", me.name))
            .unwrap_or("课题结算".into())
    })
    .pos(area.right(), area.y + 0.035)
    .anchor(1., 0.5)
    .size(0.48)
    .draw();
    let screen = ui.screen_rect();
    let rows_top = screen.y + screen.h * 0.17;
    let row_h = screen.h * 0.19;
    let card_h = screen.h * 0.162;
    let gap = row_h - card_h;
    for (i, stage) in stages.iter().enumerate() {
        let y = rows_top + i as f32 * row_h;
        // All six outer edges lie on the same two oblique lines, including the gaps.
        let slope = 0.28;
        let full_height = row_h * 3. - gap;
        let x = area.x + slope * (full_height - (i as f32 * row_h + card_h));
        let r = Rect::new(x, y, area.w - slope * full_height, card_h);
        let skew = card_h * slope;
        quad(ui, r, skew, semi_black(0.65));
        let art = Rect::new(r.x, r.y, r.w * 0.50, r.h);
        quad(ui, art, skew, (*stage.illustration, Rect::new(art.x, art.y, art.w + skew, art.h), ScaleType::CropCenter));
        quad(ui, art, skew, semi_black(0.22));
        ui.text(&stage.info.name)
            .pos(art.x + 0.035, art.bottom() - 0.018)
            .anchor(0., 1.)
            .no_baseline()
            .size(0.62)
            .max_width(art.w - 0.06)
            .draw();
        ui.text(&stage.info.level)
            .pos(art.right() - 0.015, art.y + 0.02)
            .anchor(1., 0.)
            .size(0.32)
            .draw();
        let stats_x = art.right() + 0.055;
        let stats_w = r.right() - stats_x - 0.035;
        ui.text(format!("{:07}", stage.result.score))
            .pos(r.right() - 0.03, r.y + card_h * 0.15)
            .anchor(1., 0.)
            .no_baseline()
            .size(1.1)
            .draw();
        for (j, (label, count)) in ["Perfect", "Good", "Bad", "Miss"].iter().zip(stage.result.counts[..4].iter()).enumerate() {
            let x = stats_x + stats_w * j as f32 / 5.2;
            ui.text(count.to_string())
                .pos(x, r.y + card_h * 0.68)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.50)
                .draw();
            ui.text(*label)
                .pos(x, r.y + card_h * 0.92)
                .anchor(0., 0.5)
                .no_baseline()
                .size(0.29)
                .color(semi_white(0.7))
                .draw();
        }
        let early_late = if card_h < 0.16 {
            format!("E {} · L {}", stage.result.early, stage.result.late)
        } else {
            format!("Early {}\nLate  {}", stage.result.early, stage.result.late)
        };
        ui.text(early_late)
            .pos(r.right() - 0.025, r.y + card_h * if card_h < 0.16 { 0.92 } else { 0.64 })
            .anchor(1., if card_h < 0.16 { 0.5 } else { 0. })
            .no_baseline()
            .size(0.29)
            .multiline()
            .draw();
    }
    let total = score(stages);
    let bottom = screen.y + screen.h * 0.74;
    let total_h = screen.h * 0.115;
    // Continue the exact right-hand edge used by all three cards.
    let right = area.right() - 0.28 * (bottom + total_h - rows_top);
    let total_rect = Rect::new(right - 0.68, bottom, 0.68, total_h);
    quad(ui, total_rect, total_h * 0.28, semi_black(0.55));
    ui.text(format!("{total:07}"))
        .pos(total_rect.right() - 0.035, total_rect.center().y)
        .anchor(1., 0.5)
        .no_baseline()
        .size(1.1)
        .draw();
    let rank = rank_index(total, all_fc(stages));
    let icon = &assets.ranks[rank];
    let icon_rect = Rect::new(0.84, total_rect.center().y, 0., 0.).feather((screen.h * 0.080).min(0.13));
    ui.fill_rect(icon_rect, (**icon, icon_rect, ScaleType::Fit));
    let button_y = screen.bottom() - 0.105;
    buttons[1].render_text(ui, Rect::new(area.x, button_y, 0.25, 0.065), t, "重新挑战", 0.4, false);
    if stages.len() == 3 && !aborted {
        buttons[3].render_text(ui, Rect::new(area.x + 0.27, button_y, 0.25, 0.065), t, if saved { "已保存" } else { "保存成绩" }, 0.4, saved);
    }
    if stages.len() == 3 && !aborted {
        buttons[2].render_text(ui, Rect::new(area.right() - 0.20, button_y, 0.20, 0.065), t, "继续", 0.4, true);
    }
    std::mem::swap(ui.text_painter, &mut assets.font);
}
pub(super) fn award(ui: &mut Ui, assets: &mut Assets, stages: &[Stage], buttons: &mut [DRectButton; 4], t: f32) {
    crate::scene::TEX_BACKGROUND_BLUR.with(|slot| {
        if let Some(background) = slot.borrow().as_ref() {
            ui.fill_rect(ui.screen_rect(), (**background, ui.screen_rect(), ScaleType::CropCenter));
        }
    });
    ui.fill_rect(ui.screen_rect(), semi_black(0.45));
    if let Some(index) = badge_index(score(stages)) {
        let badge = &assets.badges[index];
        // These original textures share a 60 × 20 badge, but only some include glow padding.
        // Use a common pixel scale so the glowing badges do not become smaller than blue/green.
        let pixel = 0.26 / 60.;
        let size = vec2(badge.width(), badge.height()) * pixel;
        let r = Rect::new(-size.x / 2., 0.012 - (size.y - 20. * pixel) / 2., size.x, size.y);
        ui.fill_rect(r, (**badge, r, ScaleType::Fit));
        ui.text(difficulty(stages).to_string())
            .pos(0., 0.052)
            .anchor(0.5, 1.)
            .no_baseline()
            .size(2.55)
            .draw_with_font(Some(&mut assets.font));
    }
    for (i, x, icon) in [(0, -0.18, &assets.cancel), (1, 0.18, &assets.confirm)] {
        let hit = Rect::new(x - 0.08, 0.30 - 0.08, 0.16, 0.16);
        buttons[i].build(ui, t, hit, |ui, _| {
            let r = hit.feather(-0.035);
            ui.fill_rect(r, (**icon, r, ScaleType::Fit));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_course_badges_and_rank_boundaries() {
        for (lo, hi, badge, rank) in [
            (0, 2_459_999, None, 0),
            (2_460_000, 2_699_999, Some(4), 2),
            (2_700_000, 2_849_999, Some(3), 3),
            (2_850_000, 2_939_999, Some(2), 4),
            (2_940_000, 2_999_999, Some(1), 5),
            (3_000_000, 3_000_000, Some(0), 7),
        ] {
            for score in [lo, hi] {
                assert_eq!(badge_index(score), badge);
                assert_eq!(rank_index(score, false), rank);
                assert_eq!(rank_index(score, true), if badge == Some(1) { 6 } else { rank });
            }
        }
    }
}
