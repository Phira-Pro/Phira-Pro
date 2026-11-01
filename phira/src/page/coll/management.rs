//! Local pack management shares the collection page's dialogs and chart picker.
use super::*;

impl CollectionPage {
    pub(super) fn cancel_card_touches(&mut self) {
        self.plus.cancel();
        for card in &mut self.cards {
            card.btn.cancel();
            card.long_touch.reset();
        }
    }

    pub(super) fn open_management(&mut self, index: usize, rt: f32) {
        let pack = self.cards[index].pack.clone();
        if pack.id.is_nil() {
            return;
        }
        self.cancel_card_touches();
        self.scroll.x_scroller.suspend(rt);
        self.managing = Some(pack.clone());
        let action = self.action.clone();
        Dialog::plain("管理曲包", format!("{}\n编辑曲目只调整分组。删除曲包也会保留本地谱面文件。", pack.name))
            .buttons(vec!["取消".into(), "编辑名称".into(), "编辑曲目".into(), "删除曲包".into()])
            .listener(move |_, index| {
                if (1..=3).contains(&index) {
                    action.store(index as u8 + 2, Ordering::Relaxed);
                }
                false
            })
            .show();
    }

    pub(super) fn start_selection(&mut self, pack: Option<SongPack>, s: &SharedState) {
        self.cancel_selection();
        let charts = super::super::load_local();
        let selected = pack.as_ref().map_or_else(Vec::new, |pack| {
            pack.charts
                .iter()
                .filter_map(|path| charts.iter().find(|chart| chart.local_path.as_ref() == Some(path)))
                .map(|chart| chart.to_bare_ref())
                .collect::<Vec<_>>()
        });
        if pack.as_ref().is_some_and(|pack| selected.len() < pack.charts.len()) {
            show_message("部分谱面已不在本地，保存时将从曲包分组移除").warn();
        }
        self.editing = pack.map(|pack| pack.id);
        let mut view = ChartsView::new(self.icons.clone(), s.icons.clone());
        view.can_refresh = false;
        view.allow_multi_select = false;
        view.multi_select = Some(selected);
        view.set(s.t, charts.into_iter().map(|chart| ChartDisplayItem::new(Some(chart), None)).collect());
        self.choose = Some(view);
    }

    pub(super) fn cancel_selection(&mut self) {
        self.choose = None;
        self.editing = None;
        self.pending_charts.clear();
        self.create.cancel(0.);
        self.cancel.cancel(0.);
    }

    pub(super) fn manage_action(&mut self, action: u8, s: &SharedState) -> Result<()> {
        let Some(pack) = self.managing.clone() else {
            return Ok(());
        };
        match action {
            3 => {
                self.renaming = Some(pack.id);
                request_input("songpack_rename", InputBox::new().default_text(pack.name));
            }
            4 => self.start_selection(Some(pack), s),
            5 => {
                let action = self.action.clone();
                Dialog::plain(
                    "删除曲包",
                    format!("确定删除曲包“{}”？\n仅删除曲包分组，本地谱面、音乐和曲绘文件均会保留，仍可在本地曲库中使用。", pack.name),
                )
                .buttons(vec!["取消".into(), "删除分组".into()])
                .listener(move |_, index| {
                    if index == 1 {
                        action.store(6, Ordering::Relaxed);
                    }
                    false
                })
                .show();
            }
            6 => {
                match songpacks::remove(pack.id) {
                    Ok(()) => {
                        self.reload()?;
                        show_message("曲包分组已删除，本地谱面文件已保留").ok();
                    }
                    Err(err) => show_error(err),
                }
                self.managing = None;
            }
            _ => {}
        }
        Ok(())
    }
}
