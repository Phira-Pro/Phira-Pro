use super::{
    draw_disabled_zones, draw_zones_with_touches, BlockArea, BlockRotateEvent, BpmList, Effect, JudgeLine, JudgeLineKind, Matrix, Point, Resource,
    UIElement, Vector, Zone,
};
use crate::{core::Object, fs::FileSystem, judge::JudgeStatus, ui::Ui};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use nalgebra::Rotation2;
use sasa::AudioClip;
use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
struct BlockFrame {
    timeline: super::block_timeline::BlockTimeline,
    key: Option<(f64, f32, usize)>,
    zones: Vec<Zone>,
    marker_key: Option<(f64, f32)>,
    marker_areas: Vec<BlockArea>,
}

#[derive(Default)]
pub struct ChartExtra {
    pub effects: Vec<Effect>,
    pub global_effects: Vec<Effect>,
    #[cfg(feature = "video")]
    pub videos: Vec<(super::Video, Option<super::VideoAttach>)>,
}

#[derive(Default)]
pub struct ChartSettings {
    pub pe_alpha_extension: bool,
    pub hold_partial_cover: bool,
}

pub type HitSoundMap = HashMap<String, AudioClip>;

pub struct Chart {
    pub offset: f32,
    pub lines: Vec<JudgeLine>,
    pub bpm_list: RefCell<BpmList>,

    pub settings: ChartSettings,
    pub extra: ChartExtra,

    /// Line order according to z-index, lines with attach_ui will be removed from this list
    ///
    /// Store the index of the line in z-index ascending order
    pub order: Vec<usize>,
    /// TODO: docs from RPE
    pub attach_ui: [Option<usize>; 7],

    pub hitsounds: HitSoundMap,

    /// Phigros 9th-chapter touch-blocking zones (`blockAreaList`).
    pub block_areas: Vec<BlockArea>,
    /// Chart-space positions of touches blocked by the zones this frame.
    pub blocked_touches: Vec<(u64, Vector)>,
    block_frame: RefCell<BlockFrame>,
    rpe_block_markers: Vec<super::rpe_block::Marker>,
    rpe_input_lines: Vec<usize>,
    line_transforms: Vec<Matrix>,
    line_rotations: Vec<f32>,
}

impl Chart {
    pub fn new(offset: f32, lines: Vec<JudgeLine>, bpm_list: BpmList, settings: ChartSettings, extra: ChartExtra, hitsounds: HitSoundMap) -> Self {
        let mut attach_ui = [None; 7];
        let mut order = (0..lines.len())
            .filter(|it| {
                if let Some(element) = lines[*it].attach_ui {
                    attach_ui[element as usize - 1] = Some(*it);
                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        order.sort_by_key(|it| (lines[*it].z_index, *it));
        let rpe_block_markers: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(id, line)| match &line.kind {
                JudgeLineKind::Texture(_, path) => {
                    super::rpe_block::marker_kind(path).map(|invert| super::rpe_block::Marker::new(id, invert, &line.object.alpha))
                }
                _ => None,
            })
            .collect();
        let rpe_input_lines = if rpe_block_markers.is_empty() {
            Vec::new()
        } else {
            let parents: Vec<_> = lines.iter().map(|line| line.parent).collect();
            super::rpe_block::input_lines(&parents, rpe_block_markers.iter().map(|marker| marker.line))
        };
        Self {
            offset,
            lines,
            bpm_list: RefCell::new(bpm_list),
            settings,
            extra,

            order,
            attach_ui,

            hitsounds,
            block_areas: Vec::new(),
            blocked_touches: Vec::new(),
            block_frame: RefCell::default(),
            rpe_block_markers,
            rpe_input_lines,
            line_transforms: Vec::new(),
            line_rotations: Vec::new(),
        }
    }

    #[inline]
    pub fn with_element<R>(
        &self,
        ui: &mut Ui,
        res: &Resource,
        element: UIElement,
        scale_point: Option<(f32, f32)>,
        rotation_point: (f32, f32),
        f: impl FnOnce(&mut Ui, Color) -> R,
    ) -> R {
        let scale_point = scale_point.unwrap_or(rotation_point);
        if let Some(id) = self.attach_ui[element as usize - 1] {
            let lines = &self.lines;
            let line = &lines[id];
            let obj = &line.object;
            let mut tr = line.fetch_pos(res, lines);
            tr.y = -tr.y;
            let color = self.lines[id].color.now_opt().unwrap_or(WHITE);
            let scale = obj.now_scale(Vector::new(scale_point.0, scale_point.1));
            let ro =
                Object::new_rotation_wrt_point(Rotation2::new(-obj.rotation.now().to_radians()), Vector::new(rotation_point.0, rotation_point.1));
            ui.with(Matrix::new_translation(&tr) * ro * scale, |ui| ui.alpha(obj.now_alpha().max(0.), |ui| f(ui, color)))
        } else {
            f(ui, WHITE)
        }
    }

    pub async fn load_textures(&mut self, fs: &mut dyn FileSystem) -> Result<()> {
        for line in &mut self.lines {
            if let JudgeLineKind::Texture(tex, path) = &mut line.kind {
                *tex = image::load_from_memory(&fs.load_file(path).await.with_context(|| format!("failed to load illustration {path}"))?)?.into();
            }
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.blocked_touches.clear();
        let cache = self.block_frame.get_mut();
        cache.key = None;
        cache.marker_key = None;
        super::reset_block_effects();
        self.lines
            .iter_mut()
            .flat_map(|it| it.notes.iter_mut())
            .for_each(|note| note.judge = JudgeStatus::NotJudged);
        for line in &mut self.lines {
            line.cache.reset(&mut line.notes);
        }
        #[cfg(feature = "video")]
        for (video, _) in &mut self.extra.videos {
            if let Err(err) = video.reset() {
                use crate::parse::{ptl, L10N_LOCAL};
                crate::scene::show_error(err.context(ptl!("video-load-failed", "path" => video.video_file.path().to_string_lossy())));
            }
        }
    }

    pub fn update(&mut self, res: &mut Resource) {
        for line in &mut self.lines {
            line.object.set_time(res.time);
        }
        if !self.rpe_block_markers.is_empty() {
            let cache = self.block_frame.get_mut();
            cache.key = None;
            cache.marker_key = None;
        }
        self.line_transforms.clear();
        self.line_rotations.clear();
        self.line_transforms
            .extend(self.lines.iter().map(|it| it.now_transform(res, &self.lines)));
        self.line_rotations.extend(self.lines.iter().map(|it| it.fetch_rot(&self.lines)));
        for ((line, tr), rot) in self.lines.iter_mut().zip(&self.line_transforms).zip(&self.line_rotations) {
            line.update(res, *tr, *rot);
        }
        for effect in &mut self.extra.effects {
            effect.update(res);
        }
        #[cfg(feature = "video")]
        for (video, _) in &mut self.extra.videos {
            if let Err(err) = video.update(res.time) {
                tracing::warn!("video error: {err:?}");
            }
        }
    }

    pub(crate) fn has_block_areas(&self) -> bool {
        !self.block_areas.is_empty() || !self.rpe_block_markers.is_empty()
    }

    /// RPE input depends on parents even when they occur later in line order.
    /// Move the judge's existing time evaluation here; it must not run twice.
    pub(crate) fn prepare_rpe_input_time(&mut self, time: f64) -> bool {
        if self.rpe_block_markers.is_empty() {
            return false;
        }
        for line in &mut self.lines {
            line.object.set_time(time);
        }
        true
    }

    /// Input is evaluated before Chart::update. Resolve animated marker lines
    /// (including their parents) at the input time, rather than the last frame.
    pub(crate) fn update_block_input(&mut self, res: &Resource, time: f64) {
        if self.rpe_block_markers.is_empty() {
            return;
        }
        for &id in &self.rpe_input_lines {
            self.lines[id].object.set_time(time);
        }
        let cache = self.block_frame.get_mut();
        cache.key = None;
        cache.marker_key = None;
        self.line_transforms.resize(self.lines.len(), Matrix::identity());
        self.line_rotations.resize(self.lines.len(), 0.);
        for marker in &self.rpe_block_markers {
            let id = marker.line;
            self.line_transforms[id] = self.lines[id].now_transform(res, &self.lines);
            self.line_rotations[id] = self.lines[id].fetch_rot(&self.lines);
        }
    }

    pub fn render(&self, ui: &mut Ui, res: &mut Resource) {
        #[cfg(feature = "video")]
        for (video, attach) in &self.extra.videos {
            if let Some(attach) = attach {
                let line = &self.lines[attach.line];
                let color = line.color.now_opt().unwrap_or(res.judge_line_color);
                let mat = self.lines[attach.line].object.now(res);
                res.apply_model_of(&mat, |res| {
                    video.render(res.time, res.aspect_ratio, color);
                });
            } else {
                video.render(res.time, res.aspect_ratio, WHITE);
            }
        }
        res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(if res.config.flip_x() { -1. } else { 1. }, -1.)), |res| {
            // Native Background sorting layer order 2, judge lines order 3.
            let zones = self.block_zones(res);
            draw_disabled_zones(res, res.aspect_ratio, &zones);
            let mut guard = self.bpm_list.borrow_mut();
            for id in &self.order {
                self.lines[*id].render(ui, res, &self.lines, &mut guard, &self.settings, *id);
            }
            drop(guard);
            res.note_buffer.borrow_mut().draw_all();
            if res.config.sample_count > 1 {
                unsafe { get_internal_gl() }.flush();
                if let Some(target) = &res.chart_target {
                    target.resolve_final();
                }

                unsafe {
                    get_internal_gl().quad_gl.retain_render_pass_on_flush(None);
                }
            }
            if !res.no_effect {
                let render = |res: &mut Resource| {
                    for effect in &self.extra.effects {
                        effect.render(res);
                    }
                };
                if res.config.flip_x() {
                    res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(-1., 1.)), render);
                } else {
                    render(res);
                }
            }
        });
    }

    pub fn render_block_overlay(&self, res: &mut Resource) {
        let flip_x = res.config.flip_x();
        let zones = self.block_zones(res);
        res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(if flip_x { -1. } else { 1. }, -1.)), |res| {
            draw_zones_with_touches(res, res.aspect_ratio, &zones, &self.blocked_touches, flip_x);
        });
    }

    fn block_zones(&self, res: &Resource) -> std::cell::Ref<'_, [Zone]> {
        let key = (res.time, res.aspect_ratio, self.block_areas.len());
        let mut cache = self.block_frame.borrow_mut();
        if cache.key != Some(key) {
            self.update_marker_areas(&mut cache, res.time, res.aspect_ratio);
            let BlockFrame {
                timeline,
                zones,
                marker_areas,
                ..
            } = &mut *cache;
            zones.clear();
            zones.extend(
                timeline
                    .at(&self.block_areas, res.time)
                    .iter()
                    .filter_map(|&id| Zone::from_area(&self.block_areas[id], res.time, res.aspect_ratio)),
            );
            zones.extend(marker_areas.iter().filter_map(|area| Zone::from_area(area, res.time, res.aspect_ratio)));
            cache.key = Some(key);
        }
        drop(cache);
        std::cell::Ref::map(self.block_frame.borrow(), |cache| cache.zones.as_slice())
    }

    pub(crate) fn touch_blocked(&self, p: Vector, time: f64, aspect: f32) -> bool {
        let mut cache = self.block_frame.borrow_mut();
        self.update_marker_areas(&mut cache, time, aspect);
        let BlockFrame { timeline, marker_areas, .. } = &mut *cache;
        let areas = timeline
            .at(&self.block_areas, time)
            .iter()
            .map(|&id| &self.block_areas[id])
            .chain(marker_areas.iter());
        super::block::block_touch_blocked_iter(areas, p, time, aspect)
    }

    fn update_marker_areas(&self, cache: &mut BlockFrame, time: f64, aspect: f32) {
        if self.rpe_block_markers.is_empty() || cache.marker_key == Some((time, aspect)) {
            return;
        }
        // All fingers in this input frame share the same evaluated marker pose.
        // Invalidate when that pose is updated, even if chart time is unchanged.
        cache.marker_areas.clear();
        cache.marker_areas.extend(self.rpe_marker_areas(time, aspect));
        cache.marker_key = Some((time, aspect));
    }

    /// RPE charts encode block areas as editor texture lines named
    /// `isSubtract0.png` (normal) / `isSubtract1.png` (subtract). Resolve those
    /// animated lines into the same rectangle model used by official block areas.
    fn rpe_marker_areas(&self, time: f64, aspect: f32) -> impl Iterator<Item = BlockArea> + '_ {
        self.rpe_block_markers.iter().filter_map(move |marker| {
            let id = marker.line;
            let [appear_time, enable_time, disable_time, disappear_time] = marker.timings(time)?;
            let line = self.lines.get(id)?;
            let texture = match &line.kind {
                JudgeLineKind::Texture(texture, _) => texture,
                _ => return None,
            };
            let scale = line.object.scale.now_with_def(1., 1.);
            if scale.x.abs() < 1e-6 || scale.y.abs() < 1e-6 || texture.width() <= 0. || texture.height() <= 0. {
                return None;
            }
            let center = self
                .line_transforms
                .get(id)
                .map(|matrix| matrix.transform_point(&Point::new(0., 0.)))
                .unwrap_or_else(|| {
                    let mut pos = line.object.translation.now();
                    pos.y /= aspect;
                    Point::new(pos.x, pos.y)
                });
            let rotation = self.line_rotations.get(id).copied().unwrap_or_else(|| line.object.rotation.now());
            let half_x = texture.width() * scale.x.abs() * 0.5;
            let half_y = texture.height() * scale.y.abs() * 0.5;
            let to_pct_x = |x: f32| (x + 1.) * 0.5;
            let to_pct_y = |y: f32| (y * aspect + 1.) * 0.5;
            let c = Vector::new(center.x, center.y);
            let top_right = Vector::new(to_pct_x(c.x + half_x), to_pct_y(c.y + half_y));
            let bottom_left = Vector::new(to_pct_x(c.x - half_x), to_pct_y(c.y - half_y));
            let anchor = Vector::new(to_pct_x(c.x), to_pct_y(c.y));
            Some(BlockArea {
                top_right,
                bottom_left,
                appear_time,
                enable_time,
                disable_time,
                disappear_time,
                is_subtract: marker.invert,
                rotate_events: vec![BlockRotateEvent {
                    anchor,
                    time: 0.,
                    ease: 0,
                    rotation,
                }],
                move_events: Vec::new(),
                scale_events: Vec::new(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AnimFloat, CtrlObject, JudgeLineCache, Keyframe};

    fn line(parent: Option<usize>, rotation: AnimFloat) -> JudgeLine {
        let mut notes = Vec::new();
        let cache = JudgeLineCache::new(&mut notes);
        JudgeLine {
            object: Object {
                rotation,
                ..Default::default()
            },
            ctrl_obj: RefCell::new(CtrlObject::default()),
            kind: JudgeLineKind::Normal,
            height: AnimFloat::default(),
            incline: AnimFloat::default(),
            notes,
            color: Default::default(),
            parent,
            rot_with_parent: true,
            z_index: 0,
            show_below: false,
            attach_ui: None,
            cache,
        }
    }

    fn chart() -> Chart {
        Chart::new(
            0.,
            vec![
                line(Some(1), AnimFloat::fixed(10.)),
                line(None, AnimFloat::new(vec![Keyframe::new(0., 0., 2), Keyframe::new(2., 90., 2)])),
                line(None, AnimFloat::default()),
            ],
            BpmList::new(vec![(0., 120.)]),
            ChartSettings::default(),
            ChartExtra::default(),
            HashMap::new(),
        )
    }

    #[test]
    fn rpe_input_also_updates_later_parents_of_non_marker_lines_and_seeks_backwards() {
        let mut chart = chart();
        // Marker metadata alone is sufficient to test the input schedule, with
        // no GL context or editor guide texture involved.
        chart
            .rpe_block_markers
            .push(super::super::rpe_block::Marker::new(2, false, &AnimFloat::fixed(1.)));
        assert!(chart.prepare_rpe_input_time(1.));
        assert_eq!(chart.lines[0].fetch_rot(&chart.lines), 55.);
        assert!(chart.prepare_rpe_input_time(0.));
        assert_eq!(chart.lines[0].fetch_rot(&chart.lines), 10.);
    }

    #[test]
    fn ordinary_charts_keep_the_existing_input_evaluation_path() {
        let mut chart = chart();
        assert!(!chart.prepare_rpe_input_time(1.));
        assert_eq!(chart.lines[1].object.rotation.time, 0.);
    }
}
