use super::{
    draw_disabled_zones, draw_zones_with_touches, BlockArea, BpmList, Effect, JudgeLine, JudgeLineKind, Matrix, Resource, UIElement, Vector, Zone,
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
                    target.blit();
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
            let BlockFrame { timeline, zones, .. } = &mut *cache;
            zones.clear();
            zones.extend(
                timeline
                    .at(&self.block_areas, res.time)
                    .iter()
                    .filter_map(|&id| Zone::from_area(&self.block_areas[id], res.time, res.aspect_ratio)),
            );
            cache.key = Some(key);
        }
        drop(cache);
        std::cell::Ref::map(self.block_frame.borrow(), |cache| cache.zones.as_slice())
    }

    pub(crate) fn touch_blocked(&self, p: Vector, time: f64, aspect: f32) -> bool {
        let mut cache = self.block_frame.borrow_mut();
        super::block::block_touch_blocked_iter(cache.timeline.at(&self.block_areas, time).iter().map(|&id| &self.block_areas[id]), p, time, aspect)
    }
}
