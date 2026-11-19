use super::Judgement;
use crate::config::Config;
use crate::course::CourseLife;

const TENTHS_PER_POINT: u32 = 10;

#[derive(Clone)]
pub(super) enum Health {
    Points {
        left: u32,
        start: u32,
        full: u32,
        damage: u32,
        healing: bool,
        course_life: bool,
    },
    Legacy {
        ratio: f32,
        damage: f32,
        scale: f32,
        healing: bool,
    },
}

impl Default for Health {
    fn default() -> Self {
        let full = Config::DEFAULT_HP_CAPACITY * TENTHS_PER_POINT;
        Self::Points {
            left: full,
            start: full,
            full,
            damage: TENTHS_PER_POINT,
            healing: true,
            course_life: false,
        }
    }
}

impl Health {
    pub(super) fn from_config(config: &Config, course: bool) -> Self {
        let healing = !config.hp_mode || !config.hp_disable_healing;
        if config.legacy_hp_damage {
            Self::Legacy {
                ratio: 1.,
                damage: config.hp_amount.max(0.),
                scale: config.hp_scale.max(0.),
                healing,
            }
        } else {
            let full = config.hp_capacity() * TENTHS_PER_POINT;
            let damage = (config.hp_damage() * TENTHS_PER_POINT as f32).round() as u32;
            Self::Points { left: full, start: full, full, damage, healing, course_life: course && config.hp_mode }
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
            Self::Points { left, start, .. } => *left = *start,
            Self::Legacy { ratio, .. } => *ratio = 1.,
        }
    }

    pub(super) fn course_life(&self) -> Option<CourseLife> {
        match *self {
            Self::Points { left, course_life: true, .. } => Some(CourseLife(left)),
            _ => None,
        }
    }

    pub(super) fn carry_course_life(&mut self, life: CourseLife) {
        if let Self::Points { left, start, full, course_life: true, .. } = self {
            *start = life.0.min(*full);
            *left = *start;
        }
    }

    pub(super) fn apply(&mut self, judgement: Judgement) {
        use Judgement::*;
        match self {
            Self::Points { left, full, damage, healing, course_life, .. } => match judgement {
                Bad | Miss if *course_life => *left = left.saturating_sub(*damage * 2),
                Bad | Miss => *left = left.saturating_sub(*damage),
                Good if *course_life => *left = left.saturating_sub(*damage),
                _ if *healing => {
                    let heal = match judgement {
                        PerfectPlus => 20,
                        Perfect => 10,
                        Great => 6,
                        Good => 2,
                        _ => 0,
                    };
                    *left = (*left + heal).min(*full);
                }
                _ => {}
            },
            Self::Legacy { ratio, damage, scale, healing } => {
                let base = match judgement {
                    PerfectPlus => 0.02,
                    Perfect => 0.01,
                    Great => 0.006,
                    Good => 0.002,
                    Ok | Meh => 0.,
                    Bad => -0.06,
                    Miss => -0.12,
                };
                if base > 0. && !*healing {
                    return;
                }
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
        let mut config: Config = serde_yaml::from_str("hpMode: true").unwrap();
        assert!(config.hp_disable_healing);
        for (capacity, damage) in [(100, 1.), (200, 10.), (20, 1.5)] {
            config.set_hp_capacity(capacity);
            config.hp_amount = damage;
            let mut life = Health::from_config(&config, false);
            life.apply(Judgement::Bad);
            life.apply(Judgement::Miss);
            assert!((life.ratio() * capacity as f32 - (capacity as f32 - 2. * damage)).abs() < 0.001);
            let damaged = life.ratio();
            for grade in [Judgement::PerfectPlus, Judgement::Perfect, Judgement::Great, Judgement::Good] {
                life.apply(grade);
                assert_eq!(life.ratio(), damaged);
            }
            life.reset();
            let hits = (capacity as f32 / damage).ceil() as u32;
            for _ in 1..hits {
                life.apply(Judgement::Miss);
            }
            assert!(life.ratio() > 0.);
            life.apply(Judgement::Miss);
            assert_eq!(life.ratio(), 0.);
            life.apply(Judgement::Good);
            assert_eq!(life.ratio(), 0.);
            life.reset();
            life.apply(Judgement::PerfectPlus);
            assert_eq!(life.ratio(), 1.);
        }
    }

    #[test]
    fn turning_off_disable_healing_restores_fractional_recovery() {
        let config: Config = serde_yaml::from_str("hpMode: true\nhpDisableHealing: false\nhpAmount: 10").unwrap();
        let mut life = Health::from_config(&config, false);
        life.apply(Judgement::Miss);
        for (grade, expected) in [
            (Judgement::PerfectPlus, 92.),
            (Judgement::Perfect, 93.),
            (Judgement::Great, 93.6),
            (Judgement::Good, 93.8),
        ] {
            life.apply(grade);
            assert!((life.ratio() * 100. - expected).abs() < 0.001);
        }
    }

    #[test]
    fn old_replays_keep_the_previous_damage_weights() {
        let mut config = Config::default();
        config.legacy_hp_damage = true;
        config.hp_amount = 2.;
        let mut life = Health::from_config(&config, false);
        life.apply(Judgement::Miss);
        life.apply(Judgement::Bad);
        life.apply(Judgement::PerfectPlus);
        assert!((life.ratio() - 0.66).abs() < 0.00001);
    }

    #[test]
    fn course_life_uses_weighted_damage_and_existing_healing() {
        let mut config = Config::default();
        config.hp_mode = true;
        config.hp_amount = 1.5;
        config.hp_disable_healing = false;
        let mut life = Health::from_config(&crate::course::config(&config), true);
        for (grade, expected) in [
            (Judgement::Good, 98.5),
            (Judgement::Bad, 95.5),
            (Judgement::Miss, 92.5),
            (Judgement::Perfect, 93.5),
        ] {
            life.apply(grade);
            assert!((life.ratio() * 100. - expected).abs() < 0.001);
        }
    }

    #[test]
    fn course_life_carries_exact_points_until_the_course_restarts() {
        let mut config = Config::default();
        config.hp_mode = true;
        config.set_hp_capacity(20);
        config.hp_amount = 1.5;
        let mut stages = std::array::from_fn::<_, 3, _>(|_| Health::from_config(&config, true));

        stages[0].apply(Judgement::Good);
        let after_first = stages[0].course_life().unwrap();
        assert_eq!(after_first, CourseLife(185));
        stages[1].carry_course_life(after_first);
        stages[1].reset(); // Entering a chart resets its judge.
        assert_eq!(stages[1].course_life(), Some(after_first));
        stages[1].apply(Judgement::Bad);

        let after_second = stages[1].course_life().unwrap();
        assert_eq!(after_second, CourseLife(155));
        stages[2].carry_course_life(after_second);
        stages[2].reset();
        stages[2].apply(Judgement::Perfect);
        assert_eq!(stages[2].course_life(), Some(after_second));
        for _ in 0..6 {
            stages[2].apply(Judgement::Miss);
        }
        assert_eq!(stages[2].ratio(), 0.);
        assert_eq!(Health::from_config(&config, true).course_life(), Some(CourseLife(200)));
        assert_eq!(Health::from_config(&config, false).course_life(), None);
    }
}
