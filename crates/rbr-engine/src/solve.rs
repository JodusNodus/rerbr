//! A bracketing root finder: Illinois regula falsi (deterministic, bounded iterations).

/// A root of `f` in [`a`, `b`] where f(a) and f(b) differ in sign (else the end nearer 0), to |f| ≤ `tol` or
/// |b − a| ≤ `tol_x`, in at most 60 evaluations.
pub fn root(mut a: f64, mut b: f64, mut f: impl FnMut(f64) -> f64, tol: f64, tol_x: f64) -> f64 {
    let mut fa = f(a);
    let mut fb = f(b);
    if fa == 0.0 {
        return a;
    }
    if fb == 0.0 || fa * fb > 0.0 {
        return if fa.abs() < fb.abs() { a } else { b };
    }
    let mut side = 0i32;
    let mut x = b;
    for _ in 0..60 {
        x = (a * fb - b * fa) / (fb - fa);
        if !x.is_finite() {
            x = 0.5 * (a + b);
        }
        let fx = f(x);
        if fx.abs() <= tol || (b - a).abs() <= tol_x {
            return x;
        }
        if fx * fb < 0.0 {
            a = b;
            fa = fb;
            side = 0;
        } else {
            side += 1;
            if side > 1 {
                fa *= 0.5;
            }
        }
        b = x;
        fb = fx;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_finds_roots_and_ends() {
        let mut n = 0;
        let x = root(
            0.0,
            2.0,
            |x| {
                n += 1;
                x * x - 2.0
            },
            1e-12,
            0.0,
        );
        assert!((x - 2f64.sqrt()).abs() < 1e-9 && n < 20, "{x} {n}");
        assert_eq!(root(0.0, 1.0, |x| x + 1.0, 1e-9, 0.0), 0.0);
    }
}
