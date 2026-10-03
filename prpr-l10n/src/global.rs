use std::{collections::HashMap, sync::Mutex};

use tracing::warn;
use unic_langid::LanguageIdentifier;

use crate::{fallback_langid, FALLBACK_LANG, LANG_IDENTS};

pub struct L10nGlobal {
    pub lang_map: HashMap<LanguageIdentifier, usize>,
    pub order: Mutex<Vec<usize>>,
}

impl Default for L10nGlobal {
    fn default() -> Self {
        Self::new()
    }
}

/// 把系统返回的语言标签归一到我们支持的语言之一。
///
/// 系统给的标签往往比我们的列表更细：iOS 上简体中文是 `zh-Hans-CN`，而列表里只有
/// `zh-CN`，精确比较失败就会一路回退到 en-US（表现为「iOS 首次启动是英文界面」）。
/// 这里先精确匹配，再按语言子标签退化匹配；`zh` 额外按 script / region 区分简繁。
pub fn normalize_langid(locale: &LanguageIdentifier) -> Option<LanguageIdentifier> {
    if let Some(found) = LANG_IDENTS.iter().find(|it| *it == locale) {
        return Some(found.clone());
    }
    // 退化成纯字符串处理，避免依赖 LanguageIdentifier 内部字段的具体类型。
    let tag = locale.to_string();
    let lang = tag.split('-').next().unwrap_or("");
    if lang == "zh" {
        let hant = tag.contains("Hant") || tag.contains("TW") || tag.contains("HK") || tag.contains("MO");
        let want = if hant { "zh-TW" } else { "zh-CN" };
        return LANG_IDENTS.iter().find(|it| it.to_string() == want).cloned();
    }
    LANG_IDENTS.iter().find(|it| it.to_string().split('-').next() == Some(lang)).cloned()
}

impl L10nGlobal {
    pub fn new() -> Self {
        let mut lang_map = HashMap::new();
        let mut order = Vec::new();
        let locale_lang = sys_locale::get_locale()
            .and_then(|it| it.parse::<LanguageIdentifier>().ok())
            .and_then(|it| normalize_langid(&it));
        if locale_lang.is_none() {
            warn!("unmatched system locale, falling back to `{}`", FALLBACK_LANG);
        }
        for (id, lang) in LANG_IDENTS.iter().enumerate() {
            lang_map.insert(lang.clone(), id);
            if Some(lang) == locale_lang.as_ref() {
                order.push(id);
            }
        }
        order.push(*lang_map.get(&fallback_langid!()).unwrap());
        Self {
            lang_map,
            order: order.into(),
        }
    }
}
