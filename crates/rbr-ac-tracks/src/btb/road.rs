//! Centre lines: a polyline with its arc length, sampled at a point's arc length, and re-run between two arc lengths.

/// A polyline with its cumulative arc length.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub points: Vec<[f64; 3]>,
    /// Arc length at each point.
    pub at: Vec<f64>,
    /// Road width at each point.
    pub width: Vec<f64>,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Line {
    pub fn new(points: Vec<[f64; 3]>, width: Vec<f64>) -> Line {
        let mut at = Vec::with_capacity(points.len());
        let mut s = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                s += dist(points[i - 1], *p);
            }
            at.push(s);
        }
        Line { points, at, width }
    }

    pub fn length(&self) -> f64 {
        self.at.last().copied().unwrap_or(0.0)
    }

    /// The point and unit tangent at arc length `s` (clamped).
    pub fn sample(&self, s: f64) -> ([f64; 3], [f64; 3]) {
        let i = self.at.partition_point(|&a| a <= s).clamp(1, self.points.len().max(2) - 1);
        let (a, b) = (self.points[i - 1], self.points[i]);
        let span = (self.at[i] - self.at[i - 1]).max(1e-9);
        let t = ((s - self.at[i - 1]) / span).clamp(0.0, 1.0);
        let d = dist(a, b).max(1e-9);
        (
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t],
            [(b[0] - a[0]) / d, (b[1] - a[1]) / d, (b[2] - a[2]) / d],
        )
    }

    /// The width at arc length `s`.
    pub fn width_at(&self, s: f64) -> f64 {
        let i = self.at.partition_point(|&a| a <= s).min(self.width.len().saturating_sub(1));
        self.width.get(i).copied().unwrap_or(0.0)
    }

    /// The line from arc length `from` running `length` metres on, wrapping past the end of a closed line,
    /// reversed when `forward` is false, resampled every `step` metres.
    pub fn stretch(&self, from: f64, length: f64, forward: bool, step: f64) -> Line {
        let total = self.length();
        let count = (length / step).ceil() as usize;
        let mut points = Vec::with_capacity(count + 1);
        let mut width = Vec::with_capacity(count + 1);
        for k in 0..=count {
            let d = (k as f64 * step).min(length);
            // Wrap only past the ends: a run ending exactly at the end of an open line stays there.
            let s = if forward { from + d } else { from - d };
            let s = if (0.0..=total).contains(&s) { s } else { s.rem_euclid(total) };
            points.push(self.sample(s).0);
            width.push(self.width_at(s));
        }
        Line::new(points, width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(length: usize) -> Line {
        Line::new((0..=length).map(|i| [i as f64, 0.0, 0.0]).collect(), vec![8.0; length + 1])
    }

    #[test]
    fn a_line_measures_its_length_and_stretches_back_and_to_its_end() {
        let line = straight(20);
        assert!((line.length() - 20.0).abs() < 1e-6);
        let back = line.stretch(15.0, 10.0, false, 2.5);
        assert_eq!(back.points.len(), 5);
        assert!((back.points[4][0] - 5.0).abs() < 1e-6);
        // A run to the very end stays at the end.
        let to_end = line.stretch(5.0, 15.0, true, 2.5);
        assert!((to_end.length() - 15.0).abs() < 1e-6);
    }

    #[test]
    fn a_closed_line_wraps() {
        let square = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]];
        let line = Line::new(square.to_vec(), vec![1.0; 5]);
        let around = line.stretch(line.length() - 1.0, 2.0, true, 1.0);
        assert!(around.points[2][0] < 2.0 && around.points[2][1] < 1.0);
    }
}
