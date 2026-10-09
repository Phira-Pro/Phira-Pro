// HUD 自定义布局。
//
// 每个页面把自己**顶层可编辑的组件**声明成若干「槽位」(slot)：槽位在代码里带一个
// 锚点 + 相对偏移 + 尺寸（都是 UI 的 x 单位，`y` 也按 x 单位算，因此换分辨率不会跑位），
// 用户拖动 / 缩放后的结果存到 `<data>/hud.json`。
//
// 渲染时用 `slot` 取矩形；命中区域是「渲染时顺手记下的矩形」，所以会自动跟随，不需要
// 额外维护命中表。
//
// 编辑层由 `MainScene` 承载：页面渲染时登记本帧的槽位（`begin_frame`/`slot`），
// 编辑层再据此画虚线选框、手柄与工具条。
//
// （注：模块文档注释不能写成 //! —— tl_file! 展开后必须是文件开头）
prpr_l10n::tl_file!("hud");

use anyhow::{Context, Result};
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use prpr::{
    core::BOLD_FONT,
    ext::RectExt,
    ui::{RectButton, Ui},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Mutex,
    },
};

use crate::dir;

/// 槽位相对屏幕的参考点（自动适配的关键：贴边的组件换宽高比时仍然贴边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// 参考点（x 单位；y 同样是 x 单位）。
    pub fn point(self, ui: &Ui) -> Vec2 {
        let top = ui.top;
        match self {
            Anchor::TopLeft => vec2(-1., top),
            Anchor::Top => vec2(0., top),
            Anchor::TopRight => vec2(1., top),
            Anchor::Left => vec2(-1., 0.),
            Anchor::Center => vec2(0., 0.),
            Anchor::Right => vec2(1., 0.),
            Anchor::BottomLeft => vec2(-1., -top),
            Anchor::Bottom => vec2(0., -top),
            Anchor::BottomRight => vec2(1., -top),
        }
    }
}

/// 一个槽位的可编辑能力：可移动 / 可改宽 / 可改高。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cap(pub bool, pub bool, pub bool);

/// 代码里声明的槽位默认值。
#[derive(Clone, Copy, Debug)]
pub struct SlotDef {
    pub key: &'static str,
    pub anchor: Anchor,
    /// 相对锚点中心的偏移与尺寸（x 单位）。
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub cap: Cap,
}

impl SlotDef {
    /// 由「当前写死的矩形」构造槽位（锚点居中），保证默认外观与改造前逐像素一致。
    pub const fn centered(key: &'static str, rect: [f32; 4], cap: Cap) -> Self {
        let cx = rect[0] + rect[2] / 2.;
        let cy = rect[1] + rect[3] / 2.;
        Self {
            key,
            anchor: Anchor::Center,
            offset: [cx, cy],
            size: [rect[2], rect[3]],
            cap,
        }
    }

    /// 以某个锚点为参考的槽位。
    pub const fn at(key: &'static str, anchor: Anchor, offset: [f32; 2], size: [f32; 2], cap: Cap) -> Self {
        Self { key, anchor, offset, size, cap }
    }

    fn default_rect(self, ui: &Ui) -> Rect {
        let p = self.anchor.point(ui) + vec2(self.offset[0], self.offset[1]);
        Rect::new(p.x - self.size[0] / 2., p.y - self.size[1] / 2., self.size[0], self.size[1])
    }
}

/// 存盘的一条槽位。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Slot {
    pub anchor: Anchor,
    pub offset: [f32; 2],
    pub size: [f32; 2],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageLayout {
    pub slots: BTreeMap<String, Slot>,
    /// 数值参数（列表列数 / 格高这类「不能自由拖动、只能调数值」的量）。
    pub params: BTreeMap<String, f32>,
}

impl PageLayout {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    pub version: u32,
    pub pages: BTreeMap<String, PageLayout>,
}

const VERSION: u32 = 1;

static LAYOUT: Lazy<Mutex<Option<Layout>>> = Lazy::new(|| Mutex::new(None));

/// Scope GPU regression saves to a fresh directory and restore all HUD globals.
#[cfg(test)]
pub(crate) struct IsolatedTestLayout {
    directory: tempfile::TempDir,
    old_data_path: Option<String>,
    old_layout: Option<Layout>,
    old_snap: bool,
    old_page: PageId,
}

#[cfg(test)]
impl IsolatedTestLayout {
    pub(crate) fn new() -> Self {
        let directory = tempfile::tempdir_in("target/judgement-panel-qa").unwrap();
        let isolated_path = directory.path().to_string_lossy().replace('\\', "/");
        let old_data_path = crate::DATA_PATH.lock().unwrap().replace(isolated_path);
        let old_layout = std::mem::replace(&mut *LAYOUT.lock().unwrap(), Some(Layout::default()));
        Self {
            directory,
            old_data_path,
            old_layout,
            old_snap: snap_enabled(),
            old_page: cur_page(),
        }
    }

    pub(crate) fn saved_path(&self) -> std::path::PathBuf {
        let saved = std::path::PathBuf::from(path().unwrap());
        assert!(
            saved.parent().unwrap().canonicalize().unwrap().starts_with(self.directory.path().canonicalize().unwrap()),
            "HUD test must save inside its temporary directory"
        );
        saved
    }

    pub(crate) fn reload_saved(&self) {
        *LAYOUT.lock().unwrap() = None;
    }
}

#[cfg(test)]
impl Drop for IsolatedTestLayout {
    fn drop(&mut self) {
        *LAYOUT.lock().unwrap() = self.old_layout.take();
        *crate::DATA_PATH.lock().unwrap() = self.old_data_path.take();
        set_snap(self.old_snap);
        set_cur_page(self.old_page);
    }
}

fn path() -> Result<String> {
    Ok(format!("{}/hud.json", dir::root()?))
}

fn load_inner() -> Layout {
    (|| -> Result<Layout> {
        let path = path()?;
        if !std::path::Path::new(&path).exists() {
            return Ok(Layout::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
        if text.trim().is_empty() {
            return Ok(Layout::default());
        }
        Ok(serde_json::from_str(&text).with_context(|| format!("failed to parse {path}"))?)
    })()
    .unwrap_or_default()
}

fn with_layout<T>(f: impl FnOnce(&mut Layout) -> T) -> T {
    let mut guard = LAYOUT.lock().unwrap();
    let layout = guard.get_or_insert_with(load_inner);
    f(layout)
}

pub fn save() -> Result<()> {
    let text = with_layout(|it| serde_json::to_string_pretty(it))?;
    let path = path()?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {path}"))?;
    Ok(())
}

/// 取某个页面已保存的槽位。
pub fn get(page: &str, key: &str) -> Option<Slot> {
    with_layout(|it| it.pages.get(page).and_then(|p| p.slots.get(key)).copied())
}

/// 写入/更新某个槽位（offset/size 由调用方按新矩形换算）。
pub fn set(page: &str, key: &str, slot: Slot) {
    with_layout(|it| {
        it.version = VERSION;
        it.pages.entry(page.to_owned()).or_default().slots.insert(key.to_owned(), slot);
    });
}

/// 重置某个页面（或全部）。
pub fn reset(page: Option<&str>) {
    with_layout(|it| match page {
        Some(page) => {
            it.pages.remove(page);
        }
        None => it.pages.clear(),
    });
}

fn sanitize(mut s: Slot) -> Slot {
    for v in s.offset.iter_mut().chain(s.size.iter_mut()) {
        if !v.is_finite() {
            *v = 0.;
        }
    }
    s.size[0] = s.size[0].clamp(0.02, 4.);
    s.size[1] = s.size[1].clamp(0.02, 4.);
    s
}

/// 把一个矩形换算成「以 `anchor` 为参考」的槽位。
pub fn slot_from_rect(ui: &Ui, anchor: Anchor, r: Rect) -> Slot {
    let p = anchor.point(ui);
    sanitize(Slot {
        anchor,
        offset: [r.x + r.w / 2. - p.x, r.y + r.h / 2. - p.y],
        size: [r.w, r.h],
    })
}

// ---------------------------------------------------------------- 每帧登记 ----

/// 本帧页面渲染时登记下来的槽位，供编辑层画选框。
#[derive(Clone, Debug)]
pub struct FrameSlot {
    pub page: String,
    pub key: &'static str,
    pub anchor: Anchor,
    pub cap: Cap,
    /// 本帧实际使用的矩形（已含用户保存的偏移/尺寸）。
    pub rect: Rect,
    /// 是否是用户改过的（用于「已自定义」标记）。
    pub edited: bool,
}

thread_local! {
    static FRAME: RefCell<Vec<FrameSlot>> = const { RefCell::new(Vec::new()) };
}

/// 每次渲染页面之前调用，清空本帧登记。
pub fn begin_frame() {
    FRAME.with(|it| it.borrow_mut().clear());
}

pub fn frame_slots() -> Vec<FrameSlot> {
    FRAME.with(|it| it.borrow().clone())
}

/// 该槽位是否已被用户改过。
pub fn has(page: &str, key: &str) -> bool {
    get(page, key).is_some()
}

/// 读取某页面的数值参数（列表列数 / 格高这类）。非法值回退默认。
pub fn param(page: &str, key: &str, default: f32) -> f32 {
    with_layout(|it| it.pages.get(page).and_then(|p| p.params.get(key)).copied())
        .filter(|it| it.is_finite())
        .unwrap_or(default)
}

pub fn set_param(page: &str, key: &str, v: f32) {
    if !v.is_finite() {
        return;
    }
    with_layout(|it| {
        it.version = VERSION;
        it.pages.entry(page.to_owned()).or_default().params.insert(key.to_owned(), v);
    });
}

/// 登记一个「默认矩形由运行期算出来」的槽位（画布类区域，默认值随宽高比变）。
///
/// 只把本帧要画的方框告诉编辑层，不改动布局；用户一旦拖动就会以中心锚点存下来。
pub fn register(page: &str, def: SlotDef, rect: Rect) {
    FRAME.with(|it| {
        it.borrow_mut().push(FrameSlot {
            page: page.to_owned(),
            key: def.key,
            anchor: Anchor::Center,
            cap: def.cap,
            rect,
            edited: false,
        });
    });
}

/// 取槽位；没有保存过就用**调用方给的运行期默认矩形**（并登记给编辑层），不改布局。
///
/// 用于 `content_rect()` / 链式排布这类默认值只能算出来的元素。
pub fn slot_or(ui: &Ui, page: &str, key: &'static str, cap: Cap, default: Rect) -> Rect {
    let def = SlotDef {
        key,
        anchor: Anchor::Center,
        offset: [0., 0.],
        size: [default.w, default.h],
        cap,
    };
    if has(page, key) {
        slot(ui, page, def)
    } else {
        register(page, def, default);
        default
    }
}

/// 页面渲染时用它取槽位矩形（同时登记给编辑层）。
///
/// 没有保存过就用 `def` 的默认值 —— 因此**不编辑时外观与改造前完全一致**。
pub fn slot(ui: &Ui, page: &str, def: SlotDef) -> Rect {
    let stored = get(page, def.key);
    let edited = stored.is_some();
    let s = sanitize(stored.unwrap_or(Slot {
        anchor: def.anchor,
        offset: def.offset,
        size: def.size,
    }));
    let p = s.anchor.point(ui) + vec2(s.offset[0], s.offset[1]);
    let rect = Rect::new(p.x - s.size[0] / 2., p.y - s.size[1] / 2., s.size[0], s.size[1]);
    FRAME.with(|it| {
        it.borrow_mut().push(FrameSlot {
            page: page.to_owned(),
            key: def.key,
            anchor: s.anchor,
            cap: def.cap,
            rect,
            edited,
        });
    });
    rect
}

// ---------------------------------------------------------------- 编辑开关 ----

/// 可自定义的页面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageId {
    Home,
    Library,
    Settings,
    Favorites,
    Message,
    History,
    Respack,
    Blacklist,
}

impl PageId {
    pub const ALL: [PageId; 8] = [
        PageId::Home,
        PageId::Library,
        PageId::Settings,
        PageId::Favorites,
        PageId::Message,
        PageId::History,
        PageId::Respack,
        PageId::Blacklist,
    ];

    /// 布局文件里的页面 key。
    pub fn key(self) -> &'static str {
        match self {
            PageId::Home => "home",
            PageId::Library => "library",
            PageId::Settings => "settings",
            PageId::Favorites => "favorites",
            PageId::Message => "message",
            PageId::History => "history",
            PageId::Respack => "respack",
            PageId::Blacklist => "blacklist",
        }
    }
}

static EDIT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 请求跳转到的页面（-1 = 无请求）。
static GOTO: AtomicU8 = AtomicU8::new(u8::MAX);
/// 当前正在编辑的页面。
static CUR_PAGE: AtomicU8 = AtomicU8::new(0);
/// 网格吸附开关与步长档位。
static SNAP: AtomicBool = AtomicBool::new(true);
static SNAP_STEP: AtomicU8 = AtomicU8::new(1);

/// 网格步长候选（x 单位）。默认 0.02 ≈ 半屏宽的 2%。
pub const SNAP_STEPS: [f32; 3] = [0.04, 0.02, 0.01];

pub fn edit_active() -> bool {
    EDIT_ACTIVE.load(Ordering::Relaxed)
}

pub fn set_edit(active: bool) {
    EDIT_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn cur_page() -> PageId {
    match CUR_PAGE.load(Ordering::Relaxed) {
        1 => PageId::Library,
        2 => PageId::Settings,
        3 => PageId::Favorites,
        4 => PageId::Message,
        5 => PageId::History,
        6 => PageId::Respack,
        7 => PageId::Blacklist,
        _ => PageId::Home,
    }
}

pub fn set_cur_page(p: PageId) {
    CUR_PAGE.store(
        match p {
            PageId::Home => 0,
            PageId::Library => 1,
            PageId::Settings => 2,
            PageId::Favorites => 3,
            PageId::Message => 4,
            PageId::History => 5,
            PageId::Respack => 6,
            PageId::Blacklist => 7,
        },
        Ordering::Relaxed,
    );
}

/// 编辑器请求切页；由 `MainScene` 消费后跳转。
pub fn request_goto(p: PageId) {
    GOTO.store(
        match p {
            PageId::Home => 0,
            PageId::Library => 1,
            PageId::Settings => 2,
            PageId::Favorites => 3,
            PageId::Message => 4,
            PageId::History => 5,
            PageId::Respack => 6,
            PageId::Blacklist => 7,
        },
        Ordering::Relaxed,
    );
}

pub fn take_goto() -> Option<PageId> {
    let v = GOTO.swap(u8::MAX, Ordering::Relaxed);
    match v {
        0 => Some(PageId::Home),
        1 => Some(PageId::Library),
        2 => Some(PageId::Settings),
        3 => Some(PageId::Favorites),
        4 => Some(PageId::Message),
        5 => Some(PageId::History),
        6 => Some(PageId::Respack),
        7 => Some(PageId::Blacklist),
        _ => None,
    }
}

pub fn snap_enabled() -> bool {
    SNAP.load(Ordering::Relaxed)
}

pub fn set_snap(on: bool) {
    SNAP.store(on, Ordering::Relaxed);
}

pub fn snap_step() -> f32 {
    SNAP_STEPS[(SNAP_STEP.load(Ordering::Relaxed) as usize).min(SNAP_STEPS.len() - 1)]
}

pub fn cycle_snap_step() {
    let i = (SNAP_STEP.load(Ordering::Relaxed) as usize + 1) % SNAP_STEPS.len();
    SNAP_STEP.store(i as u8, Ordering::Relaxed);
}

/// 按网格吸附（开启时）。
pub fn snap(v: f32) -> f32 {
    if snap_enabled() {
        (v / snap_step()).round() * snap_step()
    } else {
        v
    }
}

// ---------------------------------------------------------------- tab 信息 ----

/// tab 栏信息（编辑模式下据此画「切换分页」按钮）。
static TAB_COUNT: AtomicU8 = AtomicU8::new(0);
static TAB_SEL: AtomicU8 = AtomicU8::new(0);
/// 编辑器请求切换到的 tab（255 = 无请求）。
static TAB_REQUEST: AtomicU8 = AtomicU8::new(u8::MAX);

/// 由 `Tabs` 每帧上报：共几个 tab、当前选中第几个。
pub fn set_tab_info(count: usize, sel: usize) {
    TAB_COUNT.store(count.min(255) as u8, Ordering::Relaxed);
    TAB_SEL.store(sel.min(255) as u8, Ordering::Relaxed);
}

pub fn tab_info() -> (usize, usize) {
    (TAB_COUNT.load(Ordering::Relaxed) as usize, TAB_SEL.load(Ordering::Relaxed) as usize)
}

pub fn request_tab(idx: usize) {
    TAB_REQUEST.store(idx.min(254) as u8, Ordering::Relaxed);
}

pub fn take_tab_request() -> Option<usize> {
    let v = TAB_REQUEST.swap(u8::MAX, Ordering::Relaxed);
    if v == u8::MAX {
        None
    } else {
        Some(v as usize)
    }
}

/// 在指定位置画一圈虚线框（编辑器选框；用短线段拼，避免新增绘图原语）。
pub fn dashed_rect(ui: &mut Ui, r: Rect, width: f32, dash: f32, gap: f32, color: Color) {
    fn h_seg(ui: &mut Ui, x: f32, y: f32, w: f32, seg: f32, gap: f32, width: f32, color: Color) {
        let mut t = 0.;
        while t < w {
            let e = (t + seg).min(w);
            ui.fill_rect(Rect::new(x + t, y - width / 2., e - t, width), color);
            t = e + gap;
        }
    }
    fn v_seg(ui: &mut Ui, x: f32, y: f32, h: f32, seg: f32, gap: f32, width: f32, color: Color) {
        let mut t = 0.;
        while t < h {
            let e = (t + seg).min(h);
            ui.fill_rect(Rect::new(x - width / 2., y + t, width, e - t), color);
            t = e + gap;
        }
    }
    let seg = dash.max(0.002);
    let gap = gap.max(0.002);
    h_seg(ui, r.x, r.y, r.w, seg, gap, width, color);
    h_seg(ui, r.x, r.bottom(), r.w, seg, gap, width, color);
    v_seg(ui, r.x, r.y, r.h, seg, gap, width, color);
    v_seg(ui, r.right(), r.y, r.h, seg, gap, width, color);
}

// ---------------------------------------------------------------- 编辑层 ----

// 工具条第一行：页面切换。
const BTN_HOME: u8 = 0;
const BTN_LIBRARY: u8 = 1;
const BTN_SETTINGS: u8 = 2;
const BTN_FAVORITES: u8 = 3;
const BTN_MESSAGE: u8 = 4;
const BTN_HISTORY: u8 = 5;
const BTN_RESPACK: u8 = 6;
const BTN_BLACKLIST: u8 = 7;
// 工具条第二行：工具。
const BTN_SNAP: u8 = 10;
const BTN_STEP: u8 = 11;
const BTN_RESET_PAGE: u8 = 12;
const BTN_RESET_ALL: u8 = 13;
const BTN_DONE: u8 = 14;

// 参数行（选中「列表大框」时出现）
const BTN_COLS_DEC: u8 = 20;
const BTN_COLS_INC: u8 = 21;
const BTN_ROWH_DEC: u8 = 22;
const BTN_ROWH_INC: u8 = 23;

/// tab 切换按钮的起始 id。
const BTN_TAB_BASE: u8 = 30;

/// 列表参数的可调范围。
const COLS_RANGE: (f32, f32) = (1., 12.);

/// 行高参数：谱面库是「格高」，设置页是「列表行高」，历史/黑名单是「行高」。
fn row_h_default(page: &str) -> f32 {
    match page {
        "library" => 0.3,
        "settings" => 0.15,
        "history" => 0.082,
        "blacklist" => 0.085,
        _ => 0.15,
    }
}

fn row_h_range(page: &str) -> (f32, f32) {
    match page {
        "library" => (0.12, 0.6),
        "history" | "blacklist" => (0.05, 0.2),
        _ => (0.1, 0.3),
    }
}

fn row_h_step(page: &str) -> f32 {
    match page {
        "library" => 0.02,
        "history" | "blacklist" => 0.005,
        _ => 0.01,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Slot(&'static str),
    Handle(&'static str, usize),
    Btn(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DragKind {
    Move,
    /// 手柄序号：0 左上 1 上 2 右上 3 右 4 右下 5 下 6 左下 7 左
    Resize(usize),
}

struct Drag {
    kind: DragKind,
    key: &'static str,
    page: String,
    anchor: Anchor,
    cap: Cap,
    orig: Rect,
    grab: Vec2,
    ready: bool,
}

/// 编辑层：渲染时登记选框 / 手柄 / 工具条的命中矩形，触摸时据此拖拽。
#[derive(Default)]
pub struct Editor {
    sel: Option<&'static str>,
    hits: Vec<(Hit, RectButton)>,
    drag: Option<Drag>,
    pointer: Option<Vec2>,
    pressed: bool,
    dirty: bool,
}

impl Editor {
    fn frame_slot(key: &str) -> Option<FrameSlot> {
        frame_slots().into_iter().find(|it| it.key == key && it.page == cur_page().key())
    }

    pub fn touch(&mut self, touch: &Touch) -> bool {
        match touch.phase {
            TouchPhase::Started => {
                self.pointer = Some(touch.position);
                self.pressed = true;
                let pos = touch.position;
                // 命中优先级：工具条按钮 > 手柄 > 槽位框。
                // （顶部的换页按钮会被槽位框压住，必须让按钮先被点到。）
                let hit = [0u8, 1, 2].into_iter().find_map(|kind| {
                    self.hits.iter().find_map(|(it, b)| {
                        let k = match it {
                            Hit::Btn(_) => 0u8,
                            Hit::Handle(..) => 1,
                            Hit::Slot(_) => 2,
                        };
                        (k == kind && b.contains(pos)).then_some(*it)
                    })
                });
                match hit {
                    Some(Hit::Handle(key, i)) => {
                        if let Some(s) = Self::frame_slot(key) {
                            self.sel = Some(key);
                            self.drag = Some(Drag {
                                kind: DragKind::Resize(i),
                                key,
                                page: s.page,
                                anchor: s.anchor,
                                cap: s.cap,
                                orig: s.rect,
                                grab: touch.position,
                                ready: false,
                            });
                        }
                    }
                    Some(Hit::Slot(key)) => {
                        self.sel = Some(key);
                        if let Some(s) = Self::frame_slot(key) {
                            if s.cap.0 {
                                self.drag = Some(Drag {
                                    kind: DragKind::Move,
                                    key,
                                    page: s.page,
                                    anchor: s.anchor,
                                    cap: s.cap,
                                    orig: s.rect,
                                    grab: touch.position,
                                    ready: false,
                                });
                            }
                        }
                    }
                    Some(Hit::Btn(b)) => self.button(b),
                    None => {}
                }
                true
            }
            TouchPhase::Moved | TouchPhase::Stationary => {
                self.pointer = Some(touch.position);
                true
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.pressed = false;
                self.pointer = None;
                if self.drag.take().is_some_and(|it| it.ready) {
                    self.dirty = true;
                }
                true
            }
        }
    }

    fn button(&mut self, b: u8) {
        match b {
            BTN_HOME => request_goto(PageId::Home),
            BTN_LIBRARY => request_goto(PageId::Library),
            BTN_SETTINGS => request_goto(PageId::Settings),
            BTN_FAVORITES => request_goto(PageId::Favorites),
            BTN_MESSAGE => request_goto(PageId::Message),
            BTN_HISTORY => request_goto(PageId::History),
            BTN_RESPACK => request_goto(PageId::Respack),
            BTN_BLACKLIST => request_goto(PageId::Blacklist),
            BTN_SNAP => set_snap(!snap_enabled()),
            BTN_STEP => cycle_snap_step(),
            BTN_RESET_PAGE => {
                reset(Some(cur_page().key()));
                self.dirty = true;
            }
            BTN_RESET_ALL => {
                reset(None);
                self.dirty = true;
            }
            BTN_DONE => {
                set_edit(false);
                self.dirty = true;
            }
            BTN_COLS_DEC => {
                let v = param("library", "cols", 4.).round();
                set_param("library", "cols", (v - 1.).clamp(COLS_RANGE.0, COLS_RANGE.1));
                self.dirty = true;
            }
            BTN_COLS_INC => {
                let v = param("library", "cols", 4.).round();
                set_param("library", "cols", (v + 1.).clamp(COLS_RANGE.0, COLS_RANGE.1));
                self.dirty = true;
            }
            BTN_ROWH_DEC => {
                let page = cur_page().key();
                let v = param(page, "row_h", row_h_default(page));
                set_param(page, "row_h", (v - row_h_step(page)).clamp(row_h_range(page).0, row_h_range(page).1));
                self.dirty = true;
            }
            BTN_ROWH_INC => {
                let page = cur_page().key();
                let v = param(page, "row_h", row_h_default(page));
                set_param(page, "row_h", (v + row_h_step(page)).clamp(row_h_range(page).0, row_h_range(page).1));
                self.dirty = true;
            }
            // 30+ 是「切换 tab」按钮（起始 id + 分页序号）
            b if b >= BTN_TAB_BASE => request_tab((b - BTN_TAB_BASE) as usize),
            _ => {}
        }
    }

    pub fn render(&mut self, ui: &mut Ui) {
        self.hits.clear();
        let page = cur_page().key();
        // 网格：吸附开启时画出来，给吸附一个可依赖的参照（不然手感会觉得"没吸上"）。
        if snap_enabled() {
            let step = snap_step();
            let line = Color::new(0.85, 0.92, 1., 0.22);
            let w = 0.0012;
            let top = ui.top;
            let mut x = (-1. / step).ceil() * step;
            while x <= 1. {
                ui.fill_rect(Rect::new(x - w / 2., -top, w, top * 2.), line);
                x += step;
            }
            let mut y = (-top / step).ceil() * step;
            while y <= top {
                ui.fill_rect(Rect::new(-1., y - w / 2., 2., w), line);
                y += step;
            }
        }
        // 1) 先把拖拽落到布局上（需要用 ui 做屏幕坐标 → UI 坐标换算）。
        let (pressed, pointer) = (self.pressed, self.pointer);
        if let (Some(d), Some(p)) = (&mut self.drag, pointer.filter(|_| pressed)) {
            // 按下时的屏幕位置换算成 UI 坐标，和当前位置求差 —— 没真正移动就绝不改布局，
            // 否则「点一下选中」也会被网格吸附、元素自己跳一下。
            let l0 = ui.to_local((d.grab.x, d.grab.y));
            let l1 = ui.to_local((p.x, p.y));
            let delta = vec2(l1.0 - l0.0, l1.1 - l0.1);
            if delta.length_squared() > 1e-8 {
                let r = match d.kind {
                    DragKind::Move => {
                        // 吸附对齐到框的左上角，这样框边正好压在网格线上，吸附感明显。
                        let cx = snap(d.orig.x + delta.x);
                        let cy = snap(d.orig.y + delta.y);
                        Rect::new(cx, cy, d.orig.w, d.orig.h)
                    }
                    DragKind::Resize(i) => resize_rect(d.orig, i, delta, d.cap),
                };
                set(&d.page, d.key, slot_from_rect(ui, d.anchor, r));
                self.dirty = true;
                d.ready = true;
            }
        }

        // 2) 选框 / 手柄。
        let slots = frame_slots();
        let slots: Vec<_> = slots.into_iter().filter(|it| it.page == page).collect();
        let sel_rect = slots.iter().find(|it| Some(it.key) == self.sel).map(|it| it.rect);
        for it in &slots {
            let selected = Some(it.key) == self.sel;
            let color = if selected {
                Color::new(1., 0.88, 0.35, 1.)
            } else if it.edited {
                Color::new(0.55, 0.95, 0.6, 0.8)
            } else {
                Color::new(1., 0.72, 0.2, 0.7)
            };
            dashed_rect(ui, it.rect, 0.0035, 0.014, 0.01, color);
            let mut b = RectButton::new();
            b.set(ui, it.rect);
            self.hits.push((Hit::Slot(it.key), b));
        }
        if let (Some(rect), Some(key)) = (sel_rect, self.sel) {
            ui.text(key.to_owned())
                .pos(rect.x, rect.y - 0.012)
                .anchor(0., 1.)
                .size(0.4)
                .color(Color::new(1., 0.9, 0.5, 1.))
                .draw_using(&BOLD_FONT);
            let cap = slots.iter().find(|it| it.key == key).map(|it| it.cap).unwrap_or(Cap(true, true, true));
            for i in 0..8 {
                if let Some(hr) = handle_rect(rect, i, cap) {
                    ui.fill_rect(hr, Color::new(1., 0.9, 0.4, 0.95));
                    let mut b = RectButton::new();
                    b.set(ui, hr.feather(0.02));
                    self.hits.push((Hit::Handle(key, i), b));
                }
            }
        }

        // 3) 工具条：第一行页面切换，第二行工具（页数多了以后拆成两行，避免超屏）。
        // 按钮统一尺寸；若某语言的文案太长，则整行统一缩小字号（而不是截断成「玩…」）。
        let (w, h, gap) = (0.2, 0.062, 0.01);
        let radius = 0.016;
        let y = -ui.top + 0.026;
        let page_labels = [
            tl!("hud-page-home"),
            tl!("hud-page-library"),
            tl!("hud-page-settings"),
            tl!("hud-page-favorites"),
            tl!("hud-page-message"),
            tl!("hud-page-history"),
            tl!("hud-page-respack"),
            tl!("hud-page-blacklist"),
        ];
        let page_max_w = w - 0.02;
        let mut page_need = 0f32;
        for label in &page_labels {
            page_need = page_need.max(ui.text(label.as_ref()).size(0.42).no_baseline().measure().w);
        }
        let page_size = if page_need > page_max_w && page_need > 0. {
            0.42 * page_max_w / page_need
        } else {
            0.42
        };
        let total = page_labels.len() as f32 * w + (page_labels.len() as f32 - 1.) * gap;
        let mut x = -total / 2.;
        for (i, label) in page_labels.into_iter().enumerate() {
            let r = Rect::new(x, y, w, h);
            let active = cur_page() == PageId::ALL[i];
            ui.fill_path(&r.rounded(radius), if active { Color::new(1., 0.8, 0.3, 0.95) } else { Color::new(0., 0., 0., 0.6) });
            ui.text(label)
                .pos(r.x + r.w / 2., r.y + r.h / 2.)
                .anchor(0.5, 0.5)
                .no_baseline()
                .max_width(page_max_w)
                .size(page_size)
                .color(if active { Color::new(0.1, 0.1, 0.1, 1.) } else { WHITE })
                .draw();
            let mut b = RectButton::new();
            b.set(ui, r);
            self.hits.push((Hit::Btn(i as u8), b));
            x += w + gap;
        }
        let y2 = y + h + 0.008;
        let tool_labels = [
            if snap_enabled() { tl!("hud-snap-on") } else { tl!("hud-snap-off") },
            format!("{:.3}", snap_step()).into(),
            tl!("hud-reset-page"),
            tl!("hud-reset-all"),
            tl!("hud-done"),
        ];
        let mut tool_need = 0f32;
        for label in &tool_labels {
            tool_need = tool_need.max(ui.text(label.as_ref()).size(0.42).no_baseline().measure().w);
        }
        let tool_size = if tool_need > page_max_w && tool_need > 0. {
            0.42 * page_max_w / tool_need
        } else {
            0.42
        };
        let total2 = tool_labels.len() as f32 * w + (tool_labels.len() as f32 - 1.) * gap;
        let mut x = -total2 / 2.;
        for (i, label) in tool_labels.into_iter().enumerate() {
            let r = Rect::new(x, y2, w, h);
            ui.fill_path(&r.rounded(radius), Color::new(0., 0., 0., 0.6));
            ui.text(label)
                .pos(r.x + r.w / 2., r.y + r.h / 2.)
                .anchor(0.5, 0.5)
                .no_baseline()
                .max_width(page_max_w)
                .size(tool_size)
                .color(WHITE)
                .draw();
            let mut b = RectButton::new();
            b.set(ui, r);
            self.hits.push((Hit::Btn(BTN_SNAP + i as u8), b));
            x += w + gap;
        }

        // 3.5) 参数行：选中「列表大框」时出现（谱面库有列数 + 格高，其它列表页只有行高）。
        if self.sel == Some("list") && matches!(page, "library" | "settings" | "history" | "blacklist") {
            let rh = param(page, "row_h", row_h_default(page));
            let mut items: Vec<(u8, String)> = Vec::new();
            if page == "library" {
                items.push((BTN_COLS_DEC, format!("{} -", tl!("hud-cols"))));
                items.push((BTN_COLS_INC, format!("{} +", tl!("hud-cols"))));
            }
            items.push((BTN_ROWH_DEC, format!("{} -", tl!("hud-row-h"))));
            items.push((BTN_ROWH_INC, format!("{} +", tl!("hud-row-h"))));
            let readout = if page == "library" {
                format!(
                    "{} {}   {} {:.2}",
                    tl!("hud-cols"),
                    param("library", "cols", 4.).round() as i32,
                    tl!("hud-row-h"),
                    rh
                )
            } else {
                format!("{} {:.2}", tl!("hud-row-h"), rh)
            };
            let (bw, bh, bg) = (0.2, 0.07, 0.012);
            let total = items.len() as f32 * bw + (items.len() as f32 - 1.) * bg;
            let mut bx = -total / 2.;
            let by = y2 + h + 0.03;
            ui.text(readout)
                .pos(0., by + bh + 0.014)
                .anchor(0.5, 0.)
                .no_baseline()
                .size(0.38)
                .color(WHITE)
                .draw();
            for (id, label) in items {
                let r = Rect::new(bx, by, bw, bh);
                ui.fill_path(&r.rounded(0.014), Color::new(0., 0., 0., 0.6));
                ui.text(label)
                    .pos(r.x + r.w / 2., r.y + r.h / 2.)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .max_width(r.w * 0.94)
                    .size(0.38)
                    .color(WHITE)
                    .draw();
                let mut b = RectButton::new();
                b.set(ui, r);
                self.hits.push((Hit::Btn(id), b));
                bx += bw + bg;
            }
        }

        // 3.6) 分页切换行：选中 tab 栏时出现，用来编辑其它分页里的元素
        // （例如谱面库默认在「本地」，切到「在线」才能编辑它的翻页按钮）。
        let (tab_count, tab_sel) = tab_info();
        if self.sel == Some("tabs") && tab_count > 1 {
            let (bw, bh, bg) = (0.14, 0.07, 0.01);
            let total = tab_count as f32 * bw + (tab_count as f32 - 1.) * bg;
            let mut bx = -total / 2.;
            let by = y2 + h + 0.03;
            for i in 0..tab_count {
                let r = Rect::new(bx, by, bw, bh);
                let active = i == tab_sel;
                ui.fill_path(&r.rounded(0.014), if active { Color::new(1., 0.8, 0.3, 0.95) } else { Color::new(0., 0., 0., 0.6) });
                ui.text(format!("{}", i + 1))
                    .pos(r.x + r.w / 2., r.y + r.h / 2.)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(if active { Color::new(0.1, 0.1, 0.1, 1.) } else { WHITE })
                    .draw();
                let mut b = RectButton::new();
                b.set(ui, r);
                self.hits.push((Hit::Btn(BTN_TAB_BASE + i as u8), b));
                bx += bw + bg;
            }
        }

        // 4) 落盘（松手后才写文件，避免每帧 IO）。
        if self.dirty && !self.pressed {
            let _ = save();
            self.dirty = false;
        }
    }
}

thread_local! {
    static EDITOR: RefCell<Editor> = RefCell::new(Editor::default());
}

/// 编辑模式下把触摸交给编辑层（返回 true 表示已消费，页面不再收到该触摸）。
pub fn editor_touch(touch: &Touch) -> bool {
    EDITOR.with(|it| it.borrow_mut().touch(touch))
}

/// 叠加绘制编辑层（在页面渲染之后调用）。
pub fn editor_render(ui: &mut Ui) {
    EDITOR.with(|it| it.borrow_mut().render(ui));
}

/// 启动自检：设了 `PHIRA_HUD_DEBUG=1` 时把编辑器用到的文案打到日志，便于排查「文字显示为 -」这类本地化问题。
pub fn selftest() {
    if std::env::var("PHIRA_HUD_DEBUG").is_err() {
        return;
    }
    for k in [
        "hud-page-home",
        "hud-page-library",
        "hud-page-settings",
        "hud-page-favorites",
        "hud-page-message",
        "hud-page-history",
        "hud-page-respack",
        "hud-page-blacklist",
        "hud-snap-on",
        "hud-snap-off",
        "hud-reset-page",
        "hud-reset-all",
        "hud-done",
    ] {
        let v = tl!(k);
        eprintln!("hud label [{k}] = [{v}] len={}", v.len());
    }
}

/// 手柄位置（不受该槽位能力限制的手柄返回 None）。
fn handle_rect(r: Rect, i: usize, cap: Cap) -> Option<Rect> {
    let s = 0.016;
    let (x, y) = match i {
        0 => (r.x, r.y),
        1 => (r.x + r.w / 2., r.y),
        2 => (r.right(), r.y),
        3 => (r.right(), r.y + r.h / 2.),
        4 => (r.right(), r.bottom()),
        5 => (r.x + r.w / 2., r.bottom()),
        6 => (r.x, r.bottom()),
        _ => (r.x, r.y + r.h / 2.),
    };
    let (needs_x, needs_y) = match i {
        0 | 2 | 4 | 6 => (true, true),
        1 | 5 => (false, true),
        _ => (true, false),
    };
    if (needs_x && !cap.1) || (needs_y && !cap.2) {
        return None;
    }
    Some(Rect::new(x - s / 2., y - s / 2., s, s))
}

/// 按手柄拖拽得到的新矩形（只改该槽位允许的轴，并按网格吸附）。
fn resize_rect(orig: Rect, i: usize, delta: Vec2, cap: Cap) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (orig.x, orig.y, orig.right(), orig.bottom());
    let dx = delta.x;
    let dy = delta.y;
    match i {
        0 => {
            x0 += dx;
            y0 += dy;
        }
        1 => y0 += dy,
        2 => {
            x1 += dx;
            y0 += dy;
        }
        3 => x1 += dx,
        4 => {
            x1 += dx;
            y1 += dy;
        }
        5 => y1 += dy,
        6 => {
            x0 += dx;
            y1 += dy;
        }
        _ => x0 += dx,
    }
    // 不允许改的轴保持原样
    if !cap.1 {
        x0 = orig.x;
        x1 = orig.right();
    }
    if !cap.2 {
        y0 = orig.y;
        y1 = orig.bottom();
    }
    let (x0, x1) = (snap(x0).min(snap(x1)), snap(x0).max(snap(x1)));
    let (y0, y1) = (snap(y0).min(snap(y1)), snap(y0).max(snap(y1)));
    Rect::new(x0, y0, (x1 - x0).max(0.02), (y1 - y0).max(0.02))
}
