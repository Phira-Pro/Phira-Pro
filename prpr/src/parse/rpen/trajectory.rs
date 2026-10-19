use super::expression::Expr;
use super::*;
pub struct Trajectory {
    options: Value,
    vars: HashMap<String, f64>,
    expressions: HashMap<&'static str, Expr>,
    first: [f64; 3],
    angles: Vec<f64>,
}
impl Trajectory {
    pub fn new(o: &Value) -> Result<Self> {
        let mut vars = HashMap::new();
        if let Some(p) = o["parameters"].as_object() {
            ensure!(p.len() <= 32, "too many trajectory parameters");
            for (k, v) in p {
                vars.insert(k.clone(), v.as_f64().context("non-numeric trajectory parameter")?);
            }
        }
        vars.insert("seed".into(), number(&o["seed"], 1.));
        vars.insert("randomness".into(), number(&o["randomness"], 0.));
        let polar = o["mode"] == "polar";
        let mut expressions = HashMap::new();
        for (k, d) in [
            ("begin", if polar { "0" } else { "0" }),
            ("finish", if polar { "2*pi" } else { "1" }),
            ("xExpression", "200*cos(2*pi*t)"),
            ("yExpression", "200*sin(2*pi*t)"),
            ("radiusExpression", "200"),
        ] {
            let key = match k {
                "begin" => {
                    if polar {
                        "angleStart"
                    } else {
                        "parameterStart"
                    }
                }
                "finish" => {
                    if polar {
                        "angleEnd"
                    } else {
                        "parameterEnd"
                    }
                }
                _ => k,
            };
            expressions.insert(k, Expr::parse(o[key].as_str().unwrap_or(d))?);
        }
        if let Some(s) = o["rotationExpression"].as_str().filter(|s| !s.trim().is_empty()) {
            expressions.insert("rotationExpression", Expr::parse(s)?);
        }
        let mut t = Self {
            options: o.clone(),
            vars,
            expressions,
            first: [0.; 3],
            angles: Vec::new(),
        };
        ensure!(
            number(&o["trimStart"], 0.) >= 0. && number(&o["trimEnd"], 1.) <= 1. && number(&o["trimEnd"], 1.) > number(&o["trimStart"], 0.),
            "invalid trajectory trim"
        );
        t.first = t.raw(0.)?;
        if flag(&o["tangentRotation"]) {
            for i in 0..=1024 {
                let last = t.angles.last().copied().unwrap_or(0.);
                let angle = t.tangent(i as f64 / 1024.).unwrap_or(last);
                t.angles.push(angle + 360. * ((last - angle) / 360.).round());
            }
        }
        for i in 0..=64 {
            t.raw(i as f64 / 64.)?;
        }
        Ok(t)
    }
    fn raw(&self, p: f64) -> Result<[f64; 3]> {
        let o = &self.options;
        let u = number(&o["trimStart"], 0.) + (number(&o["trimEnd"], 1.) - number(&o["trimStart"], 0.)) * p;
        let begin = self.expressions["begin"].eval(&self.vars)?;
        let end = self.expressions["finish"].eval(&self.vars)?;
        let coordinate = |axis| begin + (end - begin) * crate::core::rpe_next_block::curve(number(&o[axis], 1.) as u8, u);
        let mut v = self.vars.clone();
        v.insert("u".into(), p);
        let (x, y) = if let Some(shape) = o["shape"].as_str() {
            ensure!(shape == "star" || shape == "random-polygon", "unknown trajectory shape");
            let param = |k, d| number(&o["parameters"][k], d);
            let n = param("points", 5.).round().clamp(3., 40.) * if shape == "star" { 2. } else { 1. };
            let a = u.rem_euclid(1.) * n;
            let i = a.floor();
            let vertex = |i: f64| {
                let i = i % n;
                let random = (i * 127.1 + number(&o["seed"], 1.) * 311.7).sin() * 43758.5453;
                let r = param("radius", 200.)
                    * if shape == "star" {
                        if i % 2. != 0. {
                            param("inner", 0.4)
                        } else {
                            1.
                        }
                    } else {
                        1. - number(&o["randomness"], 0.) * random.rem_euclid(1.)
                    };
                let angle = 2. * std::f64::consts::PI * i / n;
                [r * angle.cos(), r * angle.sin()]
            };
            let l = vertex(i);
            let r = vertex(i + 1.);
            (l[0] + (r[0] - l[0]) * (a - i), l[1] + (r[1] - l[1]) * (a - i))
        } else if o["mode"] == "polar" {
            let theta = coordinate("easingX");
            v.insert("theta".into(), theta);
            v.insert("t".into(), u);
            let r = self.expressions["radiusExpression"].eval(&v)?;
            (r * theta.cos(), r * theta.sin())
        } else {
            v.insert("t".into(), coordinate("easingX"));
            let x = self.expressions["xExpression"].eval(&v)?;
            v.insert("t".into(), coordinate("easingY"));
            (x, self.expressions["yExpression"].eval(&v)?)
        };
        v.insert("t".into(), if o["mode"] == "polar" { u } else { coordinate("easingX") });
        v.insert("theta".into(), coordinate("easingX"));
        let rot = self.expressions.get("rotationExpression").map(|e| e.eval(&v)).transpose()?.unwrap_or(0.);
        let a = number(&o["rotation"], 0.).to_radians();
        let x = x * number(&o["scaleX"], 1.);
        let y = y * number(&o["scaleY"], 1.);
        Ok([x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos(), rot])
    }
    fn tangent(&self, p: f64) -> Option<f64> {
        for d in [0.00001, 0.0001, 0.001, 0.01] {
            let a = self.raw((p - d).max(0.)).ok()?;
            let b = self.raw((p + d).min(1.)).ok()?;
            let (x, y) = (b[0] - a[0], b[1] - a[1]);
            if x.hypot(y) > 1e-9 {
                return Some(-y.atan2(x).to_degrees());
            }
        }
        None
    }
    pub fn value(&self, p: f64) -> [f64; 3] {
        let p = p.clamp(0., 1.);
        let mut v = self.raw(p).unwrap_or(self.first);
        if !self.angles.is_empty() {
            let a = self.angles[(p * 1024.).round() as usize];
            let t = self.tangent(p).unwrap_or(a);
            v[2] = t + 360. * ((a - t) / 360.).round();
        }
        if flag(&self.options["alignStart"]) {
            v[0] += number(&self.options["startX"], 0.) - self.first[0];
            v[1] += number(&self.options["startY"], 0.) - self.first[1];
        }
        v
    }
}
