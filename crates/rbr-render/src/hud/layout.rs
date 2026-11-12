//! Where the player put the HUD's parts and how large they draw (`viewer --hud-layout`, `--hud-scale`; the pause
//! menu's HUD page, `super::hud_page`). Ours. A part keeps its place in the look, moved by its offset and scaled about the corner or
//! edge of the screen it belongs to ([`Part::pivot`]), so a layout holds at any aspect; the look keeps a part that
//! would leave the screen inside it. The race's calls and pages (the countdown, FALSE START, WRONG WAY, the finish,
//! result and retire pages) and the pace stack neither move nor scale.

use egui::emath::TSTransform;
use egui::{Align2, Rect, Vec2};

use super::Part;

/// The scale's range, percent.
pub const SCALES: std::ops::RangeInclusive<u16> = 50..=200;

/// The HUD's scale, percent, and each part's offset from its place, points of the [`super::REFERENCE_HEIGHT`] screen
/// (x right, y down), by its index in [`Part::ALL`]. Only its own [`Part::carrier`]'s is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub scale: u16,
    pub offsets: [[i16; 2]; Part::ALL.len()],
}

impl Default for Layout {
    fn default() -> Self {
        Layout { scale: 100, offsets: [[0; 2]; Part::ALL.len()] }
    }
}

impl Part {
    /// The part this one moves with: the clock for the split times, the dial for the pedals and steering, else
    /// itself. A layout places the parts that carry themselves.
    pub const fn carrier(self) -> Part {
        match self {
            Part::Splits => Part::Clock,
            Part::Inputs => Part::Dash,
            part => part,
        }
    }

    /// The point of the screen the part scales about: the corner or edge it stands at.
    pub const fn pivot(self) -> Align2 {
        match self {
            Part::Clock | Part::Splits => Align2::LEFT_TOP,
            Part::Progress | Part::Pace => Align2::CENTER_TOP,
            Part::Messages => Align2::CENTER_CENTER,
            Part::View => Align2::RIGHT_TOP,
            Part::Car | Part::Trace => Align2::LEFT_BOTTOM,
            Part::Dash | Part::Inputs | Part::Tyres | Part::Gforce => Align2::RIGHT_BOTTOM,
        }
    }
}

impl Layout {
    /// The layout of `list`, `PART:X,Y` joined by `;` (`--hud-layout`), at 100 %.
    pub fn parse(list: &str) -> Result<Layout, String> {
        let mut layout = Layout::default();
        for item in list.split(';').filter(|i| !i.is_empty()) {
            let wrong = || format!("HUD layout {item}: PART:X,Y");
            let (name, offset) = item.split_once(':').ok_or_else(wrong)?;
            let (x, y) = offset.split_once(',').ok_or_else(wrong)?;
            let part = movable().find(|p| p.name() == name).ok_or(format!("no movable HUD part {name}"))?;
            layout.offsets[part as usize] =
                [x.trim().parse().map_err(|_| wrong())?, y.trim().parse().map_err(|_| wrong())?];
        }
        Ok(layout)
    }

    /// The moved parts as [`Layout::parse`] reads them, or `None` when every part is in its place.
    pub fn moved(&self) -> Option<String> {
        let items: Vec<String> = movable()
            .filter(|p| self.offsets[*p as usize] != [0; 2])
            .map(|p| format!("{}:{},{}", p.name(), self.offsets[p as usize][0], self.offsets[p as usize][1]))
            .collect();
        (!items.is_empty()).then(|| items.join(";"))
    }

    /// Where the look's `bounds` of `part` on `screen` go: scaled about the part's pivot, moved by its offset, and
    /// back inside the screen if that left it.
    pub fn place(&self, part: Part, bounds: Rect, screen: Rect) -> TSTransform {
        let scale = f32::from(self.scale.clamp(*SCALES.start(), *SCALES.end())) / 100.0;
        let [x, y] = self.offsets[part as usize];
        let pivot = part.pivot().pos_in_rect(&screen).to_vec2();
        let placed = TSTransform::new(pivot * (1.0 - scale) + Vec2::new(f32::from(x), f32::from(y)), scale);
        TSTransform::new(placed.translation + inside(placed * bounds, screen), scale)
    }
}

fn movable() -> impl Iterator<Item = Part> {
    Part::ALL.into_iter().filter(|part| part.carrier() == *part)
}

/// The least move that brings `rect` inside `screen` (centred on an axis it is too large for).
fn inside(rect: Rect, screen: Rect) -> Vec2 {
    let axis = |min: f32, max: f32, low: f32, high: f32| {
        if max - min > high - low {
            (low + high - min - max) / 2.0
        } else {
            (low - min).max(0.0) + (high - max).min(0.0)
        }
    };
    Vec2::new(
        axis(rect.min.x, rect.max.x, screen.min.x, screen.max.x),
        axis(rect.min.y, rect.max.y, screen.min.y, screen.max.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;

    #[test]
    fn a_layout_round_trips_through_its_list() {
        assert_eq!(Layout::parse(""), Ok(Layout::default()));
        assert_eq!(Layout::default().moved(), None);
        let layout = Layout::parse("dash:-40,0;clock:12,-3").unwrap();
        assert_eq!(layout.offsets[Part::Dash as usize], [-40, 0]);
        assert_eq!(layout.moved().as_deref(), Some("clock:12,-3;dash:-40,0"));
        assert!(Layout::parse("inputs:1,1").is_err() && Layout::parse("dash:1").is_err());
    }

    #[test]
    fn a_part_scales_about_its_corner_moves_and_stays_on_the_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0));
        let dial = Rect::from_min_max(Pos2::new(1076.0, 516.0), Pos2::new(1260.0, 700.0));
        let mut layout = Layout { scale: 200, ..Layout::default() };
        // Twice the size, its margin too, from the bottom right corner.
        assert_eq!(
            layout.place(Part::Dash, dial, screen) * dial,
            Rect::from_min_max(Pos2::new(872.0, 312.0), Pos2::new(1240.0, 680.0))
        );
        layout.offsets[Part::Dash as usize] = [-100, 500];
        let placed = layout.place(Part::Dash, dial, screen) * dial;
        assert_eq!((placed.min.x, placed.max.y), (772.0, 720.0));
    }
}
