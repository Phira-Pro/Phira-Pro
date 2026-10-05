use glyph_brush::{
    ab_glyph::{point, Font, FontArc, Rect, ScaleFont},
    FontId, GlyphCruncher, HorizontalAlign, Layout, Section, SectionGlyph, Text,
};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(super) struct InkCache(HashMap<(usize, u16), Option<Rect>>);

impl InkCache {
    fn bounds(&mut self, fonts: &[FontArc], glyph: &SectionGlyph) -> Option<Rect> {
        let key = (glyph.font_id.0, glyph.glyph.id.0);
        if self.0.len() >= 4096 && !self.0.contains_key(&key) {
            self.0.clear();
        }
        // Cache font-unit bounds, not rasterized outlines or pixel-size variants.
        let bounds = *self
            .0
            .entry(key)
            .or_insert_with(|| fonts[key.0].outline(glyph.glyph.id).map(|o| o.bounds));
        let bounds = bounds?;
        let scale = fonts[key.0].as_scaled(glyph.glyph.scale).scale_factor();
        let p = glyph.glyph.position;
        // ab_glyph outlines already store min/max in the order used for Y-down.
        Some(Rect {
            min: point(p.x + bounds.min.x * scale.horizontal, p.y - bounds.min.y * scale.vertical),
            max: point(p.x + bounds.max.x * scale.horizontal, p.y - bounds.max.y * scale.vertical),
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct LayoutOptions {
    pub scale: f32,
    pub primary_scale: f32,
    pub max_width: Option<f32>,
    pub baseline: bool,
    pub multiline: bool,
    pub h_align: HorizontalAlign,
    pub color: [f32; 4],
}

fn font_id(fonts: &[FontArc], c: char) -> usize {
    if c.is_whitespace() {
        return 0;
    }
    fonts.iter().position(|f| f.glyph_id(c).0 != 0).unwrap_or(0)
}

fn runs<'a>(text: &'a str, fonts: &[FontArc], options: LayoutOptions) -> Vec<Text<'a>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut current = 0;
    for (index, c) in text.char_indices() {
        let id = font_id(fonts, c);
        if id != current {
            if start < index {
                result.push(run(&text[start..index], current, options));
            }
            start = index;
            current = id;
        }
    }
    if start < text.len() {
        result.push(run(&text[start..], current, options));
    }
    result
}

fn run(text: &str, id: usize, options: LayoutOptions) -> Text<'_> {
    Text::new(text)
        .with_font_id(FontId(id))
        .with_scale(options.scale * if id == 0 { options.primary_scale } else { 1. })
        .with_color(options.color)
}

fn advance(fonts: &[FontArc], glyph: &SectionGlyph) -> f32 {
    glyph.glyph.position.x + fonts[glyph.font_id.0].as_scaled(glyph.glyph.scale).h_advance(glyph.glyph.id)
}

fn horizontal_bounds(cache: &mut InkCache, fonts: &[FontArc], glyphs: &[SectionGlyph]) -> (f32, f32) {
    let mut left: f32 = 0.;
    let mut right: f32 = 0.;
    for glyph in glyphs {
        left = left.min(glyph.glyph.position.x);
        right = right.max(advance(fonts, glyph));
        if let Some(ink) = cache.bounds(fonts, glyph) {
            left = left.min(ink.min.x);
            right = right.max(ink.max.x);
        }
    }
    (left, right)
}

fn previous_grapheme(text: &str, end: usize) -> usize {
    text[..end].grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
}

pub(super) fn layout_text<'a>(
    brush: &mut impl GlyphCruncher,
    cache: &mut InkCache,
    text: &'a str,
    options: LayoutOptions,
) -> (Section<'a>, (f32, f32, f32, f32)) {
    let fonts = brush.fonts().to_vec();
    let mut section = Section::default().with_text(runs(text, &fonts, options));
    let width = options.max_width.map(|w| if w.is_finite() { w.max(0.) } else { f32::INFINITY });
    let mut glyphs;
    if options.multiline {
        section = section.with_layout(Layout::default().h_align(options.h_align));
        if let Some(width) = width {
            section.bounds.0 = width;
        }
        glyphs = brush.glyphs(section.clone()).cloned().collect::<Vec<_>>();
    } else {
        // Width-limited labels are one line. Measure from the left before
        // applying alignment so centered labels use the full width as well.
        section = section.with_layout(Layout::default_single_line());
        glyphs = brush.glyphs(section.clone()).cloned().collect::<Vec<_>>();
        if let Some(width) = width {
            let (left, right) = horizontal_bounds(cache, &fonts, &glyphs);
            if right - left > width {
                let suffix = if fonts.iter().any(|f| f.glyph_id('…').0 != 0) { "…" } else { "..." };
                let suffix_runs = runs(suffix, &fonts, options);
                let suffix_section = Section::default()
                    .with_layout(Layout::default_single_line())
                    .with_text(suffix_runs.clone());
                let suffix_glyphs = brush.glyphs(suffix_section).cloned().collect::<Vec<_>>();
                let (suffix_left, suffix_right) = horizontal_bounds(cache, &fonts, &suffix_glyphs);
                let budget = width - (suffix_right - suffix_left);
                let mut end = 0;
                let mut run_starts = Vec::new();
                let mut start = 0;
                for run in &section.text {
                    run_starts.push(start);
                    start += run.text.len();
                }
                for glyph in &glyphs {
                    // byte_index belongs to its font run, never the whole label.
                    let global = run_starts[glyph.section_index] + glyph.byte_index;
                    if advance(&fonts, glyph) > budget {
                        break;
                    }
                    if let Some(c) = text[global..].chars().next() {
                        end = global + c.len_utf8();
                    }
                }
                end = text
                    .grapheme_indices(true)
                    .map(|(i, g)| i + g.len())
                    .take_while(|&i| i <= end)
                    .last()
                    .unwrap_or(0);
                loop {
                    let mut clipped = runs(&text[..end], &fonts, options);
                    if budget >= 0. {
                        clipped.extend(suffix_runs.clone());
                    }
                    section.text = clipped;
                    glyphs = brush.glyphs(section.clone()).cloned().collect();
                    let (left, right) = horizontal_bounds(cache, &fonts, &glyphs);
                    if right - left <= width + 0.001 || end == 0 {
                        break;
                    }
                    end = previous_grapheme(text, end);
                }
            }
        }
        if options.h_align != HorizontalAlign::Left {
            section.layout = Layout::default_single_line().h_align(options.h_align);
            glyphs = brush.glyphs(section.clone()).cloned().collect();
        }
    }

    let line_font = fonts[0].as_scaled(options.scale * options.primary_scale);
    let line_height = if options.baseline { line_font.ascent() } else { line_font.height() };
    let (left, right) = horizontal_bounds(cache, &fonts, &glyphs);
    if !options.baseline {
        let mut ink: Option<Rect> = None;
        for glyph in &glyphs {
            if let Some(bounds) = cache.bounds(&fonts, glyph) {
                ink = Some(match ink {
                    None => bounds,
                    Some(old) => Rect {
                        min: point(old.min.x.min(bounds.min.x), old.min.y.min(bounds.min.y)),
                        max: point(old.max.x.max(bounds.max.x), old.max.y.max(bounds.max.y)),
                    },
                });
            }
        }
        if let Some(ink) = ink {
            return (section, (left, ink.min.y, right - left, ink.height()));
        }
    }
    if options.multiline {
        let bound = brush.glyph_bounds(&section).unwrap_or_default();
        let height = (bound.height() + if options.baseline { line_font.descent() } else { 0. }).max(0.);
        (section, (left, bound.min.y, right - left, height))
    } else {
        (section, (left, 0., right - left, line_height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_brush::{GlyphBrush, GlyphBrushBuilder};

    fn setup() -> (GlyphBrush<()>, InkCache, LayoutOptions) {
        let primary = FontArc::try_from_slice(include_bytes!("../../../../assets/phigros.ttf")).unwrap();
        let fallback = FontArc::try_from_slice(include_bytes!("../../../../assets/harmonyos.ttf")).unwrap();
        let brush = GlyphBrushBuilder::using_fonts(vec![primary, fallback]).build();
        (
            brush,
            InkCache::default(),
            LayoutOptions {
                scale: 30.,
                primary_scale: 1.25,
                max_width: None,
                baseline: false,
                multiline: false,
                h_align: HorizontalAlign::Left,
                color: [1.; 4],
            },
        )
    }

    #[test]
    fn mixed_font_truncation_is_utf8_safe_and_preserves_fallback() {
        let (mut brush, mut cache, mut options) = setup();
        for text in [
            "AP 全连 All Perfect 即死模式测试",
            "中文ABC中文DEF中文GHI测试",
            "开启后，打击音效在 Perfect 时机播放。",
        ] {
            for width in (0..460).step_by(3) {
                options.max_width = Some(width as f32);
                let (section, bounds) = layout_text(&mut brush, &mut cache, text, options);
                assert!(bounds.2 <= width as f32 + 0.001, "{text}: {width}, {bounds:?}");
                for run in &section.text {
                    for c in run.text.chars().filter(|c| !c.is_whitespace()) {
                        assert_eq!(run.font_id.0, font_id(brush.fonts(), c));
                    }
                }
            }
        }
    }

    #[test]
    fn clipping_does_not_split_combining_or_emoji_clusters() {
        let (mut brush, mut cache, mut options) = setup();
        let text = "Aa\u{0301}中文👩‍👩‍👧‍👦BB中文";
        let boundaries = text.grapheme_indices(true).map(|(i, g)| i + g.len()).collect::<Vec<_>>();
        for width in 0..260 {
            options.max_width = Some(width as f32);
            let (section, _) = layout_text(&mut brush, &mut cache, text, options);
            let rendered: String = section.text.iter().map(|r| r.text).collect();
            if let Some(prefix) = rendered.strip_suffix('…') {
                assert!(prefix.is_empty() || boundaries.contains(&prefix.len()));
            }
        }
    }

    #[test]
    fn optical_height_matches_glyph_outlines_for_fallback_and_descenders() {
        let (mut brush, mut cache, options) = setup();
        for text in ["HIM", "gjpq", "中文 Abc", "() +", "MODS"] {
            let (section, bounds) = layout_text(&mut brush, &mut cache, text, options);
            let glyphs = brush.glyphs(section).cloned().collect::<Vec<_>>();
            let mut top = f32::INFINITY;
            let mut bottom = f32::NEG_INFINITY;
            for glyph in glyphs {
                if let Some(outline) = brush.fonts()[glyph.font_id.0].outline_glyph(glyph.glyph) {
                    top = top.min(outline.px_bounds().min.y);
                    bottom = bottom.max(outline.px_bounds().max.y);
                }
            }
            assert!((bounds.1 - top).abs() <= 1.01, "{text}: {bounds:?}, {top}");
            assert!((bounds.1 + bounds.3 - bottom).abs() <= 1.01, "{text}: {bounds:?}, {bottom}");
        }
    }

    #[test]
    fn alignment_empty_text_and_multiline_stay_finite() {
        let (mut brush, mut cache, mut options) = setup();
        for text in ["", "   ", "\n", "\n中文\nAB\n", "Mixed 中文 very long line"] {
            for multiline in [false, true] {
                for align in [HorizontalAlign::Left, HorizontalAlign::Center, HorizontalAlign::Right] {
                    options.multiline = multiline;
                    options.h_align = align;
                    options.max_width = Some(100.);
                    let (_, b) = layout_text(&mut brush, &mut cache, text, options);
                    assert!([b.0, b.1, b.2, b.3].iter().all(|f| f.is_finite()));
                    assert!(b.2 >= 0. && b.3 >= 0.);
                }
            }
        }
    }

    #[test]
    fn user_font_and_all_localized_mod_labels_are_safe() {
        let Some(path) = std::env::var_os("PHIRA_TEST_FONT") else {
            return;
        };
        let primary = super::super::parse_font(std::fs::read(path).unwrap()).unwrap();
        let fallback = FontArc::try_from_slice(include_bytes!("../../../../assets/harmonyos.ttf")).unwrap();
        let mut brush: GlyphBrush<()> = GlyphBrushBuilder::using_fonts(vec![primary, fallback]).build();
        let mut cache = InkCache::default();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../phira/locales");
        let mut count = 0;
        for locale in std::fs::read_dir(root).unwrap() {
            let path = locale.unwrap().path().join("song.ftl");
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            for line in source.lines().filter(|l| l.starts_with("mods-")) {
                let Some((_, label)) = line.split_once('=') else {
                    continue;
                };
                for display_scale in [0.8, 1., 1.2] {
                    for width in [0., 12., 90., 230., 460.] {
                        let options = LayoutOptions {
                            scale: 30. * display_scale,
                            primary_scale: 1.4,
                            max_width: Some(width),
                            baseline: false,
                            multiline: false,
                            h_align: HorizontalAlign::Left,
                            color: [1.; 4],
                        };
                        let (_, b) = layout_text(&mut brush, &mut cache, label.trim(), options);
                        assert!(b.2 <= width + 0.001 && b.3.is_finite(), "{label}: {b:?}");
                    }
                }
                count += 1;
            }
        }
        assert!(count > 100);
    }

    #[test]
    fn zero_line_height_is_rejected_before_layout() {
        let mut bytes = include_bytes!("../../../../assets/phigros.ttf").to_vec();
        let table_count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        for i in 0..table_count {
            let p = 12 + i * 16;
            let offset = u32::from_be_bytes(bytes[p + 8..p + 12].try_into().unwrap()) as usize;
            match &bytes[p..p + 4] {
                b"hhea" => bytes[offset + 4..offset + 10].fill(0),
                b"OS/2" => bytes[offset + 68..offset + 78].fill(0),
                _ => {}
            }
        }
        assert!(super::super::parse_font(bytes).is_err());
    }
}
