use super::{animation as a, geometry as g, *};
use crate::{
    ext::{semi_black, ScaleType},
    judge::icon_index,
};

pub fn draw(ui: &mut Ui, c: &FrameContext, options: &PresentationOptions, info: &ChartInfo, assets: &PresentationAssets, result: &PlayResult) {
    let time = a::ending_time(c.stage_time - c.delta, c.stage_duration);
    if time < 0. {
        return;
    }
    let display = &options.result;
    let score = if display.custom { display.score } else { result.score };
    let accuracy = if display.custom { display.accuracy } else { result.accuracy * 100. };
    let combo = if display.custom { display.max_combo } else { result.max_combo };
    let counts = if display.custom {
        display.counts
    } else {
        [result.counts[0] + result.counts[4], result.counts[1], result.counts[2], result.counts[3]]
    };
    for (x, y, w, h, padding, start, end) in [
        (0.535, -0.1748, 0.43078, 0.18656, 1838., 0.1, 1.),
        (0.513, 0.011, 0.40313, 0.08227, 1720., 0.2, 1.1),
        (0.4917, 0.0918, 0.40313, 0.08227, 1720., 0.25, 1.3),
    ] {
        ui.scope(|ui| {
            ui.dx(2. * a::result_slide(time, start, end));
            g::panel(ui, g::centered(ui.top, x, y, w, h), padding);
        });
    }
    let art_slide = a::result_slide(time, 0., 1.);
    ui.scope(|ui| {
        ui.dx(art_slide * 2.);
        let art = g::centered(ui.top, 0.05371, -0.1631, 0.5209, 0.3248);
        for i in (1..=8).rev() {
            let margin = i as f32 * 0.0018;
            g::slanted(ui, Rect::new(art.x - margin, art.y - margin, art.w + margin * 2., art.h + margin * 2.), None, semi_black(0.023));
        }
        g::slanted(ui, art, Some(*assets.illustration), WHITE);
        g::bottom_shade(ui, art, 0.25, 0.85);
        let level_w = g::width(ui, assets, &info.level, 0.015).min(0.12);
        g::text(ui, assets, &info.name, 0.07612, 0.68105, 0.5, 0.03543, (0.3844 - level_w).max(0.1), vec2(0., 1.), WHITE);
        g::text(ui, assets, &info.level, 0.4823, 0.69125, 0.5, 0.015, 0.12, vec2(1., 1.), WHITE);
    });
    let aspect = 1. / ui.top;
    let player_scale = ((aspect - 4. / 3.) * 0.1321 / (4. / 9.) + 1.) * (4. / 3. / aspect);
    let player_alpha = a::result_alpha(time, 0.6, 1.1);
    ui.alpha(player_alpha, |ui| {
        let name = if display.player_name.is_empty() {
            "Player"
        } else {
            &display.player_name
        };
        let p = g::point(ui.top, 0.80367 - (player_scale - 1.) * 0.0127, 0.07104 + (player_scale - 1.) * 0.01609954, 0.);
        // Keep the right-anchored player block at a stable physical height.
        let x = 1. + (p.x - 1.) * player_scale;
        let y = -ui.top + (p.y + ui.top) * player_scale;
        let name_width = g::width(ui, assets, name, 0.02852 * player_scale).min(0.3032 * player_scale);
        let block = player_plate(ui.top, player_scale, (x + 1.) / 2., name_width);
        g::slanted(ui, block, None, BLACK);
        let avatar = g::rect(ui.top, 1. - player_scale * 0.1935, player_scale * 0.014, player_scale * 0.13055, player_scale * 0.07453);
        g::slanted(ui, avatar, Some(*assets.avatar), WHITE);
        g::text(
            ui,
            assets,
            name,
            (x + 1.) / 2.,
            (y + ui.top) / 1.5,
            0.,
            0.02852 * player_scale,
            0.3032 * player_scale,
            vec2(1., assets.font_line_height * 0.5),
            WHITE,
        );
        let (r, badge) = rating_rects(ui.top, player_scale);
        if let Some(rks) = display.rks {
            g::slanted(ui, r, None, WHITE);
            let pos = player_point(ui.top, player_scale, 0.96007, 0.09245 + (player_scale - 1.) * 0.010234385);
            g::text(
                ui,
                assets,
                &format!("{rks:.2}"),
                (pos.x + 1.) / 2.,
                (pos.y + ui.top) / 1.5,
                0.,
                0.01813 * player_scale,
                0.055 * player_scale,
                vec2(0.5, assets.font_line_height * 0.5),
                Color::from_rgba(50, 50, 50, 255),
            );
        }
        if !display.challenge.is_empty() {
            let colors = [WHITE, GREEN, SKYBLUE, RED, GOLD, Color::new(0.65, 0.9, 1., 1.)];
            let color = display.challenge_color.min(5);
            let texture = color.checked_sub(1).and_then(|index| assets.challenge_frames[index].as_ref());
            if let Some(texture) = texture {
                // These dimensions describe the complete asset, including glow.
                ui.fill_rect(badge, (**texture, badge, ScaleType::Fit));
            } else {
                g::slanted(ui, challenge_core(badge), None, colors[color]);
            }
            let pos = player_point(ui.top, player_scale, 0.9653, 0.05672 + (player_scale - 1.) * 0.029938875);
            g::text(
                ui,
                assets,
                &display.challenge,
                (pos.x + 1.) / 2.,
                (pos.y + ui.top) / 1.5,
                0.,
                0.02175 * player_scale,
                0.038 * player_scale,
                vec2(0.5, 1.),
                if texture.is_some() { WHITE } else { BLACK },
            );
        }
    });
    let score_alpha = a::result_alpha(time, 0.6, 0.9);
    ui.scope(|ui| {
        ui.dx(2. * a::result_slide(time, 0.1, 1.));
        ui.alpha(score_alpha, |ui| {
            g::text(ui, assets, &format!("{score:07}"), 0.57542, 0.43622, 0.5, 0.04023, 0.225, vec2(0., 1.), WHITE);
            let label = "AUTOPLAY";
            let label_w = g::width(ui, assets, label, 0.01189);
            g::text(ui, assets, label, 0.58356, 0.46214, 0.5, 0.01189, 0.1, vec2(0., 0.5), WHITE);
            g::text(ui, assets, &display.score_increase, 0.59166 + label_w, 0.46214, 0.5, 0.01189, 0.2, vec2(0., 0.5), WHITE);
        });
    });
    ui.scope(|ui| {
        ui.dx(2. * a::result_slide(time, 0.2, 1.1));
        ui.alpha(a::result_alpha(time, 0.9, 1.2), |ui| {
            for (value, label, x, align, em) in [
                (combo.to_string(), "Max Combo", 0.55252, 0., 0.02509),
                (format!("{accuracy:.2}%"), "Accuracy", 0.88309, 1., 0.02403),
            ] {
                g::text(ui, assets, &value, x, 0.55464, 0.5, em, 0.2, vec2(align, 0.5), WHITE);
                g::text(ui, assets, label, x, 0.58169, 0.5, 0.0119, 0.2, vec2(align, 0.5), WHITE);
            }
        });
    });
    ui.scope(|ui| {
        ui.dx(2. * a::result_slide(time, 0.25, 1.3));
        ui.alpha(a::result_alpha(time, 1.2, 1.5), |ui| {
            for (i, label) in ["Perfect", "Good", "Bad", "Miss"].into_iter().enumerate() {
                let x = [0.55145, 0.6246, 0.67672, 0.72884][i];
                g::text(ui, assets, &counts[i].to_string(), x, 0.66644, 0.5, 0.02088, 0.067, vec2(0.5, 0.5), WHITE);
                g::text(ui, assets, label, x, 0.69089, 0.5, 0.0082, 0.067, vec2(0.5, 0.5), WHITE);
            }
            for (label, value, y) in [("Early", result.early, 0.66707), ("Late", result.late, 0.68526)] {
                g::text(ui, assets, label, 0.7726, y, 0.5, 0.01167, 0.07, vec2(0., 0.5), WHITE);
                g::text(ui, assets, &value.to_string(), 0.8574, y, 0.5, 0.01195, 0.06, vec2(1., 0.5), WHITE);
            }
        });
    });
    let rating_alpha = a::result_alpha(time, 1.3, 1.6);
    let zoom = 1. + (1. - a::in_out_cubic(a::part(time, 1.6, 1.9))) * 0.5;
    let size = vec2(0.1069, 0.10596) * 2. * zoom;
    let center = g::point(ui.top, 0.861, 0.3893067, 0.5);
    let r = Rect::new(center.x - size.x / 2., center.y - size.y / 2., size.x, size.y);
    let icon = &assets.icons[icon_index(score, counts[1..].iter().all(|&v| v == 0))];
    ui.alpha(rating_alpha, |ui| ui.fill_rect(r, (**icon, r, ScaleType::Fit)));
    if !display.collected_text.is_empty() {
        let alpha = a::result_alpha(time, 2.7, 3.2);
        ui.alpha(alpha, |ui| {
            let band = Rect::new(-0.32, ui.top - 0.16, 0.85, 0.11);
            g::slanted(ui, band, None, semi_black(0.5));
            g::text(ui, assets, &display.collected_title, 0.37451, 0.94122, 1., 0.01802, 0.0838, vec2(0., 0.5), WHITE);
            g::text(ui, assets, &display.collected_text, 0.49149, 0.94296, 1., 0.01273, 0.3, vec2(0., 0.5), WHITE);
        });
    }
    let edge_offset = 0.27 * player_scale * (1. - a::in_out_cubic(a::part(time, 0.2, 1.6)));
    ui.scope(|ui| {
        ui.dx(-edge_offset);
        let top = ui.top;
        g::slanted(ui, Rect::new(-1.04, -top, 0.222, 0.128), None, BLACK);
        g::line(ui, vec2(-0.825, -top + 0.012), vec2(-0.86, -top + 0.128), 0.012, WHITE);
        let center = vec2(-0.928, -top + 0.073);
        for i in 0..22 {
            let p = |k: f32| {
                let angle = k / 22. * 5.35 - 1.5;
                center + vec2(angle.cos(), angle.sin()) * 0.027
            };
            g::line(ui, p(i as f32), p(i as f32 + 1.), 0.0015, WHITE);
        }
    });
    ui.scope(|ui| {
        ui.dx(edge_offset);
        let top = ui.top;
        g::slanted(ui, Rect::new(0.812, top - 0.13, 0.21, 0.13), None, BLACK);
        g::line(ui, vec2(0.855, top - 0.13), vec2(0.82, top - 0.012), 0.012, WHITE);
        let x = 0.925;
        let y = top - 0.07;
        ui.fill_rect(Rect::new(x - 0.025, y - 0.002, 0.05, 0.004), WHITE);
        g::line(ui, vec2(x + 0.025, y), vec2(x + 0.006, y - 0.018), 0.003, WHITE);
        g::line(ui, vec2(x + 0.025, y), vec2(x + 0.006, y + 0.018), 0.003, WHITE);
    });
}

/// The right edge stays under the avatar and rating; only the left edge moves.
fn player_plate(top: f32, scale: f32, name_right: f32, fitted_width: f32) -> Rect {
    let left = name_right - fitted_width - 0.025 * scale;
    let mut r = g::rect(top, left, scale * 0.019, 1. - left, scale * 0.064);
    // The plate continues offscreen at the top; its bottom-right corner meets
    // the canvas edge so the right-anchored rating column stays on black.
    r.w += r.h * 0.26f32.tan();
    r
}

fn player_point(top: f32, scale: f32, x: f32, y: f32) -> Vec2 {
    let p = g::point(top, x, y, 0.);
    vec2(1. + (p.x - 1.) * scale, -top + (p.y + top) * scale)
}

fn rating_rects(top: f32, scale: f32) -> (Rect, Rect) {
    (
        g::rect(top, 1. - scale * 0.074, scale * 0.059, scale * 0.0715, scale * 0.02),
        g::rect(top, 1. - scale * 0.06049, scale * 0.0308, scale * 0.05158, scale * 0.02539),
    )
}

fn challenge_core(frame: Rect) -> Rect {
    Rect::new(frame.x + frame.w * 43. / 390., frame.y + frame.h * 46. / 192., frame.w * 304. / 390., frame.h * 100. / 192.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_plate_grows_leftward_and_keeps_padding_for_the_sloping_edge() {
        for scale in [0.7, 0.85, 1., 1.1] {
            let mut previous = f32::INFINITY;
            for width in [0.02, 0.09, 0.2, 0.3032] {
                let r = player_plate(0.5625, scale, 0.83, width * scale);
                assert!(r.x < previous);
                previous = r.x;
                assert!((r.right() - r.h * 0.26f32.tan() - 1.).abs() < 1e-6);
                let text_left = (0.83 - width * scale) * 2. - 1.;
                assert!(r.x + r.h * 0.26f32.tan() < text_left);
                assert!(r.x > -1.);
            }
        }
    }

    #[test]
    fn ratings_and_challenge_glow_stay_inside_the_black_right_edge() {
        for top in [0.375, 0.5625, 0.75, 1.] {
            for scale in [0.7, 0.85, 1., 1.1] {
                let plate = player_plate(top, scale, 0.83, 0.09 * scale);
                let (rks, badge) = rating_rects(top, scale);
                let black_right = |y: f32| plate.right() - (y - plate.y) * 0.26f32.tan();
                assert!(rks.right() < black_right(rks.y));
                assert!(rks.right() - rks.h * 0.26f32.tan() < black_right(rks.bottom()));
                assert!(badge.right() + 0.005 * scale < black_right(badge.bottom()));
                assert!(rks.w > badge.w * 1.35);
                assert!(badge.bottom() < rks.y);
                assert!(rks.y >= plate.y && rks.bottom() <= plate.bottom());
                assert!(rks.right() < 1. && badge.right() < 1.);
            }
        }
    }
}
