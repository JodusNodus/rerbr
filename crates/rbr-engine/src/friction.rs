//! Friction (Sandoval–Heywood, SAE 2003-01-0725) with the oil's viscosity, and pumping (`notes/engine-model.md`).

use libm::{exp, pow, sqrt};
use std::f64::consts::PI;

/// The reference oil temperature, K (90 °C, the model's warm engine).
pub const T_OIL_REF: f64 = 363.0;

/// Vogel oil viscosity, Pa·s: μ = 0.00862·exp(2040/(T − 89.6)) (a fit to Anton Paar's 10W-40).
fn viscosity(t: f64) -> f64 {
    0.00862 * exp(2040.0 / (t - 89.6))
}

/// φ = √(μ(T)/μ(363 K)) for the input's engine temperature, clamped 273–420 K, 363 K when it is not finite or 0.
pub fn phi(t: f64) -> f64 {
    let t = if t.is_finite() && t != 0.0 { t.clamp(273.0, 420.0) } else { T_OIL_REF };
    sqrt(viscosity(t) / viscosity(T_OIL_REF))
}

/// The engine's geometry for the friction terms: bore = stroke from the cylinder's volume, a DOHC
/// four-valve head with direct-acting buckets.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// Displacement, m³.
    pub v_d: f64,
    pub cylinders: f64,
    /// Bore and stroke, mm.
    pub bore: f64,
    /// Compression ratio.
    pub r_c: f64,
}

impl Geometry {
    pub fn new(v_d: f64, cylinders: u32, turbo: bool) -> Self {
        let v_cyl = v_d / f64::from(cylinders);
        let bore = pow(4.0 * v_cyl / PI, 1.0 / 3.0) * 1000.0;
        Self { v_d, cylinders: f64::from(cylinders), bore, r_c: if turbo { 9.5 } else { 11.0 } }
    }

    /// N·m per Pa of mean effective pressure: V_d/(4π).
    pub fn per_pa(&self) -> f64 {
        self.v_d / (4.0 * PI)
    }

    /// Sandoval–Heywood FMEP, Pa, at `rpm` (at least 300, where the 1 + 500/N boundary terms stop growing), the
    /// viscosity scale `phi` and the intake over ambient pressure `p_ratio` (ring gas loading). Terms (ESA.NET
    /// FrictionModel.cs after the Sandoval thesis eqs. 7.b–11): crankshaft, reciprocating, ring gas loading,
    /// valvetrain (flat follower C_ff 133, C_oh 0.5, C_om 10.7), auxiliaries; bearings D_b 0.67B, L_b 0.25B,
    /// D_r 0.59B, L_r 0.22B, n_b = n_c + 1, 2(n_c + 1) cam bearings, valve lift 0.11B.
    pub fn fmep(&self, rpm: f64, phi: f64, p_ratio: f64) -> f64 {
        let n = rpm.max(300.0);
        let (b, s, nc) = (self.bore, self.bore, self.cylinders);
        let (db, lb, nb) = (0.67 * b, 0.25 * b, nc + 1.0);
        let (dr, lr) = (0.59 * b, 0.22 * b);
        let ncb = 2.0 * (nc + 1.0);
        let nv = 4.0 * nc;
        let lv = 0.11 * b;
        let sp = 2.0 * s / 1000.0 * n / 60.0;
        let boundary = 1.0 + 500.0 / n;
        let crank = 1.22e5 * db / (b * b * s * nc)
            + 3.03e-4 * phi * n * db * db * db * lb * nb / (b * b * s * nc)
            + 1.35e-10 * db * db * n * n * nb / nc;
        let recip =
            2.94e2 * phi * sp / b + 4.06e4 * boundary / (b * b) + 3.03e-4 * phi * n * dr * dr * dr * lr / (b * b * s);
        let gas = 6.89 * p_ratio.max(0.0) * (0.088 * phi * self.r_c + 0.182 * pow(self.r_c, 1.33 - 2.0 * 2.38e-2 * sp));
        let valve = 244.0 * phi * n * ncb / (b * b * s * nc)
            + 4.12
            + 133.0 * boundary * nv / (s * nc)
            + 0.5 * phi * pow(lv, 1.5) * sqrt(n) * nv / (b * s * nc)
            + 10.7 * boundary * lv * nv / (s * nc);
        let aux = 6.23 + 5.22e-3 * n - 1.79e-7 * n * n;
        (crank + recip + gas + valve + aux) * 1000.0
    }
}

/// The pumping mean effective pressure, Pa: (p_em − p_im) plus the valves' and ports' flow loss
/// 0.2 bar·(p_im/p_a)·(n/n_limit)² (generic form, TAMU; 0.2 bar guessed).
pub fn pmep(p_em: f64, p_im: f64, p_a: f64, rpm: f64, rpm_limit: f64) -> f64 {
    let r = rpm / rpm_limit;
    p_em - p_im + 0.2e5 * (p_im / p_a) * r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report's worked example (`fric.py`): 2.0 L, B = S = 86 mm, p_i/p_a 0.2, warm oil.
    #[test]
    fn the_worked_example_matches_the_report() {
        let g = Geometry { v_d: 4.0 * PI / 4.0 * 0.086 * 0.086 * 0.086, cylinders: 4.0, bore: 86.0, r_c: 11.0 };
        // fric.py uses L_v = 10 mm; ours is 0.11·B = 9.46 mm, so allow a little.
        for (rpm, kpa) in [(1000.0, 64.0), (4000.0, 120.0), (8000.0, 215.0)] {
            let got = g.fmep(rpm, 1.0, 0.2) / 1000.0;
            assert!((got - kpa).abs() < 0.04 * kpa, "{rpm}: {got}");
        }
    }

    #[test]
    fn cold_oil_raises_friction_and_odd_temperatures_are_warm() {
        assert!((phi(363.0) - 1.0).abs() < 1e-12);
        assert_eq!(phi(f64::NAN), 1.0);
        assert_eq!(phi(0.0), 1.0);
        assert!((phi(293.15) - 3.6).abs() < 0.2);
        assert_eq!(phi(1e9), phi(420.0));
    }
}
