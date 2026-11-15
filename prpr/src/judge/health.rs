use super::Judgement;
use crate::config::Config;

const TENTHS_PER_POINT: u32 = 10;

#[derive(Clone)]
pub(super) enum Health {
    Points { left: u32, full: u32, damage: u32 },
    Legacy { ratio: f32, damage: f32, scale: f32 },
}

impl Default for Health {
    fn default() -> Self {
        let full = Config::DEFAULT_HP_CAPACITY * TENTHS_PER_POINT;
        Self::Points {
            left: full,
            full,
            damage: TENTHS_PER_POINT,
        }
    }
}

impl Health {
    pub(super) fn from_config(config: &Config) -> Self {
        if config.legacy_hp_damage {
            Self::Legacy {
                ratio: 1.,
                damage: config.hp_amount.max(0.),
                scale: config.hp_scale.max(0.),
            }
        } else {
            let full = config.hp_capacity() * TENTHS_PER_POINT;
            let damage = (config.hp_damage() * TENTHS_PER_POINT as f32).round() as u32;
            Self::Points { left: full, full, damage }
        }
    }

    pub(super) fn ratio(&self) -> f32 {
        match *self {
            Self::Points { left, full, .. } => left as f32 / full as f32,
            Self::Legacy { ratio, .. } => ratio,
        }
    }

    pub(super) fn reset(&mut self) {
        match self {
            Self::Points { left, full, .. } => *left = *full,
            Self::Legacy { ratio, .. } => *ratio = 1.,
        }
    }

    pub(super) fn apply(&mut self, judgement: Judgement) {
        use Judgement::*;
        match self {
            Self::Points { left, full, damage } => {
                // Keep the existing fractional healing while making each damaging hit exact.
                match judgement {
                    Bad | Miss => *left = left.saturating_sub(*damage),
                    _ => {
                        let heal = match judgement {
                            PerfectPlus => 20,
                            Perfect => 10,
                            Great => 6,
                            Good => 2,
                            _ => 0,
                        };
                        *left = (*left + heal).min(*full);
                    }
                }
            }
            Self::Legacy { ratio, damage, scale } => {
                let base = match judgement {
                    PerfectPlus => 0.02,
                    Perfect => 0.01,
                    Great => 0.006,
                    Good => 0.002,
                    Ok | Meh => 0.,
                    Bad => -0.06,
                    Miss => -0.12,
                };
                let delta = base * *scale * if base < 0. { *damage } else { 1. };
                *ratio = (*ratio + delta).clamp(0., 1.);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_damage_grades_use_life_points_and_reach_exactly_zero() {
        let mut config = Config::default();
        for (capacity, damage) in [(100, 1.), (200, 10.), (20, 1.5)] {
            config.set_hp_capacity(capacity);
            config.hp_amount = damage;
            let mut life = Health::from_config(&config);
            life.apply(Judgement::Bad);
            life.apply(Judgement::Miss);
            assert!((life.ratio() * capacity as f32 - (capacity as f32 - 2. * damage)).abs() < 0.001);
            life.reset();
            let hits = (capacity as f32 / damage).ceil() as u32;
            for _ in 1..hits {
                life.apply(Judgement::Miss);
            }
            assert!(life.ratio() > 0.);
            life.apply(Judgement::Miss);
            assert_eq!(life.ratio(), 0.);
            life.apply(Judgement::Good);
            assert!((life.ratio() * capacity as f32 - 0.2).abs() < 0.001);
            life.reset();
            life.apply(Judgement::PerfectPlus);
            assert_eq!(life.ratio(), 1.);
        }
    }

    #[test]
    fn old_replays_keep_the_previous_damage_weights() {
        let mut config = Config::default();
        config.legacy_hp_damage = true;
        config.hp_amount = 2.;
        let mut life = Health::from_config(&config);
        life.apply(Judgement::Miss);
        life.apply(Judgement::Bad);
        life.apply(Judgement::PerfectPlus);
        assert!((life.ratio() - 0.66).abs() < 0.00001);
    }
}
