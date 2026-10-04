//! Configuration module of the playing environment.\
//! e.g. player name, volume, speed, autoplay, etc.

use bitflags::bitflags;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub static TIPS: Lazy<Vec<String>> = Lazy::new(|| include_str!("tips.txt").split('\n').map(str::to_owned).collect());

/// Phira Pro：自服（`phira-pro-api`）基础地址。
///
/// 「读官服、写自服」：谱面 / 用户 / 全局 rks 仍走官服；成绩固定上传到这里，
/// 单谱排行榜把官服与自服的记录合并展示。**固定值，不可配置。**
pub const PRO_API_URL: &str = "https://api.phira.pro";

/// Phira Pro：结算界面「应用推荐偏移」请求的落点。
///
/// `prpr` 拿不到客户端持有的全局配置，因此这里放一个单向通道：结算界面把要叠加到
/// `config.offset` 的增量（秒）写入，客户端主循环取出后写回配置并持久化。
static OFFSET_DELTA_PENDING: AtomicBool = AtomicBool::new(false);
static PENDING_OFFSET_DELTA: AtomicU32 = AtomicU32::new(0);

/// 请求把 `delta`（秒）叠加到全局 `config.offset`。`delta` 为 0 时忽略。
pub fn request_offset_delta(delta: f32) {
    if delta != 0. {
        PENDING_OFFSET_DELTA.store(delta.to_bits(), Ordering::Relaxed);
        OFFSET_DELTA_PENDING.store(true, Ordering::Relaxed);
    }
}

/// 取出并清除待应用的偏移增量；返回 `None` 表示没有待处理请求。
pub fn take_offset_delta() -> Option<f32> {
    if OFFSET_DELTA_PENDING.swap(false, Ordering::Relaxed) {
        Some(f32::from_bits(PENDING_OFFSET_DELTA.load(Ordering::Relaxed)))
    } else {
        None
    }
}

/// 血条颜色的可选值（对齐上游改版 Phirc Mod++ 的 `HealthBarColor`）。
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Debug)]
pub enum HealthBarColor {
    #[default]
    White,
    Green,
    Blue,
    Red,
    Golden,
    Rainbow,
}

impl HealthBarColor {
    pub const ALL: [HealthBarColor; 6] = [Self::White, Self::Green, Self::Blue, Self::Red, Self::Golden, Self::Rainbow];

    /// 血条填充色（RGB，0..=1）。
    pub fn rgb(self) -> (f32, f32, f32) {
        match self {
            Self::White => (1., 1., 1.),
            Self::Green => (0.35, 0.85, 0.45),
            Self::Blue => (0.4, 0.65, 1.),
            Self::Red => (1., 0.42, 0.42),
            Self::Golden => (1., 0.85, 0.35),
            Self::Rainbow => (0.8, 0.55, 1.),
        }
    }

    /// 轮换到下一个颜色（设置页的色块按钮用）。
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|it| *it == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

bitflags! {
    #[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Debug)]
    #[serde(transparent)]
    pub struct Mods: i32 {
        const AUTOPLAY = 0x0001;
        const FLIP_X = 0x0002;
        const FADE_OUT = 0x0004;
        const FADE_IN = 0x0008;
        const NIGHTCORE = 0x0010;
        const RAINBOW = 0x0020;
        const NO_SHADER = 0x0040;
        const INSTANT_DEATH_AP = 0x0080;
        const INSTANT_DEATH_FC = 0x0100;
        /// Phira Pro：所有判定窗口减半（更严格的判定）。因为只会让成绩更难，
        /// 所以不计入 `UNRATED`。
        const STRICT_JUDGE = 0x0200;
        /// Phira Pro：忽略横向位置，点屏幕任意处即可判定到最近的音符。
        /// 会让成绩更容易，因此计入 `UNRATED`。
        const FULLSCREEN_JUDGE = 0x0400;
        /// Phira Pro：失败只记录、不中止本局（练习用，血条与即时死亡都不会中断游玩）。
        /// 会让成绩更容易，因此计入 `UNRATED`。
        const NO_FAIL = 0x0800;
        /// 去连击分：连击不再计入分数，`score = accuracy * 1_000_000`。
        /// 会让成绩更容易，因此计入 `UNRATED`。
        ///
        /// 注：上游改版 Phirc Mod++ 把这个标志放在 `0x0200`，但我们的 `0x0200` 已被
        /// `STRICT_JUDGE` 占用，所以顺延到 `0x1000`（两边配置互不通用）。
        const NO_COMBO_SCORE = 0x1000;

        const UNRATED = Self::AUTOPLAY.bits()
            | Self::NO_SHADER.bits()
            | Self::FULLSCREEN_JUDGE.bits()
            | Self::NO_FAIL.bits()
            | Self::NO_COMBO_SCORE.bits();
    }
}

impl Mods {
    pub fn toggle_mod(&mut self, flag: Mods) {
        if self.contains(flag) {
            self.remove(flag);
        } else {
            for &conflict in Mods::conflicts(flag) {
                self.remove(conflict);
            }
            self.insert(flag);
        }
    }
    fn conflicts(flag: Mods) -> &'static [Mods] {
        match flag {
            Mods::FADE_IN => &[Mods::FADE_OUT],
            Mods::FADE_OUT => &[Mods::FADE_IN],
            Mods::INSTANT_DEATH_AP => &[Mods::INSTANT_DEATH_FC, Mods::NO_FAIL],
            Mods::INSTANT_DEATH_FC => &[Mods::INSTANT_DEATH_AP, Mods::NO_FAIL],
            Mods::NO_FAIL => &[Mods::INSTANT_DEATH_AP, Mods::INSTANT_DEATH_FC],
            _ => &[],
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(rename = "adjust_time_new")]
    pub adjust_time: bool,
    pub aggressive: bool,
    /// Flat block ranges; skips materials, hover and music filtering, not judgement.
    pub block_area_simple: bool,
    pub ap_fc_indicator: bool,
    pub aspect_ratio: Option<f32>,
    pub audio_buffer_size: Option<u32>,
    pub chart_debug: bool,
    /// 判定线调试：把本该隐藏/淡出的判定线以淡影保留，并在线旁显示编号 / 线高 / z / 类型。
    pub chart_debug_line: bool,
    /// 音符调试：在音符旁显示线号 / 时间 / 高度 / 类型，并画出它的横向判定范围。
    pub chart_debug_note: bool,
    pub disable_effect: bool,
    pub double_click_to_pause: bool,
    pub double_hint: bool,
    pub fullscreen_mode: bool,
    pub fxaa: bool,
    /// Phira Pro：血条模式。
    pub hp_mode: bool,
    /// 血条模式的扣血倍率（越大越难）。
    pub hp_amount: f32,
    /// 血条厚度相对暂停按钮高度的倍率。
    pub hp_height: f32,
    /// 血条长度。
    pub hp_width: f32,
    /// 血条整体倍率：回血与扣血同乘（对齐上游改版的 `health_scale`）。
    pub hp_scale: f32,
    /// 血条颜色。
    pub hp_color: HealthBarColor,
    /// 晚按补偿（毫秒）：晚按（偏差为负）时额外放宽的量，默认 0 = 早/晚完全对称。
    /// 上游把 70ms 写死在代码里且只作用在晚按一侧（等于「晚按白送 70ms」），这里改为可配置。
    pub late_leniency_ms: f32,
    /// 黄键保护：点击（蓝键）不会被叠在附近的 Drag（黄键）抢走判定。
    pub drag_protect: bool,
    /// 红键保护：点击（蓝键）不会被叠在附近的 Flick（红键）抢走判定。
    pub flick_protect: bool,
    /// 连击数下方显示的文字；留空则回退到「COMBO / AUTOPLAY」。
    pub combo_text: String,
    /// 结算画面是否画判定时间分布图（早 ← → 晚）。
    pub ending_judge_chart: bool,
    /// 尾判：开启后 hold 与 osu!mania 一样头尾各判一次（偏移条也会显示两次）。
    pub hold_tail_judge: bool,
    /// 软件 UI 主题：强调色（十六进制 RRGGBB，例如 "2196f3"）。
    pub ui_accent: String,
    /// 软件 UI 主题：表面色（按钮与弹窗底色，十六进制 RRGGBB）。
    pub ui_surface: String,
    /// 是否在左下角显示当前帧率数字。
    pub show_fps: bool,
    /// 失败后自动重试的次数上限（0 表示关闭）。
    pub auto_retry: f32,
    /// 自动重试/续练的提前量（秒）：从「失败时刻 − 提前量」处重开，0 表示从头开始。
    pub retry_lead: f32,
    /// 变速练习：练习模式每完成一圈就提速一档。
    pub practice_ramp: bool,
    /// 变速练习的起始速度。
    pub practice_speed_start: f32,
    /// 变速练习每圈提升的速度。
    pub practice_speed_step: f32,
    pub interactive: bool,
    pub lim_bad_ms: f32,
    pub lim_good_ms: f32,
    pub lim_perfect_plus_ms: f32,
    pub lim_perfect_ms: f32,
    pub mods: Mods,
    /// Phira API 基础地址（登录、谱面列表、成绩上传等）。留空则回退到官方地址。
    /// 自建/私服时改成自己的 API 地址即可脱离官方服务。
    pub api_url: String,
    /// Phira 网页前端地址（谱面页 / 用户页 / 合集页 / 条款链接等）。留空则回退到官方地址。
    pub web_url: String,
    /// 服务器状态页地址（设置页「服务器状态」按钮）。留空则回退到官方地址。
    pub status_url: String,
    pub note_scale: f32,
    pub offline_mode: bool,
    pub offset: f32,
    pub particle: bool,
    pub player_name: String,
    pub player_rks: f32,
    pub preferred_sample_rate: Option<u32>,
    pub res_pack_path: Option<String>,
    pub sample_count: u32,
    pub show_acc: bool,
    pub show_avg_fps: bool,
    pub speed: f32,
    pub touch_debug: bool,
    /// 调试触点颜色（十六进制 RRGGBB），只作用于实时触点指示；回放触点固定为蓝色。
    pub touch_point_color: u32,
    /// 调试触点透明度（0..=1），实时触点与回放触点共用。
    pub touch_point_alpha: f32,
    /// 调试触点半径（屏幕坐标；1.0 约等于屏幕高度的一半），实时触点与回放触点共用。
    pub touch_point_size: f32,
    pub use_keyboard: bool,
    /// Phira Pro：谱面流速。只等比缩放音符的**视觉**流速，音乐与音调完全不变。
    /// `1.0` 为官方表现。会改变成绩可比性，因此计入「不可上传」项。
    pub flow_speed: f32,
    /// Phira Pro：上/下隐强度，即音符出现（上隐）/ 消失（下隐）的高度，
    /// 取值为判定区高度的比例。`0.0` 表示沿用官方表现（按 Bad 判定窗口计时渐变）。
    /// 由于上隐/下隐 Mod 本身就属于 UNRATED，这里不需要再单独参与成绩闸门。
    pub fade_strength: f32,
    /// Phira Pro：局内判定偏移条（屏幕上方那条早/晚指示）。默认开启。
    pub offset_indicator: bool,
    pub volume_bgm: f32,
    pub volume_music: f32,
    pub volume_sfx: f32,
    // for compatibility
    autoplay: Option<bool>,
}

/// 一次游玩实际使用的判定窗口（单位：秒）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JudgeWindows {
    pub perfect_plus: f64,
    pub perfect: f64,
    pub good: f64,
    pub bad: f64,
    /// 是否开启「全屏判定」（忽略横向位置）。
    pub fullscreen: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            adjust_time: false,
            aggressive: true,
            block_area_simple: false,
            ap_fc_indicator: true,
            aspect_ratio: None,
            audio_buffer_size: None,
            chart_debug: false,
            chart_debug_line: false,
            chart_debug_note: false,
            disable_effect: false,
            double_click_to_pause: true,
            double_hint: true,
            fxaa: false,
            hp_mode: false,
            hp_amount: 1.0,
            hp_height: 1.0,
            hp_width: 0.6,
            hp_scale: 1.0,
            hp_color: HealthBarColor::default(),
            late_leniency_ms: 0.,
            drag_protect: false,
            flick_protect: false,
            flow_speed: 1.,
            fade_strength: 0.,
            offset_indicator: true,
            combo_text: "COMBO".to_owned(),
            ending_judge_chart: false,
            hold_tail_judge: false,
            ui_accent: "2196f3".to_owned(),
            ui_surface: "2a323c".to_owned(),
            show_fps: false,
            auto_retry: 0.,
            retry_lead: 0.,
            practice_ramp: false,
            practice_speed_start: 0.7,
            practice_speed_step: 0.1,
            interactive: true,
            lim_bad_ms: (crate::judge::LIMIT_BAD * 1000.) as f32,
            lim_good_ms: (crate::judge::LIMIT_GOOD * 1000.) as f32,
            lim_perfect_plus_ms: (crate::judge::LIMIT_PERFECT_PLUS * 1000.) as f32,
            lim_perfect_ms: (crate::judge::LIMIT_PERFECT * 1000.) as f32,
            mods: Mods::default(),
            api_url: String::new(),
            web_url: String::new(),
            status_url: String::new(),
            note_scale: 1.0,
            offline_mode: false,
            fullscreen_mode: false,
            offset: 0.,
            particle: true,
            player_name: "Mivik".to_string(),
            player_rks: 15.,
            preferred_sample_rate: None,
            res_pack_path: None,
            sample_count: 1,
            show_acc: false,
            show_avg_fps: false,
            speed: 1.,
            touch_debug: false,
            touch_point_color: 0xff3b30,
            touch_point_alpha: 0.4,
            touch_point_size: 0.04,
            use_keyboard: false,
            volume_music: 1.,
            volume_sfx: 1.,
            volume_bgm: 1.,

            autoplay: None,
        }
    }
}

impl Config {
    /// 强制各档窗口单调不减（perfect+ ≤ perfect ≤ good ≤ bad）。滑块越界或
    /// `data.json` 被手改成倒挂后，都会回到合法状态。
    pub fn clamp_judge_windows(&mut self) {
        self.lim_perfect_ms = self.lim_perfect_ms.max(self.lim_perfect_plus_ms);
        self.lim_good_ms = self.lim_good_ms.max(self.lim_perfect_ms);
        self.lim_bad_ms = self.lim_bad_ms.max(self.lim_good_ms);
    }

    /// 各档窗口的填写范围（毫秒），顺序为 perfect+ / perfect / good / bad。
    pub const JUDGE_WINDOW_RANGES: [(f32, f32); 4] = [(1., 80.), (1., 120.), (1., 250.), (1., 400.)];

    /// 按序号（0=perfect+, 1=perfect, 2=good, 3=bad）改写某一档判定窗口。
    ///
    /// 会保证 `perfect+ < perfect < good < bad` 严格递增：改动沿链条把不合规的
    /// 相邻档自动推到 `相邻值 ± 1ms`（往后加、往前减），最后再夹回各自的范围。
    pub fn set_judge_window(&mut self, idx: usize, value: f32) {
        let mut v = [self.lim_perfect_plus_ms, self.lim_perfect_ms, self.lim_good_ms, self.lim_bad_ms];
        let ranges = Self::JUDGE_WINDOW_RANGES;
        let idx = idx.min(3);
        v[idx] = if value.is_finite() { value } else { ranges[idx].0 };
        v[idx] = v[idx].clamp(ranges[idx].0, ranges[idx].1);
        // 后面的档必须依次更大
        for j in (idx + 1)..4 {
            v[j] = v[j].max(v[j - 1] + 1.);
        }
        // 前面的档必须依次更小
        for j in (0..idx).rev() {
            v[j] = v[j].min(v[j + 1] - 1.);
        }
        for j in 0..4 {
            v[j] = v[j].clamp(ranges[j].0, ranges[j].1);
        }
        // 夹取可能把两档重新拉平，最后再拉开一次
        for j in 1..4 {
            if v[j] <= v[j - 1] {
                v[j] = v[j - 1] + 1.;
            }
        }
        self.lim_perfect_plus_ms = v[0];
        self.lim_perfect_ms = v[1];
        self.lim_good_ms = v[2];
        self.lim_bad_ms = v[3];
    }

    /// 晚按补偿的上限（毫秒）。
    pub const LATE_LENIENCY_MAX: f32 = 200.;

    /// 晚按补偿（秒），夹在 0..=200ms；默认 0 = 早按晚按完全对称。
    #[inline]
    pub fn late_leniency(&self) -> f64 {
        if !self.late_leniency_ms.is_finite() {
            return 0.;
        }
        (self.late_leniency_ms.clamp(0., Self::LATE_LENIENCY_MAX) as f64) / 1000.
    }

    /// 把配置里的毫秒窗口折算成判定用的秒数；开启 `Mods::STRICT_JUDGE` 时各档整体减半。
    ///
    /// 这里会再夹一次单调性，因此即使配置倒挂也不会影响实际判定。
    pub fn judge_windows(&self) -> JudgeWindows {
        let perfect_plus = self.lim_perfect_plus_ms;
        let perfect = self.lim_perfect_ms.max(perfect_plus);
        let good = self.lim_good_ms.max(perfect);
        let bad = self.lim_bad_ms.max(good);
        let scale = if self.mods.contains(Mods::STRICT_JUDGE) { 0.5 } else { 1. };
        let secs = |ms: f32| ms as f64 / 1000. * scale;
        JudgeWindows {
            perfect_plus: secs(perfect_plus),
            perfect: secs(perfect),
            good: secs(good),
            bad: secs(bad),
            fullscreen: self.mods.contains(Mods::FULLSCREEN_JUDGE),
        }
    }

    /// 列出当前配置里「会改变判定 / 玩法、且不是官方默认值」的项。
    ///
    /// 返回空表示本局与官方默认模式完全一致。返回的是稳定的英文标识，用于日志与提示。
    ///
    /// 这些项之所以被算进来，是因为它们会让成绩与官方默认模式**不可比**
    /// （要么放宽了判定，要么改变了判定归属 / 生存条件）：
    /// - `offline_mode` / `use_keyboard` / 降速（上游 `rated` 口径）
    /// - 谱面 mod 或全局 mod 里带 `UNRATED`（自动游玩、全屏判定、无失败、去连击分……）
    ///   或 `STRICT_JUDGE`（判定窗口减半，成绩同样不可比）
    /// - 判定窗口 `lim_*_ms` 不是官方默认值
    /// - 晚按补偿 `late_leniency_ms` 不为 0
    /// - 黄键 / 红键保护、Hold 尾判
    /// - 血条扣血倍率 / 整体倍率不是 1.0
    pub fn non_official_items(&self, run_mods: Mods) -> Vec<&'static str> {
        let mods = self.mods | run_mods;
        let mut items = Vec::new();
        if self.offline_mode {
            items.push("offline_mode");
        }
        if self.use_keyboard {
            items.push("use_keyboard");
        }
        if self.speed < 1. - 1e-3 {
            items.push("speed");
        }
        if (self.flow_speed - 1.).abs() > 1e-3 {
            items.push("flow_speed");
        }
        if mods.intersects(Mods::UNRATED) {
            items.push("mods");
        }
        if mods.contains(Mods::STRICT_JUDGE) {
            items.push("strict_judge");
        }
        for (name, value, default) in [
            ("lim_perfect_plus", self.lim_perfect_plus_ms, crate::judge::LIMIT_PERFECT_PLUS),
            ("lim_perfect", self.lim_perfect_ms, crate::judge::LIMIT_PERFECT),
            ("lim_good", self.lim_good_ms, crate::judge::LIMIT_GOOD),
            ("lim_bad", self.lim_bad_ms, crate::judge::LIMIT_BAD),
        ] {
            if (value - (default * 1000.) as f32).abs() >= 0.5 {
                items.push(name);
            }
        }
        if self.late_leniency_ms > 0.5 {
            items.push("late_leniency");
        }
        if self.drag_protect {
            items.push("drag_protect");
        }
        if self.flick_protect {
            items.push("flick_protect");
        }
        if self.hold_tail_judge {
            items.push("hold_tail_judge");
        }
        if (self.hp_amount - 1.).abs() > 1e-3 {
            items.push("hp_amount");
        }
        if (self.hp_scale - 1.).abs() > 1e-3 {
            items.push("hp_scale");
        }
        items
    }

    /// 本局是否完全按官方默认的判定 / 玩法进行。
    ///
    /// 联机对战与「官方游玩模式」都用它来判断本局是否与官方口径一致。
    pub fn is_official_play(&self, run_mods: Mods) -> bool {
        self.non_official_items(run_mods).is_empty()
    }

    /// 把 [`Config::non_official_items`] 列出的项强制还原成官方默认值
    /// （联机对战用：只有严格按官方默认判定才有公平性可言）。
    ///
    /// 只改动真正偏离默认的项，因此玩家合法的设置（例如提速到 1.5x）不会被误伤。
    /// 返回被改动过的项（空表示本来就一致）。
    pub fn force_official_play(&mut self, run_mods: &mut Mods) -> Vec<&'static str> {
        let changed = self.non_official_items(*run_mods);
        for item in &changed {
            match *item {
                "offline_mode" => self.offline_mode = false,
                "use_keyboard" => self.use_keyboard = false,
                "speed" => self.speed = 1.,
                "flow_speed" => self.flow_speed = 1.,
                "mods" => {
                    self.mods.remove(Mods::UNRATED);
                    run_mods.remove(Mods::UNRATED);
                }
                "strict_judge" => {
                    self.mods.remove(Mods::STRICT_JUDGE);
                    run_mods.remove(Mods::STRICT_JUDGE);
                }
                "lim_perfect_plus" => self.lim_perfect_plus_ms = (crate::judge::LIMIT_PERFECT_PLUS * 1000.) as f32,
                "lim_perfect" => self.lim_perfect_ms = (crate::judge::LIMIT_PERFECT * 1000.) as f32,
                "lim_good" => self.lim_good_ms = (crate::judge::LIMIT_GOOD * 1000.) as f32,
                "lim_bad" => self.lim_bad_ms = (crate::judge::LIMIT_BAD * 1000.) as f32,
                "late_leniency" => self.late_leniency_ms = 0.,
                "drag_protect" => self.drag_protect = false,
                "flick_protect" => self.flick_protect = false,
                "hold_tail_judge" => self.hold_tail_judge = false,
                "hp_amount" => self.hp_amount = 1.,
                "hp_scale" => self.hp_scale = 1.,
                _ => {}
            }
        }
        changed
    }

    /// Phira Pro Flash（轻量版）：本构建不提供改判 / 降难度功能，加载配置时把相关项
    /// 一律夹回官方默认，防止手改 `data.json` 绕过界面。
    #[cfg(flash)]
    pub fn apply_flash_limits(&mut self) {
        self.offline_mode = false;
        self.use_keyboard = false;
        self.speed = if self.speed.is_finite() { self.speed.max(1.) } else { 1. };
        self.lim_perfect_plus_ms = (crate::judge::LIMIT_PERFECT_PLUS * 1000.) as f32;
        self.lim_perfect_ms = (crate::judge::LIMIT_PERFECT * 1000.) as f32;
        self.lim_good_ms = (crate::judge::LIMIT_GOOD * 1000.) as f32;
        self.lim_bad_ms = (crate::judge::LIMIT_BAD * 1000.) as f32;
        self.late_leniency_ms = 0.;
        self.drag_protect = false;
        self.flick_protect = false;
        self.hold_tail_judge = false;
        self.practice_ramp = false;
        self.mods
            .remove(Mods::FULLSCREEN_JUDGE | Mods::NO_FAIL | Mods::NO_COMBO_SCORE | Mods::STRICT_JUDGE);
    }

    /// Phira Pro Flash：开启「自动游玩」时调用。
    ///
    /// 自动游玩是官方 Phira 自带的 Mod，因此保留；但它会让成绩不可上传
    /// （`AUTOPLAY` 在 `UNRATED` 里，`is_official_play` 会判定为 false）。
    /// 这里把**所有会影响成绩可比性的设置项**恢复成官方默认。`mods` 本身不动。
    #[cfg(flash)]
    pub fn sanitize_on_autoplay(&mut self) {
        self.offline_mode = false;
        self.use_keyboard = false;
        self.speed = 1.;
        self.flow_speed = 1.;
        self.lim_perfect_plus_ms = (crate::judge::LIMIT_PERFECT_PLUS * 1000.) as f32;
        self.lim_perfect_ms = (crate::judge::LIMIT_PERFECT * 1000.) as f32;
        self.lim_good_ms = (crate::judge::LIMIT_GOOD * 1000.) as f32;
        self.lim_bad_ms = (crate::judge::LIMIT_BAD * 1000.) as f32;
        self.late_leniency_ms = 0.;
        self.drag_protect = false;
        self.flick_protect = false;
        self.hold_tail_judge = false;
        self.hp_amount = 1.;
        self.hp_scale = 1.;
    }

    /// 把当前主题色写入 `prpr::ui` 的全局量，供 `Ui::accent` / `Ui::background` 读取。
    /// 数值非法（或为空）时回落到默认色，绝不 panic。
    pub fn apply_ui_colors(&self) {
        /// 解析 "rrggbb"（允许 `#` 前缀），非法值回落到 `def`。
        fn parse_hex(s: &str, def: u32) -> u32 {
            let s = s.trim().trim_start_matches('#');
            if s.len() != 6 {
                return def;
            }
            u32::from_str_radix(s, 16).unwrap_or(def)
        }
        crate::ui::UI_ACCENT.store(parse_hex(&self.ui_accent, 0x2196f3), std::sync::atomic::Ordering::Relaxed);
        crate::ui::UI_SURFACE.store(parse_hex(&self.ui_surface, 0x2a323c), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn init(&mut self) {
        if let Some(flag) = self.autoplay {
            self.mods.set(Mods::AUTOPLAY, flag);
        }
        // 兼容旧配置：血条的三项数值换过量纲（厚度曾是绝对值），这里夹回滑块区间。
        self.hp_amount = self.hp_amount.clamp(0.2, 3.0);
        self.hp_width = self.hp_width.clamp(0.1, 1.0);
        self.hp_height = self.hp_height.clamp(0.5, 3.0);
        self.hp_scale = if self.hp_scale.is_finite() { self.hp_scale.clamp(0.2, 3.0) } else { 1.0 };
        // 晚按补偿：NaN / 越界都夹回合法区间。
        self.late_leniency_ms = if self.late_leniency_ms.is_finite() {
            self.late_leniency_ms.clamp(0., Self::LATE_LENIENCY_MAX)
        } else {
            0.
        };
        self.apply_ui_colors();
        crate::ui::SHOW_FPS.store(self.show_fps, std::sync::atomic::Ordering::Relaxed);
        #[cfg(target_env = "ohos")]
        {
            // Due to the fucking poor performance of the Maloon GPU, the sample count must be set to 1.
            self.sample_count = 1;
        }
        // Phira Pro Flash（轻量版）：不提供改判 / 降难度功能，这里把残留配置夹回官方默认。
        #[cfg(flash)]
        self.apply_flash_limits();
    }

    #[inline]
    pub fn has_mod(&self, m: Mods) -> bool {
        self.mods.contains(m)
    }

    #[inline]
    pub fn autoplay(&self) -> bool {
        self.has_mod(Mods::AUTOPLAY)
    }

    #[inline]
    pub fn flip_x(&self) -> bool {
        self.has_mod(Mods::FLIP_X)
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Mods};

    /// 默认配置必须与接入配置前的硬编码窗口完全一致（±80/160/220 ms），否则就是行为变更。
    #[test]
    fn judge_windows_default_unchanged() {
        let conf = Config::default();
        assert_eq!(conf.lim_perfect_ms, 80.0);
        assert_eq!(conf.lim_good_ms, 160.0);
        assert_eq!(conf.lim_bad_ms, 220.0);
        assert_eq!(conf.lim_perfect_plus_ms, 16.0);
        let w = conf.judge_windows();
        assert!((w.perfect_plus - 0.016).abs() < 1e-12);
        assert!((w.perfect - 0.08).abs() < 1e-12);
        assert!((w.good - 0.16).abs() < 1e-12);
        assert!((w.bad - 0.22).abs() < 1e-12);
    }

    /// 严格判定 Mod 在各档基础上整体减半。
    #[test]
    fn judge_windows_strict_halves() {
        let mut conf = Config::default();
        conf.mods.insert(Mods::STRICT_JUDGE);
        let w = conf.judge_windows();
        assert!((w.perfect_plus - 0.008).abs() < 1e-12);
        assert!((w.perfect - 0.04).abs() < 1e-12);
        assert!((w.good - 0.08).abs() < 1e-12);
        assert!((w.bad - 0.11).abs() < 1e-12);
    }

    /// 自定义毫秒窗口按比例生效。
    #[test]
    fn judge_windows_custom() {
        let mut conf = Config::default();
        conf.lim_perfect_ms = 40.0;
        conf.lim_good_ms = 100.0;
        conf.lim_bad_ms = 150.0;
        let w = conf.judge_windows();
        assert!((w.perfect - 0.04).abs() < 1e-12);
        assert!((w.good - 0.1).abs() < 1e-12);
        assert!((w.bad - 0.15).abs() < 1e-12);
    }

    /// 倒挂的配置即使没经过 `clamp_judge_windows` 也不会产出倒挂的窗口；
    /// 夹过之后配置本身也变合法。
    #[test]
    fn ui_colors_parse_and_fallback() {
        let mut config = Config::default();
        config.ui_accent = "#ff8800".to_owned();
        config.ui_surface = "not-a-color".to_owned();
        config.apply_ui_colors();
        assert_eq!(crate::ui::UI_ACCENT.load(std::sync::atomic::Ordering::Relaxed), 0xff8800);
        assert_eq!(crate::ui::UI_SURFACE.load(std::sync::atomic::Ordering::Relaxed), 0x2a323c);
    }

    #[test]
    fn judge_windows_clamped_monotonic() {
        let mut conf = Config::default();
        conf.lim_perfect_ms = 200.0;
        conf.lim_good_ms = 100.0;
        conf.lim_bad_ms = 50.0;
        let w = conf.judge_windows();
        assert!(w.perfect_plus <= w.perfect && w.perfect <= w.good && w.good <= w.bad);
        assert!((w.bad - 0.2).abs() < 1e-12);

        conf.clamp_judge_windows();
        assert_eq!(conf.lim_good_ms, 200.0);
        assert_eq!(conf.lim_bad_ms, 200.0);
    }

    /// 全屏判定由 Mod 控制。
    #[test]
    fn judge_windows_fullscreen_flag() {
        let mut conf = Config::default();
        assert!(!conf.judge_windows().fullscreen);
        conf.mods.insert(Mods::FULLSCREEN_JUDGE);
        assert!(conf.judge_windows().fullscreen);
    }

    /// 改某一档时其它档会被连带调整，保证 perfect+ < perfect < good < bad 严格递增。
    #[test]
    fn judge_window_strict_order_on_edit() {
        let mut conf = Config::default();

        // 把 perfect 顶到上限：后面的档被连带顶上去。
        conf.set_judge_window(1, 200.);
        assert_eq!(conf.lim_perfect_ms, 120.);
        assert!(conf.lim_perfect_plus_ms < conf.lim_perfect_ms);
        assert!(conf.lim_perfect_ms < conf.lim_good_ms);
        assert!(conf.lim_good_ms < conf.lim_bad_ms);

        // 把 good 压到很小：前面的档被连带压下来。
        conf.set_judge_window(2, 3.);
        assert!(conf.lim_perfect_plus_ms < conf.lim_perfect_ms);
        assert!(conf.lim_perfect_ms < conf.lim_good_ms);
        assert!(conf.lim_good_ms < conf.lim_bad_ms);

        // 越界值会被夹回范围，且仍然严格递增。
        conf.set_judge_window(0, 999.);
        assert_eq!(conf.lim_perfect_plus_ms, 80.);
        assert!(conf.lim_perfect_plus_ms < conf.lim_perfect_ms);
        conf.set_judge_window(3, -5.);
        assert!(conf.lim_good_ms < conf.lim_bad_ms);
    }

    /// 公平性闸门：任何改动判定 / 玩法的项都必须被识别出来，且能被还原。
    #[test]
    fn official_play_gate() {
        let mut conf = Config::default();
        assert!(conf.is_official_play(Mods::empty()));
        assert!(conf.non_official_items(Mods::empty()).is_empty());

        // 加速是官方允许的（上游 rated 口径是 speed >= 1.0）。
        conf.speed = 1.5;
        assert!(conf.is_official_play(Mods::empty()));

        // 判定窗口被改动。
        conf.lim_perfect_ms += 20.;
        assert_eq!(conf.non_official_items(Mods::empty()), vec!["lim_perfect"]);
        assert_eq!(conf.force_official_play(&mut Mods::empty()), vec!["lim_perfect"]);
        assert!(conf.is_official_play(Mods::empty()));

        // Pro 的判定修饰项：晚按补偿 / 黄键保护 / 尾判。
        conf.late_leniency_ms = 70.;
        conf.drag_protect = true;
        conf.hold_tail_judge = true;
        assert_eq!(conf.non_official_items(Mods::empty()).len(), 3);
        conf.force_official_play(&mut Mods::empty());
        assert!(conf.is_official_play(Mods::empty()));

        // 降速 + 键盘 + 自动游玩 / 严格判定。
        conf.speed = 0.8;
        conf.use_keyboard = true;
        let mut mods = Mods::AUTOPLAY | Mods::STRICT_JUDGE;
        let items = conf.non_official_items(mods);
        for expect in ["speed", "use_keyboard", "mods", "strict_judge"] {
            assert!(items.contains(&expect), "missing {expect} in {items:?}");
        }
        let changed = conf.force_official_play(&mut mods);
        assert_eq!(changed.len(), items.len());
        assert!(conf.is_official_play(mods));
        assert!(!mods.intersects(Mods::UNRATED | Mods::STRICT_JUDGE));
    }
}
