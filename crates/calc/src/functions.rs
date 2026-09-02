//! Klerq Calc function library.
//!
//! One dispatch entry point — [`call`] — maps an upper-cased function name to a
//! numeric implementation over a flattened argument list (`&[f64]`; ranges are
//! expanded by the caller). Every callable name is also listed in
//! [`FUNCTION_NAMES`] so the set is introspectable and testable.
//!
//! The engine is numeric (`f64`); booleans are `1.0`/`0.0`. Lazy functions that
//! need the raw AST (`IF`, `IFERROR`) are handled by the evaluator, not here.
//!
//! Built TDD-first — see `tests` in `lib.rs`.

use std::f64::consts::{E, PI};

type R = Result<f64, String>;

fn arg(a: &[f64], i: usize) -> R {
    a.get(i)
        .copied()
        .ok_or_else(|| format!("missing argument {}", i + 1))
}

fn opt(a: &[f64], i: usize, default: f64) -> f64 {
    a.get(i).copied().unwrap_or(default)
}

// ---- numeric helpers ----

fn gcd2(mut x: u64, mut y: u64) -> u64 {
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }
    x
}

fn factorial(n: u64) -> f64 {
    (1..=n).fold(1.0_f64, |acc, k| acc * k as f64)
}

fn factdouble(n: i64) -> f64 {
    let mut k = n;
    let mut acc = 1.0;
    while k > 1 {
        acc *= k as f64;
        k -= 2;
    }
    acc
}

/// Lanczos approximation of ln(Γ(x)).
fn lgamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // reflection
        (PI / (PI * x).sin()).ln() - lgamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = C[0];
        let t = x + G + 0.5;
        for (i, &c) in C.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        0.5 * (2.0 * PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

fn gamma(x: f64) -> f64 {
    if x > 0.0 {
        lgamma(x).exp()
    } else {
        // reflection for completeness
        PI / ((PI * x).sin() * lgamma(1.0 - x).exp())
    }
}

fn combin(n: f64, k: f64) -> f64 {
    (lgamma(n + 1.0) - lgamma(k + 1.0) - lgamma(n - k + 1.0))
        .exp()
        .round()
}

/// Error function (Abramowitz & Stegun 7.1.26).
fn erf(x: f64) -> f64 {
    let sign = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

// ---- aggregate helpers ----

fn mean(a: &[f64]) -> R {
    if a.is_empty() {
        Err("empty range".into())
    } else {
        Ok(a.iter().sum::<f64>() / a.len() as f64)
    }
}

fn variance(a: &[f64], sample: bool) -> R {
    let n = a.len();
    let denom = if sample { n as f64 - 1.0 } else { n as f64 };
    if n < 2 || denom <= 0.0 {
        return Err("need >= 2 values".into());
    }
    let m = a.iter().sum::<f64>() / n as f64;
    Ok(a.iter().map(|v| (v - m).powi(2)).sum::<f64>() / denom)
}

fn sorted(a: &[f64]) -> Vec<f64> {
    let mut v = a.to_vec();
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    v
}

fn percentile(a: &[f64], p: f64) -> R {
    if a.is_empty() {
        return Err("empty range".into());
    }
    let v = sorted(a);
    let rank = p.clamp(0.0, 1.0) * (v.len() as f64 - 1.0);
    let lo = (rank.floor() as usize).min(v.len() - 1);
    let hi = (rank.ceil() as usize).min(v.len() - 1);
    let frac = rank - lo as f64;
    Ok(v[lo] + (v[hi] - v[lo]) * frac)
}

// ---- financial helpers ----

fn pow1(rate: f64, nper: f64) -> f64 {
    (1.0 + rate).powf(nper)
}

fn fv(rate: f64, nper: f64, pmt: f64, pv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        -(pv + pmt * nper)
    } else {
        let p = pow1(rate, nper);
        -(pv * p + pmt * (1.0 + rate * typ) * (p - 1.0) / rate)
    }
}

fn pv(rate: f64, nper: f64, pmt: f64, fvv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        -(fvv + pmt * nper)
    } else {
        let p = pow1(rate, nper);
        -(fvv + pmt * (1.0 + rate * typ) * (p - 1.0) / rate) / p
    }
}

fn pmt(rate: f64, nper: f64, pv0: f64, fvv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        -(pv0 + fvv) / nper
    } else {
        let p = pow1(rate, nper);
        -(fvv + pv0 * p) * rate / ((1.0 + rate * typ) * (p - 1.0))
    }
}

fn npv(rate: f64, flows: &[f64]) -> f64 {
    flows
        .iter()
        .enumerate()
        .map(|(i, c)| c / (1.0 + rate).powi(i as i32 + 1))
        .sum()
}

fn irr(flows: &[f64]) -> R {
    // Bisection over a plausible rate range.
    let f = |r: f64| -> f64 {
        flows
            .iter()
            .enumerate()
            .map(|(i, c)| c / (1.0 + r).powi(i as i32))
            .sum()
    };
    let (mut lo, mut hi) = (-0.999_999, 10.0);
    let (mut flo, fhi) = (f(lo), f(hi));
    if flo * fhi > 0.0 {
        return Err("IRR did not converge".into());
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fm = f(mid);
        if fm.abs() < 1e-9 {
            return Ok(mid);
        }
        if flo * fm < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            flo = fm;
        }
    }
    Ok((lo + hi) / 2.0)
}

// ---- regression / forecasting helpers ----

/// Split a flattened list into two equal halves: `(first, second)`.
/// Convention for paired functions is `(known_y, known_x)`.
fn split_pairs(a: &[f64]) -> Result<(&[f64], &[f64]), String> {
    if a.is_empty() || a.len() % 2 != 0 {
        return Err("paired function needs two equal-length arrays".into());
    }
    Ok(a.split_at(a.len() / 2))
}

/// Ordinary least squares over `(xs, ys)`. Returns `(slope, intercept, r, sxx, syy, sxy, n, mx, my)`.
struct Reg {
    slope: f64,
    intercept: f64,
    r: f64,
    steyx: f64,
}

fn regress(ys: &[f64], xs: &[f64]) -> Result<Reg, String> {
    let n = xs.len();
    if n != ys.len() || n < 2 {
        return Err("need two equal arrays of >= 2 points".into());
    }
    let nf = n as f64;
    let mx = xs.iter().sum::<f64>() / nf;
    let my = ys.iter().sum::<f64>() / nf;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    let mut sxy = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    if sxx == 0.0 {
        return Err("zero variance in x".into());
    }
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let r = sxy / (sxx.sqrt() * syy.sqrt());
    // standard error of the estimate
    let steyx = if n > 2 {
        ((syy - slope * sxy) / (nf - 2.0)).max(0.0).sqrt()
    } else {
        0.0
    };
    Ok(Reg {
        slope,
        intercept,
        r,
        steyx,
    })
}

fn covariance(ys: &[f64], xs: &[f64], sample: bool) -> Result<f64, String> {
    let n = xs.len();
    if n != ys.len() || n == 0 {
        return Err("need two equal arrays".into());
    }
    let nf = n as f64;
    let mx = xs.iter().sum::<f64>() / nf;
    let my = ys.iter().sum::<f64>() / nf;
    let s: f64 = (0..n).map(|i| (xs[i] - mx) * (ys[i] - my)).sum();
    let denom = if sample { nf - 1.0 } else { nf };
    if denom <= 0.0 {
        return Err("need >= 2 for sample covariance".into());
    }
    Ok(s / denom)
}

// ---- extended financial helpers ----

/// Interest portion of payment `per` (1-based).
fn ipmt(rate: f64, per: f64, nper: f64, pv0: f64, fvv: f64, typ: f64) -> f64 {
    let pay = pmt(rate, nper, pv0, fvv, typ);
    // Balance just before period `per`.
    let mut bal = pv0;
    for _ in 0..(per as i64 - 1) {
        let interest = bal * rate;
        bal += interest + pay; // pay is negative
    }
    if typ == 1.0 && per == 1.0 {
        0.0
    } else {
        bal * rate
    }
}

fn rate_newton(nper: f64, pmt0: f64, pv0: f64, fvv: f64, typ: f64) -> Result<f64, String> {
    let f = |r: f64| fv(r, nper, pmt0, pv0, typ) - fvv;
    // Bisection over a wide bracket.
    let (mut lo, mut hi) = (-0.999_999, 10.0);
    let (mut flo, fhi) = (f(lo), f(hi));
    if flo * fhi > 0.0 {
        return Err("RATE did not converge".into());
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fm = f(mid);
        if fm.abs() < 1e-10 {
            return Ok(mid);
        }
        if flo * fm < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            flo = fm;
        }
    }
    Ok((lo + hi) / 2.0)
}

// ---- disruptive / novel helpers ----

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    if n < 4 {
        return true;
    }
    if n % 2 == 0 {
        return false;
    }
    let mut i = 3u64;
    while i.saturating_mul(i) <= n {
        if n % i == 0 {
            return false;
        }
        i += 2;
    }
    true
}

fn fib(n: u64) -> f64 {
    let (mut a, mut b) = (0.0f64, 1.0f64);
    for _ in 0..n {
        let t = a + b;
        a = b;
        b = t;
    }
    a
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

fn softplus(x: f64) -> f64 {
    x.max(0.0) + (1.0 + (-x.abs()).exp()).ln()
}

fn stdev_pop(a: &[f64]) -> Option<f64> {
    if a.is_empty() {
        return None;
    }
    let m = a.iter().sum::<f64>() / a.len() as f64;
    Some((a.iter().map(|v| (v - m).powi(2)).sum::<f64>() / a.len() as f64).sqrt())
}

/// Central moment of order `k` about the mean.
fn moment(a: &[f64], k: i32) -> Option<f64> {
    if a.is_empty() {
        return None;
    }
    let m = a.iter().sum::<f64>() / a.len() as f64;
    Some(a.iter().map(|v| (v - m).powi(k)).sum::<f64>() / a.len() as f64)
}

/// Dispatch `name` (already upper-cased) over flattened numeric args.
/// Returns `None` if the name is unknown, else `Some(Ok|Err)`.
pub fn call(name: &str, a: &[f64]) -> Option<R> {
    let r = |res: R| Some(res);
    Some(match name {
        // ----- basic math -----
        "SUM" => Ok(a.iter().sum()),
        "PRODUCT" => Ok(a.iter().product()),
        "SUMSQ" => Ok(a.iter().map(|v| v * v).sum()),
        "ABS" => return r(arg(a, 0).map(f64::abs)),
        "SIGN" => return r(arg(a, 0).map(|v| v.signum() * (v != 0.0) as i32 as f64)),
        "SQRT" => return r(arg(a, 0).map(f64::sqrt)),
        "CBRT" => return r(arg(a, 0).map(f64::cbrt)),
        "SQRTPI" => return r(arg(a, 0).map(|v| (v * PI).sqrt())),
        "POWER" => return r(arg(a, 0).and_then(|x| arg(a, 1).map(|y| x.powf(y)))),
        "EXP" => return r(arg(a, 0).map(f64::exp)),
        "EXPM1" => return r(arg(a, 0).map(f64::exp_m1)),
        "LN" => return r(arg(a, 0).map(f64::ln)),
        "LN1P" => return r(arg(a, 0).map(f64::ln_1p)),
        "LOG10" => return r(arg(a, 0).map(f64::log10)),
        "LOG2" => return r(arg(a, 0).map(f64::log2)),
        "LOG" => return r(arg(a, 0).map(|x| x.log(opt(a, 1, 10.0)))),
        "MOD" => return r(arg(a, 0).and_then(|x| arg(a, 1).map(|y| x.rem_euclid(y)))),
        "QUOTIENT" => return r(arg(a, 0).and_then(|x| arg(a, 1).map(|y| (x / y).trunc()))),
        "GCD" => Ok(a.iter().fold(0u64, |g, &v| gcd2(g, v.abs() as u64)) as f64),
        "LCM" => {
            let l = a.iter().fold(1u64, |l, &v| {
                let v = v.abs() as u64;
                if v == 0 {
                    0
                } else {
                    l / gcd2(l, v) * v
                }
            });
            Ok(l as f64)
        }
        "PI" => Ok(PI),
        "E" => Ok(E),
        "TAU" => Ok(std::f64::consts::TAU),
        "PHI" => Ok(1.618_033_988_749_895),
        "DEGREES" => return r(arg(a, 0).map(f64::to_degrees)),
        "RADIANS" => return r(arg(a, 0).map(f64::to_radians)),
        "HYPOT" => return r(arg(a, 0).and_then(|x| arg(a, 1).map(|y| x.hypot(y)))),

        // ----- rounding -----
        "INT" => return r(arg(a, 0).map(f64::floor)),
        "TRUNC" => {
            return r(arg(a, 0).map(|x| {
                let f = 10f64.powf(opt(a, 1, 0.0));
                (x * f).trunc() / f
            }))
        }
        "ROUND" => {
            return r(arg(a, 0).map(|x| {
                let f = 10f64.powf(opt(a, 1, 0.0));
                (x * f).round() / f
            }))
        }
        "ROUNDUP" => {
            return r(arg(a, 0).map(|x| {
                let f = 10f64.powf(opt(a, 1, 0.0));
                let y = (x * f).abs().ceil() / f;
                y * x.signum()
            }))
        }
        "ROUNDDOWN" => {
            return r(arg(a, 0).map(|x| {
                let f = 10f64.powf(opt(a, 1, 0.0));
                let y = (x * f).abs().floor() / f;
                y * x.signum()
            }))
        }
        "MROUND" => {
            return r(arg(a, 0)
                .and_then(|x| arg(a, 1).map(|m| if m == 0.0 { 0.0 } else { (x / m).round() * m })))
        }
        "CEILING" => {
            return r(arg(a, 0).map(|x| {
                let m = opt(a, 1, 1.0);
                if m == 0.0 {
                    0.0
                } else {
                    (x / m).ceil() * m
                }
            }))
        }
        "FLOOR" => {
            return r(arg(a, 0).map(|x| {
                let m = opt(a, 1, 1.0);
                if m == 0.0 {
                    0.0
                } else {
                    (x / m).floor() * m
                }
            }))
        }
        "EVEN" => {
            return r(arg(a, 0).map(|x| {
                let s = if x < 0.0 { -1.0 } else { 1.0 };
                (x.abs() / 2.0).ceil() * 2.0 * s
            }))
        }
        "ODD" => {
            return r(arg(a, 0).map(|x| {
                let s = if x < 0.0 { -1.0 } else { 1.0 };
                let mut y = x.abs().ceil();
                if (y as i64) % 2 == 0 {
                    y += 1.0;
                }
                y * s
            }))
        }

        // ----- trig -----
        "SIN" => return r(arg(a, 0).map(f64::sin)),
        "COS" => return r(arg(a, 0).map(f64::cos)),
        "TAN" => return r(arg(a, 0).map(f64::tan)),
        "ASIN" => return r(arg(a, 0).map(f64::asin)),
        "ACOS" => return r(arg(a, 0).map(f64::acos)),
        "ATAN" => return r(arg(a, 0).map(f64::atan)),
        "ATAN2" => return r(arg(a, 0).and_then(|x| arg(a, 1).map(|y| y.atan2(x)))),
        "SINH" => return r(arg(a, 0).map(f64::sinh)),
        "COSH" => return r(arg(a, 0).map(f64::cosh)),
        "TANH" => return r(arg(a, 0).map(f64::tanh)),
        "ASINH" => return r(arg(a, 0).map(f64::asinh)),
        "ACOSH" => return r(arg(a, 0).map(f64::acosh)),
        "ATANH" => return r(arg(a, 0).map(f64::atanh)),
        "SEC" => return r(arg(a, 0).map(|x| 1.0 / x.cos())),
        "CSC" => return r(arg(a, 0).map(|x| 1.0 / x.sin())),
        "COT" => return r(arg(a, 0).map(|x| 1.0 / x.tan())),
        "SECH" => return r(arg(a, 0).map(|x| 1.0 / x.cosh())),
        "CSCH" => return r(arg(a, 0).map(|x| 1.0 / x.sinh())),
        "COTH" => return r(arg(a, 0).map(|x| 1.0 / x.tanh())),
        "ACOT" => return r(arg(a, 0).map(|x| (PI / 2.0) - x.atan())),
        "ACOTH" => return r(arg(a, 0).map(|x| 0.5 * ((x + 1.0) / (x - 1.0)).ln())),

        // ----- combinatorics / special -----
        "FACT" => return r(arg(a, 0).map(|x| factorial(x.max(0.0) as u64))),
        "FACTDOUBLE" => return r(arg(a, 0).map(|x| factdouble(x as i64))),
        "COMBIN" => return r(arg(a, 0).and_then(|n| arg(a, 1).map(|k| combin(n, k)))),
        "COMBINA" => return r(arg(a, 0).and_then(|n| arg(a, 1).map(|k| combin(n + k - 1.0, k)))),
        "PERMUT" => {
            return r(arg(a, 0).and_then(|n| {
                arg(a, 1).map(|k| (lgamma(n + 1.0) - lgamma(n - k + 1.0)).exp().round())
            }))
        }
        "MULTINOMIAL" => {
            let s: f64 = a.iter().sum();
            let denom: f64 = a.iter().map(|&x| lgamma(x + 1.0)).sum();
            Ok((lgamma(s + 1.0) - denom).exp().round())
        }
        "GAMMA" => return r(arg(a, 0).map(gamma)),
        "GAMMALN" => return r(arg(a, 0).map(lgamma)),
        "ERF" => return r(arg(a, 0).map(erf)),
        "ERFC" => return r(arg(a, 0).map(|x| 1.0 - erf(x))),

        // ----- statistics -----
        "COUNT" => Ok(a.len() as f64),
        "AVERAGE" => return r(mean(a)),
        "AVERAGEA" => return r(mean(a)),
        "MEDIAN" => {
            if a.is_empty() {
                Err("empty range".into())
            } else {
                let v = sorted(a);
                let n = v.len();
                Ok(if n % 2 == 1 {
                    v[n / 2]
                } else {
                    (v[n / 2 - 1] + v[n / 2]) / 2.0
                })
            }
        }
        "MIN" => a
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or_else(|| "empty".into()),
        "MAX" => a
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or_else(|| "empty".into()),
        "SMALL" => {
            let k = opt(a, a.len().saturating_sub(1), 1.0) as usize;
            let body = &a[..a.len().saturating_sub(1)];
            let v = sorted(body);
            v.get(k.saturating_sub(1))
                .copied()
                .ok_or_else(|| "k out of range".into())
        }
        "LARGE" => {
            let k = opt(a, a.len().saturating_sub(1), 1.0) as usize;
            let body = &a[..a.len().saturating_sub(1)];
            let mut v = sorted(body);
            v.reverse();
            v.get(k.saturating_sub(1))
                .copied()
                .ok_or_else(|| "k out of range".into())
        }
        "VAR" | "VARS" => return r(variance(a, true)),
        "VARP" => return r(variance(a, false)),
        "STDEV" | "STDEVS" => return r(variance(a, true).map(f64::sqrt)),
        "STDEVP" => return r(variance(a, false).map(f64::sqrt)),
        "GEOMEAN" => {
            if a.is_empty() {
                Err("empty".into())
            } else {
                Ok((a.iter().map(|v| v.ln()).sum::<f64>() / a.len() as f64).exp())
            }
        }
        "HARMEAN" => {
            if a.is_empty() {
                Err("empty".into())
            } else {
                Ok(a.len() as f64 / a.iter().map(|v| 1.0 / v).sum::<f64>())
            }
        }
        "AVEDEV" => {
            let n = a.len() as f64;
            return r(mean(a).map(|m| a.iter().map(|v| (v - m).abs()).sum::<f64>() / n));
        }
        "DEVSQ" => {
            return r(mean(a).map(|m| a.iter().map(|v| (v - m).powi(2)).sum()));
        }
        "MEDIANRANGE" => return r(mean(a)),
        "MODE" => {
            // Most frequent value (first by appearance on ties).
            let v = sorted(a);
            if v.is_empty() {
                return r(Err("empty".into()));
            }
            let mut best = v[0];
            let (mut best_c, mut cur, mut cur_c) = (0usize, v[0], 0usize);
            for &x in &v {
                if x == cur {
                    cur_c += 1;
                } else {
                    cur = x;
                    cur_c = 1;
                }
                if cur_c > best_c {
                    best_c = cur_c;
                    best = cur;
                }
            }
            Ok(best)
        }
        "RANGE" => {
            let mn = a.iter().copied().reduce(f64::min);
            let mx = a.iter().copied().reduce(f64::max);
            match (mn, mx) {
                (Some(a0), Some(b0)) => Ok(b0 - a0),
                _ => Err("empty".into()),
            }
        }
        "PERCENTILE" => {
            let p = opt(a, a.len().saturating_sub(1), 0.5);
            return r(percentile(&a[..a.len().saturating_sub(1)], p));
        }
        "QUARTILE" => {
            let q = opt(a, a.len().saturating_sub(1), 2.0);
            return r(percentile(&a[..a.len().saturating_sub(1)], q / 4.0));
        }

        // ----- logical (numeric truthiness) -----
        "NOT" => return r(arg(a, 0).map(|x| (x == 0.0) as u8 as f64)),
        "AND" => Ok(a.iter().all(|v| *v != 0.0) as u8 as f64),
        "OR" => Ok(a.iter().any(|v| *v != 0.0) as u8 as f64),
        "XOR" => Ok((a.iter().filter(|v| **v != 0.0).count() % 2 == 1) as u8 as f64),
        "TRUE" => Ok(1.0),
        "FALSE" => Ok(0.0),
        "DELTA" => Ok((opt(a, 0, 0.0) == opt(a, 1, 0.0)) as u8 as f64),
        "GESTEP" => Ok((opt(a, 0, 0.0) >= opt(a, 1, 0.0)) as u8 as f64),

        // ----- bitwise (32/48-bit integer semantics) -----
        "BITAND" => Ok(((opt(a, 0, 0.0) as u64) & (opt(a, 1, 0.0) as u64)) as f64),
        "BITOR" => Ok(((opt(a, 0, 0.0) as u64) | (opt(a, 1, 0.0) as u64)) as f64),
        "BITXOR" => Ok(((opt(a, 0, 0.0) as u64) ^ (opt(a, 1, 0.0) as u64)) as f64),
        "BITLSHIFT" => Ok(((opt(a, 0, 0.0) as u64) << (opt(a, 1, 0.0) as u64)) as f64),
        "BITRSHIFT" => Ok(((opt(a, 0, 0.0) as u64) >> (opt(a, 1, 0.0) as u64)) as f64),

        // ----- financial -----
        "FV" => Ok(fv(
            opt(a, 0, 0.0),
            opt(a, 1, 0.0),
            opt(a, 2, 0.0),
            opt(a, 3, 0.0),
            opt(a, 4, 0.0),
        )),
        "PV" => Ok(pv(
            opt(a, 0, 0.0),
            opt(a, 1, 0.0),
            opt(a, 2, 0.0),
            opt(a, 3, 0.0),
            opt(a, 4, 0.0),
        )),
        "PMT" => Ok(pmt(
            opt(a, 0, 0.0),
            opt(a, 1, 0.0),
            opt(a, 2, 0.0),
            opt(a, 3, 0.0),
            opt(a, 4, 0.0),
        )),
        "NPV" => {
            if a.is_empty() {
                Err("NPV needs a rate".into())
            } else {
                Ok(npv(a[0], &a[1..]))
            }
        }
        "IRR" => return r(irr(a)),
        "SLN" => Ok((opt(a, 0, 0.0) - opt(a, 1, 0.0)) / opt(a, 2, 1.0)),
        "SYD" => {
            let (cost, salvage, life, per) = (
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 1.0),
                opt(a, 3, 1.0),
            );
            Ok((cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
        }
        "DDB" => {
            let (cost, salvage, life, per) = (
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 1.0),
                opt(a, 3, 1.0),
            );
            let factor = opt(a, 4, 2.0);
            let rate = factor / life;
            let mut book = cost;
            let mut dep = 0.0;
            for _ in 0..(per as i64) {
                dep = (book * rate).min(book - salvage).max(0.0);
                book -= dep;
            }
            Ok(dep)
        }
        "EFFECT" => {
            let (nominal, npery) = (opt(a, 0, 0.0), opt(a, 1, 1.0));
            Ok((1.0 + nominal / npery).powf(npery) - 1.0)
        }
        "NOMINAL" => {
            let (effect, npery) = (opt(a, 0, 0.0), opt(a, 1, 1.0));
            Ok(((1.0 + effect).powf(1.0 / npery) - 1.0) * npery)
        }
        "RATE" => {
            return r(rate_newton(
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 0.0),
                opt(a, 3, 0.0),
                opt(a, 4, 0.0),
            ))
        }
        "NPER" => {
            let (rate, pay, pv0, fvv, typ) = (
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 0.0),
                opt(a, 3, 0.0),
                opt(a, 4, 0.0),
            );
            if rate == 0.0 {
                Ok(-(pv0 + fvv) / pay)
            } else {
                let adj = pay * (1.0 + rate * typ);
                Ok(((adj - fvv * rate) / (adj + pv0 * rate)).ln() / (1.0 + rate).ln())
            }
        }
        "IPMT" => Ok(ipmt(
            opt(a, 0, 0.0),
            opt(a, 1, 1.0),
            opt(a, 2, 1.0),
            opt(a, 3, 0.0),
            opt(a, 4, 0.0),
            opt(a, 5, 0.0),
        )),
        "PPMT" => {
            let (rate, per, nper, pv0, fvv, typ) = (
                opt(a, 0, 0.0),
                opt(a, 1, 1.0),
                opt(a, 2, 1.0),
                opt(a, 3, 0.0),
                opt(a, 4, 0.0),
                opt(a, 5, 0.0),
            );
            Ok(pmt(rate, nper, pv0, fvv, typ) - ipmt(rate, per, nper, pv0, fvv, typ))
        }
        "CUMIPMT" => {
            let (rate, nper, pv0) = (opt(a, 0, 0.0), opt(a, 1, 1.0), opt(a, 2, 0.0));
            let (start, end) = (opt(a, 3, 1.0) as i64, opt(a, 4, 1.0) as i64);
            let typ = opt(a, 5, 0.0);
            Ok((start..=end)
                .map(|p| ipmt(rate, p as f64, nper, pv0, 0.0, typ))
                .sum())
        }
        "CUMPRINC" => {
            let (rate, nper, pv0) = (opt(a, 0, 0.0), opt(a, 1, 1.0), opt(a, 2, 0.0));
            let (start, end) = (opt(a, 3, 1.0) as i64, opt(a, 4, 1.0) as i64);
            let typ = opt(a, 5, 0.0);
            let pay = pmt(rate, nper, pv0, 0.0, typ);
            Ok((start..=end)
                .map(|p| pay - ipmt(rate, p as f64, nper, pv0, 0.0, typ))
                .sum())
        }
        "RRI" => {
            let (nper, pv0, fvv) = (opt(a, 0, 1.0), opt(a, 1, 0.0), opt(a, 2, 0.0));
            Ok((fvv / pv0).powf(1.0 / nper) - 1.0)
        }
        "PDURATION" => {
            let (rate, pv0, fvv) = (opt(a, 0, 0.0), opt(a, 1, 0.0), opt(a, 2, 0.0));
            Ok((fvv.ln() - pv0.ln()) / (1.0 + rate).ln())
        }
        "DOLLARDE" => {
            let (frac_dollar, frac) = (opt(a, 0, 0.0), opt(a, 1, 1.0));
            let whole = frac_dollar.trunc();
            let frac_part = (frac_dollar - whole) * 100.0 / frac;
            Ok(whole + frac_part)
        }
        "DOLLARFR" => {
            let (dec_dollar, frac) = (opt(a, 0, 0.0), opt(a, 1, 1.0));
            let whole = dec_dollar.trunc();
            let frac_part = (dec_dollar - whole) * frac / 100.0;
            Ok(whole + frac_part)
        }
        "DB" => {
            let (cost, salvage, life, period) = (
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 1.0),
                opt(a, 3, 1.0),
            );
            let month = opt(a, 4, 12.0);
            let rate = (1.0 - (salvage / cost).powf(1.0 / life) * 1.0).max(0.0);
            let rate = (rate * 1000.0).round() / 1000.0;
            let mut total = cost * rate * month / 12.0; // period 1
            if (period as i64) == 1 {
                return r(Ok(total));
            }
            let mut dep = total;
            for _ in 2..=(period as i64) {
                dep = (cost - total) * rate;
                total += dep;
            }
            Ok(dep)
        }
        "FVSCHEDULE" => {
            if a.is_empty() {
                Err("FVSCHEDULE needs a principal".into())
            } else {
                Ok(a[1..].iter().fold(a[0], |acc, &rate| acc * (1.0 + rate)))
            }
        }
        "MIRR" => {
            // MIRR(values..., finance_rate, reinvest_rate)
            if a.len() < 3 {
                return r(Err("MIRR needs cashflows + 2 rates".into()));
            }
            let (flows, rates) = a.split_at(a.len() - 2);
            let (fin, rein) = (rates[0], rates[1]);
            let n = flows.len() as f64;
            let pos: f64 = flows
                .iter()
                .enumerate()
                .filter(|(_, &c)| c > 0.0)
                .map(|(i, &c)| c * (1.0 + rein).powf(n - 1.0 - i as f64))
                .sum();
            let neg: f64 = flows
                .iter()
                .enumerate()
                .filter(|(_, &c)| c < 0.0)
                .map(|(i, &c)| c / (1.0 + fin).powi(i as i32))
                .sum();
            if neg == 0.0 || pos == 0.0 {
                return r(Err("MIRR needs both inflows and outflows".into()));
            }
            Ok((pos / -neg).powf(1.0 / (n - 1.0)) - 1.0)
        }

        // ----- forecasting / regression -----
        "SLOPE" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(regress(ys, xs).map(|g| g.slope));
        }
        "INTERCEPT" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(regress(ys, xs).map(|g| g.intercept));
        }
        "PEARSON" | "CORREL" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(regress(ys, xs).map(|g| g.r));
        }
        "RSQ" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(regress(ys, xs).map(|g| g.r * g.r));
        }
        "STEYX" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(regress(ys, xs).map(|g| g.steyx));
        }
        "COVAR" | "COVARIANCEP" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(covariance(ys, xs, false));
        }
        "COVARIANCES" => {
            let (ys, xs) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(covariance(ys, xs, true));
        }
        "FORECAST" | "TREND" => {
            // FORECAST(x, known_y..., known_x...)
            if a.len() < 3 {
                return r(Err("FORECAST needs x plus paired arrays".into()));
            }
            let x = a[0];
            let (ys, xs) = match split_pairs(&a[1..]) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            return r(regress(ys, xs).map(|g| g.intercept + g.slope * x));
        }
        "GROWTH" => {
            // Exponential fit: ln(y) = a + b*x, predict exp(a + b*x).
            if a.len() < 3 {
                return r(Err("GROWTH needs x plus paired arrays".into()));
            }
            let x = a[0];
            let (ys, xs) = match split_pairs(&a[1..]) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            let logy: Vec<f64> = ys.iter().map(|v| v.ln()).collect();
            return r(regress(&logy, xs).map(|g| (g.intercept + g.slope * x).exp()));
        }

        // ----- paired sums (first half x, second half y) -----
        "SUMXMY2" => {
            let (xs, ys) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            Ok(xs.iter().zip(ys).map(|(x, y)| (x - y).powi(2)).sum())
        }
        "SUMX2MY2" => {
            let (xs, ys) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            Ok(xs.iter().zip(ys).map(|(x, y)| x * x - y * y).sum())
        }
        "SUMX2PY2" => {
            let (xs, ys) = match split_pairs(a) {
                Ok(v) => v,
                Err(e) => return r(Err(e)),
            };
            Ok(xs.iter().zip(ys).map(|(x, y)| x * x + y * y).sum())
        }

        // ============================================================
        //  DISRUPTIVE — primitives Excel does not ship natively.
        // ============================================================

        // --- interpolation / shaping ---
        "CLAMP" => {
            let (x, lo, hi) = (opt(a, 0, 0.0), opt(a, 1, 0.0), opt(a, 2, 1.0));
            Ok(x.clamp(lo.min(hi), lo.max(hi)))
        }
        "SATURATE" | "CLAMP01" => Ok(opt(a, 0, 0.0).clamp(0.0, 1.0)),
        "LERP" => {
            let (u, v, t) = (opt(a, 0, 0.0), opt(a, 1, 0.0), opt(a, 2, 0.0));
            Ok(u + (v - u) * t)
        }
        "INVLERP" => {
            let (u, v, x) = (opt(a, 0, 0.0), opt(a, 1, 1.0), opt(a, 2, 0.0));
            Ok(if v == u { 0.0 } else { (x - u) / (v - u) })
        }
        "REMAP" => {
            let (x, a0, a1, b0, b1) = (
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 1.0),
                opt(a, 3, 0.0),
                opt(a, 4, 1.0),
            );
            Ok(if a1 == a0 {
                b0
            } else {
                b0 + (x - a0) * (b1 - b0) / (a1 - a0)
            })
        }
        "SMOOTHSTEP" => {
            let (e0, e1, x) = (opt(a, 0, 0.0), opt(a, 1, 1.0), opt(a, 2, 0.0));
            let t = if e1 == e0 {
                0.0
            } else {
                ((x - e0) / (e1 - e0)).clamp(0.0, 1.0)
            };
            Ok(t * t * (3.0 - 2.0 * t))
        }
        "STEP" => Ok((opt(a, 1, 0.0) >= opt(a, 0, 0.0)) as u8 as f64),
        "FRACT" => Ok({
            let x = opt(a, 0, 0.0);
            x - x.floor()
        }),
        "WRAP" => {
            let (x, lo, hi) = (opt(a, 0, 0.0), opt(a, 1, 0.0), opt(a, 2, 1.0));
            let span = hi - lo;
            Ok(if span == 0.0 {
                lo
            } else {
                lo + (x - lo).rem_euclid(span)
            })
        }
        "ROUNDSIG" => {
            let (x, sig) = (opt(a, 0, 0.0), opt(a, 1, 3.0).max(1.0));
            if x == 0.0 {
                Ok(0.0)
            } else {
                let d = sig - 1.0 - x.abs().log10().floor();
                let f = 10f64.powf(d);
                Ok((x * f).round() / f)
            }
        }

        // --- neural-net activations (built in!) ---
        "SIGMOID" | "LOGISTIC" => return r(arg(a, 0).map(sigmoid)),
        "RELU" => return r(arg(a, 0).map(|x| x.max(0.0))),
        "LEAKYRELU" => {
            let (x, al) = (opt(a, 0, 0.0), opt(a, 1, 0.01));
            Ok(if x > 0.0 { x } else { al * x })
        }
        "ELU" => {
            let (x, al) = (opt(a, 0, 0.0), opt(a, 1, 1.0));
            Ok(if x > 0.0 { x } else { al * (x.exp() - 1.0) })
        }
        "GELU" => return r(arg(a, 0).map(|x| 0.5 * x * (1.0 + erf(x / 2f64.sqrt())))),
        "SOFTPLUS" => return r(arg(a, 0).map(softplus)),
        "SWISH" | "SILU" => return r(arg(a, 0).map(|x| x * sigmoid(x))),
        "MISH" => return r(arg(a, 0).map(|x| x * softplus(x).tanh())),
        "HARDSIGMOID" => return r(arg(a, 0).map(|x| (0.2 * x + 0.5).clamp(0.0, 1.0))),
        "SOFTSIGN" => return r(arg(a, 0).map(|x| x / (1.0 + x.abs()))),

        // --- distribution / information stats over a list ---
        "RMS" => {
            if a.is_empty() {
                Err("empty".into())
            } else {
                Ok((a.iter().map(|v| v * v).sum::<f64>() / a.len() as f64).sqrt())
            }
        }
        "LOGSUMEXP" => {
            if a.is_empty() {
                Err("empty".into())
            } else {
                let m = a.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                Ok(m + a.iter().map(|v| (v - m).exp()).sum::<f64>().ln())
            }
        }
        "ENTROPY" => {
            // Shannon entropy (nats); inputs normalized to a distribution.
            let s: f64 = a.iter().filter(|v| **v > 0.0).sum();
            if s <= 0.0 {
                Err("need positive weights".into())
            } else {
                Ok(-a
                    .iter()
                    .filter(|v| **v > 0.0)
                    .map(|v| {
                        let p = v / s;
                        p * p.ln()
                    })
                    .sum::<f64>())
            }
        }
        "GINI" => {
            if a.is_empty() {
                Err("empty".into())
            } else {
                let v = sorted(a);
                let n = v.len() as f64;
                let sum: f64 = v.iter().sum();
                if sum == 0.0 {
                    Ok(0.0)
                } else {
                    let weighted: f64 = v
                        .iter()
                        .enumerate()
                        .map(|(i, x)| (i as f64 + 1.0) * x)
                        .sum();
                    Ok((2.0 * weighted) / (n * sum) - (n + 1.0) / n)
                }
            }
        }
        "CV" => {
            return r((|| {
                let m = mean(a)?;
                let s = stdev_pop(a).ok_or_else(|| "empty".to_string())?;
                Ok(s / m)
            })());
        }
        "SKEW" => {
            return r((|| {
                let s = stdev_pop(a).ok_or("empty".to_string())?;
                let m3 = moment(a, 3).ok_or("empty".to_string())?;
                if s == 0.0 {
                    Err("zero variance".into())
                } else {
                    Ok(m3 / s.powi(3))
                }
            })());
        }
        "KURT" => {
            return r((|| {
                let s = stdev_pop(a).ok_or("empty".to_string())?;
                let m4 = moment(a, 4).ok_or("empty".to_string())?;
                if s == 0.0 {
                    Err("zero variance".into())
                } else {
                    Ok(m4 / s.powi(4) - 3.0) // excess kurtosis
                }
            })());
        }
        "IQR" => {
            return r((|| Ok(percentile(a, 0.75)? - percentile(a, 0.25)?))());
        }
        "MAD" => {
            return r((|| {
                let med = percentile(a, 0.5)?;
                let dev: Vec<f64> = a.iter().map(|v| (v - med).abs()).collect();
                percentile(&dev, 0.5)
            })());
        }

        // --- geo ---
        "HAVERSINE" => {
            // Great-circle distance in km between two lat/lon points.
            let (la1, lo1, la2, lo2) = (
                opt(a, 0, 0.0),
                opt(a, 1, 0.0),
                opt(a, 2, 0.0),
                opt(a, 3, 0.0),
            );
            let r_km = 6371.0088;
            let (p1, p2) = (la1.to_radians(), la2.to_radians());
            let dp = (la2 - la1).to_radians();
            let dl = (lo2 - lo1).to_radians();
            let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
            Ok(2.0 * r_km * h.sqrt().asin())
        }

        // --- number theory ---
        "ISPRIME" => Ok(is_prime(opt(a, 0, 0.0).max(0.0) as u64) as u8 as f64),
        "NEXTPRIME" => {
            let mut n = (opt(a, 0, 0.0).max(1.0) as u64) + 1;
            while !is_prime(n) {
                n += 1;
            }
            Ok(n as f64)
        }
        "FIB" | "FIBONACCI" => Ok(fib(opt(a, 0, 0.0).max(0.0) as u64)),
        "TRIANGULAR" => {
            let n = opt(a, 0, 0.0);
            Ok(n * (n + 1.0) / 2.0)
        }
        "DIGITSUM" => {
            let mut n = opt(a, 0, 0.0).abs() as u64;
            let mut s = 0u64;
            while n > 0 {
                s += n % 10;
                n /= 10;
            }
            Ok(s as f64)
        }
        "POPCOUNT" => Ok((opt(a, 0, 0.0) as u64).count_ones() as f64),
        "DIVISORS" => {
            let n = opt(a, 0, 0.0).abs() as u64;
            if n == 0 {
                Ok(0.0)
            } else {
                let mut c = 0u64;
                let mut i = 1u64;
                while i.saturating_mul(i) <= n {
                    if n % i == 0 {
                        c += if i * i == n { 1 } else { 2 };
                    }
                    i += 1;
                }
                Ok(c as f64)
            }
        }

        // --- paired vectors: ML metrics & similarity (first half a, second b) ---
        "MSE" | "RMSE" | "MAE" | "MAPE" | "RMSLE" | "COSINE" | "EUCLID" | "MANHATTAN"
        | "CHEBYSHEV" | "DOT" | "KLDIV" | "CROSSENTROPY" | "HAMMINGDIST" | "R2" | "BETA" => {
            let (u, v) = match split_pairs(a) {
                Ok(p) => p,
                Err(e) => return r(Err(e)),
            };
            let n = u.len() as f64;
            let val = match name {
                "MSE" => u.iter().zip(v).map(|(x, y)| (x - y).powi(2)).sum::<f64>() / n,
                "RMSE" => (u.iter().zip(v).map(|(x, y)| (x - y).powi(2)).sum::<f64>() / n).sqrt(),
                "MAE" => u.iter().zip(v).map(|(x, y)| (x - y).abs()).sum::<f64>() / n,
                "MAPE" => {
                    u.iter()
                        .zip(v)
                        .map(|(x, y)| ((x - y) / x).abs())
                        .sum::<f64>()
                        / n
                        * 100.0
                }
                "RMSLE" => (u
                    .iter()
                    .zip(v)
                    .map(|(x, y)| ((x + 1.0).ln() - (y + 1.0).ln()).powi(2))
                    .sum::<f64>()
                    / n)
                    .sqrt(),
                "COSINE" => {
                    let dot: f64 = u.iter().zip(v).map(|(x, y)| x * y).sum();
                    let nu = u.iter().map(|x| x * x).sum::<f64>().sqrt();
                    let nv = v.iter().map(|y| y * y).sum::<f64>().sqrt();
                    if nu == 0.0 || nv == 0.0 {
                        return r(Err("zero-length vector".into()));
                    }
                    dot / (nu * nv)
                }
                "EUCLID" => u
                    .iter()
                    .zip(v)
                    .map(|(x, y)| (x - y).powi(2))
                    .sum::<f64>()
                    .sqrt(),
                "MANHATTAN" => u.iter().zip(v).map(|(x, y)| (x - y).abs()).sum::<f64>(),
                "CHEBYSHEV" => u
                    .iter()
                    .zip(v)
                    .map(|(x, y)| (x - y).abs())
                    .fold(0.0, f64::max),
                "DOT" => u.iter().zip(v).map(|(x, y)| x * y).sum::<f64>(),
                "KLDIV" => u
                    .iter()
                    .zip(v)
                    .filter(|(p, _)| **p > 0.0)
                    .map(|(p, q)| p * (p / q).ln())
                    .sum::<f64>(),
                "CROSSENTROPY" => -u
                    .iter()
                    .zip(v)
                    .map(|(p, q)| p * q.max(1e-15).ln())
                    .sum::<f64>(),
                "HAMMINGDIST" => u.iter().zip(v).filter(|(x, y)| x != y).count() as f64,
                "R2" => {
                    // 1 - SS_res/SS_tot, treating first half as y_true, second as y_pred.
                    let mean_y = u.iter().sum::<f64>() / n;
                    let ss_tot: f64 = u.iter().map(|y| (y - mean_y).powi(2)).sum();
                    let ss_res: f64 = u.iter().zip(v).map(|(y, p)| (y - p).powi(2)).sum();
                    if ss_tot == 0.0 {
                        return r(Err("constant target".into()));
                    }
                    1.0 - ss_res / ss_tot
                }
                "BETA" => match covariance(u, v, true) {
                    Ok(cov) => match variance(v, true) {
                        Ok(var) if var != 0.0 => cov / var,
                        _ => return r(Err("zero market variance".into())),
                    },
                    Err(e) => return r(Err(e)),
                },
                _ => unreachable!(),
            };
            Ok(val)
        }

        // --- quant finance over a return/price series ---
        "CAGR" => {
            let (begin, end, years) = (opt(a, 0, 1.0), opt(a, 1, 1.0), opt(a, 2, 1.0));
            Ok((end / begin).powf(1.0 / years) - 1.0)
        }
        "ROI" => {
            let (gain, cost) = (opt(a, 0, 0.0), opt(a, 1, 1.0));
            Ok((gain - cost) / cost)
        }
        "VOLATILITY" => return r(variance(a, true).map(f64::sqrt)),
        "SHARPE" => {
            // SHARPE(returns…, risk_free)
            if a.len() < 3 {
                return r(Err("SHARPE needs returns + risk-free".into()));
            }
            let rf = a[a.len() - 1];
            let rets = &a[..a.len() - 1];
            let m = rets.iter().sum::<f64>() / rets.len() as f64;
            return r(stdev_pop(rets)
                .map(|s| if s == 0.0 { 0.0 } else { (m - rf) / s })
                .ok_or_else(|| "empty".into()));
        }
        "SORTINO" => {
            if a.len() < 3 {
                return r(Err("SORTINO needs returns + risk-free".into()));
            }
            let rf = a[a.len() - 1];
            let rets = &a[..a.len() - 1];
            let m = rets.iter().sum::<f64>() / rets.len() as f64;
            let downside: Vec<f64> = rets.iter().map(|x| (x - rf).min(0.0)).collect();
            let dd = (downside.iter().map(|x| x * x).sum::<f64>() / downside.len() as f64).sqrt();
            Ok(if dd == 0.0 { 0.0 } else { (m - rf) / dd })
        }
        "MAXDRAWDOWN" => {
            if a.is_empty() {
                return r(Err("empty".into()));
            }
            let mut peak = a[0];
            let mut mdd = 0.0f64;
            for &x in a {
                peak = peak.max(x);
                if peak > 0.0 {
                    mdd = mdd.max((peak - x) / peak);
                }
            }
            Ok(mdd)
        }
        "EWMA" => {
            // EWMA(series…, alpha)
            if a.len() < 2 {
                return r(Err("EWMA needs a series + alpha".into()));
            }
            let alpha = a[a.len() - 1].clamp(0.0, 1.0);
            let series = &a[..a.len() - 1];
            let mut acc = series[0];
            for &x in &series[1..] {
                acc = alpha * x + (1.0 - alpha) * acc;
            }
            Ok(acc)
        }

        // ----- aliases (spreadsheet-compatible names) -----
        "COUNTA" => Ok(a.len() as f64),
        "MAXA" => a
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or_else(|| "empty".into()),
        "MINA" => a
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or_else(|| "empty".into()),
        "STDEVA" => return r(variance(a, true).map(f64::sqrt)),
        "VARA" => return r(variance(a, true)),
        "LOG1P" => return r(arg(a, 0).map(f64::ln_1p)),

        _ => return None,
    })
}

/// Every function name recognized by [`call`]. Kept in sync for introspection
/// (the GUI function palette) and the coverage test.
pub const FUNCTION_NAMES: &[&str] = &[
    // math
    "SUM",
    "PRODUCT",
    "SUMSQ",
    "ABS",
    "SIGN",
    "SQRT",
    "CBRT",
    "SQRTPI",
    "POWER",
    "EXP",
    "EXPM1",
    "LN",
    "LN1P",
    "LOG10",
    "LOG2",
    "LOG",
    "MOD",
    "QUOTIENT",
    "GCD",
    "LCM",
    "PI",
    "E",
    "TAU",
    "PHI",
    "DEGREES",
    "RADIANS",
    "HYPOT",
    // rounding
    "INT",
    "TRUNC",
    "ROUND",
    "ROUNDUP",
    "ROUNDDOWN",
    "MROUND",
    "CEILING",
    "FLOOR",
    "EVEN",
    "ODD",
    // trig
    "SIN",
    "COS",
    "TAN",
    "ASIN",
    "ACOS",
    "ATAN",
    "ATAN2",
    "SINH",
    "COSH",
    "TANH",
    "ASINH",
    "ACOSH",
    "ATANH",
    "SEC",
    "CSC",
    "COT",
    "SECH",
    "CSCH",
    "COTH",
    "ACOT",
    "ACOTH",
    // combinatorics / special
    "FACT",
    "FACTDOUBLE",
    "COMBIN",
    "COMBINA",
    "PERMUT",
    "MULTINOMIAL",
    "GAMMA",
    "GAMMALN",
    "ERF",
    "ERFC",
    // statistics
    "COUNT",
    "AVERAGE",
    "AVERAGEA",
    "MEDIAN",
    "MIN",
    "MAX",
    "SMALL",
    "LARGE",
    "VAR",
    "VARS",
    "VARP",
    "STDEV",
    "STDEVS",
    "STDEVP",
    "GEOMEAN",
    "HARMEAN",
    "AVEDEV",
    "DEVSQ",
    "MEDIANRANGE",
    "MODE",
    "RANGE",
    "PERCENTILE",
    "QUARTILE",
    // logical
    "NOT",
    "AND",
    "OR",
    "XOR",
    "TRUE",
    "FALSE",
    "DELTA",
    "GESTEP",
    // bitwise
    "BITAND",
    "BITOR",
    "BITXOR",
    "BITLSHIFT",
    "BITRSHIFT",
    // financial
    "FV",
    "PV",
    "PMT",
    "NPV",
    "IRR",
    "SLN",
    "SYD",
    "DDB",
    "EFFECT",
    "NOMINAL",
    "RATE",
    "NPER",
    "IPMT",
    "PPMT",
    "CUMIPMT",
    "CUMPRINC",
    "RRI",
    "PDURATION",
    "DOLLARDE",
    "DOLLARFR",
    "DB",
    "FVSCHEDULE",
    "MIRR",
    // forecasting / regression
    "SLOPE",
    "INTERCEPT",
    "PEARSON",
    "CORREL",
    "RSQ",
    "STEYX",
    "COVAR",
    "COVARIANCEP",
    "COVARIANCES",
    "FORECAST",
    "TREND",
    "GROWTH",
    // paired sums + aliases
    "SUMXMY2",
    "SUMX2MY2",
    "SUMX2PY2",
    "COUNTA",
    "MAXA",
    "MINA",
    "STDEVA",
    "VARA",
    "LOG1P",
    // ===== DISRUPTIVE — not in Excel =====
    // interpolation / shaping
    "CLAMP",
    "SATURATE",
    "CLAMP01",
    "LERP",
    "INVLERP",
    "REMAP",
    "SMOOTHSTEP",
    "STEP",
    "FRACT",
    "WRAP",
    "ROUNDSIG",
    // neural-net activations
    "SIGMOID",
    "LOGISTIC",
    "RELU",
    "LEAKYRELU",
    "ELU",
    "GELU",
    "SOFTPLUS",
    "SWISH",
    "SILU",
    "MISH",
    "HARDSIGMOID",
    "SOFTSIGN",
    // information / distribution stats
    "RMS",
    "LOGSUMEXP",
    "ENTROPY",
    "GINI",
    "CV",
    "SKEW",
    "KURT",
    "IQR",
    "MAD",
    // geo + number theory
    "HAVERSINE",
    "ISPRIME",
    "NEXTPRIME",
    "FIB",
    "FIBONACCI",
    "TRIANGULAR",
    "DIGITSUM",
    "POPCOUNT",
    "DIVISORS",
    // paired ML metrics & similarity
    "MSE",
    "RMSE",
    "MAE",
    "MAPE",
    "RMSLE",
    "COSINE",
    "EUCLID",
    "MANHATTAN",
    "CHEBYSHEV",
    "DOT",
    "KLDIV",
    "CROSSENTROPY",
    "HAMMINGDIST",
    "R2",
    "BETA",
    // quant finance
    "CAGR",
    "ROI",
    "VOLATILITY",
    "SHARPE",
    "SORTINO",
    "MAXDRAWDOWN",
    "EWMA",
];
