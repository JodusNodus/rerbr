//! A car model's moving gauge needles (ours): `<model>_gauges.ini` beside its `.sgc`, one section per needle mesh,
//! named by its node. The renderer turns the mesh about `pivot` and `axis` (the model's frame) by `sweep` degrees
//! (right handed about the axis) as the reading goes from `from` to `to`, held at the ends:
//!
//! ```ini
//! [i_needle_tach]
//! reading = rpm
//! from = 0
//! to = 12000
//! pivot = 0.35, 0.83, -1.21
//! axis = 0, 0.42, -0.91
//! sweep = -338.9
//! ```
//!
//! `reading` is `rpm`, `kmh` or `bar` (the turbo's boost). The DiRT Rally converter writes it from the game's
//! interior files (`rbr-dirt-rally`'s `needles`).
use crate::ini;

/// What a needle shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Rpm,
    Kmh,
    Bar,
}

impl Reading {
    const NAMES: [(Reading, &'static str); 3] = [(Reading::Rpm, "rpm"), (Reading::Kmh, "kmh"), (Reading::Bar, "bar")];

    fn name(self) -> &'static str {
        Reading::NAMES.iter().find(|(r, _)| *r == self).map_or("", |(_, n)| n)
    }
}

/// One needle mesh and how it turns.
#[derive(Debug, Clone, PartialEq)]
pub struct Needle {
    /// The mesh's node name.
    pub node: String,
    pub reading: Reading,
    pub from: f32,
    pub to: f32,
    pub pivot: [f32; 3],
    /// A unit vector.
    pub axis: [f32; 3],
    /// Degrees about `axis` from `from` to `to`.
    pub sweep: f32,
}

impl Needle {
    /// The turn at `value`, radians about [`Needle::axis`]: none at `from` or below, [`Needle::sweep`] at `to` or above.
    pub fn angle(&self, value: f32) -> f32 {
        let share = if self.to == self.from { 0.0 } else { (value - self.from) / (self.to - self.from) };
        share.clamp(0.0, 1.0) * self.sweep.to_radians()
    }
}

/// The gauges file of model `<stem>.sgc`.
pub fn file_name(stem: &str) -> String {
    format!("{stem}_gauges.ini")
}

/// The needles of a gauges file, in its order.
pub fn parse(text: &str) -> Result<Vec<Needle>, String> {
    let sections = ini::parse(text).map_err(|e| format!("line {}: {}", e.line, e.message))?;
    sections
        .iter()
        .map(|s| {
            let get = |key: &str| s.get(key).ok_or_else(|| format!("[{}]: no {key}", s.name));
            let number = |key: &str| get(key)?.parse::<f32>().map_err(|e| format!("[{}] {key}: {e}", s.name));
            let vector = |key: &str| -> Result<[f32; 3], String> {
                let parts: Vec<f32> = get(key)?
                    .split(',')
                    .map(|v| v.trim().parse::<f32>())
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("[{}] {key}: {e}", s.name))?;
                parts.try_into().map_err(|_| format!("[{}] {key}: want x, y, z", s.name))
            };
            let reading = get("reading")?;
            let reading = Reading::NAMES
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(reading))
                .map(|(r, _)| *r)
                .ok_or_else(|| format!("[{}] reading: {reading} is not rpm, kmh or bar", s.name))?;
            Ok(Needle {
                node: s.name.clone(),
                reading,
                from: number("from")?,
                to: number("to")?,
                pivot: vector("pivot")?,
                axis: vector("axis")?,
                sweep: number("sweep")?,
            })
        })
        .collect()
}

/// The gauges file of `needles`.
pub fn write(needles: &[Needle]) -> String {
    let vector = |v: [f32; 3]| format!("{}, {}, {}", v[0], v[1], v[2]);
    needles
        .iter()
        .map(|n| {
            format!(
                "[{}]\nreading = {}\nfrom = {}\nto = {}\npivot = {}\naxis = {}\nsweep = {}\n",
                n.node,
                n.reading.name(),
                n.from,
                n.to,
                vector(n.pivot),
                vector(n.axis),
                n.sweep
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tach() -> Needle {
        Needle {
            node: "i_needle_tach".into(),
            reading: Reading::Rpm,
            from: 0.0,
            to: 12000.0,
            pivot: [0.35, 0.83, -1.21],
            axis: [0.0, 0.6, -0.8],
            sweep: -338.9,
        }
    }

    #[test]
    fn a_written_file_reads_back() {
        let needles = vec![tach(), Needle { node: "i_needle_boost".into(), reading: Reading::Bar, ..tach() }];
        assert_eq!(parse(&write(&needles)).unwrap(), needles);
    }

    #[test]
    fn the_needle_turns_with_its_reading_and_holds_at_the_ends() {
        let n = tach();
        assert_eq!(n.angle(-100.0), 0.0);
        assert!((n.angle(6000.0) - (-338.9f32 / 2.0).to_radians()).abs() < 1e-6);
        assert_eq!(n.angle(20000.0), n.sweep.to_radians());
    }

    #[test]
    fn a_bad_section_says_what_it_lacks() {
        assert_eq!(parse("[i_needle_tach]\nreading = rpm\n").unwrap_err(), "[i_needle_tach]: no from");
        assert!(parse("[n]\nreading = volts\n").unwrap_err().contains("not rpm, kmh or bar"));
    }
}
