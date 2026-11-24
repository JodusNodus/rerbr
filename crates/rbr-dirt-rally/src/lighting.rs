//! The look a stage's `settings.ini` states (`[ReRbrAir]`), from the area's `lighting_day.xml` and the day's post
//! effect in `postprocess/effects.xml` (`t~<location>~<area>~day`): the haze, the grade, the sun's tint. Which
//! variant a DiRT Rally event runs is not on disk, so the `day` one stands for the stage; our weather rows still
//! draw wet and overcast on top. Constants are calibrated against gameplay frames (`notes/dirt-rally.md`).
use crate::binxml::{self, Element};
use crate::install::Route;
use std::fmt::Write;

/// The values of one stage's `[ReRbrAir]`; `None` leaves the renderer's own.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lighting {
    /// Aerosol optical depth of a clear day (0.05 crisp to 0.2 hazy).
    pub aerosol: Option<f32>,
    /// The ground haze's display colour.
    pub fog_color: Option<[f32; 3]>,
    /// The post grade's saturation (`ColourBalance.colour`) and exposure bias (`ToneMap.targetLuminance`).
    pub saturation: Option<f32>,
    pub exposure: Option<f32>,
    /// The sun's colour, as a tint of unit luminance (`specular_colour`).
    pub sun_tint: Option<[f32; 3]>,
    /// The `TrackSettings.ini` sections of map 41's rows its Good and Bad weathers take (`track.ini`'s `weather_good`,
    /// `weather_bad`): DiRT Rally's own conditions for the area.
    pub weather_good: Option<&'static str>,
    pub weather_bad: Option<&'static str>,
}

/// Aerosol depth per kilometre of the haze's reach: `fogHazeDistance` (the nearer of its two) 12 km gives 0.1, 6 km and under the hazy 0.2.
const AEROSOL_KM: f32 = 1.2;
const AEROSOL_RANGE: (f32, f32) = (0.05, 0.2);
/// How much of the sun side's warm haze colour (`fogHazeColour2`) joins the haze's own in the ground haze.
const WARM_SHARE: f32 = 0.5;
/// The day's `HDRRange` under which its light is overcast (Hafren's 0.7; the others' 2.2 to 2.4).
const OVERCAST_HDR: f32 = 1.5;
/// A day is hazy when its sun-side haze is this bright (`fogHazeBrightness2`: Greece 1.37, Turini 1.3, Sweden 1.46,
/// Finland 1.19, Germany 1.0) or reaches under [`HAZY_REACH`] metres (Turini 3000, Sweden 6000).
const HAZY_BRIGHTNESS: f32 = 1.3;
const HAZY_REACH: f32 = 6000.0;
/// Map 41's rows by their sections: overcast, hazy, and the rain a wet variant stands for.
const OVERCAST: &str = "41O_M_norain_lightcloud";
const HAZY: &str = "41M_hazy_clear";
const RAIN: &str = "41O_M_lightrain_heavycloud";

fn param<'a>(lighting: &'a Element, name: &str) -> Option<&'a str> {
    lighting.children.iter().find_map(|p| p.attr(name))
}

fn number(lighting: &Element, name: &str) -> Option<f32> {
    param(lighting, name)?.trim().parse().ok()
}

fn colour(lighting: &Element, name: &str) -> Option<[f32; 3]> {
    binxml::numbers(param(lighting, name)?)
}

/// The effect `name`'s `group.param` value as text.
pub(crate) fn effect_param<'a>(effects: &'a Element, name: &str, group: &str, key: &str) -> Option<&'a str> {
    let effect = effects.children.iter().find(|e| e.attr("name") == Some(name))?;
    let group = effect.children_named("ParameterGroup").find(|g| g.attr("name") == Some(group))?;
    group.children_named("Param").find(|p| p.attr("name") == Some(key))?.text.as_deref()
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

impl Lighting {
    /// The `day` variant of `route`'s area; `None` where the files are missing.
    pub fn day(route: &Route) -> Option<Lighting> {
        let wet = route.area_dir().join("lighting_wet.xml").is_file();
        let light = binxml::parse(&std::fs::read(route.area_dir().join("lighting_day.xml")).ok()?).ok()?;
        let effects = route
            .dir
            .ancestors()
            .nth(5)
            .and_then(|data| std::fs::read(data.join("postprocess/effects.xml")).ok())
            .and_then(|bytes| binxml::parse(&bytes).ok());
        Some(Lighting::from_files(&light, effects.as_ref(), &format!("t~{}~{}~day", route.location, route.area), wet))
    }

    /// `wet`: the area has a wet variant of its light, so its Bad weather is rain.
    pub fn from_files(light: &Element, effects: Option<&Element>, effect: &str, wet: bool) -> Lighting {
        let reach = number(light, "fogHazeDistance").zip(number(light, "fogHazeDistance2")).map(|(a, b)| a.min(b));
        let haze = |name: &str, bright: &str| {
            let c = colour(light, name)?;
            let k = number(light, bright).unwrap_or(1.0);
            Some(c.map(|v| v * k))
        };
        let fog_color = haze("fogHazeColour", "fogHazeBrightness").map(|own| {
            let warm = haze("fogHazeColour2", "fogHazeBrightness2").unwrap_or(own);
            let norm = |c: [f32; 3]| c.map(|v| v / c[0].max(c[1]).max(c[2]).max(1e-3));
            let (own, warm) = (norm(own), norm(warm));
            std::array::from_fn(|i| own[i] + (warm[i] - own[i]) * WARM_SHARE)
        });
        let post =
            |group: &str, key: &str| -> Option<f32> { effect_param(effects?, effect, group, key)?.trim().parse().ok() };
        let sun_tint = colour(light, "specular_colour").map(|c| c.map(|v| v / luma(c).max(1e-3)));
        Lighting {
            aerosol: reach.map(|m| (AEROSOL_KM / (m / 1000.0)).clamp(AEROSOL_RANGE.0, AEROSOL_RANGE.1)),
            fog_color,
            saturation: post("ColourBalance", "colour"),
            exposure: post("ToneMap", "targetLuminance"),
            sun_tint,
            weather_good: if number(light, "HDRRange").is_some_and(|r| r < OVERCAST_HDR) {
                Some(OVERCAST)
            } else if number(light, "fogHazeBrightness2").is_some_and(|b| b >= HAZY_BRIGHTNESS)
                || reach.is_some_and(|m| m <= HAZY_REACH)
            {
                Some(HAZY)
            } else {
                None
            },
            weather_bad: wet.then_some(RAIN),
        }
    }

    /// The exposure taken back by the light the area's grade adds to a sunlit road (`Grade::lift`), so the frame keeps the
    /// brightness the stage had without the grade (the gameplay frames' roads were matched before it).
    pub fn graded(mut self, lift: f32) -> Lighting {
        self.exposure = Some(self.exposure.unwrap_or(1.0) / lift.max(0.1));
        self
    }

    /// The `track.ini` `[INFO]` lines of its own weathers.
    pub fn info_ini(&self) -> String {
        let mut text = String::new();
        for (key, value) in [("weather_good", self.weather_good), ("weather_bad", self.weather_bad)] {
            if let Some(v) = value {
                let _ = writeln!(text, "{key}={v}");
            }
        }
        text
    }

    /// The `[ReRbrAir]` lines (each ends in a newline) after the stage's fixed ones.
    pub fn ini(&self) -> String {
        let mut text = String::new();
        let triple = |c: [f32; 3]| format!("{:.3}, {:.3}, {:.3}", c[0], c[1], c[2]);
        if let Some(v) = self.aerosol {
            let _ = writeln!(text, "aerosol={v:.3}");
        }
        if let Some(c) = self.fog_color {
            let _ = writeln!(text, "fog_color={}", triple(c));
        }
        if let Some(v) = self.saturation {
            let _ = writeln!(text, "saturation={v:.3}");
        }
        if let Some(v) = self.exposure {
            let _ = writeln!(text, "exposure={v:.3}");
        }
        if let Some(c) = self.sun_tint {
            let _ = writeln!(text, "sun_tint={}", triple(c));
        }
        // DiRT Rally's sunny day is not held 1.5 stops down as the renderer's photographic one is (the gameplay frames'
        // sunlit gravel reads 175 to 200 of 255 against 100 here; cloud and haze rows keep the meter's exposure either way).
        text.push_str("sun_stops=0\n");
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, value: &str) -> Element {
        Element { name: "param".into(), attrs: vec![(name.into(), value.into())], ..Element::default() }
    }

    #[test]
    fn the_haze_grade_and_sun_come_from_the_files() {
        let light = Element {
            children: vec![
                p("fogHazeDistance", "12000"),
                p("fogHazeDistance2", "12000"),
                p("fogHazeColour", "0.5 0.6 0.8"),
                p("fogHazeBrightness", "1.0"),
                p("fogHazeColour2", "1.0 0.9 0.8"),
                p("fogHazeBrightness2", "1.4"),
                p("specular_colour", "1.0 0.9 0.8"),
            ],
            ..Element::default()
        };
        let l = Lighting::from_files(&light, None, "t", true);
        assert_eq!(l.aerosol, Some(0.1));
        assert_eq!((l.saturation, l.exposure), (None, None));
        let tint = l.sun_tint.unwrap();
        assert!((luma(tint) - 1.0).abs() < 1e-5 && tint[0] > tint[2]);
        assert!(l.ini().contains("aerosol=0.100\nfog_color="));
        assert!(l.ini().ends_with("sun_stops=0\n"));
        // Brightness 1.4 of the sun-side haze: hazy; a wet variant makes the Bad weather rain.
        assert_eq!(l.info_ini(), "weather_good=41M_hazy_clear\nweather_bad=41O_M_lightrain_heavycloud\n");
    }
}
