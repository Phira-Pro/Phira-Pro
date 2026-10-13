//! Observer-only extension boundary. Extensions cannot change chart or judgement state.
use crate::{config::Config, core::Chart, info::ChartInfo};
type ChartObserver = fn(&Chart, &ChartInfo, &Config, &[u8]);
thread_local! { static CHART_OBSERVER: std::cell::Cell<Option<ChartObserver>> = const { std::cell::Cell::new(None) }; }
pub fn set_chart_observer(observer: ChartObserver) {
    CHART_OBSERVER.with(|it| it.set(Some(observer)));
}
pub fn chart_loaded(chart: &Chart, info: &ChartInfo, config: &Config, raw: &[u8]) {
    CHART_OBSERVER.with(|it| {
        if let Some(f) = it.get() {
            f(chart, info, config, raw);
        }
    });
}
