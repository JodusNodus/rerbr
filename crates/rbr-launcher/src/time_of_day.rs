//! The Custom weather's Time of day: a moment of the day (`rbr_render::relight::Moment`), where the sun stands on the
//! stage's site and date (`relight::site`, `solar`; for a stage outside the 1.02 rallies a guess from its country and
//! surface), or the bake's sun; and the sun's path drawn for the Drive page. The choice is a moment, never a clock, and
//! no clock is shown (user decision): the site is a guess for most stages, the moment's light is not. The moment goes to
//! the viewer (`--sun`), which solves its clock, golden hour's on the stage itself (where the sun reaches the road,
//! `relight::Clock::golden`), and gives it to the start board (`--start-time`). The path's golden-hour tick is the
//! 6-degree sun's.

use egui::{Color32, Pos2, Sense, Stroke};
use rbr_render::relight::solar;

use crate::catalog::Map;

/// A stage's latitude, longitude and day of the year (`relight::site`).
pub type Site = (f32, f32, f32);

/// Map `map`'s site, as the viewer places it (`viewer`'s `stage_place`).
pub fn site(map: &Map) -> Site {
    let rsf = map.community();
    let snow = rsf.is_some_and(|s| s.surface == Some(rbr_formats::rsf::Surface::Snow));
    let (country, name) = (rsf.and_then(|s| s.country.as_deref()), rsf.map_or("", |s| s.name.as_str()));
    rbr_render::relight::site(map.sky_map(), country, name, snow)
}

pub use rbr_render::relight::Moment;

/// A moment's name in the Time of day box.
fn label(moment: Moment) -> &'static str {
    match moment {
        Moment::Best => "Best light",
        Moment::Midday => "Midday",
        Moment::Golden => "Golden hour",
        Moment::Dusk => "Dusk",
        Moment::Night => "Night",
    }
}

/// A moment's clock at `site`, minutes past midnight, as the viewer solves it (`Moment::hour`), but best light's and
/// golden hour's: the viewer finds those on the stage (`relight::Clock::best`, `golden`).
pub fn minutes(moment: Moment, site: Site) -> u32 {
    (moment.hour(site) * 60.0).round() as u32
}

/// The Time of day (`start_time` in the settings): a moment, or the bake's sun (the viewer relights the stage under
/// it, at its hour; `relight::Clock::new`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartTime {
    Moment(Moment),
    Baked,
}

/// Best light.
impl Default for StartTime {
    fn default() -> Self {
        Self::Moment(Moment::Best)
    }
}

impl StartTime {
    /// The moments, then the bake's sun.
    pub fn all() -> impl Iterator<Item = Self> {
        Moment::ALL.map(Self::Moment).into_iter().chain([Self::Baked])
    }

    /// The clock at `site`, minutes past midnight ([`minutes`]); none for the bake's sun.
    pub fn minutes(self, site: Site) -> Option<u32> {
        match self {
            Self::Moment(moment) => Some(minutes(moment, site)),
            Self::Baked => None,
        }
    }

    /// The viewer's `--sun` arguments: the moment's, none for the bake's sun.
    pub fn sun_args(self) -> Vec<String> {
        match self {
            Self::Moment(moment) => vec!["--sun".into(), moment.word().into()],
            Self::Baked => Vec::new(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Moment(moment) => label(moment),
            Self::Baked => "As baked",
        }
    }

    pub fn to_text(self) -> String {
        match self {
            Self::Moment(moment) => moment.word().into(),
            Self::Baked => "baked".into(),
        }
    }

    /// A moment's word or `baked`; the words of the moments since dropped read as the nearest one (dawn and morning as
    /// best light, afternoon as golden hour); anything else (an older launcher's `HH:MM`) is none, the default.
    pub fn parse(text: &str) -> Option<Self> {
        let text = match text {
            "dawn" | "morning" => "best",
            "afternoon" => "golden",
            other => other,
        };
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
        let at = point(minutes(moment, (latitude, 0.0, day)) as f32);
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
        let retail = [10, 20, 31, 41, 51, 61].map(|map| relight::site(map, None, "", false));
        let guessed = [
            relight::site(500, Some("fi"), "", true),
            relight::site(501, Some("KE"), "", false),
            relight::site(502, None, "", false),
            relight::site(503, Some("GB"), "Sherwood Forest I Summer", false),
        ];
        for site in retail.into_iter().chain(guessed) {
            let elevation = |moment: Moment| solar(site.0, site.2, minutes(moment, site) as f32 / 60.0).0;
            assert!((5.5..relight::GOLDEN).contains(&elevation(Moment::Golden)), "{site:?}");
            assert!((-3.5..-3.0).contains(&elevation(Moment::Dusk)), "{site:?}");
            assert!(elevation(Moment::Night) < relight::NIGHT, "{site:?}");
            let order =
                [Moment::Midday, Moment::Best, Moment::Golden, Moment::Dusk, Moment::Night].map(|m| minutes(m, site));
            assert!(order.windows(2).all(|w| w[0] < w[1]), "{site:?}: {order:?}");
        }
        let site = retail[0];
        assert_eq!(StartTime::default().minutes(site), Some(minutes(Moment::Best, site)));
        assert_eq!(StartTime::Baked.minutes(site), None);
    }

    #[test]
    fn a_start_time_reads_back_as_written() {
        for start in StartTime::all() {
            assert_eq!(StartTime::parse(&start.to_text()), Some(start));
        }
        assert_eq!(StartTime::parse("morning"), Some(StartTime::Moment(Moment::Best)));
        assert_eq!(StartTime::parse("afternoon"), Some(StartTime::Moment(Moment::Golden)));
        assert_eq!(StartTime::parse("16:30"), None);
        assert_eq!(StartTime::parse("noon"), None);
    }
}
