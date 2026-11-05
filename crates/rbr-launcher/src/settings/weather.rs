//! The Drive page's weather choices and the road wetness words.

use rbr_gameplay::weather::{self, Preset};

use crate::pages::Choice;

/// The Drive page's weather: a game preset, or one season row of the map (its `TrackSettings.ini` section name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherChoice {
    Preset(Preset),
    Row(String),
}

impl WeatherChoice {
    pub(super) fn parse(text: &str) -> Option<WeatherChoice> {
        match text {
            "good" => Some(WeatherChoice::Preset(Preset::Good)),
            "random" => Some(WeatherChoice::Preset(Preset::Random)),
            "bad" => Some(WeatherChoice::Preset(Preset::Bad)),
            section if !section.is_empty() => Some(WeatherChoice::Row(section.to_owned())),
            _ => None,
        }
    }

    pub(super) fn to_text(&self) -> String {
        match self {
            WeatherChoice::Preset(preset) => weather::name(*preset).to_owned(),
            WeatherChoice::Row(section) => section.clone(),
        }
    }
}

/// Our fog over the stage (`viewer --fog`), not the game's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fog {
    Off,
    Light,
    Heavy,
}

impl Fog {
    /// The fogs offered, in the dropdown's order.
    pub const ALL: [Fog; 3] = [Fog::Off, Fog::Light, Fog::Heavy];
}

/// Our cloud cover over the stage (`viewer --clouds`), not the game's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clouds {
    Clear,
    Partly,
    Light,
    Heavy,
}

impl Clouds {
    pub const ALL: [Clouds; 4] = [Clouds::Clear, Clouds::Partly, Clouds::Light, Clouds::Heavy];
}

/// Our rain or snow over the stage (`viewer --precipitation`), not the game's; the road's wetness stays separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Light,
    Moderate,
    Heavy,
}

impl Precipitation {
    pub const ALL: [Precipitation; 4] =
        [Precipitation::None, Precipitation::Light, Precipitation::Moderate, Precipitation::Heavy];
}

impl Choice for Fog {
    const ALL: &[Self] = &Fog::ALL;
    fn name(self) -> &'static str {
        match self {
            Fog::Off => "off",
            Fog::Light => "light",
            Fog::Heavy => "heavy",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Fog::Off => "Off",
            Fog::Light => "Light",
            Fog::Heavy => "Heavy",
        }
    }
}

impl Choice for Clouds {
    const ALL: &[Self] = &Clouds::ALL;
    fn name(self) -> &'static str {
        ["clear", "partly", "light", "heavy"][self as usize]
    }
    fn label(self) -> &'static str {
        ["Clear", "Partly cloudy", "Light clouds", "Overcast"][self as usize]
    }
}

impl Choice for Precipitation {
    const ALL: &[Self] = &Precipitation::ALL;
    fn name(self) -> &'static str {
        ["none", "light", "moderate", "heavy"][self as usize]
    }
    fn label(self) -> &'static str {
        ["None", "Light", "Moderate", "Heavy"][self as usize]
    }
}

/// The road wetness word of `wetness` (0 dry, 1 damp, 2 wet).
pub fn wetness_name(wetness: u32) -> &'static str {
    ["dry", "damp", "wet"][wetness.min(2) as usize]
}

/// Parses a road wetness word.
pub fn parse_wetness(text: &str) -> Option<u32> {
    match text {
        "dry" => Some(0),
        "damp" => Some(1),
        "wet" => Some(2),
        _ => None,
    }
}
