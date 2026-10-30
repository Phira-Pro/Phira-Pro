use macroquad::prelude::*;

pub(super) struct Layout {
    pub outer: Rect,
    pub chart: Rect,
    pub tabs: Rect,
    pub settings: Rect,
    pub status: Rect,
    pub footer: [Rect; 2],
}
impl Layout {
    pub fn new(content: Rect) -> Self {
        let outer = Rect::new(content.x + 0.015, content.y + 0.01, content.w - 0.03, content.h - 0.02);
        let x = outer.x + 0.02;
        let w = outer.w - 0.04;
        let chart = Rect::new(x, outer.y + 0.018, w, 0.09);
        let tabs = Rect::new(x, chart.bottom() + 0.014, w, 0.065);
        let footer_y = outer.bottom() - 0.09;
        let button_w = ((w - 0.016) / 2.).min(0.5);
        let footer = std::array::from_fn(|i| Rect::new(x + i as f32 * (button_w + 0.016), footer_y, button_w, 0.07));
        let status = Rect::new(x, footer_y - 0.12, w, 0.105);
        let settings = Rect::new(x, tabs.bottom() + 0.016, w, (status.y - 0.016 - tabs.bottom() - 0.016).max(0.05));
        Self {
            outer,
            chart,
            tabs,
            settings,
            status,
            footer,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_tabs_status_and_footer_do_not_overlap_at_supported_aspects() {
        for aspect in [1., 4. / 3., 3. / 2., 16. / 9., 21. / 9.] {
            let content = Rect::new(-0.7, -1. / aspect + 0.15, 1.67, 2. / aspect - 0.18);
            let layout = Layout::new(content);
            assert!(layout.chart.bottom() < layout.tabs.y);
            assert!(layout.tabs.bottom() < layout.settings.y);
            assert!(layout.settings.bottom() < layout.status.y);
            assert!(layout.status.bottom() < layout.footer[0].y);
            assert!(layout.footer[0].right() < layout.footer[1].x);
        }
    }
}
