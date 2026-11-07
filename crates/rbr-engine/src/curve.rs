//! A function of one variable given at knots: linear between them, held outside.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve(pub Vec<(f64, f64)>);

impl Curve {
    /// The value at `x`; 0 without knots.
    pub fn at(&self, x: f64) -> f64 {
        let k = &self.0;
        let Some(&(x0, y0)) = k.first() else { return 0.0 };
        if x.is_nan() || x <= x0 {
            return y0;
        }
        for w in k.windows(2) {
            let ((xa, ya), (xb, yb)) = (w[0], w[1]);
            if x <= xb {
                return if xb > xa { ya + (yb - ya) * (x - xa) / (xb - xa) } else { yb };
            }
        }
        k[k.len() - 1].1
    }

    pub fn max(&self) -> f64 {
        self.0.iter().map(|k| k.1).fold(f64::MIN, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_interpolates_and_holds() {
        let c = Curve(vec![(0.0, 1.0), (10.0, 3.0)]);
        assert_eq!((c.at(-1.0), c.at(5.0), c.at(20.0), c.at(f64::NAN)), (1.0, 2.0, 3.0, 1.0));
        assert_eq!(Curve::default().at(1.0), 0.0);
    }
}
