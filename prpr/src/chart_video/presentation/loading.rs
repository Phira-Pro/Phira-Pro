use super::{animation, geometry as g, *};

pub fn draw(ui: &mut Ui, c: &FrameContext, options: &PresentationOptions, info: &ChartInfo, assets: &PresentationAssets) {
    let frame = animation::loading(c.stage_time, c.stage_duration, c.delta);
    let dx = 4. * frame.slide;
    let card = g::centered(ui.top, 0.437, -0.167, 0.50781, 0.26779);
    ui.scope(|ui| {
        ui.dx(dx);
        g::slanted(ui, card, Some(*assets.illustration), WHITE);
        let shadow_skew = 0.0758 * 0.26f32.tan();
        let shadow = g::centered(ui.top, 0.0837 - shadow_skew, -0.094, 0.346 + 2. * shadow_skew, 0.0758);
        g::slanted(ui, shadow, None, Color::new(0., 0., 0., 160. / 255.));
        let skew = 0.0821 * 0.26f32.tan();
        let badge = g::centered(ui.top, 0.3627 - skew, -0.0973, 0.056 + 2. * skew, 0.0821);
        g::slanted(ui, badge, None, WHITE);
        g::text(ui, assets, &info.name, 0.09434, 0.40798, 0.5, 0.02786, 0.23133, vec2(0., 0.5), WHITE);
        g::text(ui, assets, &info.composer, 0.09732, 0.44867, 0.5, 0.01387, 0.21916, vec2(0., 0.5), WHITE);
        let level = if options.metadata.level.is_empty() {
            format!("{:.1}", info.difficulty)
        } else {
            options.metadata.level.clone()
        };
        let level_w = g::width(ui, assets, &level, 0.03609).min(0.065);
        g::text(ui, assets, &level, 0.39223, 0.41024, 0.5, 0.03609, 0.065, vec2(0.5, 0.5), Color::from_rgba(50, 50, 50, 255));
        let side = 0.39223 + level_w * 0.445;
        g::text(ui, assets, &options.metadata.superscript, side, 0.39941, 0.5, 0.016, 0.03, vec2(0., 0.5), Color::from_rgba(50, 50, 50, 230));
        g::text(ui, assets, &options.metadata.subscript, side, 0.42648, 0.5, 0.012, 0.03, vec2(0., 0.5), Color::from_rgba(50, 50, 50, 230));
        let difficulty = info.level.split_whitespace().next().unwrap_or("IN");
        g::text(ui, assets, difficulty, 0.39212, 0.44643, 0.5, 0.01228, 0.045, vec2(0.5, 0.5), Color::from_rgba(50, 50, 50, 255));
        for (label, value, lx, ly, x, y, em) in [
            ("Chart", info.charter.as_str(), 0.12624, 0.51251, 0.12562, 0.53209, 0.01024),
            ("Illustration", info.illustrator.as_str(), 0.11042, 0.57117, 0.10952, 0.59131, 0.00998),
        ] {
            g::text(ui, assets, label, lx, ly, 0.5, em, 0.15, vec2(0., 0.5), WHITE);
            g::text(ui, assets, value, x, y, 0.5, 0.01616, 0.26966, vec2(0., 0.5), WHITE);
        }
        ui.alpha(frame.tip, |ui| {
            let tip = if options.metadata.tip.is_empty() {
                info.tip.as_deref().unwrap_or("Tip:")
            } else {
                &options.metadata.tip
            };
            let tip = tip.trim();
            let tip = if tip.starts_with("Tip:") || tip.starts_with("Tip：") {
                tip.to_owned()
            } else {
                format!("Tip: {tip}")
            };
            g::text(ui, assets, &tip, 0.04908, 0.952095, 1., 0.01807, 0.72685, vec2(0., 1.), WHITE);
        });
        let word = if options.metadata.loading_text.is_empty() {
            "Loading..."
        } else {
            &options.metadata.loading_text
        };
        g::text(ui, assets, word, 0.89605, 0.93771, 1., 0.01609, 0.09, vec2(0.5, 0.5), WHITE);
        let (left, right) = animation::loading_wipe(c.stage_time);
        if right > left {
            let r = Rect::new((0.8441 + 0.1039 * left) * 2. - 1., ui.top - 0.0569 * 2., 0.1039 * 2. * (right - left), 0.025 * 2.);
            ui.scissor(r, |ui| {
                ui.fill_rect(r, WHITE);
                g::text(ui, assets, word, 0.89605, 0.93771, 1., 0.01609, 0.09, vec2(0.5, 0.5), BLACK);
            });
        }
    });
}
