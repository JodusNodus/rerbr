//! The Drive page's weather choices and the road wetness words.

use rbr_gameplay::weather::{self, Preset};
use rbr_render::atmosphere::{Clouds, Fog, Rain};

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

/// The fogs offered, in the dropdown's order.
pub const FOGS: [Fog; 3] = [Fog::Off, Fog::Light, Fog::Heavy];
/// The cloud covers offered.
pub const CLOUDS: [Clouds; 5] = [Clouds::Clear, Clouds::Dimpled, Clouds::Partly, Clouds::Broken, Clouds::Heavy];
/// What falls, offered.
pub const RAINS: [Rain; 4] = [Rain::None, Rain::Light, Rain::Moderate, Rain::Heavy];

impl Choice for Fog {
    const ALL: &[Self] = &FOGS;
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
    const ALL: &[Self] = &CLOUDS;
    fn name(self) -> &'static str {
        ["clear", "dimpled", "partly", "broken", "heavy"][self as usize]
    }
    fn label(self) -> &'static str {
        ["Clear", "Dimpled clouds", "Partly cloudy", "Broken clouds", "Overcast"][self as usize]
    }
    /// `light`, the word the broken cover had, still reads as it.
    fn parse(text: &str) -> Option<Self> {
        Clouds::parse(text)
    }
}

impl Choice for Rain {
    const ALL: &[Self] = &RAINS;
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
