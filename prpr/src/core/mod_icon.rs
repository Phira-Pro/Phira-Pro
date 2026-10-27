//! Stable texture slots shared by asset loading and result badges.
use crate::ext::SafeTexture;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ModIcon {
    Mirror,
    FadeOut,
    FadeIn,
    Nightcore,
    Rainbow,
    Autoplay,
    NoShader,
    SuddenDeath,
    PerfectSound,
    FullscreenJudge,
    NoFail,
    NoComboScore,
}

impl ModIcon {
    pub const PATHS: [&'static str; 12] = [
        "mod/flip_x.png",
        "mod/fade_out.png",
        "mod/fade_in.png",
        "mod/nightcore.png",
        "mod/rainbow.png",
        "mod/autoplay.png",
        "mod/no-shader.png",
        "mod/sudden-death.png",
        "mod/perfect-sound.png",
        "mod/fullscreen-judge.png",
        "mod/no-fail.png",
        "mod/no-combo-score.png",
    ];
}

pub type ModIcons = [SafeTexture; ModIcon::PATHS.len()];
