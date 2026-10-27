//! Result-strip names and texture choices. Upload eligibility lives in Config.
use crate::{config::Mods, core::ModIcon};

pub(super) struct ModBadge {
    pub name: &'static str,
    pub icon: Option<ModIcon>,
}

pub(super) fn active(mut mods: Mods) -> Vec<ModBadge> {
    // Match note-behaviour precedence and use one shared sudden-death glyph.
    if mods.contains(Mods::NOTE_TAP) {
        mods.remove(Mods::NOTE_DRAG | Mods::NOTE_FLICK);
    } else if mods.contains(Mods::NOTE_DRAG) {
        mods.remove(Mods::NOTE_FLICK);
    }
    if mods.contains(Mods::INSTANT_DEATH_AP) {
        mods.remove(Mods::INSTANT_DEATH_FC);
    }
    [
        (Mods::AUTOPLAY, "AUTOPLAY", Some(ModIcon::Autoplay)),
        (Mods::FLIP_X, "MIRROR", Some(ModIcon::Mirror)),
        (Mods::FADE_OUT, "FADE OUT", Some(ModIcon::FadeOut)),
        (Mods::FADE_IN, "FADE IN", Some(ModIcon::FadeIn)),
        (Mods::NIGHTCORE, "NIGHTCORE", Some(ModIcon::Nightcore)),
        (Mods::RAINBOW, "RAINBOW", Some(ModIcon::Rainbow)),
        (Mods::NO_SHADER, "NO SHADER", Some(ModIcon::NoShader)),
        (Mods::INSTANT_DEATH_AP, "AP CHALLENGE", Some(ModIcon::SuddenDeath)),
        (Mods::INSTANT_DEATH_FC, "FC CHALLENGE", Some(ModIcon::SuddenDeath)),
        (Mods::FULLSCREEN_JUDGE, "FULLSCREEN JUDGE", Some(ModIcon::FullscreenJudge)),
        (Mods::NO_FAIL, "NO FAIL", Some(ModIcon::NoFail)),
        (Mods::NO_COMBO_SCORE, "NO COMBO SCORE", Some(ModIcon::NoComboScore)),
        (Mods::PERFECT_SOUND, "PERFECT SOUND", Some(ModIcon::PerfectSound)),
        (Mods::NOTE_TAP, "ALL TAP", None),
        (Mods::NOTE_DRAG, "ALL DRAG", None),
        (Mods::NOTE_FLICK, "ALL FLICK", None),
        (Mods::STRICT_JUDGE, "STRICT MODE", None),
    ]
    .into_iter()
    .filter(|(flag, _, _)| mods.contains(*flag))
    .map(|(_, name, icon)| ModBadge { name, icon })
    .collect()
}
