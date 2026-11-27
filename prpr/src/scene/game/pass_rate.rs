use super::L10N_LOCAL;
use crate::{
    config::Config,
    ext::RectExt,
    judge::Judgement,
    pass_rate::{add_node, move_node, Attempt, PassRate, Segment},
    practice::format_time,
    time::TimeManager,
    ui::{DRectButton, Ui},
};
use macroquad::prelude::*;
use std::path::PathBuf;

pub(super) struct Session {
    pub data: Option<PassRate>,
    writer: Option<PathBuf>,
    attempt: Option<Attempt>,
}

impl Session {
    pub fn new(config: &Config, record: bool) -> Self {
        let path = config.pass_rate_path.as_ref();
        let data = path.and_then(|path| match PassRate::load(path) {
            Ok(data) => data,
            Err(err) => {
                tracing::warn!(?err, "could not load segment pass rates");
                None
            }
        });
        let writer = path.filter(|_| record && !config.autoplay()).cloned();
        Self { data, writer, attempt: None }
    }

    pub fn recording(&self) -> bool {
        self.attempt.is_some()
    }

    pub fn begin(&mut self) {
        self.attempt = (self.writer.is_some() && self.data.is_some()).then(Attempt::default);
    }

    pub fn advance(&mut self, time: f64) {
        if let Some(attempt) = &mut self.attempt {
            attempt.advance(time);
        }
    }

    pub fn start_at(&mut self, time: f64) {
        if let Some(attempt) = &mut self.attempt {
            attempt.start_at(time);
        }
    }

    pub fn mark(&mut self, time: f64, grade: Judgement) {
        let (Some(data), Some(attempt)) = (&self.data, &mut self.attempt) else {
            return;
        };
        let state = match grade {
            Judgement::Perfect | Judgement::PerfectPlus => 0,
            Judgement::Bad | Judgement::Miss => 2,
            _ => 1,
        };
        attempt.mark(data.index(time), state);
    }

    pub fn finish(&mut self) {
        if let (Some(attempt), Some(path), Some(data)) = (self.attempt.take(), &self.writer, &mut self.data) {
            if attempt.finish(data) {
                if let Err(err) = data.save(path) {
                    tracing::warn!(?err, "could not save segment pass rates");
                    crate::scene::show_error(err);
                }
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.finish();
    }
}

fn rates(segment: Segment) -> Option<(String, String)> {
    let fc = format!("{:.1}%", segment.fc_rate?);
    let ap = format!("{:.1}%", segment.ap_rate?);
    Some((tl!("pass-rate-fc", "rate" => fc), tl!("pass-rate-ap", "rate" => ap)))
}

pub(super) fn draw_hud(ui: &mut Ui, segment: Segment, pause: Rect, color: Color) {
    let (fc, ap) = rates(segment).unwrap_or_else(|| {
        let unavailable = tl!("pass-rate-empty");
        (tl!("pass-rate-fc", "rate" => unavailable.as_ref()), tl!("pass-rate-ap", "rate" => unavailable.as_ref()))
    });
    for (i, text) in [fc, ap].into_iter().enumerate() {
        ui.text(text)
            .pos(pause.x, pause.bottom() + 0.018 + i as f32 * 0.043)
            .size(0.4)
            .color(Color { a: color.a * 0.7, ..color })
            .draw_using(&crate::core::PGR_FONT);
    }
}

pub(super) enum Action {
    None,
    Exit,
    Save,
    Preview,
}

pub(super) struct Editor {
    duration: f64,
    nodes: Vec<f64>,
    undo: Vec<Vec<f64>>,
    selected: Option<usize>,
    region: usize,
    cursor: f64,
    pub playing: bool,
    drag: Option<(u64, usize, f32, Vec<f64>)>,
    timeline: Rect,
    buttons: [DRectButton; 6],
}

impl Editor {
    pub fn new(duration: f64, data: Option<&PassRate>) -> Self {
        let duration = duration.max(0.001);
        let nodes = data
            .filter(|data| (data.duration - duration).abs() <= 0.001)
            .map_or_else(Vec::new, |data| data.nodes.clone());
        Self {
            duration,
            nodes,
            undo: Vec::new(),
            selected: None,
            region: 0,
            cursor: 0.,
            playing: false,
            drag: None,
            timeline: Rect::default(),
            buttons: std::array::from_fn(|_| DRectButton::new().with_radius(0.009)),
        }
    }

    pub fn time(&self) -> f64 {
        self.cursor
    }

    pub fn seek_preview(&mut self, time: f64) {
        self.cursor = time.clamp(0., self.duration);
        self.region = self.nodes.partition_point(|node| *node <= self.cursor);
    }

    pub fn preview_ended(&self) -> bool {
        self.cursor >= self.duration - 0.03
    }

    pub fn suspend(&mut self) {
        self.playing = false;
        if let Some((_, index, _, before)) = self.drag.take() {
            self.nodes = before;
            self.cursor = self.nodes[index];
        }
    }

    pub fn save(&self, session: &mut Session, config: &Config) -> anyhow::Result<()> {
        let path = config
            .pass_rate_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No PassRate storage path"))?;
        if session
            .data
            .as_ref()
            .is_some_and(|data| data.nodes == self.nodes && (data.duration - self.duration).abs() <= 0.001)
        {
            return Ok(());
        }
        let data = PassRate::new(self.duration, self.nodes.clone());
        data.save(path)?;
        session.data = Some(data);
        Ok(())
    }

    fn remember(&mut self, before: Vec<f64>) {
        if before != self.nodes {
            self.undo.push(before);
        }
    }

    pub fn touch(&mut self, touch: &Touch, tm: &TimeManager) -> Action {
        let t = tm.real_time() as f32;
        if let Some((id, index, start_x, before)) = &self.drag {
            if touch.id == *id {
                let index = *index;
                match touch.phase {
                    TouchPhase::Cancelled => {
                        self.nodes = self.drag.take().unwrap().3;
                        self.cursor = self.nodes[index];
                    }
                    TouchPhase::Moved => {
                        let time = before[index] + f64::from((touch.position.x - *start_x) / self.timeline.w) * self.duration;
                        self.nodes[index] = move_node(&self.nodes, self.duration, index, time);
                        self.cursor = self.nodes[index];
                    }
                    TouchPhase::Ended => {
                        let before = self.drag.take().unwrap().3;
                        self.remember(before);
                    }
                    _ => {}
                }
            }
            return Action::None;
        }
        if touch.phase == TouchPhase::Started && self.timeline.w > 0. && self.timeline.feather(0.024).contains(touch.position) {
            self.playing = false;
            let time = f64::from((touch.position.x - self.timeline.x) / self.timeline.w).clamp(0., 1.) * self.duration;
            self.selected = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| (*node - time).abs() / self.duration * f64::from(self.timeline.w) <= 0.026)
                .min_by(|(_, a), (_, b)| (*a - time).abs().total_cmp(&(*b - time).abs()))
                .map(|(i, _)| i);
            self.cursor = time;
            self.region = self.nodes.partition_point(|node| *node <= time);
            if let Some(index) = self.selected {
                self.cursor = self.nodes[index];
                self.region = index + 1;
                self.drag = Some((touch.id, index, touch.position.x, self.nodes.clone()));
            }
            return Action::None;
        }
        if self.buttons[0].touch(touch, t) {
            return Action::Exit;
        }
        if self.buttons[5].touch(touch, t) {
            return Action::Preview;
        }
        if let Some((index, time)) = add_node(&self.nodes, self.duration) {
            if self.buttons[1].touch(touch, t) {
                self.playing = false;
                let before = self.nodes.clone();
                self.nodes.insert(index, time);
                self.remember(before);
                self.selected = None;
                self.cursor = time;
                self.region = index + 1;
            }
        }
        if self.selected.is_some() && self.buttons[2].touch(touch, t) {
            self.playing = false;
            if let Some(index) = self.selected.take() {
                let before = self.nodes.clone();
                self.nodes.remove(index);
                self.remember(before);
                self.region = index;
            }
        }
        if !self.undo.is_empty() && self.buttons[3].touch(touch, t) {
            self.playing = false;
            if let Some(before) = self.undo.pop() {
                self.nodes = before;
                self.selected = None;
                self.region = self.nodes.partition_point(|node| *node <= self.cursor);
            }
        }
        if self.buttons[4].touch(touch, t) {
            return Action::Save;
        }
        Action::None
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, data: Option<&PassRate>) {
        let screen = ui.screen_rect();
        let h = (screen.h - 0.06).min(0.78);
        let y = -h / 2.;
        let panel = Rect::new(-0.88, y, 1.76, h);
        ui.fill_rect(screen, Color::new(0., 0., 0., 0.52));
        ui.fill_path(&panel.rounded(0.018), Color::from_rgba(26, 30, 38, 244));
        let tracks = data.filter(|data| data.nodes == self.nodes && (data.duration - self.duration).abs() <= 0.001);
        let text = |ui: &mut Ui, value: String, x, y, size, width, color| {
            ui.text(value)
                .pos(x, y)
                .size(size)
                .max_width(width)
                .anchor(0.5, 0.5)
                .no_baseline()
                .color(color)
                .draw();
        };
        self.buttons[0].render_text(ui, Rect::new(-0.82, y + h * 0.07, 0.09, h * 0.12), t, "‹", 0.55, false);
        self.buttons[5].render_text(
            ui,
            Rect::new(0.63, y + h * 0.07, 0.19, h * 0.12),
            t,
            if self.playing {
                tl!("pass-rate-pause")
            } else {
                tl!("pass-rate-preview")
            },
            0.34,
            false,
        );
        text(ui, tl!("pass-rate-title").into_owned(), 0., y + h * 0.12, 0.66, 1.2, WHITE);
        text(ui, format_time(self.cursor), 0., y + h * 0.25, 0.50, 0.5, WHITE);
        self.timeline = Rect::new(-0.78, y + h * 0.38, 1.56, h * 0.22);
        let r = self.timeline;
        let x_at = |time: f64| r.x + (time / self.duration) as f32 * r.w;
        let green = Color::from_rgba(53, 197, 134, 255);
        let mut start = 0.;
        for (index, end) in self.nodes.iter().copied().chain(std::iter::once(self.duration)).enumerate() {
            let cell = Rect::new(x_at(start), r.y, x_at(end) - x_at(start), r.h);
            let shade = if index == self.region {
                Color::from_rgba(55, 83, 83, 255)
            } else {
                Color::from_rgba(42, 50, 63, 255)
            };
            ui.fill_rect(cell, shade);
            let width = (cell.w - 0.022).max(0.);
            if width >= 0.065 {
                if let Some((fc, ap)) = tracks.and_then(|data| rates(data.segments[index])) {
                    text(ui, fc, cell.center().x, r.y + r.h * 0.31, 0.34, width, WHITE);
                    text(ui, ap, cell.center().x, r.y + r.h * 0.67, 0.34, width, WHITE);
                } else {
                    text(ui, tl!("pass-rate-empty").into_owned(), cell.center().x, cell.center().y, 0.32, width, GRAY);
                }
            }
            start = end;
        }
        ui.fill_rect(Rect::new(x_at(self.cursor) - 0.001, r.y - 0.01, 0.002, r.h + 0.02), WHITE);
        for (index, &time) in self.nodes.iter().enumerate() {
            let x = x_at(time);
            ui.fill_rect(Rect::new(x - 0.002, r.y, 0.004, r.h), green);
            if self.selected == Some(index) {
                ui.fill_circle(x, r.y, 0.027, WHITE);
            }
            ui.fill_circle(x, r.y, 0.022, green);
            text(ui, (index + 1).to_string(), x, r.y, 0.34, 0.043, BLACK);
        }
        let bounds_y = y + h * 0.66;
        text(ui, format_time(0.), r.x + 0.08, bounds_y, 0.34, 0.22, GRAY);
        text(ui, format_time(self.duration), r.right() - 0.08, bounds_y, 0.34, 0.22, GRAY);
        let selection = tracks
            .and_then(|data| rates(data.segments[self.region]))
            .map_or_else(|| tl!("pass-rate-empty").into_owned(), |(fc, ap)| format!("{fc}    {ap}"));
        text(ui, tl!("pass-rate-region", "number" => self.region + 1, "rates" => selection), 0., y + h * 0.75, 0.38, 1.32, WHITE);
        let buttons = [
            (tl!("pass-rate-add"), add_node(&self.nodes, self.duration).is_some()),
            (tl!("pass-rate-delete"), self.selected.is_some()),
            (tl!("pass-rate-undo"), !self.undo.is_empty()),
            (tl!("offset-save"), true),
        ];
        for (index, (label, enabled)) in buttons.into_iter().enumerate() {
            ui.alpha(if enabled { 1. } else { 0.36 }, |ui| {
                self.buttons[index + 1].render_text(
                    ui,
                    Rect::new(-0.57 + index as f32 * 0.30, y + h * 0.83, 0.24, h * 0.105),
                    t,
                    label,
                    0.42,
                    index == 3,
                );
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_exit_and_drop_only_persist_each_attempt_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chart.passrate");
        PassRate::new(25., vec![5., 10.]).save(&path).unwrap();
        let mut config = Config::default();
        config.pass_rate_path = Some(path.clone());
        let mut session = Session::new(&config, true);
        session.begin();
        session.advance(4.);
        session.mark(3., Judgement::Good);
        session.finish();
        session.advance(7.);
        session.finish();
        session.begin();
        session.start_at(5.);
        session.advance(12.);
        session.mark(8., Judgement::Miss);
        drop(session);
        let data = PassRate::load(&path).unwrap().unwrap();
        assert_eq!(data.segments.iter().map(|s| s.attempts).collect::<Vec<_>>(), [1, 1, 1]);
        assert_eq!(data.segments[0].fc_rate, Some(100.));
        assert_eq!(data.segments[0].ap_rate, Some(0.));
        assert_eq!(data.segments[1].fc_rate, Some(0.));
        assert_eq!(data.segments[2].ap_rate, Some(100.));
        let mut editor = Editor::new(25., Some(&data));
        let mut preview = Session::new(&config, false);
        editor.save(&mut preview, &config).unwrap();
        assert_eq!(PassRate::load(&path).unwrap().unwrap().segments[0].attempts, 1);
        editor.nodes[0] = 6.;
        editor.save(&mut preview, &config).unwrap();
        let reset = PassRate::load(&path).unwrap().unwrap();
        assert_eq!(reset.nodes, [6., 10.]);
        assert!(reset
            .segments
            .iter()
            .all(|s| s.attempts == 0 && s.fc_rate.is_none() && s.ap_rate.is_none()));
    }

    #[test]
    #[ignore = "requires desktop OpenGL"]
    fn pass_rate_editor_controls() {
        std::env::set_current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        macroquad::Window::from_config(
            Conf {
                window_width: 960,
                window_height: 540,
                headless: true,
                ..Default::default()
            },
            async {
                let font = crate::ui::FontArc::try_from_vec(std::fs::read("assets/harmonyos.ttf").unwrap()).unwrap();
                let mut painter = crate::ui::TextPainter::new(font, None);
                let data = PassRate::new(12., vec![3.]);
                let mut editor = Editor::new(12., Some(&data));
                let tm = TimeManager::default();
                let render = |editor: &mut Editor, painter: &mut crate::ui::TextPainter| {
                    editor.render(&mut Ui::new(painter, None), 0., Some(&data));
                };
                let touch = |editor: &mut Editor, position, phase| {
                    editor.touch(
                        &Touch {
                            id: 1,
                            position,
                            phase,
                            time: 0.,
                        },
                        &tm,
                    )
                };
                let click = |editor: &mut Editor, position| {
                    touch(editor, position, TouchPhase::Started);
                    touch(editor, position, TouchPhase::Ended)
                };
                render(&mut editor, &mut painter);
                let controls = 0.78 * (0.83 + 0.105 / 2. - 0.5);
                click(&mut editor, vec2(-0.15, controls));
                assert_eq!(editor.nodes, [3.]); // Delete is disabled before selection.
                click(&mut editor, vec2(-0.45, controls));
                assert_eq!(editor.nodes, [3., 4.]);
                let r = editor.timeline;
                let point = |time: f32| vec2(r.x + time / 12. * r.w, r.y);
                touch(&mut editor, point(4.), TouchPhase::Started);
                touch(&mut editor, point(2.), TouchPhase::Moved);
                touch(&mut editor, point(2.), TouchPhase::Ended);
                assert_eq!(editor.nodes, [3., 4.]);
                touch(&mut editor, point(4.), TouchPhase::Started);
                touch(&mut editor, point(6.), TouchPhase::Moved);
                touch(&mut editor, point(6.), TouchPhase::Ended);
                assert!((editor.nodes[1] - 6.).abs() < 1e-5);
                click(&mut editor, vec2(-0.15, controls));
                assert_eq!(editor.nodes, [3.]);
                click(&mut editor, vec2(0.15, controls));
                assert_eq!(editor.nodes.len(), 2);
                render(&mut editor, &mut painter);
                assert!(matches!(click(&mut editor, vec2(0.45, controls)), Action::Save));
            },
        );
    }
}
