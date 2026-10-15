//! Search is navigation over the existing controls, so it never creates a second settings state.
use super::*;
use prpr::ui::RectButton;
#[cfg(all(test, record, target_os = "windows"))]
pub(crate) mod qa;

pub struct Search {
    query: String,
    bar: DRectButton,
    clear: RectButton,
    search_icon: SafeTexture,
    close_icon: SafeTexture,
    rows: Vec<RectButton>,
    results: Vec<(usize, String)>,
    pub target: Option<String>,
}
impl Search {
    pub fn new(search_icon: SafeTexture, close_icon: SafeTexture) -> Self {
        Self {
            query: String::new(),
            bar: DRectButton::new(),
            clear: RectButton::new(),
            search_icon,
            close_icon,
            rows: Vec::new(),
            results: Vec::new(),
            target: None,
        }
    }
}
impl Search {
    pub fn active(&self) -> bool {
        !self.query.trim().is_empty()
    }
    pub fn cancel(&mut self) -> bool {
        let consumed = self.active();
        self.query.clear();
        self.results.clear();
        consumed
    }
    pub fn update(&mut self) -> bool {
        if let Some((id, text)) = take_input() {
            if id == "settings_search" {
                self.query = text.trim().to_owned();
                self.results = catalog()
                    .into_iter()
                    .filter(|(_, title)| title.to_lowercase().contains(&self.query.to_lowercase()))
                    .collect();
                return true;
            }
            return_input(id, text);
        }
        false
    }
    pub fn touch(&mut self, touch: &Touch, t: f32) -> (bool, Option<usize>) {
        if self.active() && self.clear.touch(touch) {
            self.query.clear();
            return (true, None);
        }
        if (!self.active() || !self.clear.contains(touch.position)) && self.bar.touch(touch, t) {
            request_input("settings_search", InputBox::new().default_text(&self.query));
            return (true, None);
        }
        if self.active() {
            for (i, row) in self.rows.iter_mut().enumerate() {
                if row.touch(touch) {
                    if let Some((tab, title)) = self.results.get(i) {
                        self.target = Some(title.clone());
                        self.query.clear();
                        return (true, Some(*tab));
                    }
                }
            }
        }
        (false, None)
    }
    pub fn bar(&mut self, ui: &mut Ui, right: f32, t: f32) {
        let r = crate::page::search_bar::rect(right, -ui.top + 0.04, crate::page::search_bar::HEIGHT, !self.active());
        crate::page::search_bar::render(ui, &mut self.bar, &mut self.clear, r, t, &self.query, &self.search_icon, &self.close_icon);
    }
    pub fn render(&mut self, ui: &mut Ui, r: Rect) -> (f32, f32) {
        self.rows.resize_with(self.results.len(), RectButton::new);
        for (i, ((tab, title), row)) in self.results.iter().zip(&mut self.rows).enumerate() {
            let rect = Rect::new(0.02, i as f32 * 0.12, r.w - 0.04, 0.105);
            ui.fill_path(&rect.rounded(0.008), prpr::ext::semi_black(0.25));
            row.set(ui, rect);
            ui.text(title)
                .pos(rect.x + 0.02, rect.y + 0.013)
                .size(0.46)
                .max_width(rect.w - 0.04)
                .draw();
            ui.text(["通用", "音频", "谱面", "调试", "体验"][*tab])
                .pos(rect.x + 0.02, rect.y + 0.066)
                .size(0.3)
                .color(semi_white(0.6))
                .draw();
        }
        if self.results.is_empty() {
            ui.text("没有匹配的设置").pos(0.04, 0.04).size(0.45).draw();
        }
        (r.w, (self.results.len() as f32 * 0.12).max(0.15))
    }
}

pub fn catalog() -> Vec<(usize, String)> {
    // Generated from the production setting labels; translations remain owned by settings.ftl.
    let keys: &[(usize, &str)] = include!("search_catalog.rs");
    let mut entries: Vec<_> = keys
        .iter()
        .filter(|(_, key)| {
            !(*key == "item-refresh-rate" && !cfg!(any(target_os = "android", target_os = "ios")))
                && !(*key == "item-desktop-fps" && !cfg!(target_os = "windows"))
        })
        .map(|(tab, key)| (*tab, tl!(*key).into_owned()))
        .collect();
    entries.extend([(3, "画质设置".into()), (3, "问题诊断".into())]);
    entries
}

thread_local! { pub static FOCUS: std::cell::RefCell<(Option<String>, Option<f32>)> = const { std::cell::RefCell::new((None,None)) }; }
pub fn focus(ui: &mut Ui, title: &str) {
    FOCUS.with(|f| {
        let mut f = f.borrow_mut();
        if f.0.as_deref() == Some(title) {
            f.1 = Some(ui.to_global((0., 0.).into()).1);
            ui.fill_path(&Rect::new(0.02, 0., 1.53, item_row_h() - 0.01).rounded(0.008), semi_white(0.10));
        }
    });
}
