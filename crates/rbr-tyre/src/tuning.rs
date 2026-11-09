//! Experimental tuning of the brush (ours): a few of the model's values a driver sets per drive to feel their effect
//! (`rbr-standalone --tyre-tuning`, the launcher's "Experimental tuning"). Each default is the model as it is, and at
//! the defaults every force is the untuned brush's, bit for bit: the factors are exact at 1 and the slide grip is
//! the model's own fall when unset.

use std::fmt;

/// The tuning of a brush ([`crate::car::Brush`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Slide grip on hard surfaces: μ_y at 30° over its peak at 4 kN, 15 m/s, dry tarmac (the lab's
    /// `tarmac-mu-y30-over-peak`). `None`: the model's (about 0.886). Set, the lateral axis's Coulomb level over its
    /// static one is solved per wheel for it ([`crate::fit::calibration`]); the peak stays NGP's.
    pub slide_grip: Option<f64>,
    /// Factors on the grip NGP hands the brush by the surface: tarmac (hard surfaces but ice), gravel (soil but snow:
    /// gravel, grass, rock), snow (soil of NGP's snow class) and ice (hard surfaces of NGP's class 5: material 116).
    pub tarmac: f64,
    pub gravel: f64,
    pub snow: f64,
    pub ice: f64,
    /// Factor on the slip stiffnesses: how fast the side and braking forces build with slip (the brush's and the loose
    /// tyre's stiffnesses, and gravel's shear law), the peaks held at NGP's.
    pub stiffness: f64,
    /// Factor on the relaxation lengths (how fast the force follows a slip change): the carcass stiffnesses over it.
    pub response: f64,
}

impl Tuning {
    /// The model as it is (the default).
    pub const MODEL: Tuning =
        Tuning { slide_grip: None, tarmac: 1.0, gravel: 1.0, snow: 1.0, ice: 1.0, stiffness: 1.0, response: 1.0 };
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning::MODEL
    }
}

/// The slide grip's range.
pub const SLIDE_GRIP: (f64, f64) = (0.70, 0.95);
/// The model's own slide grip (the lab's `tarmac-mu-y30-over-peak` on the scorecard's tarmac wheel, 2026-11-09).
pub const SLIDE_GRIP_MODEL: f64 = 0.886;
/// The grip factors' range, per surface.
pub const GRIP: (f64, f64) = (0.70, 1.30);
/// The stiffness factor's range.
pub const STIFFNESS: (f64, f64) = (0.70, 1.30);
/// The response factor's range.
pub const RESPONSE: (f64, f64) = (0.5, 2.0);

/// NGP's surface class of its ice (material 116, hard); its soil materials of the class are grass.
pub const ICE_CLASS: u32 = 5;

impl Tuning {
    /// The keys [`Tuning::parse`] takes, in order, with their ranges.
    pub const KEYS: [(&'static str, (f64, f64)); 7] = [
        ("slide-grip", SLIDE_GRIP),
        ("tarmac", GRIP),
        ("gravel", GRIP),
        ("snow", GRIP),
        ("ice", GRIP),
        ("stiffness", STIFFNESS),
        ("response", RESPONSE),
    ];

    /// Whether it is the model as it is.
    pub fn is_default(&self) -> bool {
        *self == Tuning::default()
    }

    /// The value of `key` (the slide grip unset: `None`).
    pub fn get(&self, key: &str) -> Option<f64> {
        match key {
            "slide-grip" => self.slide_grip,
            "tarmac" => Some(self.tarmac),
            "gravel" => Some(self.gravel),
            "snow" => Some(self.snow),
            "ice" => Some(self.ice),
            "stiffness" => Some(self.stiffness),
            "response" => Some(self.response),
            _ => None,
        }
    }

    /// Sets `key` to `value` within its range.
    pub fn set(&mut self, key: &str, value: f64) -> Result<(), String> {
        let (_, (lo, hi)) = Tuning::KEYS
            .iter()
            .find(|(k, _)| *k == key)
            .ok_or_else(|| format!("tyre tuning {key}: one of {}", Tuning::KEYS.map(|(k, _)| k).join(", ")))?;
        if !(value >= *lo && value <= *hi) {
            return Err(format!("tyre tuning {key}={value}: from {lo} to {hi}"));
        }
        match key {
            "slide-grip" => self.slide_grip = Some(value),
            "tarmac" => self.tarmac = value,
            "gravel" => self.gravel = value,
            "snow" => self.snow = value,
            "ice" => self.ice = value,
            "stiffness" => self.stiffness = value,
            _ => self.response = value,
        }
        Ok(())
    }

    /// `key=value,...` ([`fmt::Display`]'s form; empty: the defaults).
    pub fn parse(text: &str) -> Result<Tuning, String> {
        let mut t = Tuning::default();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (key, value) = item.split_once('=').ok_or_else(|| format!("tyre tuning {item}: key=value"))?;
            let value = value.trim().parse::<f64>().map_err(|e| format!("tyre tuning {item}: {e}"))?;
            t.set(key.trim(), value)?;
        }
        Ok(t)
    }

    /// The grip factor on a surface: `soil` (NGP's soil path) and the classes of its two materials, the second
    /// weighted `blend` ([`crate::loose::class_share`]).
    pub fn grip(&self, soil: bool, classes: [u32; 2], blend: f32) -> f64 {
        let (class, own, rest) = match soil {
            true => (crate::loose::SNOW_CLASS, self.snow, self.gravel),
            false => (ICE_CLASS, self.ice, self.tarmac),
        };
        if own == rest {
            return own;
        }
        let share = crate::loose::class_share(classes, blend, class);
        own * share + rest * (1.0 - share)
    }
}

/// The keys off their default, `key=value` joined by commas (empty at the defaults), [`Tuning::parse`]'s form.
impl fmt::Display for Tuning {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let default = Tuning::default();
        let set: Vec<String> = Tuning::KEYS
            .iter()
            .filter(|(k, _)| self.get(k) != default.get(k))
            .filter_map(|(k, _)| Some(format!("{k}={}", self.get(k)?)))
            .collect();
        f.write_str(&set.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_back_what_it_writes_and_refuses_out_of_range() {
        assert!(Tuning::parse("").unwrap().is_default());
        assert_eq!(Tuning::default().to_string(), "");
        let t = Tuning::parse("slide-grip=0.82, gravel=1.1,response=0.5").unwrap();
        assert_eq!((t.slide_grip, t.gravel, t.response, t.tarmac), (Some(0.82), 1.1, 0.5, 1.0));
        assert_eq!(t.to_string(), "slide-grip=0.82,gravel=1.1,response=0.5");
        assert_eq!(Tuning::parse(&t.to_string()), Ok(t));
        assert!(Tuning::parse("tarmac=1.31").is_err());
        assert!(Tuning::parse("slide-grip=0.5").is_err());
        assert!(Tuning::parse("grip=1").is_err());
        assert!(Tuning::parse("ice").is_err());
        assert!(Tuning::parse("ice=NaN").is_err());
    }

    #[test]
    fn the_grip_factor_goes_by_the_surface_class() {
        let t = Tuning { tarmac: 1.2, gravel: 0.8, snow: 0.9, ice: 0.7, ..Tuning::default() };
        assert_eq!(t.grip(false, [2, 2], 0.0), 1.2);
        assert_eq!(t.grip(false, [ICE_CLASS, 2], 0.0), 0.7);
        assert!((t.grip(false, [ICE_CLASS, 2], 0.25) - (0.7 * 0.75 + 1.2 * 0.25)).abs() < 1e-12);
        assert_eq!(t.grip(true, [1, 1], 0.0), 0.8);
        assert_eq!(t.grip(true, [5, 4], 0.5), 0.8, "grass and rock take gravel's");
        assert_eq!(t.grip(true, [3, 1], 0.0), 0.9);
        assert_eq!(Tuning::default().grip(true, [3, 1], 0.3), 1.0);
    }
}
