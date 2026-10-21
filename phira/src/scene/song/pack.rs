//! Pack browsing reuses SongScene's preview, toolbar, edit drawers and launch path.
use super::*;
use crate::charts_view::{ChartDisplayItem, ChartsView};

pub(super) struct PackView {
    pub title: String,
    pub view: ChartsView,
    pub old_background: Option<SafeTexture>,
    pub change_time: f32,
    pub background_time: f32,
    pub background: SafeTexture,
    background_task: Option<Task<Result<prpr::loading_cpu::Illustration>>>,
}
fn background(path: String) -> Task<Result<prpr::loading_cpu::Illustration>> {
    Task::new(async move {
        let mut fs = fs_from_path(&path)?;
        let info = prpr::fs::load_info(fs.as_mut()).await?;
        let bytes = fs.load_file(&info.illustration).await?;
        tokio::task::spawn_blocking(move || prpr::loading_cpu::decode_illustration(&bytes, 0.5)).await?
    })
}
pub(super) fn scale(top: f32) -> f32 {
    (top / (9. / 16.)).clamp(0.65, 1.)
}
fn header(top: f32) -> f32 {
    if top < 0.5 {
        0.16
    } else {
        0.22
    }
}
fn art_rect(top: f32) -> Rect {
    let available = (top * 2. - header(top) - 0.07 * scale(top) - 0.24 * scale(top) - 0.04).max(0.1);
    let width = 0.94_f32.min(available * 16. / 9.);
    let height = width * 9. / 16.;
    Rect::new(0.015 + (0.94 - width) / 2., -top + header(top) + (available - height).max(0.) * 0.25, width, height)
}
impl SongScene {
    pub(super) fn delete_pack_chart(&mut self, tm: &mut TimeManager) -> Result<bool> {
        if self.pack_view.is_none() {
            return Ok(false);
        }
        let path = self.local_path.clone().context("本地谱面路径缺失")?;
        let root = std::path::PathBuf::from(dir::charts()?).canonicalize()?;
        let target = root.join(&path).canonicalize()?;
        if target == root || !target.starts_with(&root) {
            bail!("无效的本地谱面路径");
        }
        std::fs::remove_dir_all(target)?;
        get_data_mut().charts.retain(|chart| chart.local_path != path);
        save_data()?;
        let mut packs = crate::songpacks::load()?;
        for pack in &mut packs {
            pack.charts.retain(|p| p != &path);
        }
        crate::songpacks::save(&packs)?;
        NEED_UPDATE.store(true, Ordering::Relaxed);
        let pack = self.pack_view.as_mut().unwrap();
        if let Some(charts) = &mut pack.view.charts {
            charts.retain(|c| c.chart.as_ref().is_none_or(|c| c.local_path.as_deref() != Some(path.as_str())));
            pack.view.picked = charts.first().and_then(|c| c.chart.clone());
        }
        if pack.view.picked.is_some() {
            self.update_pack(tm)?;
        } else {
            self.next_scene = Some(NextScene::PopWithResult(Box::new(false)));
        }
        Ok(true)
    }
    pub(super) fn sync_pack_card(&mut self) {
        if let Some(pack) = &mut self.pack_view {
            if let Some(item) = pack.view.charts.as_mut().and_then(|cards| {
                cards
                    .iter_mut()
                    .filter_map(|c| c.chart.as_mut())
                    .find(|c| c.local_path == self.local_path)
            }) {
                if item.info.name != self.info.name || item.info.level != self.info.level || item.info.difficulty != self.info.difficulty {
                    item.info = self.info.clone();
                    item.illu = self.illu.clone();
                }
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn pack_selected_path(&self) -> Option<&str> {
        self.local_path.as_deref()
    }
    pub fn from_pack(pack: &crate::songpacks::SongPack, icons: Arc<Icons>, ranks: [SafeTexture; 8]) -> Result<Self> {
        // Only allocate image tasks for this pack, rather than every chart in
        // the library each time a pack is opened.
        let local: std::collections::HashMap<_, _> = get_data().charts.iter().map(|c| (c.local_path.as_str(), c)).collect();
        let items: Vec<_> = pack
            .charts
            .iter()
            .filter_map(|path| {
                local.get(path.as_str()).map(|chart| ChartItem {
                    info: chart.info.clone(),
                    local_path: Some(path.clone()),
                    illu: local_illustration(path.clone(), icons.r#abstract.clone(), false),
                    chart_type: ChartType::Imported,
                })
            })
            .collect();
        let first = items.first().cloned().context("此曲包的谱面已不在本地谱面库中")?;
        let mods = first
            .local_path
            .as_ref()
            .and_then(|p| get_data().charts.iter().find(|c| &c.local_path == p))
            .map(|c| c.mods)
            .unwrap_or_default();
        let mut scene = Self::new(first.clone(), first.local_path.clone(), icons.clone(), ranks.clone(), mods);
        let mut view = ChartsView::new(icons, ranks);
        view.can_refresh = false;
        view.allow_multi_select = false;
        view.select_on_click = true;
        view.highlighted = Some(first.to_bare_ref());
        view.fixed_layout = Some((2, 0.28));
        view.set(-1., items.into_iter().map(|c| ChartDisplayItem::new(Some(c), None)).collect());
        scene.pack_view = Some(PackView {
            title: pack.name.clone(),
            view,
            old_background: None,
            change_time: 0.,
            background_time: 0.,
            background: scene.illu.texture.0.clone(),
            background_task: first.local_path.clone().map(background),
        });
        Ok(scene)
    }
    pub(super) fn update_pack(&mut self, tm: &mut TimeManager) -> Result<()> {
        let Some(pack) = &mut self.pack_view else {
            return Ok(());
        };
        if let Some(result) = pack.background_task.as_mut().and_then(Task::take) {
            pack.background_task = None;
            match result {
                Ok(image) => {
                    if pack.old_background.is_none() {
                        pack.old_background = Some(pack.background.clone());
                    }
                    pack.background = Texture2D::from_rgba8(image.width as _, image.height as _, &image.blurred).into();
                    pack.background_time = tm.now() as _;
                }
                Err(err) => tracing::warn!(?err, "pack background unavailable"),
            }
        }
        pack.view.update(tm.now() as _)?;
        let Some(chart) = pack.view.picked.take() else {
            return Ok(());
        };
        if self.local_path == chart.local_path {
            return Ok(());
        }
        let mut pack = self.pack_view.take().unwrap();
        pack.old_background = Some(pack.background.clone());
        pack.background_task = chart.local_path.clone().map(background);
        pack.change_time = tm.now() as _;
        pack.background_time = f32::INFINITY;
        self.pause(tm)?;
        let mods = chart
            .local_path
            .as_ref()
            .and_then(|p| get_data().charts.iter().find(|c| &c.local_path == p))
            .map(|c| c.mods)
            .unwrap_or_default();
        let mut replacement = Self::new(chart.clone(), chart.local_path.clone(), self.icons.clone(), self.rank_icons.clone(), mods);
        replacement.pack_view = Some(pack);
        // Keep the scene clock and list scroll. Only the new selection fades in.
        replacement.first_in = false;
        replacement.fade_start = tm.now() as f32 + fade_in_time().unwrap_or_default();
        replacement.update_menu();
        replacement.load_ldb();
        replacement.start_ppsr_analysis();
        if let Some(path) = &replacement.local_path {
            replacement.pack_view.as_mut().unwrap().view.highlighted = Some(crate::client::ChartRef::new_bare(replacement.info.id, Some(path)));
        }
        *self = replacement;
        Ok(())
    }
    pub(super) fn render_pack_body(&mut self, ui: &mut Ui, t: f32) {
        let top = ui.top;
        let list = Rect::new(-0.95, -top + header(top), 0.91, top * 2. - header(top) - 0.05);
        let row_height = ((list.w / 2. - 0.026) * 9. / 16. + 0.026).max(0.17);
        if let Some(pack) = &mut self.pack_view {
            pack.view.fixed_layout = Some((2, row_height));
            pack.view.render(ui, list, t);
        }
        let alpha = if get_data().prefer_reduced_motion {
            1.
        } else {
            ((t - self.pack_view.as_ref().unwrap().change_time) / 0.3).clamp(0., 1.)
        };
        ui.alpha(alpha, |ui| {
            let art = art_rect(top);
            ui.fill_path(&art.rounded(0.008), (*self.illu.texture.1, art, ScaleType::CropCenter));
            ui.text(&self.info.name)
                .pos(art.x, art.bottom() + 0.02)
                .size(0.65 * scale(top))
                .max_width(art.w)
                .draw_using(&prpr::core::BOLD_FONT);
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn art_keeps_16_by_9_and_clears_score_and_title_on_phones_and_tablets() {
        for aspect in [4. / 3., 16. / 9., 21. / 9., 2.4] {
            let top = 1. / aspect;
            let r = super::art_rect(top);
            assert!((r.w / r.h - 16. / 9.).abs() < 1e-5);
            assert!(r.x >= 0. && r.right() <= 0.96);
            assert!(r.bottom() + 0.07 * super::scale(top) <= top - 0.20 * super::scale(top) - 0.06);
        }
    }
}
