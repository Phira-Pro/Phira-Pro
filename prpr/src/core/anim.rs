use super::{StaticTween, TweenId, TweenRef, Tweenable, Vector};

#[derive(Clone)]
pub struct Keyframe<T> {
    pub time: f64,
    pub value: T,
    pub tween: TweenRef,
}

impl<T> Keyframe<T> {
    pub fn new(time: f64, value: T, tween: TweenId) -> Self {
        Self {
            time,
            value,
            tween: StaticTween::get_rc(tween),
        }
    }
}

#[derive(Clone)]
/// Anim Tween Function is using the `tween` value of the first keyframe of an interval `(kf1, kf2)`
pub struct Anim<T: Tweenable> {
    pub time: f64,
    pub keyframes: Box<[Keyframe<T>]>,
    pub cursor: usize,
    /// Next Anim to chain
    ///
    /// e.g. `a1.next = a2` we have a1(t) = a1.keyframes(t) + a2(t)
    /// and if `a2.next = a3` we have a1(t) = a1.keyframes(t) + a2.keyframes(t) + a3(t)
    /// ...
    pub next: Option<Box<Anim<T>>>,
}

impl<T: Tweenable> Default for Anim<T> {
    fn default() -> Self {
        Self {
            time: 0.0,
            keyframes: [].into(),
            cursor: 0,
            next: None,
        }
    }
}

impl<T: Tweenable> Anim<T> {
    pub fn new(keyframes: Vec<Keyframe<T>>) -> Self {
        assert!(!keyframes.is_empty());
        // assert_eq!(keyframes[0].time, 0.0);
        // assert_eq!(keyframes.last().unwrap().tween, 0);
        Self {
            keyframes: keyframes.into_boxed_slice(),
            time: 0.0,
            cursor: 0,
            next: None,
        }
    }

    pub fn fixed(value: T) -> Self {
        Self {
            keyframes: Box::new([Keyframe::new(0.0, value, 0)]),
            time: 0.0,
            cursor: 0,
            next: None,
        }
    }

    pub fn is_default(&self) -> bool {
        self.keyframes.is_empty() && self.next.is_none()
    }

    pub fn chain(elements: Vec<Anim<T>>) -> Self {
        if elements.is_empty() {
            return Self::default();
        }
        let mut elements: Vec<_> = elements.into_iter().map(Box::new).collect();
        elements.last_mut().unwrap().next = None;
        while elements.len() > 1 {
            let last = elements.pop().unwrap();
            elements.last_mut().unwrap().next = Some(last);
        }
        *elements.into_iter().next().unwrap()
    }

    pub fn dead(&self) -> bool {
        self.cursor + 1 >= self.keyframes.len()
    }

    pub fn set_time(&mut self, time: f64) {
        if time == self.time {
            return;
        }
        if !self.keyframes.is_empty() {
            if time < self.keyframes[self.cursor].time || self.keyframes.get(self.cursor + 4).is_some_and(|kf| kf.time <= time) {
                self.cursor = self.keyframes.partition_point(|kf| kf.time <= time).saturating_sub(1);
            } else {
                while self.keyframes.get(self.cursor + 1).is_some_and(|kf| kf.time <= time) {
                    self.cursor += 1;
                }
            }
        }
        self.time = time;
        if let Some(next) = &mut self.next {
            next.set_time(time);
        }
    }

    fn now_opt_inner(&self) -> Option<T> {
        if self.keyframes.is_empty() {
            return None;
        }
        Some(if self.cursor == self.keyframes.len() - 1 {
            self.keyframes[self.cursor].value.clone()
        } else {
            let kf1 = &self.keyframes[self.cursor];
            let kf2 = &self.keyframes[self.cursor + 1];
            let t = (self.time - kf1.time) / (kf2.time - kf1.time);
            T::tween(&kf1.value, &kf2.value, kf1.tween.y(t as f32))
        })
    }

    pub fn now_opt(&self) -> Option<T> {
        let next = self.next.as_ref().and_then(|it| it.now_opt());
        match (self.now_opt_inner(), next) {
            (Some(a), Some(b)) => Some(T::add(&a, &b)),
            (a, b) => a.or(b),
        }
    }

    pub fn map_value(&mut self, mut f: impl FnMut(T) -> T) {
        self.keyframes.iter_mut().for_each(|it| it.value = f(it.value.clone()));
        if let Some(next) = &mut self.next {
            next.map_value(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_storyboard_tracks_seek_in_both_directions() {
        let mut track = Anim::new((0..10000).map(|i| Keyframe::new(i as f64, i as f32, 2)).collect());
        for t in [9998.5, 0.5, 8990.25, 8990.75, 1., 0., 10001.] {
            track.set_time(t);
            assert!((track.now() - t.min(9999.) as f32).abs() < 1e-4);
        }
    }

    #[test]
    fn an_empty_track_does_not_freeze_its_chained_animation() {
        let animated = Anim::new(vec![Keyframe::new(0., 2f32, 2), Keyframe::new(2., 6., 2)]);
        let mut track = Anim::chain(vec![Anim::default(), animated, Anim::fixed(1.)]);
        track.set_time(1.);
        assert_eq!(track.now(), 5.);
        track.set_time(0.);
        assert_eq!(track.now(), 3.);
    }
}

impl<T: Tweenable + Default> Anim<T> {
    pub fn now(&self) -> T {
        self.now_opt().unwrap_or_default()
    }
}

pub type AnimFloat = Anim<f32>;
#[derive(Default)]
pub struct AnimVector(pub AnimFloat, pub AnimFloat);

impl AnimVector {
    pub fn fixed(v: Vector) -> Self {
        Self(AnimFloat::fixed(v.x), AnimFloat::fixed(v.y))
    }

    pub fn set_time(&mut self, time: f64) {
        self.0.set_time(time);
        self.1.set_time(time);
    }

    pub fn now(&self) -> Vector {
        Vector::new(self.0.now(), self.1.now())
    }

    pub fn now_with_def(&self, x: f32, y: f32) -> Vector {
        Vector::new(self.0.now_opt().unwrap_or(x), self.1.now_opt().unwrap_or(y))
    }
}
