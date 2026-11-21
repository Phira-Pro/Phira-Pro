prpr_l10n::tl_file!("chart_order");

use std::{
    borrow::Cow,
    cmp::{Ordering, Reverse},
};

use crate::{client::Chart, page::ChartItem};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum NamePart {
    Number(usize, String),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_names_sort_without_reversing_equal_items() {
        let mut names = ["song 10", "Song 2", "song 02", "alpha"];
        sort_names(&mut names, false, |name| name);
        assert_eq!(names, ["alpha", "Song 2", "song 02", "song 10"]);
        sort_names(&mut names, true, |name| name);
        assert_eq!(names, ["song 10", "Song 2", "song 02", "alpha"]);
    }

    #[test]
    fn unknown_numeric_values_stay_after_known_values_in_both_directions() {
        let mut values = [None, Some(12.0), Some(f32::NAN), Some(2.0)];
        values.sort_by(|a, b| compare_floats(*a, *b, false));
        assert_eq!(values[..3], [Some(2.0), Some(12.0), None]);
        values.sort_by(|a, b| compare_floats(*a, *b, true));
        assert_eq!(values[..3], [Some(12.0), Some(2.0), None]);
    }
}

fn name_key(name: &str) -> Vec<NamePart> {
    name.trim()
        .to_lowercase()
        .as_bytes()
        .chunk_by(|a, b| a.is_ascii_digit() == b.is_ascii_digit())
        .map(|part| {
            let text = std::str::from_utf8(part).unwrap();
            if part[0].is_ascii_digit() {
                let digits = text.trim_start_matches('0');
                NamePart::Number(digits.len(), digits.to_owned())
            } else {
                NamePart::Text(text.to_owned())
            }
        })
        .collect()
}

fn sort_names<T>(items: &mut [T], descending: bool, name: impl Fn(&T) -> &str) {
    if descending {
        items.sort_by_cached_key(|item| Reverse(name_key(name(item))));
    } else {
        items.sort_by_cached_key(|item| name_key(name(item)));
    }
}

fn direction(order: Ordering, descending: bool) -> Ordering {
    if descending {
        order.reverse()
    } else {
        order
    }
}

fn compare_known<T>(a: Option<T>, b: Option<T>, descending: bool, compare: impl FnOnce(T, T) -> Ordering) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => direction(compare(a, b), descending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_floats(a: Option<f32>, b: Option<f32>, descending: bool) -> Ordering {
    compare_known(a.filter(|x| x.is_finite()), b.filter(|x| x.is_finite()), descending, |a, b| a.total_cmp(&b))
}

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

    pub fn uses_local_data(self) -> bool {
        matches!(self, Self::Composer | Self::Charter | Self::Score | Self::Accuracy | Self::Unplayed)
    }

    pub fn apply_popular(&self, charts: &mut [Chart], descending: bool) {
        match self {
            Self::Default if !descending => charts.reverse(),
            Self::Name => sort_names(charts, descending, |chart| &chart.name),
            Self::Difficulty => charts.sort_by(|a, b| compare_floats(Some(a.difficulty), Some(b.difficulty), descending)),
            Self::Rating => charts.sort_by(|a, b| compare_floats(a.rating, b.rating, descending)),
            _ => {}
        }
    }

    pub fn apply<T>(&self, charts: &mut [T], descending: bool, f: impl Fn(&T) -> &ChartItem) {
        match self {
            Self::Default if descending => charts.reverse(),
            Self::Default => {}
            Self::Name => sort_names(charts, descending, |item| &f(item).info.name),
            Self::Difficulty => charts.sort_by(|a, b| compare_floats(Some(f(a).info.difficulty), Some(f(b).info.difficulty), descending)),
            Self::Rating => {}
            Self::Composer => sort_names(charts, descending, |item| &f(item).info.composer),
            Self::Charter => sort_names(charts, descending, |item| &f(item).info.charter),
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
                let played: std::collections::HashSet<_> = if *self == Self::Unplayed {
                    crate::history::all().into_iter().map(|r| r.key).collect()
                } else {
                    Default::default()
                };
                let record = |x: &T| {
                    let c = f(x);
                    if let Some(value) = c.local_path.as_deref().and_then(|p| records.get(p)) {
                        *value
                    } else {
                        c.info.id.and_then(|id| {
                            by_id
                                .get(&id)
                                .copied()
                                .flatten()
                                .or_else(|| crate::get_data().local_records.get(&format!("download/{id}")).and_then(Option::as_ref))
                        })
                    }
                };
                charts.sort_by(|a, b| match self {
                    Self::Score => compare_known(record(a).map(|r| r.score), record(b).map(|r| r.score), descending, |a, b| a.cmp(&b)),
                    Self::Accuracy => compare_floats(record(a).map(|r| r.accuracy), record(b).map(|r| r.accuracy), descending),
                    _ => {
                        let is_played = |item| {
                            let chart = f(item);
                            record(item).is_some() || played.contains(&crate::replay::key_for(chart.info.id, chart.local_path.as_deref()))
                        };
                        direction(is_played(a).cmp(&is_played(b)), descending)
                    }
                });
            }
        }
    }
}
