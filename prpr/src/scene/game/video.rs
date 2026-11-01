//! Offline-only indicators that remain available when the main HUD is hidden.
use super::*;

impl GameScene {
    pub(super) fn draw_video_hud(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        if !self.hide_ui {
            self.draw_gameplay_ui(tm, ui)?;
        }
        if let Some(early_late) = self.video_indicators {
            let old_top = std::mem::replace(&mut ui.top, 1. / self.res.aspect_ratio);
            let alpha = self.res.alpha * self.video_entrance.map_or(1., |(_, hud)| hud);
            ui.alpha(alpha, |ui| self.draw_video_indicators(ui, early_late));
            ui.top = old_top;
        }
        Ok(())
    }

    fn draw_video_indicators(&self, ui: &mut Ui, early_late: bool) {
        let config = &self.res.config;
        let top = -ui.top;
        let score_top = top + 0.044 / self.res.aspect_ratio;
        let details_top = score_top + 0.07 + if config.show_acc { 0.045 } else { 0. };
        if self.hide_ui {
            if config.offset_indicator {
                draw_offset_indicator(ui, &self.res, self.judge.recent_hits(), top + (-top) / 3.);
            }
            if config.show_acc {
                ui.text(format!("{:05.2}%", self.judge.real_time_accuracy() * 100.))
                    .pos(0.97, score_top + 0.07)
                    .anchor(1., 0.)
                    .size(0.4)
                    .color(Color { a: 0.7, ..WHITE })
                    .draw_using(&PGR_FONT);
            }
            if config.show_judgement_details {
                judgement_details::draw(ui, config.judge_grading, self.judge.counts(), 0.97, details_top, 1.);
            }
        }
        if early_late {
            let result = self.judge.result(false);
            let rows = if config.show_judgement_details {
                config.judge_grading.visible_grades().count()
            } else {
                0
            };
            ui.text(format!("EARLY:{}  LATE:{}", result.early, result.late))
                .pos(0.97, details_top + rows as f32 * 0.043)
                .anchor(1., 0.)
                .size(0.4)
                .color(Color { a: 0.7, ..WHITE })
                .draw_using(&PGR_FONT);
        }
    }
}
