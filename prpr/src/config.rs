//! Configuration module of the playing environment.\
//! e.g. player name, volume, speed, autoplay, etc.

use bitflags::bitflags;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub static TIPS: Lazy<Vec<String>> = Lazy::new(|| include_str!("tips.txt").split('\n').map(str::to_owned).collect());

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
    pub ap_fc_indicator: bool,
    pub aspect_ratio: Option<f32>,
    pub audio_buffer_size: Option<u32>,
    pub chart_debug: bool,
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
    pub mp_address: String,
    pub mp_enabled: bool,
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
    pub use_keyboard: bool,
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
            ap_fc_indicator: true,
            aspect_ratio: None,
            audio_buffer_size: None,
            chart_debug: false,
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
            combo_text: "COMBO".to_owned(),
            ending_judge_chart: false,
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
            mp_address: "mp2.phira.cn:12345".to_owned(),
            mp_enabled: false,
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
        self.hp_scale = if self.hp_scale.is_finite() {
            self.hp_scale.clamp(0.2, 3.0)
        } else {
            1.0
        };
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
}
