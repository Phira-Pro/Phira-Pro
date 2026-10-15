prpr_l10n::tl_file!("chart_order");

use std::borrow::Cow;

use crate::page::ChartItem;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChartOrder {
    Default,
    Name,
    Difficulty,
    Rating,
    Composer,
    Charter,
    Score,
    Accuracy,
    Unplayed,
}

impl ChartOrder {
    pub fn label(&self) -> Cow<'static, str> {
        match self {
            Self::Default => tl!("time"),
            Self::Name => tl!("name"),
            Self::Difficulty => tl!("difficulty"),
            Self::Rating => tl!("rating"),
            Self::Composer => "曲师".into(),
            Self::Charter => "谱师".into(),
            Self::Score => "本地分数".into(),
            Self::Accuracy => "本地准度".into(),
            Self::Unplayed => "游玩状态".into(),
        }
    }

    pub fn apply<T>(&self, charts: &mut [T], f: impl Fn(&T) -> &ChartItem) {
        match self {
            Self::Default => {}
            Self::Name => {
                charts.sort_by(|x, y| f(x).info.name.cmp(&f(y).info.name));
            }
            Self::Difficulty => {
                charts.sort_by(|x, y| {
                    f(x).info
                        .difficulty
                        .partial_cmp(&f(y).info.difficulty)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            Self::Rating => {}
            Self::Composer => charts.sort_by_key(|x| f(x).info.composer.to_lowercase()),
            Self::Charter => charts.sort_by_key(|x| f(x).info.charter.to_lowercase()),
            Self::Score | Self::Accuracy | Self::Unplayed => {
                let records: std::collections::HashMap<_, _> = crate::get_data()
                    .charts
                    .iter()
                    .map(|c| (c.local_path.as_str(), c.record.as_ref()))
                    .collect();
                let by_id: std::collections::HashMap<_, _> = crate::get_data()
                    .charts
                    .iter()
                    .filter_map(|c| c.info.id.map(|id| (id, c.record.as_ref())))
                    .collect();
                let played: std::collections::HashSet<_> = crate::history::all().iter().map(|r| r.key.clone()).collect();
                let record = |x: &T| {
                    let c = f(x);
                    if let Some(value) = c.local_path.as_deref().and_then(|p| records.get(p)) {
                        *value
                    } else {
                        c.info.id.and_then(|id| by_id.get(&id).copied().flatten())
                    }
                };
                charts.sort_by(|a, b| {
                    let ap = record(a).is_some() || played.contains(&crate::replay::key_for(f(a).info.id, f(a).local_path.as_deref()));
                    let bp = record(b).is_some() || played.contains(&crate::replay::key_for(f(b).info.id, f(b).local_path.as_deref()));
                    let a = record(a);
                    let b = record(b);
                    match self {
                        Self::Score => a.map(|r| r.score).cmp(&b.map(|r| r.score)),
                        Self::Accuracy => a
                            .map(|r| r.accuracy)
                            .partial_cmp(&b.map(|r| r.accuracy))
                            .unwrap_or(std::cmp::Ordering::Equal),
                        _ => ap.cmp(&bp),
                    }
                });
            }
        }
    }
}
