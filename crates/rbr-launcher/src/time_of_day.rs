//! The Custom weather's Time of day: a moment of the day, solved from where the sun stands on the stage's site and
//! date (`rbr_render::relight::site`, `solar`; for a stage outside the 1.02 rallies a guess from its country and
//! surface), or the bake's sun; and the sun's path drawn for the Drive page. The choice is a moment, never a clock, and
//! no clock is shown (user decision): the site is a guess for most stages, the moment's light is not. The clock it
//! gives still goes to the viewer (`--clock`) and the start board (`--start-time`).

use egui::{Color32, Pos2, Sense, Stroke};
use rbr_render::relight::solar;

use crate::catalog::Map;

/// A stage's latitude, longitude and day of the year (`relight::site`).
pub type Site = (f32, f32, f32);

/// Map `map`'s site, as the viewer places it (`viewer`'s `stage_place`).
pub fn site(map: &Map) -> Site {
    let rsf = map.rsf.as_ref();
    let snow = rsf.is_some_and(|s| s.surface == Some(rbr_formats::rsf::Surface::Snow));
    rbr_render::relight::site(map.number, rsf.and_then(|s| s.country.as_deref()), snow)
}

/// A moment of the day: the sun at an elevation, in the morning or the evening, or at its highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    Dawn,
    Morning,
    Midday,
    Afternoon,
    Golden,
    Dusk,
    Night,
}

impl Moment {
    pub const ALL: [Moment; 7] =
        [Self::Dawn, Self::Morning, Self::Midday, Self::Afternoon, Self::Golden, Self::Dusk, Self::Night];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dawn => "Dawn",
            Self::Morning => "Morning",
            Self::Midday => "Midday",
            Self::Afternoon => "Afternoon",
            Self::Golden => "Golden hour",
            Self::Dusk => "Dusk",
            Self::Night => "Night",
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Dawn => "dawn",
            Self::Morning => "morning",
            Self::Midday => "midday",
            Self::Afternoon => "afternoon",
            Self::Golden => "golden",
            Self::Dusk => "dusk",
            Self::Night => "night",
        }
    }

    /// The moment's clock at `site`, minutes past midnight (local solar time). Dawn and dusk are the sun 3 degrees
    /// down, inside civil twilight; night 12 down, well past the renderer's `relight::NIGHT`; golden hour 6 up in the
    /// evening; morning and afternoon half the day's highest sun; midday solar noon. A sun that never climbs as high
    /// (golden hour where the day's highest sun is low) takes half the highest instead.
    pub fn minutes(self, (latitude, _, day): Site) -> u32 {
        let highest = solar(latitude, day, 12.0).0;
        let (elevation, morning) = match self {
            Self::Midday => return 12 * 60,
            Self::Dawn => (-3.0, true),
            Self::Morning => (highest / 2.0, true),
            Self::Afternoon => (highest / 2.0, false),
            Self::Golden => (GOLDEN.min(highest / 2.0), false),
            Self::Dusk => (-3.0, false),
            Self::Night => (-12.0, false),
        };
        let at = |m: u32| solar(latitude, day, m as f32 / 60.0).0;
        let found = if morning {
            (0..12 * 60).find(|&m| at(m) >= elevation)
        } else {
            (12 * 60..24 * 60).find(|&m| at(m) < elevation)
        };
        found.unwrap_or(if morning { 0 } else { 24 * 60 - 1 })
    }
}

/// Golden hour's sun, degrees up: low and warm, still clear of dusk for a stage's length.
const GOLDEN: f32 = 6.0;

/// The Time of day (`start_time` in the settings): a moment, or the bake's sun (the viewer relights the stage under
/// it, at its hour; `relight::Clock::new`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartTime {
    Moment(Moment),
    Baked,
}

/// Golden hour (user decision).
impl Default for StartTime {
    fn default() -> Self {
        Self::Moment(Moment::Golden)
    }
}

impl StartTime {
    /// The moments, then the bake's sun.
    pub fn all() -> impl Iterator<Item = Self> {
        Moment::ALL.map(Self::Moment).into_iter().chain([Self::Baked])
    }

    /// The clock at `site`, minutes past midnight; none for the bake's sun.
    pub fn minutes(self, site: Site) -> Option<u32> {
        match self {
            Self::Moment(moment) => Some(moment.minutes(site)),
            Self::Baked => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Moment(moment) => moment.label(),
            Self::Baked => "As baked",
        }
    }

    pub fn to_text(self) -> String {
        match self {
            Self::Moment(moment) => moment.word().into(),
            Self::Baked => "baked".into(),
        }
    }

    /// A moment's word or `baked`; anything else (an older launcher's `HH:MM`) is none, the default.
    pub fn parse(text: &str) -> Option<Self> {
        Self::all().find(|s| s.to_text() == text)
    }
}

/// Draws the sun's path over the day at `site` (midnight to midnight), `width` wide: the horizon, each moment's tick
/// and the `start` clock's sun or moon on it.
pub fn sun_arc(ui: &mut egui::Ui, width: f32, (latitude, _, day): Site, start: Option<u32>) {
    const HEIGHT: f32 = 56.0;
    const TOP: f32 = 70.0;
    const BOTTOM: f32 = -30.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::hover());
    let point = |minutes: f32| {
        let elevation = solar(latitude, day, minutes / 60.0).0.clamp(BOTTOM, TOP);
        Pos2::new(
            rect.left() + rect.width() * minutes / (24.0 * 60.0),
            rect.bottom() - rect.height() * (elevation - BOTTOM) / (TOP - BOTTOM),
        )
    };
    let painter = ui.painter_at(rect);
    let horizon = rect.bottom() - rect.height() * -BOTTOM / (TOP - BOTTOM);
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));
    painter.hline(rect.x_range(), horizon, Stroke::new(1.0, Color32::from_gray(80)));
    let path: Vec<Pos2> = (0..=96).map(|k| point(k as f32 * 15.0)).collect();
    for pair in path.windows(2) {
        let up = pair[0].y.min(pair[1].y) < horizon;
        let colour = if up { Color32::from_rgb(235, 190, 90) } else { Color32::from_rgb(70, 80, 120) };
        painter.line_segment([pair[0], pair[1]], Stroke::new(1.5, colour));
    }
    for moment in Moment::ALL {
        let at = point(moment.minutes((latitude, 0.0, day)) as f32);
        painter.line_segment([at - egui::vec2(0.0, 3.0), at + egui::vec2(0.0, 3.0)], Stroke::new(1.0, Color32::GRAY));
    }
    if let Some(minutes) = start {
        let at = point(minutes as f32);
        let fill = if at.y < horizon { Color32::from_rgb(255, 210, 110) } else { Color32::from_rgb(150, 165, 220) };
        painter.circle(at, 5.0, fill, Stroke::new(1.5, Color32::WHITE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_render::relight;

    #[test]
    fn the_moments_follow_the_sun_of_each_site() {
        // One map of each 1.02 rally, and guessed sites: a Finnish snow stage, a Kenyan one, one with no country.
        let retail = [10, 20, 31, 41, 51, 61].map(|map| relight::site(map, None, false));
        let guessed = [
            relight::site(500, Some("fi"), true),
            relight::site(501, Some("KE"), false),
            relight::site(502, None, false),
        ];
        for site in retail.into_iter().chain(guessed) {
            let elevation = |moment: Moment| solar(site.0, site.2, moment.minutes(site) as f32 / 60.0).0;
            assert!((5.5..GOLDEN).contains(&elevation(Moment::Golden)), "{site:?}");
            assert!((-3.5..-3.0).contains(&elevation(Moment::Dusk)), "{site:?}");
            assert!(elevation(Moment::Night) < relight::NIGHT, "{site:?}");
            let order = Moment::ALL.map(|m| m.minutes(site));
            assert!(order.windows(2).all(|w| w[0] < w[1]), "{site:?}: {order:?}");
        }
        let site = retail[0];
        assert_eq!(StartTime::default().minutes(site), Some(Moment::Golden.minutes(site)));
        assert_eq!(StartTime::Baked.minutes(site), None);
    }

    #[test]
    fn a_start_time_reads_back_as_written() {
        for start in StartTime::all() {
            assert_eq!(StartTime::parse(&start.to_text()), Some(start));
        }
        assert_eq!(StartTime::parse("16:30"), None);
        assert_eq!(StartTime::parse("noon"), None);
    }
}
