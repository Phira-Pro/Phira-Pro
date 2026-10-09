prpr_l10n::tl_file!("settings");
mod experience;
mod judgement;
use experience::ExperienceList;
use judgement::JudgementPage;

use super::{BlacklistPage, HistoryPage, NextPage, OffsetPage, Page, SharedState, TransferPage};
use crate::{
    dir, get_data, get_data_mut,
    popup::ChooseButton,
    save_data,
    scene::BGM_VOLUME_UPDATED,
    sync_data,
    tabs::{Tabs, TitleFn},
};
use anyhow::Result;
use bytesize::ByteSize;
use inputbox::InputBox;
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use prpr::ui::Dialog;
use prpr::{
    core::BOLD_FONT,
    ext::{open_url, poll_future, semi_white, LocalTask, RectExt, SafeTexture},
    scene::{request_input, return_input, show_error, show_message, take_input},
    task::Task,
    ui::{DRectButton, RectButton, Scroll, Slider, Ui, PREFER_REDUCED_MOTION, UI_SFX_VOLUME},
};
use prpr_l10n::{LanguageIdentifier, LANG_IDENTS, LANG_NAMES};
use reqwest::Url;
use serde::Deserialize;
use std::{borrow::Cow, fs, io, path::PathBuf, sync::atomic::Ordering};

/// HUD 自定义：设置列表的行高（默认 0.15，可在编辑模式里调）。
fn item_row_h() -> f32 {
    crate::hud::param("settings", "row_h", 0.15).clamp(0.1, 0.3)
}
const INTERACT_WIDTH: f32 = 0.26;

/// 软件 UI 主题预设：(强调色, 表面色)，均为 0xRRGGBB。
const UI_PRESETS: [(u32, u32); 6] = [
    (0x2196f3, 0x2a323c),
    (0x9c6bff, 0x2e2a3c),
    (0x22c55e, 0x243029),
    (0xff7043, 0x3a2a26),
    (0xf06292, 0x3a2833),
    (0xb0bec5, 0x263238),
];
/// 调试触点可选颜色（0xRRGGBB）。点一次循环切到下一个。
const TOUCH_POINT_COLORS: [u32; 8] = [0xff3b30, 0x34c759, 0x0a84ff, 0xffd60a, 0xff2d55, 0x5ac8fa, 0xffffff, 0x8e8e93];

/// 主题预设的显示名。
fn ui_preset_name(i: usize) -> String {
    match i {
        0 => tl!("theme-blue"),
        1 => tl!("theme-violet"),
        2 => tl!("theme-emerald"),
        3 => tl!("theme-sunset"),
        4 => tl!("theme-rose"),
        _ => tl!("theme-graphite"),
    }
    .into_owned()
}

struct NameList(String);
impl<'de> Deserialize<'de> for NameList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = Vec::<String>::deserialize(deserializer)?;
        Ok(Self(s.join(", ")))
    }
}

#[derive(Deserialize)]
struct LocalizationListRaw {
    #[serde(rename = "en-US")]
    en_us: NameList,
    #[serde(rename = "fr-FR")]
    fr_fr: NameList,
    #[serde(rename = "de-DE")]
    de_de: NameList,
    #[serde(rename = "id-ID")]
    id_id: NameList,
    #[serde(rename = "ja-JP")]
    ja_jp: NameList,
    #[serde(rename = "ko-KR")]
    ko_kr: NameList,
    #[serde(rename = "pl-PL")]
    pl_pl: NameList,
    #[serde(rename = "pt-BR")]
    pt_br: NameList,
    #[serde(rename = "ru-RU")]
    ru_ru: NameList,
    #[serde(rename = "th-TH")]
    th_th: NameList,
    #[serde(rename = "zh-TW")]
    zh_tw: NameList,
    #[serde(rename = "tr-TR")]
    tr_tr: NameList,
    #[serde(rename = "vi-VN")]
    vi_vn: NameList,
}

struct LocalizationList(String);
impl<'de> Deserialize<'de> for LocalizationList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = LocalizationListRaw::deserialize(deserializer)?;
        Ok(Self(format!(
            "\
English (en-US)\n{}\n
French (fr-FR)\n{}\n
German (de-DE)\n{}\n
Indonesian (id-ID)\n{}\n
Japanese (ja-JP)\n{}\n
Korean (ko-KR)\n{}\n
Polish (pl-PL)\n{}\n
Portuguese (pt-BR)\n{}\n
Russian (ru-RU)\n{}\n
Thai (th-TH)\n{}\n
Traditional Chinese (zh-TW)\n{}\n
Turkish (tr-TR)\n{}\n
Vietnamese (vi-VN)\n{}",
            raw.en_us.0,
            raw.fr_fr.0,
            raw.de_de.0,
            raw.id_id.0,
            raw.ja_jp.0,
            raw.ko_kr.0,
            raw.pl_pl.0,
            raw.pt_br.0,
            raw.ru_ru.0,
            raw.th_th.0,
            raw.zh_tw.0,
            raw.tr_tr.0,
            raw.vi_vn.0
        )))
    }
}

#[derive(Deserialize)]
struct StaffList {
    development: NameList,
    operations: NameList,
    documentation: NameList,
    art: NameList,
    music: NameList,
    audio: NameList,
    community: NameList,
    revision: NameList,
    localization: LocalizationList,
}

static STAFF_LIST: Lazy<StaffList> = Lazy::new(|| {
    let data = include_str!("../../staff.yml");
    serde_yaml::from_str(data).unwrap()
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingListType {
    General,
    Audio,
    Chart,
    Debug,
    Experience,
    About,
}

pub struct SettingsPage {
    list_general: GeneralList,
    list_audio: AudioList,
    list_chart: ChartList,
    list_debug: DebugList,
    list_experience: ExperienceList,

    tabs: Tabs<SettingListType>,

    scroll: Scroll,
    save_time: f32,

    icon: SafeTexture,
}

impl SettingsPage {
    const SAVE_TIME: f32 = 0.5;

    pub fn new(icon: SafeTexture, icon_lang: SafeTexture) -> Self {
        Self {
            list_general: GeneralList::new(icon_lang),
            list_audio: AudioList::new(),
            list_chart: ChartList::new(),
            list_debug: DebugList::new(),
            list_experience: ExperienceList::new(),

            tabs: Tabs::new([
                (SettingListType::General, || tl!("general")),
                (SettingListType::Audio, || tl!("audio")),
                (SettingListType::Chart, || tl!("chart")),
                (SettingListType::Debug, || tl!("debug")),
                (SettingListType::Experience, || tl!("experience")),
                (SettingListType::About, || tl!("about")),
            ] as [(SettingListType, TitleFn); 6]),

            scroll: Scroll::new(),
            save_time: f32::INFINITY,

            icon,
        }
    }
}

impl Page for SettingsPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn exit(&mut self) -> Result<()> {
        BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
        if self.save_time.is_finite() {
            save_data()?;
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if match self.tabs.selected() {
            SettingListType::General => self.list_general.top_touch(touch, t),
            SettingListType::Audio => self.list_audio.top_touch(touch, t),
            SettingListType::Chart => self.list_chart.top_touch(touch, t),
            SettingListType::Debug => self.list_debug.top_touch(touch, t),
            SettingListType::Experience => self.list_experience.top_touch(touch, t),
            SettingListType::About => false,
        } {
            return Ok(true);
        }

        if self.tabs.touch(touch, s.rt) {
            return Ok(true);
        }

        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if let Some(p) = match self.tabs.selected() {
            SettingListType::General => self.list_general.touch(touch, t)?,
            SettingListType::Audio => self.list_audio.touch(touch, t)?,
            SettingListType::Chart => self.list_chart.touch(touch, t)?,
            SettingListType::Debug => self.list_debug.touch(touch, t)?,
            SettingListType::Experience => self.list_experience.touch(touch, t)?,
            SettingListType::About => None,
        } {
            if p {
                self.save_time = t;
            }
            self.scroll.y_scroller.halt();
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        if self.tabs.changed() { self.scroll.y_scroller.reset(); }
        let changed = match self.tabs.selected() {
            SettingListType::General => self.list_general.update(t)?,
            SettingListType::Audio => self.list_audio.update(t)?,
            SettingListType::Chart => self.list_chart.update(t)?,
            SettingListType::Debug => self.list_debug.update(t)?,
            SettingListType::Experience => self.list_experience.update(t)?,
            SettingListType::About => false,
        };
        self.scroll.update(t);
        if changed {
            self.save_time = t;
        }
        if t > self.save_time + Self::SAVE_TIME {
            save_data()?;
            self.save_time = f32::INFINITY;
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;

        s.fader.render(ui, s.t, |ui| {
            let r = ui.content_rect();
            self.tabs.render(ui, rt, r, |ui, item| {
                // HUD 自定义：设置列表区域（整体移动 / 缩放；行高仍是固定的 item_row_h()）。
                const SLOT_LIST: crate::hud::SlotDef = crate::hud::SlotDef::centered("list", [0., 0., 1.65, 1.1], crate::hud::Cap(true, true, true));
                let r = if crate::hud::has("settings", "list") {
                    crate::hud::slot(ui, "settings", SLOT_LIST)
                } else {
                    let r0 = r.feather(-0.01);
                    crate::hud::register("settings", SLOT_LIST, r0);
                    r0
                };
                self.scroll.size((r.w, r.h));
                ui.scope(|ui| {
                    ui.dx(r.x);
                    ui.dy(r.y);
                    self.scroll.render(ui, |ui| match item {
                        SettingListType::General => self.list_general.render(ui, r, t),
                        SettingListType::Audio => self.list_audio.render(ui, r, t),
                        SettingListType::Chart => self.list_chart.render(ui, r, t),
                        SettingListType::Debug => self.list_debug.render(ui, r, t),
                        SettingListType::Experience => self.list_experience.render(ui, r, t),
                        SettingListType::About => render_about(ui, r, &self.icon),
                    });
                });

                Ok(())
            })
        })?;

        if *self.tabs.selected() == SettingListType::Experience {
            self.list_experience.render_top(ui, t);
        }

        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page_inner()
    }
}

impl SettingsPage {
    fn next_page_inner(&mut self) -> NextPage {
        if matches!(self.tabs.selected(), SettingListType::General) {
            return self.list_general.next_page().unwrap_or_default();
        }
        if matches!(self.tabs.selected(), SettingListType::Audio) {
            return self.list_audio.next_page().unwrap_or_default();
        }
        if matches!(self.tabs.selected(), SettingListType::Chart) {
            return self.list_chart.next_page().unwrap_or_default();
        }
        NextPage::None
    }
}

fn render_about(ui: &mut Ui, mut r: Rect, icon: &SafeTexture) -> (f32, f32) {
    r.x = 0.;
    r.y = 0.;
    let ow = r.w;
    let r = r.feather(-0.02);

    let ct = r.center();
    let s = 0.1;
    let ir = Rect::new(ct.x - s, r.y + 0.05, s * 2., s * 2.);
    ui.fill_path(&ir.rounded(0.02), (**icon, ir));

    let staff = &*STAFF_LIST;
    let text = tl!(
        "about-content",
        "version" => format!("{} ({})", crate::PRO_VERSION, env!("GIT_HASH")),

        "development" => &staff.development.0,
        "operations" => &staff.operations.0,
        "documentation" => &staff.documentation.0,
        "art" => &staff.art.0,
        "music" => &staff.music.0,
        "audio" => &staff.audio.0,
        "community" => &staff.community.0,
        "revision" => &staff.revision.0,
        "localization" => &staff.localization.0
    );
    let (first, text) = text.split_once('\n').unwrap();
    let tr = ui
        .text(first)
        .pos(ct.x, ir.bottom() + 0.03)
        .anchor(0.5, 0.)
        .size(0.6)
        .draw_using(&BOLD_FONT);

    let r = ui
        .text(text.trim())
        .pos(r.x, tr.bottom() + 0.06)
        .size(0.55)
        .multiline()
        .max_width(r.w)
        .h_center()
        .draw();

    (ow, r.bottom() + 0.09)
}

fn render_title<'a>(ui: &mut Ui, title: impl Into<Cow<'a, str>>, subtitle: Option<Cow<'a, str>>) -> f32 {
    const TITLE_SIZE: f32 = 0.6;
    const SUBTITLE_SIZE: f32 = 0.35;
    const LEFT: f32 = 0.06;
    const PAD: f32 = 0.01;
    const SUB_MAX_WIDTH: f32 = 1.2;
    if let Some(subtitle) = subtitle {
        let title = title.into();
        let r1 = ui.text(Cow::clone(&title)).no_baseline().size(TITLE_SIZE).max_width(SUB_MAX_WIDTH).measure();
        let r2 = ui
            .text(Cow::clone(&subtitle))
            .size(SUBTITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .no_baseline()
            .measure();
        let h = r1.h + PAD + r2.h;
        let r1 = ui
            .text(subtitle)
            .pos(LEFT, (item_row_h() + h) / 2.)
            .anchor(0., 1.)
            .no_baseline()
            .size(SUBTITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .color(semi_white(0.6))
            .draw()
            .right();
        let r2 = ui
            .text(title)
            .pos(LEFT, (item_row_h() - h) / 2.)
            .no_baseline()
            .size(TITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .draw()
            .right();
        r1.max(r2)
    } else {
        ui.text(title.into())
            .pos(LEFT, item_row_h() / 2.)
            .anchor(0., 0.5)
            .no_baseline()
            .size(TITLE_SIZE)
            .draw()
            .right()
    }
}

#[inline]
fn render_switch(ui: &mut Ui, r: Rect, t: f32, btn: &mut DRectButton, on: bool) {
    btn.render_text(ui, r, t, if on { ttl!("switch-on") } else { ttl!("switch-off") }, 0.5, on);
}

#[inline]
fn right_rect(w: f32) -> Rect {
    let rh = item_row_h() * 2. / 3.;
    Rect::new(w - 0.3, (item_row_h() - rh) / 2., INTERACT_WIDTH, rh)
}

/// 「恢复默认设置」的确认弹窗。
fn confirm_reset_settings() {
    Dialog::plain(tl!("reset-settings-title"), tl!("reset-settings-text").into_owned())
        .buttons(vec![tl!("reset-settings-cancel").into_owned(), tl!("reset-settings-confirm").into_owned()])
        .listener(|_dialog, pos| {
            if pos == 1 {
                reset_all_settings();
            }
            false
        })
        .show();
}

/// 一键把「设置」恢复为默认值：设置页上的全部选项（含语言、主题、判定 / 玩法、调试等）
/// 都会回到默认；账号、谱面、成绩与已导入资源都不受影响。
fn reset_all_settings() {
    let defaults = crate::data::Data::default();
    {
        let data = get_data_mut();
        data.config = defaults.config;
        data.judge_preset_id = None;
        // 语言回到「跟随系统」，下面 sync_data() 会立刻生效。
        data.language = None;
        data.prefer_reduced_motion = defaults.prefer_reduced_motion;
        data.accept_invalid_cert = defaults.accept_invalid_cert;
        data.enable_anys = defaults.enable_anys;
        data.anys_gateway = defaults.anys_gateway;
    }
    // 同步依赖这些配置的全局状态，否则要重启才生效。
    {
        let data = get_data();
        PREFER_REDUCED_MOTION.store(data.prefer_reduced_motion, Ordering::Relaxed);
        prpr::ui::SHOW_FPS.store(data.config.show_fps, Ordering::Relaxed);
        UI_SFX_VOLUME.store(data.config.volume_sfx.to_bits(), Ordering::Relaxed);
        data.config.apply_ui_colors();
    }
    BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
    sync_data();
    let _ = save_data();
    show_message(tl!("reset-settings-done")).ok();
}

/// 移动端（Android / iOS / OHOS）触发系统文件选择器；桌面端为空操作，
/// 桌面走 `rfd::FileDialog`。选中的文件在 `GeneralList::update` 里按 id 取回。
#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
fn request_mobile_file(id: &'static str) {
    prpr::scene::request_file(id);
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
fn request_mobile_file(_id: &'static str) {}

/// 桌面端选图（PNG / JPG / WebP / BMP）。移动端走系统选择器（iOS 为相册）。
#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
fn pick_image(title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("image", &["png", "jpg", "jpeg", "webp", "bmp"])
        .pick_file()
}


/// 桌面端选音频（背景音乐）。
#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
fn pick_audio(title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter("audio", &["mp3", "ogg", "wav", "flac", "m4a", "aac"])
        .pick_file()
}

struct GeneralList {
    icon_lang: SafeTexture,

    lang_btn: ChooseButton,

    #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
    fullscreen_btn: DRectButton,

    /// 自定义 APP 图标（导入 / 恢复默认）。桌面端改窗口图标，重启后生效。
    app_icon_btn: DRectButton,
    app_icon_reset_btn: DRectButton,
    /// 自定义主界面背景（导入 / 恢复默认）。
    app_bg_btn: DRectButton,
    app_bg_reset_btn: DRectButton,
    /// 自定义主界面背景音乐（导入 / 恢复默认）。
    app_bgm_btn: DRectButton,
    app_bgm_reset_btn: DRectButton,
    /// 自定义立绘（导入 / 恢复默认）。
    appearance_import_btn: DRectButton,
    appearance_reset_btn: DRectButton,
    font_btn: DRectButton,
    font_reset_btn: DRectButton,
    /// 当前是否已导入自定义界面字体（每次进设置页时探测一次）。
    has_custom_font: bool,
    /// 界面主题下拉框，用法与语言选择一致。
    theme_btn: ChooseButton,
    /// 主题下拉框里的文案对应的界面语言；语言变了要重建选项。
    theme_lang: Option<String>,
    cache_btn: DRectButton,
    offline_btn: DRectButton,
    server_status_btn: DRectButton,
    /// 打开「API 地址」输入（自建 / 私服用）。
    api_url_btn: DRectButton,
    /// 打开「Web 前端地址」输入（自建 / 私服用）。
    web_url_btn: DRectButton,
    /// 打开「服务器状态页地址」输入（自建 / 私服用）。
    status_url_btn: DRectButton,
    #[cfg(not(target_env = "ohos"))]
    lowq_btn: DRectButton,
    prefer_reduced_motion_btn: DRectButton,
    insecure_btn: DRectButton,
    enable_anys_btn: DRectButton,
    anys_gateway_btn: DRectButton,
    /// 打开「玩家黑名单」管理页。
    blacklist_btn: DRectButton,
    /// 进入 HUD 自定义编辑模式。
    hud_btn: DRectButton,
    /// 打开「数据迁移」页。
    transfer_btn: DRectButton,
    /// 打开「备份与还原」页。
    backup_btn: DRectButton,
    /// 一键把设置恢复为默认值。
    reset_settings_btn: DRectButton,
    next_page: Option<NextPage>,

    cache_size: Option<u64>,
    cache_task: Option<Task<Result<u64>>>,
}

impl GeneralList {
    pub fn new(icon_lang: SafeTexture) -> Self {
        let mut this = Self {
            icon_lang,

            lang_btn: ChooseButton::new()
                .with_options(LANG_NAMES.iter().map(|s| s.to_string()).collect())
                .with_selected(
                    get_data()
                        .language
                        .as_ref()
                        .and_then(|it| it.parse::<LanguageIdentifier>().ok())
                        .and_then(|ident| LANG_IDENTS.iter().position(|it| *it == ident))
                        .unwrap_or_default(),
                ),

            #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
            fullscreen_btn: DRectButton::new(),

            app_icon_btn: DRectButton::new(),
            app_icon_reset_btn: DRectButton::new(),
            app_bg_btn: DRectButton::new(),
            app_bg_reset_btn: DRectButton::new(),
            app_bgm_btn: DRectButton::new(),
            app_bgm_reset_btn: DRectButton::new(),
            appearance_import_btn: DRectButton::new(),
            appearance_reset_btn: DRectButton::new(),
            font_btn: DRectButton::new(),
            font_reset_btn: DRectButton::new(),
            has_custom_font: dir::custom_font_path().map(|it| PathBuf::from(it).exists()).unwrap_or(false),
            theme_btn: {
                // 按当前配置的强调色定位到对应预设。
                let cur = u32::from_str_radix(get_data().config.ui_accent.trim_start_matches('#'), 16).ok();
                let idx = UI_PRESETS.iter().position(|it| Some(it.0) == cur).unwrap_or(0);
                ChooseButton::new()
                    .with_options((0..UI_PRESETS.len()).map(ui_preset_name).collect())
                    .with_selected(idx)
            },
            theme_lang: None,
            cache_btn: DRectButton::new(),
            offline_btn: DRectButton::new(),
            server_status_btn: DRectButton::new(),
            api_url_btn: DRectButton::new(),
            web_url_btn: DRectButton::new(),
            status_url_btn: DRectButton::new(),
            #[cfg(not(target_env = "ohos"))]
            lowq_btn: DRectButton::new(),
            prefer_reduced_motion_btn: DRectButton::new(),
            insecure_btn: DRectButton::new(),
            enable_anys_btn: DRectButton::new(),
            anys_gateway_btn: DRectButton::new(),
            blacklist_btn: DRectButton::new(),
            hud_btn: DRectButton::new(),
            transfer_btn: DRectButton::new(),
            backup_btn: DRectButton::new(),
            reset_settings_btn: DRectButton::new(),
            next_page: None,

            cache_size: None,
            cache_task: None,
        };
        let _ = this.update_cache_size();
        this
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.lang_btn.top_touch(touch, t) {
            return true;
        }
        if self.theme_btn.top_touch(touch, t) {
            return true;
        }
        false
    }

    pub fn next_page(&mut self) -> Option<NextPage> {
        self.next_page.take()
    }

    fn dir_size(path: impl Into<PathBuf>) -> io::Result<u64> {
        fn inner(mut dir: fs::ReadDir) -> io::Result<u64> {
            dir.try_fold(0, |acc, file| {
                let file = file?;
                let size = match file.metadata()? {
                    data if data.is_dir() => inner(fs::read_dir(file.path())?)?,
                    data => data.len(),
                };
                Ok(acc + size)
            })
        }

        inner(fs::read_dir(path.into())?)
    }

    fn update_cache_size(&mut self) -> Result<()> {
        self.cache_size = None;

        let cache_dir = dir::cache()?;
        self.cache_task = Some(Task::new(async { Ok(Self::dir_size(cache_dir)?) }));
        Ok(())
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.lang_btn.touch(touch, t) {
            return Ok(Some(false));
        }

        // Phira Pro：自定义 APP 图标（桌面改窗口图标，重启后生效；移动端存入 data/appearance/icon.*）。
        if self.app_icon_btn.touch(touch, t) {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
            if let Some(path) = pick_image(&tl!("item-app-icon")) {
                match dir::import_appearance("icon", &path) {
                    Ok(()) => {
                        show_message(tl!("item-app-icon-imported")).ok();
                    }
                    Err(err) => show_error(err),
                }
            }
            request_mobile_file("icon_import");
            return Ok(Some(true));
        }
        if self.app_icon_reset_btn.touch(touch, t) {
            match dir::clear_appearance("icon") {
                Ok(_) => {
                    show_message(tl!("item-app-icon-reset-done")).ok();
                }
                Err(err) => show_error(err),
            }
            return Ok(Some(true));
        }
        // Phira Pro：自定义背景。
        if self.app_bg_btn.touch(touch, t) {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
            if let Some(path) = pick_image(&tl!("item-app-bg")) {
                match dir::import_appearance("background", &path) {
                    Ok(()) => {
                        crate::scene::BACKGROUND_UPDATED.store(true, Ordering::Relaxed);
                        show_message(tl!("item-app-bg-imported")).ok();
                    }
                    Err(err) => show_error(err),
                }
            }
            request_mobile_file("background_import");
            return Ok(Some(true));
        }
        if self.app_bg_reset_btn.touch(touch, t) {
            match dir::clear_appearance("background") {
                Ok(_) => {
                    crate::scene::BACKGROUND_UPDATED.store(true, Ordering::Relaxed);
                    show_message(tl!("item-app-bg-reset-done")).ok();
                }
                Err(err) => show_error(err),
            }
            return Ok(Some(true));
        }
        // Phira Pro：自定义背景音乐。
        if self.app_bgm_btn.touch(touch, t) {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
            if let Some(path) = pick_audio(&tl!("item-app-bgm")) {
                match dir::import_appearance_audio("bgm", &path) {
                    Ok(()) => {
                        crate::scene::BGM_UPDATED.store(true, Ordering::Relaxed);
                        show_message(tl!("item-app-bgm-imported")).ok();
                    }
                    Err(err) => show_error(err),
                }
            }
            request_mobile_file("bgm_import");
            return Ok(Some(true));
        }
        if self.app_bgm_reset_btn.touch(touch, t) {
            match dir::clear_appearance_audio("bgm") {
                Ok(_) => {
                    crate::scene::BGM_UPDATED.store(true, Ordering::Relaxed);
                    show_message(tl!("item-app-bgm-reset-done")).ok();
                }
                Err(err) => show_error(err),
            }
            return Ok(Some(true));
        }
        if self.theme_btn.touch(touch, t) {
            return Ok(Some(false));
        }
        // Phira Pro：自定义立绘。
        if self.appearance_import_btn.touch(touch, t) {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
            if let Some(path) = pick_image(&tl!("item-appearance-import")) {
                match dir::import_appearance("character", &path) {
                    Ok(()) => {
                        crate::scene::APPEARANCE_UPDATED.store(true, Ordering::Relaxed);
                        show_message(tl!("item-appearance-imported")).ok();
                    }
                    Err(err) => show_error(err),
                }
            }
            request_mobile_file("appearance_import");
            return Ok(Some(true));
        }
        if self.appearance_reset_btn.touch(touch, t) {
            match dir::clear_appearance("character") {
                Ok(_) => {
                    crate::scene::APPEARANCE_UPDATED.store(true, Ordering::Relaxed);
                    show_message(tl!("item-appearance-reset-done")).ok();
                }
                Err(err) => show_error(err),
            }
            return Ok(Some(true));
        }
        if self.font_btn.touch(touch, t) {
            #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
            if let Some(path) = rfd::FileDialog::new()
                .set_title(tl!("import-font"))
                .add_filter("font", &["ttf", "otf", "ttc"])
                .pick_file()
            {
                match dir::import_font(&path) {
                    Ok(_) => {
                        self.has_custom_font = true;
                        show_message(tl!("font-imported")).ok();
                    }
                    Err(err) => show_error(err.context(tl!("font-import-failed"))),
                }
            }
            request_mobile_file("font_import");
            return Ok(Some(true));
        }
        if self.font_reset_btn.touch(touch, t) {
            match dir::reset_font() {
                Ok(()) => {
                    self.has_custom_font = false;
                    show_message(tl!("font-reset-done")).ok();
                }
                Err(err) => show_error(err),
            }
            return Ok(Some(true));
        }

        #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
        if self.fullscreen_btn.touch(touch, t) {
            config.fullscreen_mode ^= true;

            macroquad::window::set_fullscreen(config.fullscreen_mode);

            return Ok(Some(true));
        }

        if self.cache_btn.touch(touch, t) {
            fs::remove_dir_all(dir::cache()?)?;
            #[cfg(target_os = "android")]
            prpr::ext::clear_android_diagnostic_logs()?;
            self.update_cache_size()?;
            show_message(tl!("item-cache-cleared")).ok();
            return Ok(Some(false));
        }
        if self.offline_btn.touch(touch, t) {
            config.offline_mode ^= true;
            return Ok(Some(true));
        }
        if self.server_status_btn.touch(touch, t) {
            let _ = open_url(&crate::client::status_url());
            return Ok(Some(true));
        }
        if self.api_url_btn.touch(touch, t) {
            request_input("api_url", InputBox::new().default_text(&config.api_url));
            return Ok(Some(true));
        }
        if self.web_url_btn.touch(touch, t) {
            request_input("web_url", InputBox::new().default_text(&config.web_url));
            return Ok(Some(true));
        }
        if self.status_url_btn.touch(touch, t) {
            request_input("status_url", InputBox::new().default_text(&config.status_url));
            return Ok(Some(true));
        }
        #[cfg(not(target_env = "ohos"))]
        if self.lowq_btn.touch(touch, t) {
            config.sample_count = if config.sample_count == 1 { 2 } else { 1 };
            return Ok(Some(true));
        }
        if self.prefer_reduced_motion_btn.touch(touch, t) {
            data.prefer_reduced_motion ^= true;
            PREFER_REDUCED_MOTION.store(data.prefer_reduced_motion, Ordering::Relaxed);
            return Ok(Some(true));
        }
        if self.insecure_btn.touch(touch, t) {
            data.accept_invalid_cert ^= true;
            return Ok(Some(true));
        }
        if self.enable_anys_btn.touch(touch, t) {
            data.enable_anys ^= true;
            return Ok(Some(true));
        }
        if self.anys_gateway_btn.touch(touch, t) {
            request_input("anys_gateway", InputBox::new().default_text(&data.anys_gateway));
            return Ok(Some(true));
        }
        if self.blacklist_btn.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(BlacklistPage::new())));
            return Ok(Some(true));
        }
        if self.hud_btn.touch(touch, t) {
            // 关掉设置页、回到主菜单并进入编辑模式（切页由 MainScene 处理）。
            crate::hud::set_edit(true);
            crate::hud::request_goto(crate::hud::PageId::Home);
            self.next_page = Some(NextPage::Pop);
            return Ok(Some(true));
        }
        if self.transfer_btn.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(TransferPage::new())));
            return Ok(Some(true));
        }
        if self.backup_btn.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(TransferPage::new())));
            return Ok(Some(true));
        }
        if self.reset_settings_btn.touch(touch, t) {
            confirm_reset_settings();
            return Ok(Some(true));
        }
        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        // 主题名是构建选项时求值的：界面语言一变就得重建，否则会一直显示旧语言
        // （表现为「主题列表永远是启动时的语言」）。
        let cur_lang = get_data().language.clone();
        if self.theme_lang != cur_lang {
            self.theme_lang = cur_lang;
            self.theme_btn.set_options((0..UI_PRESETS.len()).map(ui_preset_name).collect());
        }
        self.lang_btn.update(t);
        self.theme_btn.update(t);
        if self.theme_btn.changed() {
            let (accent, surface) = UI_PRESETS[self.theme_btn.selected()];
            let config = &mut get_data_mut().config;
            config.ui_accent = format!("{accent:06x}");
            config.ui_surface = format!("{surface:06x}");
            config.apply_ui_colors();
            return Ok(true);
        }
        // 移动端系统文件选择器的返回结果（自定义图标 / 背景 / 立绘 / 字体导入）。
        #[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
        if let Some((id, file)) = prpr::scene::take_file() {
            match id.as_str() {
                "icon_import" => match dir::import_appearance("icon", std::path::Path::new(&file)) {
                    Ok(()) => {
                        show_message(tl!("item-app-icon-imported")).ok();
                    }
                    Err(err) => show_error(err),
                },
                "background_import" => match dir::import_appearance("background", std::path::Path::new(&file)) {
                    Ok(()) => {
                        crate::scene::BACKGROUND_UPDATED.store(true, Ordering::Relaxed);
                        show_message(tl!("item-app-bg-imported")).ok();
                    }
                    Err(err) => show_error(err),
                },
                "bgm_import" => match dir::import_appearance_audio("bgm", std::path::Path::new(&file)) {
                    Ok(()) => {
                        crate::scene::BGM_UPDATED.store(true, Ordering::Relaxed);
                        show_message(tl!("item-app-bgm-imported")).ok();
                    }
                    Err(err) => show_error(err),
                },
                "appearance_import" => match dir::import_appearance("character", std::path::Path::new(&file)) {
                    Ok(()) => {
                        crate::scene::APPEARANCE_UPDATED.store(true, Ordering::Relaxed);
                        show_message(tl!("item-appearance-imported")).ok();
                    }
                    Err(err) => show_error(err),
                },
                "font_import" => match dir::import_font(std::path::Path::new(&file)) {
                    Ok(_) => {
                        self.has_custom_font = true;
                        show_message(tl!("font-imported")).ok();
                    }
                    Err(err) => show_error(err.context(tl!("font-import-failed"))),
                },
                _ => prpr::scene::return_file(id, file),
            }
        }
        let data = get_data_mut();
        if self.lang_btn.changed() {
            data.language = Some(LANG_IDENTS[self.lang_btn.selected()].to_string());
            sync_data();
            return Ok(true);
        }
        if let Some((id, text)) = take_input() {
            if matches!(id.as_str(), "api_url" | "web_url" | "status_url") {
                let text = text.trim().trim_end_matches('/').to_owned();
                // 官服地址留空表示回退到官方地址；其余情况必须是合法的 http(s) URL。
                let valid =
                    text.is_empty() || ((text.starts_with("http://") || text.starts_with("https://")) && Url::parse(&text).is_ok());
                if !valid {
                    show_error(anyhow::anyhow!("{}", tl!("item-url-invalid")));
                    return Ok(false);
                }
                match id.as_str() {
                    "api_url" => data.config.api_url = text,
                    "web_url" => data.config.web_url = text,
                    _ => data.config.status_url = text,
                }
                return Ok(true);
            } else if id == "anys_gateway" {
                if let Err(err) = Url::parse(&text) {
                    show_error(anyhow::Error::new(err).context(tl!("item-anys-gateway-invalid")));
                    return Ok(false);
                } else {
                    data.anys_gateway = text.trim_end_matches('/').to_string();
                    return Ok(true);
                }
            } else {
                return_input(id, text);
            }
        }
        if let Some(task) = &mut self.cache_task {
            if let Some(size) = task.take() {
                self.cache_size = size.ok();
                self.cache_task = None;
            }
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(item_row_h());
                h += item_row_h();
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            let rt = render_title(ui, tl!("item-lang"), None);
            let w = 0.06;
            let r = Rect::new(rt + 0.01, (item_row_h() - w) / 2., w, w);
            ui.fill_rect(r, (*self.icon_lang, r));
            self.lang_btn.render(ui, rr, t);
        }

        item! {
            render_title(ui, tl!("item-app-icon"), Some(tl!("item-app-icon-sub")));
            self.app_icon_btn.render_text(ui, rr, t, tl!("item-appearance-import-btn"), 0.5, true);
        }
        item! {
            render_title(ui, tl!("item-app-icon-reset"), None);
            self.app_icon_reset_btn.render_text(ui, rr, t, tl!("font-reset-btn"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-app-bg"), Some(tl!("item-app-bg-sub")));
            self.app_bg_btn.render_text(ui, rr, t, tl!("item-appearance-import-btn"), 0.5, true);
        }
        item! {
            render_title(ui, tl!("item-app-bg-reset"), None);
            self.app_bg_reset_btn.render_text(ui, rr, t, tl!("font-reset-btn"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-app-bgm"), Some(tl!("item-app-bgm-sub")));
            self.app_bgm_btn.render_text(ui, rr, t, tl!("item-appearance-import-btn"), 0.5, true);
        }
        item! {
            render_title(ui, tl!("item-app-bgm-reset"), None);
            self.app_bgm_reset_btn.render_text(ui, rr, t, tl!("font-reset-btn"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-appearance-import"), Some(tl!("item-appearance-import-sub")));
            self.appearance_import_btn.render_text(ui, rr, t, tl!("item-appearance-import-btn"), 0.5, true);
        }
        item! {
            render_title(ui, tl!("item-appearance-reset"), None);
            self.appearance_reset_btn.render_text(ui, rr, t, tl!("font-reset-btn"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-font"), Some(tl!("item-font-sub")));
            self.font_btn.render_text(ui, rr, t, tl!("import-font"), 0.5, self.has_custom_font);
        }
        item! {
            render_title(ui, tl!("item-font-reset"), Some(tl!("item-font-reset-sub")));
            self.font_reset_btn.render_text(ui, rr, t, tl!("font-reset-btn"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-ui-theme"), Some(tl!("item-ui-theme-sub")));
            self.theme_btn.render(ui, rr, t);
        }

        #[cfg(all(any(target_os = "windows", target_os = "linux"), not(target_env = "ohos")))]
        item! {
            render_title(ui, tl!("item-fullscreen"), None);
            render_switch(ui, rr, t, &mut self.fullscreen_btn, config.fullscreen_mode);
        }

        item! {
            render_title(ui, tl!("item-offline"), Some(tl!("item-offline-sub")));
            render_switch(ui, rr, t, &mut self.offline_btn, config.offline_mode);
        }
        item! {
            render_title(ui, tl!("item-server-status"), Some(tl!("item-server-status-sub")));
            self.server_status_btn.render_text(ui, rr, t, tl!("check-status"), 0.5, true);
        }
        item! {
            render_title(ui, tl!("item-api-url"), Some(tl!("item-api-url-sub")));
            // 留空时显示实际生效的官方地址，让用户知道当前用的是哪个。
            let shown = if config.api_url.is_empty() {
                Cow::Borrowed(crate::client::DEFAULT_API_URL)
            } else {
                Cow::Owned(config.api_url.clone())
            };
            self.api_url_btn.render_text(ui, rr, t, shown, 0.4, false);
        }
        item! {
            render_title(ui, tl!("item-web-url"), Some(tl!("item-web-url-sub")));
            let shown = if config.web_url.is_empty() {
                Cow::Borrowed(crate::client::DEFAULT_WEB_URL)
            } else {
                Cow::Owned(config.web_url.clone())
            };
            self.web_url_btn.render_text(ui, rr, t, shown, 0.4, false);
        }
        item! {
            render_title(ui, tl!("item-status-url"), Some(tl!("item-status-url-sub")));
            let shown = if config.status_url.is_empty() {
                Cow::Borrowed(crate::client::DEFAULT_STATUS_URL)
            } else {
                Cow::Owned(config.status_url.clone())
            };
            self.status_url_btn.render_text(ui, rr, t, shown, 0.4, false);
        }
        item! {
            render_title(ui, tl!("item-prefer-reduced-motion"), Some(tl!("item-prefer-reduced-motion-sub")));
            render_switch(ui, rr, t, &mut self.prefer_reduced_motion_btn, data.prefer_reduced_motion);
        }
        #[cfg(not(target_env = "ohos"))]
        item! {
            render_title(ui, tl!("item-lowq"), Some(tl!("item-lowq-sub")));
            render_switch(ui, rr, t, &mut self.lowq_btn, config.sample_count == 1);
        }
        item! {
            let cache_size = if let Some(size) = self.cache_size {
                Cow::Owned(tl!("item-cache-size", "size" => ByteSize(size).to_string()))
            } else {
                tl!("item-cache-size-loading")
            };
            render_title(ui, tl!("item-clear-cache"), Some(cache_size));
            self.cache_btn.render_text(ui, rr, t, tl!("item-clear-cache-btn"), 0.5, true);
        }
        #[cfg(target_os = "android")]
        item! {
            render_title(ui, tl!("item-android-logs"), Some(tl!("item-android-logs-sub")));
        }
        ui.dy(0.04);
        h += 0.04;
        item! {
            render_title(ui, tl!("item-insecure"), Some(tl!("item-insecure-sub")));
            render_switch(ui, rr, t, &mut self.insecure_btn, data.accept_invalid_cert);
        }
        item! {
            render_title(ui, tl!("item-enable-anys"), Some(tl!("item-enable-anys-sub")));
            render_switch(ui, rr, t, &mut self.enable_anys_btn, data.enable_anys);
        }
        item! {
            render_title(ui, tl!("item-anys-gateway"), Some(tl!("item-anys-gateway-sub")));
            self.anys_gateway_btn.render_text(ui, rr, t, &data.anys_gateway, 0.4, false);
        }
        item! {
            render_title(ui, tl!("item-blacklist"), Some(tl!("item-blacklist-sub")));
            self.blacklist_btn.render_text(ui, rr, t, tl!("item-blacklist-open"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-hud"), Some(tl!("item-hud-sub")));
            self.hud_btn.render_text(ui, rr, t, tl!("item-hud-open"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-transfer"), Some(tl!("item-transfer-sub")));
            self.transfer_btn.render_text(ui, rr, t, tl!("transfer-open"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-backup"), Some(tl!("item-backup-sub")));
            self.backup_btn.render_text(ui, rr, t, tl!("backup-open"), 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-reset-settings"), Some(tl!("item-reset-settings-sub")));
            self.reset_settings_btn.render_text(ui, rr, t, tl!("item-reset-settings-btn"), 0.5, false);
        }
        self.lang_btn.render_top(ui, t, 1.);
        self.theme_btn.render_top(ui, t, 1.);
        (w, h)
    }
}

struct AudioList {
    adjust_btn: DRectButton,
    music_slider: Slider,
    sfx_slider: Slider,
    bgm_slider: Slider,
    cali_btn: DRectButton,
    #[cfg(not(target_os = "android"))]
    preferred_sample_rate_btn: DRectButton,
    #[cfg(target_env = "ohos")]
    audio_buffer_size_btn: DRectButton,
    cali_task: LocalTask<Result<OffsetPage>>,
    next_page: Option<NextPage>,
}

impl AudioList {
    pub fn new() -> Self {
        Self {
            adjust_btn: DRectButton::new(),
            music_slider: Slider::new(0.0..2.0, 0.05),
            sfx_slider: Slider::new(0.0..2.0, 0.05),
            bgm_slider: Slider::new(0.0..2.0, 0.05),
            cali_btn: DRectButton::new(),
            #[cfg(not(target_os = "android"))]
            preferred_sample_rate_btn: DRectButton::new(),
            #[cfg(target_env = "ohos")]
            audio_buffer_size_btn: DRectButton::new(),

            cali_task: None,
            next_page: None,
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.adjust_btn.touch(touch, t) {
            config.adjust_time ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.music_slider.touch(touch, t, &mut config.volume_music) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.sfx_slider.touch(touch, t, &mut config.volume_sfx) {
            UI_SFX_VOLUME.store(config.volume_sfx.to_bits(), Ordering::Relaxed);
            return Ok(wt);
        }
        let old = config.volume_bgm;
        if let wt @ Some(_) = self.bgm_slider.touch(touch, t, &mut config.volume_bgm) {
            if (config.volume_bgm - old).abs() > 0.001 {
                BGM_VOLUME_UPDATED.store(true, Ordering::Relaxed);
            }
            return Ok(wt);
        }
        if self.cali_btn.touch(touch, t) {
            self.cali_task = Some(Box::pin(OffsetPage::new()));
            return Ok(Some(false));
        }
        #[cfg(not(target_os = "android"))]
        if self.preferred_sample_rate_btn.touch(touch, t) {
            let options = [None, Some(44100), Some(48000), Some(88200), Some(96000), Some(192000)];
            let current = config.preferred_sample_rate;
            let selected = options.iter().position(|&r| r == current).unwrap_or(0);
            config.preferred_sample_rate = options[(selected + 1) % options.len()];
            return Ok(Some(true));
        }
        #[cfg(target_env = "ohos")]
        if self.audio_buffer_size_btn.touch(touch, t) {
            let options = [128u32, 256u32, 512u32];
            let current = config.audio_buffer_size.unwrap_or(256);
            let selected = options.iter().position(|&r| r == current).unwrap_or(1);
            config.audio_buffer_size = Some(options[(selected + 1) % options.len()]);
            return Ok(Some(true));
        }
        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        if let Some(task) = &mut self.cali_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => show_error(err.context(tl!("load-cali-failed"))),
                    Ok(page) => {
                        self.next_page = Some(NextPage::Overlay(Box::new(page)));
                    }
                }
                self.cali_task = None;
            }
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(item_row_h());
                h += item_row_h();
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, tl!("item-adjust"), Some(tl!("item-adjust-sub")));
            render_switch(ui, rr, t, &mut self.adjust_btn, config.adjust_time);
        }
        item! {
            render_title(ui, tl!("item-music"), None);
            self.music_slider.render(ui, rr, t, config.volume_music, format!("{:.2}", config.volume_music));
        }
        item! {
            render_title(ui, tl!("item-sfx"), None);
            self.sfx_slider.render(ui, rr, t, config.volume_sfx, format!("{:.2}", config.volume_sfx));
        }
        item! {
            render_title(ui, tl!("item-bgm"), None);
            self.bgm_slider.render(ui, rr, t, config.volume_bgm, format!("{:.2}", config.volume_bgm));
        }
        item! {
            render_title(ui, tl!("item-cali"), None);
            self.cali_btn.render_text(ui, rr, t, format!("{:.0}ms", config.offset * 1000.), 0.5, true);
        }
        #[cfg(not(target_os = "android"))]
        item! {
            render_title(ui, tl!("item-preferred-sample-rate"), None);
            let text = if let Some(rate) = config.preferred_sample_rate {
                format!("{} Hz", rate)
            } else {
                tl!("preferred-sample-rate-default").to_string()
            };
            self.preferred_sample_rate_btn.render_text(ui, rr, t, text, 0.5, false);
        }
        #[cfg(target_env = "ohos")]
        item! {
            render_title(ui, tl!("item-audio-buffer-size"), None);
            let buf_size = config.audio_buffer_size.unwrap_or(256);
            self.audio_buffer_size_btn.render_text(ui, rr, t, format!("{}", buf_size), 0.5, false);
        }
        (w, h)
    }

    pub fn next_page(&mut self) -> Option<NextPage> {
        self.next_page.take()
    }
}

struct ChartList {
    show_acc_btn: DRectButton,
    ap_fc_indicator_btn: DRectButton,
    show_avg_fps_btn: DRectButton,
    dc_pause_btn: DRectButton,
    dhint_btn: DRectButton,
    opt_btn: DRectButton,
    block_simple_btn: DRectButton,
    shader_pre_render_btn: DRectButton,
    use_keyboard_btn: DRectButton,
    speed_slider: Slider,
    size_slider: Slider,
    /// 谱面流速：只等比缩放音符的视觉流速，音乐与音调不变。
    flow_speed_slider: Slider,
    /// 上/下隐强度：音符出现/消失的高度，0 为官方表现。
    fade_strength_slider: Slider,
    /// 「自定义游玩宽高比」开关。
    custom_aspect_btn: DRectButton,
    /// 打开游玩宽高比输入框。
    aspect_btn: DRectButton,
    /// 局内判定偏移条开关。
    offset_indicator_btn: DRectButton,
    auto_retry_slider: Slider,
    retry_lead_slider: Slider,
    practice_ramp_btn: DRectButton,
    practice_speed_slider: Slider,
    practice_step_slider: Slider,
    hp_mode_btn: DRectButton,
    hp_amount_slider: Slider,
    hp_width_slider: Slider,
    hp_height_slider: Slider,
    hp_scale_slider: Slider,
    hp_color_btn: DRectButton,
    combo_text_btn: DRectButton,
    judge_chart_btn: DRectButton,
    judgement_btn: DRectButton,
    replays_btn: DRectButton,
    history_btn: DRectButton,
    next_page: Option<NextPage>,
}

impl ChartList {
    pub fn new() -> Self {
        Self {
            show_acc_btn: DRectButton::new(),
            ap_fc_indicator_btn: DRectButton::new(),
            show_avg_fps_btn: DRectButton::new(),
            dc_pause_btn: DRectButton::new(),
            dhint_btn: DRectButton::new(),
            opt_btn: DRectButton::new(),
            block_simple_btn: DRectButton::new(),
            shader_pre_render_btn: DRectButton::new(),
            use_keyboard_btn: DRectButton::new(),
            speed_slider: Slider::new(if cfg!(flash) { 1.0..2.0 } else { 0.5..2.0 }, 0.05),
            size_slider: Slider::new(0.8..1.2, 0.005),
            flow_speed_slider: Slider::new(0.5..4.0, 0.05),
            fade_strength_slider: Slider::new(0.0..1.0, 0.05),
            custom_aspect_btn: DRectButton::new(),
            aspect_btn: DRectButton::new(),
            offset_indicator_btn: DRectButton::new(),
            auto_retry_slider: Slider::new(0.0..10.0, 1.0),
            retry_lead_slider: Slider::new(0.0..10.0, 0.5),
            practice_ramp_btn: DRectButton::new(),
            practice_speed_slider: Slider::new(0.3..1.0, 0.05),
            practice_step_slider: Slider::new(0.05..0.5, 0.05),
            hp_mode_btn: DRectButton::new(),
            hp_amount_slider: Slider::new(0.2..3.0, 0.1),
            hp_width_slider: Slider::new(0.1..1.0, 0.01),
            hp_height_slider: Slider::new(0.5..3.0, 0.1),
            hp_scale_slider: Slider::new(0.2..3.0, 0.1),
            hp_color_btn: DRectButton::new(),
            combo_text_btn: DRectButton::new(),
            judge_chart_btn: DRectButton::new(),
            judgement_btn: DRectButton::new(),
            replays_btn: DRectButton::new(),
            history_btn: DRectButton::new(),
            next_page: None,
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.show_acc_btn.touch(touch, t) {
            config.show_acc ^= true;
            return Ok(Some(true));
        }
        if self.ap_fc_indicator_btn.touch(touch, t) {
            config.ap_fc_indicator ^= true;
            return Ok(Some(true));
        }
        if self.show_avg_fps_btn.touch(touch, t) {
            config.show_avg_fps ^= true;
            return Ok(Some(true));
        }
        if self.dc_pause_btn.touch(touch, t) {
            config.double_click_to_pause ^= true;
            return Ok(Some(true));
        }
        if self.dhint_btn.touch(touch, t) {
            config.double_hint ^= true;
            return Ok(Some(true));
        }
        if self.opt_btn.touch(touch, t) {
            config.aggressive ^= true;
            return Ok(Some(true));
        }
        if self.block_simple_btn.touch(touch, t) {
            config.block_area_simple ^= true;
            return Ok(Some(true));
        }
        if self.shader_pre_render_btn.touch(touch, t) {
            config.shader_pre_render ^= true;
            return Ok(Some(true));
        }
        // Phira Pro Flash（轻量版）：不提供键盘模式。
        if !cfg!(flash) && self.use_keyboard_btn.touch(touch, t) {
            config.use_keyboard ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.speed_slider.touch(touch, t, &mut config.speed) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.flow_speed_slider.touch(touch, t, &mut config.flow_speed) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.fade_strength_slider.touch(touch, t, &mut config.fade_strength) {
            return Ok(wt);
        }
        // 自定义游玩宽高比：开关直接切换「覆盖 / 跟随谱面」，数值行打开输入框。
        if self.custom_aspect_btn.touch(touch, t) {
            config.aspect_ratio = if config.aspect_ratio.is_some() { None } else { Some(16. / 9.) };
            return Ok(Some(true));
        }
        if let Some(cur) = config.aspect_ratio {
            if self.aspect_btn.touch(touch, t) {
                request_input("aspect_ratio", InputBox::new().default_text(prpr::format_aspect_ratio(cur)));
                return Ok(Some(true));
            }
        }
        if self.offset_indicator_btn.touch(touch, t) {
            config.offset_indicator ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.size_slider.touch(touch, t, &mut config.note_scale) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.auto_retry_slider.touch(touch, t, &mut config.auto_retry) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.retry_lead_slider.touch(touch, t, &mut config.retry_lead) {
            return Ok(wt);
        }
        // Phira Pro Flash（轻量版）：不提供变速练习。
        if !cfg!(flash) {
            if let wt @ Some(_) = self.practice_speed_slider.touch(touch, t, &mut config.practice_speed_start) {
                return Ok(wt);
            }
            if let wt @ Some(_) = self.practice_step_slider.touch(touch, t, &mut config.practice_speed_step) {
                return Ok(wt);
            }
            if self.practice_ramp_btn.touch(touch, t) {
                config.practice_ramp ^= true;
                return Ok(Some(true));
            }
        }
        if !cfg!(flash) && self.judgement_btn.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(JudgementPage::new())));
            return Ok(Some(false));
        }
        if self.replays_btn.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(super::replays::ReplayManager::new(None, false))));
            return Ok(Some(false));
        }
        if self.combo_text_btn.touch(touch, t) {
            request_input("combo_text", InputBox::new().default_text(&config.combo_text));
            return Ok(Some(true));
        }
        if self.judge_chart_btn.touch(touch, t) {
            config.ending_judge_chart ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.hp_scale_slider.touch(touch, t, &mut config.hp_scale) {
            config.hp_scale = config.hp_scale.clamp(0.2, 3.0);
            return Ok(wt);
        }
        if self.hp_color_btn.touch(touch, t) {
            config.hp_color = config.hp_color.next();
            return Ok(Some(true));
        }
        if self.history_btn.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(HistoryPage::new())));
            return Ok(Some(true));
        }
        if self.hp_mode_btn.touch(touch, t) {
            config.hp_mode ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.hp_amount_slider.touch(touch, t, &mut config.hp_amount) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.hp_width_slider.touch(touch, t, &mut config.hp_width) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.hp_height_slider.touch(touch, t, &mut config.hp_height) {
            return Ok(wt);
        }
        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        if let Some((id, text)) = take_input() {
            // 连击文字：这一行渲染在「谱面」页，输入处理也必须在这里。
            // 之前放在通用页的 update 里，而 `SettingsPage::update` 只更新**当前选中页**，
            // 于是在谱面页改完的输入会被原样退回、永远不生效（表现为「局内还是 COMBO」）。
            if id == "combo_text" {
                let mut text = text.trim().to_owned();
                if text.chars().count() > 16 {
                    text = text.chars().take(16).collect();
                }
                get_data_mut().config.combo_text = text;
                return Ok(true);
            }
            // 自定义游玩宽高比：接受 `16:9` 或小数两种写法。
            if id == "aspect_ratio" {
                return match prpr::parse_aspect_ratio(&text) {
                    Some(v) => {
                        get_data_mut().config.aspect_ratio = Some(v);
                        Ok(true)
                    }
                    None => {
                        show_error(anyhow::anyhow!(tl!("aspect-invalid").into_owned()));
                        Ok(false)
                    }
                };
            }
            return_input(id, text);
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(item_row_h());
                h += item_row_h();
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, tl!("item-show-acc"), None);
            render_switch(ui, rr, t, &mut self.show_acc_btn, config.show_acc);
        }
        item! {
            render_title(ui, tl!("item-ap-fc-indicator"), Some(tl!("item-ap-fc-indicator-sub")));
            render_switch(ui, rr, t, &mut self.ap_fc_indicator_btn, config.ap_fc_indicator);
        }
        item! {
            render_title(ui, tl!("item-show-avg-fps"), Some(tl!("item-show-avg-fps-sub")));
            render_switch(ui, rr, t, &mut self.show_avg_fps_btn, config.show_avg_fps);
        }
        item! {
            render_title(ui, tl!("item-dc-pause"), None);
            render_switch(ui, rr, t, &mut self.dc_pause_btn, config.double_click_to_pause);
        }
        item! {
            render_title(ui, tl!("item-dhint"), Some(tl!("item-dhint-sub")));
            render_switch(ui, rr, t, &mut self.dhint_btn, config.double_hint);
        }
        item! {
            render_title(ui, tl!("item-opt"), Some(tl!("item-opt-sub")));
            render_switch(ui, rr, t, &mut self.opt_btn, config.aggressive);
        }
        item! {
            render_title(ui, tl!("item-block-simple"), Some(tl!("item-block-simple-sub")));
            render_switch(ui, rr, t, &mut self.block_simple_btn, config.block_area_simple);
        }
        item! {
            render_title(ui, tl!("item-shader-pre-render"), Some(tl!("item-shader-pre-render-sub")));
            render_switch(ui, rr, t, &mut self.shader_pre_render_btn, config.shader_pre_render);
        }
        // Phira Pro Flash（轻量版）：不提供键盘模式。
        if !cfg!(flash) {
            item! {
                render_title(ui, tl!("item-use-keyboard"), Some(tl!("item-use-keyboard-sub")));
                render_switch(ui, rr, t, &mut self.use_keyboard_btn, config.use_keyboard);
            }
        }
        item! {
            render_title(ui, tl!("item-speed"), None);
            self.speed_slider.render(ui, rr, t, config.speed, format!("{:.2}", config.speed));
        }
        item! {
            render_title(ui, tl!("item-flow-speed"), Some(tl!("item-flow-speed-sub")));
            self.flow_speed_slider.render(ui, rr, t, config.flow_speed, format!("{:.2}x", config.flow_speed));
        }
        item! {
            render_title(ui, tl!("item-fade-strength"), Some(tl!("item-fade-strength-sub")));
            let label = if config.fade_strength <= 0. {
                tl!("aspect-official").into_owned()
            } else {
                format!("{:.0}%", config.fade_strength * 100.)
            };
            self.fade_strength_slider.render(ui, rr, t, config.fade_strength, label);
        }
        item! {
            render_title(ui, tl!("item-note-size"), None);
            self.size_slider.render(ui, rr, t, config.note_scale, format!("{:.3}", config.note_scale));
        }
        item! {
            render_title(ui, tl!("item-custom-aspect"), Some(tl!("item-custom-aspect-sub")));
            render_switch(ui, rr, t, &mut self.custom_aspect_btn, config.aspect_ratio.is_some());
        }
        if let Some(cur) = config.aspect_ratio {
            item! {
                render_title(ui, tl!("item-aspect-ratio"), Some(tl!("item-aspect-ratio-sub")));
                self.aspect_btn
                    .render_text(ui, rr, t, prpr::format_aspect_ratio(cur), 0.5, false);
            }
        }
        item! {
            render_title(ui, tl!("item-offset-indicator"), Some(tl!("item-offset-indicator-sub")));
            render_switch(ui, rr, t, &mut self.offset_indicator_btn, config.offset_indicator);
        }
        if !cfg!(flash) {
            item! {
                render_title(ui, tl!("judgement-settings"), Some(tl!("judgement-settings-sub")));
                self.judgement_btn.render_text(ui, rr, t, tl!("judgement-open"), 0.42, false);
            }
        }
        item! {
            render_title(ui, tl!("replays-settings"), Some(tl!("replays-settings-sub")));
            self.replays_btn.render_text(ui, rr, t, tl!("judgement-open"), 0.42, false);
        }
        item! {
            render_title(ui, tl!("item-auto-retry"), Some(tl!("item-auto-retry-sub")));
            self.auto_retry_slider.render(ui, rr, t, config.auto_retry, format!("{}", config.auto_retry.round() as u32));
        }
        item! {
            render_title(ui, tl!("item-retry-lead"), Some(tl!("item-retry-lead-sub")));
            self.retry_lead_slider.render(ui, rr, t, config.retry_lead, format!("{:.1}s", config.retry_lead));
        }
        // Phira Pro Flash（轻量版）：不提供变速练习。
        if !cfg!(flash) {
            item! {
                render_title(ui, tl!("item-practice-ramp"), Some(tl!("item-practice-ramp-sub")));
                render_switch(ui, rr, t, &mut self.practice_ramp_btn, config.practice_ramp);
            }
            item! {
                render_title(ui, tl!("item-practice-speed"), None);
                self.practice_speed_slider.render(ui, rr, t, config.practice_speed_start, format!("{:.2}x", config.practice_speed_start));
            }
            item! {
                render_title(ui, tl!("item-practice-step"), None);
                self.practice_step_slider.render(ui, rr, t, config.practice_speed_step, format!("+{:.2}x", config.practice_speed_step));
            }
        }
        h += 0.04;
        item! {
            render_title(ui, tl!("item-hp-mode"), Some(tl!("item-hp-mode-sub")));
            render_switch(ui, rr, t, &mut self.hp_mode_btn, config.hp_mode);
        }
        item! {
            render_title(ui, tl!("item-hp-amount"), None);
            self.hp_amount_slider.render(ui, rr, t, config.hp_amount, format!("{:.1}x", config.hp_amount));
        }
        item! {
            render_title(ui, tl!("item-hp-width"), None);
            self.hp_width_slider.render(ui, rr, t, config.hp_width, format!("{:.2}", config.hp_width));
        }
        item! {
            render_title(ui, tl!("item-hp-height"), None);
            self.hp_height_slider.render(ui, rr, t, config.hp_height, format!("{:.1}x", config.hp_height));
        }
        h += 0.04;
        item! {
            render_title(ui, tl!("item-combo-text"), Some(tl!("item-combo-text-sub")));
            let label = if config.combo_text.is_empty() {
                tl!("combo-text-default").into_owned()
            } else {
                config.combo_text.clone()
            };
            self.combo_text_btn.render_text(ui, rr, t, label, 0.5, false);
        }
        item! {
            render_title(ui, tl!("item-judge-chart"), Some(tl!("item-judge-chart-sub")));
            render_switch(ui, rr, t, &mut self.judge_chart_btn, config.ending_judge_chart);
        }
        item! {
            render_title(ui, tl!("item-hp-scale"), None);
            self.hp_scale_slider.render(ui, rr, t, config.hp_scale, format!("{:.1}x", config.hp_scale));
        }
        item! {
            render_title(ui, tl!("item-hp-color"), None);
            // 按钮本体只画底衬，色块由上面覆一层填充色表示当前颜色。
            self.hp_color_btn.render_text(ui, rr, t, "", 0.5, false);
            let (cr, cg, cb) = config.hp_color.rgb();
            // feather 正数向外扩、负数才向内收；这里要的是按钮内的一小块色块预览。
            ui.fill_rect(rr.feather(-0.012), Color::new(cr, cg, cb, 0.95));
        }
        h += 0.04;
        item! {
            render_title(ui, tl!("item-history"), Some(tl!("item-history-sub")));
            self.history_btn.render_text(ui, rr, t, tl!("item-history-open"), 0.5, false);
        }
        (w, h)
    }
}

impl ChartList {
    pub fn next_page(&mut self) -> Option<NextPage> {
        self.next_page.take()
    }
}

struct DebugList {
    chart_debug_btn: DRectButton,
    chart_debug_line_btn: DRectButton,
    chart_debug_note_btn: DRectButton,
    touch_debug_btn: DRectButton,
    touch_color_btn: RectButton,
    touch_alpha_slider: Slider,
    touch_size_slider: Slider,
    show_fps_btn: DRectButton,
}

impl DebugList {
    pub fn new() -> Self {
        Self {
            chart_debug_btn: DRectButton::new(),
            chart_debug_line_btn: DRectButton::new(),
            chart_debug_note_btn: DRectButton::new(),
            touch_debug_btn: DRectButton::new(),
            touch_color_btn: RectButton::new(),
            touch_alpha_slider: Slider::new(0.05..1.0, 0.05),
            touch_size_slider: Slider::new(0.01..0.15, 0.005),
            show_fps_btn: DRectButton::new(),
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.chart_debug_btn.touch(touch, t) {
            config.chart_debug ^= true;
            return Ok(Some(true));
        }
        if self.chart_debug_line_btn.touch(touch, t) {
            config.chart_debug_line ^= true;
            return Ok(Some(true));
        }
        if self.chart_debug_note_btn.touch(touch, t) {
            config.chart_debug_note ^= true;
            return Ok(Some(true));
        }
        if self.show_fps_btn.touch(touch, t) {
            config.show_fps ^= true;
            // 左下角帧率由 prpr 的全局覆盖层绘制，这里同步开关。
            prpr::ui::SHOW_FPS.store(config.show_fps, Ordering::Relaxed);
            return Ok(Some(true));
        }
        if self.touch_debug_btn.touch(touch, t) {
            config.touch_debug ^= true;
            return Ok(Some(true));
        }
        if self.touch_color_btn.touch(touch) {
            let idx = TOUCH_POINT_COLORS.iter().position(|it| *it == config.touch_point_color).unwrap_or(0);
            config.touch_point_color = TOUCH_POINT_COLORS[(idx + 1) % TOUCH_POINT_COLORS.len()];
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.touch_alpha_slider.touch(touch, t, &mut config.touch_point_alpha) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.touch_size_slider.touch(touch, t, &mut config.touch_point_size) {
            return Ok(wt);
        }
        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(item_row_h());
                h += item_row_h();
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, tl!("item-chart-debug"), Some(tl!("item-chart-debug-sub")));
            render_switch(ui, rr, t, &mut self.chart_debug_btn, config.chart_debug);
        }
        item! {
            render_title(ui, tl!("item-debug-line"), Some(tl!("item-debug-line-sub")));
            render_switch(ui, rr, t, &mut self.chart_debug_line_btn, config.chart_debug_line);
        }
        item! {
            render_title(ui, tl!("item-debug-note"), Some(tl!("item-debug-note-sub")));
            render_switch(ui, rr, t, &mut self.chart_debug_note_btn, config.chart_debug_note);
        }
        item! {
            render_title(ui, tl!("item-show-fps"), Some(tl!("item-show-fps-sub")));
            render_switch(ui, rr, t, &mut self.show_fps_btn, config.show_fps);
        }
        item! {
            render_title(ui, tl!("item-touch-debug"), Some(tl!("item-touch-debug-sub")));
            render_switch(ui, rr, t, &mut self.touch_debug_btn, config.touch_debug);
        }
        item! {
            render_title(ui, tl!("item-touch-color"), Some(tl!("item-touch-color-sub")));
            let r = rr.feather(-0.01);
            self.touch_color_btn.set(ui, r);
            let c = Color::from_hex_rgb(config.touch_point_color);
            ui.fill_path(&r.rounded(0.01), c);
            let lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
            ui.text(format!("#{:06X}", config.touch_point_color))
                .pos(r.center().x, r.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.45)
                .color(if lum > 0.55 { BLACK } else { WHITE })
                .draw();
        }
        item! {
            render_title(ui, tl!("item-touch-alpha"), None);
            self.touch_alpha_slider
                .render(ui, rr, t, config.touch_point_alpha, format!("{:.2}", config.touch_point_alpha));
        }
        item! {
            render_title(ui, tl!("item-touch-size"), None);
            self.touch_size_slider
                .render(ui, rr, t, config.touch_point_size, format!("{:.3}", config.touch_point_size));
        }
        (w, h)
    }
}
