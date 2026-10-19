//! Bounded mathematical expressions, with no object access, I/O or script execution.
use super::*;
#[derive(Clone)]
pub enum Expr {
    Num(f64),
    Var(String),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}
impl Expr {
    pub fn parse(s: &str) -> Result<Self> {
        let s = s
            .trim()
            .replace("$t$", "t")
            .replace('θ', "theta")
            .replace('π', "pi")
            .replace("Pi", "pi")
            .replace('，', ",")
            .replace('（', "(")
            .replace('）', ")");
        ensure!(!s.is_empty() && s.len() <= 8192, "invalid expression length");
        let re = regex::Regex::new(r"^(\s+|(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?|[A-Za-z_][A-Za-z_0-9]*|\*\*|&&|\|\||==|!=|<=|>=|[+\-*/%^<>()!,?:])")
            .unwrap();
        let (mut tokens, mut rest) = (Vec::new(), s.as_str());
        while !rest.is_empty() {
            let token = re.find(rest).context("unsupported expression token")?.as_str();
            if !token.trim().is_empty() {
                tokens.push(token.to_owned())
            }
            rest = &rest[token.len()..];
            ensure!(tokens.len() <= 512, "expression too complex");
        }
        struct Parser {
            t: Vec<String>,
            i: usize,
        }
        impl Parser {
            fn peek(&self) -> &str {
                self.t.get(self.i).map(String::as_str).unwrap_or("")
            }
            fn take(&mut self, s: &str) -> bool {
                if self.peek() == s {
                    self.i += 1;
                    true
                } else {
                    false
                }
            }
            fn need(&mut self, s: &str) -> Result<()> {
                ensure!(self.take(s), "expected {s}");
                Ok(())
            }
            fn read(&mut self, min: u8) -> Result<Expr> {
                let token = self.peek().to_owned();
                self.i += 1;
                let mut l = match token.as_str() {
                    "(" => {
                        let e = self.read(0)?;
                        self.need(")")?;
                        e
                    }
                    "+" | "-" | "!" => Expr::Unary(token, Box::new(self.read(8)?)),
                    _ => {
                        if let Ok(n) = token.parse::<f64>() {
                            ensure!(n.is_finite(), "invalid number");
                            Expr::Num(n)
                        } else {
                            ensure!(!token.is_empty(), "missing operand");
                            if self.take("(") {
                                let mut args = Vec::new();
                                if !self.take(")") {
                                    loop {
                                        args.push(self.read(0)?);
                                        if self.take(")") {
                                            break;
                                        }
                                        self.need(",")?;
                                    }
                                }
                                Expr::Call(token, args)
                            } else {
                                Expr::Var(token)
                            }
                        }
                    }
                };
                loop {
                    let op = self.peek();
                    let prec = match op {
                        "||" => 1,
                        "&&" => 2,
                        "==" | "!=" => 3,
                        "<" | ">" | "<=" | ">=" => 4,
                        "+" | "-" => 5,
                        "*" | "/" | "%" => 6,
                        "^" | "**" => 7,
                        _ => 0,
                    };
                    if prec == 0 || prec < min {
                        break;
                    }
                    let op = op.to_owned();
                    self.i += 1;
                    let r = self.read(if prec == 7 { prec } else { prec + 1 })?;
                    l = Expr::Binary(op, Box::new(l), Box::new(r));
                }
                if min == 0 && self.take("?") {
                    let a = self.read(0)?;
                    self.need(":")?;
                    let b = self.read(0)?;
                    l = Expr::Cond(Box::new(l), Box::new(a), Box::new(b));
                }
                Ok(l)
            }
        }
        let mut p = Parser { t: tokens, i: 0 };
        let e = p.read(0)?;
        ensure!(p.i == p.t.len(), "extra expression tokens");
        Ok(e)
    }
    pub fn eval(&self, vars: &HashMap<String, f64>) -> Result<f64> {
        let boolean = |b| if b { 1. } else { 0. };
        let v = match self {
            Self::Num(n) => *n,
            Self::Var(k) => match k.as_str() {
                "pi" => std::f64::consts::PI,
                "e" => std::f64::consts::E,
                _ => *vars.get(k).with_context(|| format!("unknown variable {k}"))?,
            },
            Self::Unary(op, a) => {
                let a = a.eval(vars)?;
                match op.as_str() {
                    "-" => -a,
                    "!" => boolean(a == 0.),
                    _ => a,
                }
            }
            Self::Cond(c, a, b) => {
                if c.eval(vars)? != 0. {
                    a.eval(vars)?
                } else {
                    b.eval(vars)?
                }
            }
            Self::Binary(op, a, b) => {
                let a = a.eval(vars)?;
                if op == "&&" && a == 0. {
                    return Ok(0.);
                }
                if op == "||" && a != 0. {
                    return Ok(1.);
                }
                let b = b.eval(vars)?;
                match op.as_str() {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => a / b,
                    "%" => a % b,
                    "^" | "**" => a.powf(b),
                    "<" => boolean(a < b),
                    ">" => boolean(a > b),
                    "<=" => boolean(a <= b),
                    ">=" => boolean(a >= b),
                    "==" => boolean(a == b),
                    "!=" => boolean(a != b),
                    "&&" | "||" => boolean(b != 0.),
                    _ => bail!("invalid operator"),
                }
            }
            Self::Call(name, args) => {
                let a = args.iter().map(|e| e.eval(vars)).collect::<Result<Vec<_>>>()?;
                let x = *a.first().context("missing function argument")?;
                let y = a.get(1).copied().unwrap_or(0.);
                match name.as_str() {
                    "abs" => x.abs(),
                    "floor" => x.floor(),
                    "ceil" => x.ceil(),
                    "round" => (x + 0.5).floor(),
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "sqrt" => x.sqrt(),
                    "pow" => x.powf(y),
                    "min" => a.iter().copied().fold(f64::INFINITY, f64::min),
                    "max" => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    "clamp" => x.max(y).min(*a.get(2).context("clamp needs 3 arguments")?),
                    "lerp" => x + (y - x) * a.get(2).context("lerp needs 3 arguments")?,
                    "ease" => crate::core::rpe_next_block::curve(a.get(1).copied().unwrap_or(1.) as u8, x),
                    _ => bail!("unknown mathematical function {name}"),
                }
            }
        };
        ensure!(v.is_finite(), "non-finite expression result");
        Ok(v)
    }
}
