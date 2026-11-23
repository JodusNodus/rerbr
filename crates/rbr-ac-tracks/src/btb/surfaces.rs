//! RBR surface classes and their weather variants.

pub fn surface(ground: Option<&str>, steep: bool) -> u8 {
    let g = ground.unwrap_or("").to_uppercase();
    let has = |k: &str| g.contains(k);
    if has("ASPHALT") {
        37
    } else if has("RUMBLE") || has("CONCRETE") {
        65
    } else if has("METAL") && !steep {
        37
    } else if has("COBBLE") || has("ROCK") || has("STONE") {
        69
    } else if has("GRASS") || has("FOLIAGE") || has("MOSS") {
        113
    } else if has("GRAVEL") || has("SAND") {
        1
    } else if has("MUD") {
        149
    } else if has("DIRT") || has("SOIL") {
        133
    } else if steep {
        64
    } else {
        37
    }
}

/// A surface id in dry, damp and wet weather: a `_Dry` material's `_Damp` and `_Wet` are the next two ids (its id
/// 1 mod 4: 26 of 26 in 1.02's table, `Tarmac_Fine_Clean` 37/38/39); another (the metal barrier, 64) stays.
pub fn weathers(id: u8) -> [u8; 3] {
    if id % 4 == 1 {
        [id, id + 1, id + 2]
    } else {
        [id; 3]
    }
}

/// The stage's `physics=`: snow where over half of the drive lies on snow ids, else gravel where over half lies on
/// gravel or dirt, else tarmac. `along` pairs a surface id with the length of road it covers. (No AC surface becomes
/// snow by itself, but CSP's `ICE` (117) or `--surface ROAD=117`.)
pub fn physics(along: impl IntoIterator<Item = (u8, f64)>) -> &'static str {
    let (mut snow, mut loose, mut all) = (0., 0., 0.);
    for (id, length) in along {
        all += length;
        if matches!(id, 101..=103 | 117..=119) {
            snow += length;
        } else if matches!(id, 1 | 9 | 17 | 133 | 141 | 149) {
            loose += length;
        }
    }
    if snow * 2. > all {
        "snow"
    } else if loose * 2. > all {
        "gravel"
    } else {
        "tarmac"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_is_gravel_when_most_of_its_drive_is_loose() {
        assert_eq!(physics([(17, 6.), (37, 4.)]), "gravel");
        assert_eq!(physics([(133, 5.), (37, 5.)]), "tarmac");
        assert_eq!(physics([(37, 9.), (1, 1.)]), "tarmac");
        assert_eq!(physics([]), "tarmac");
    }
    #[test]
    fn a_stage_is_snow_when_most_of_its_drive_is_snow() {
        assert_eq!(physics([(117, 6.), (102, 1.), (17, 3.)]), "snow");
        assert_eq!(physics([(103, 4.), (17, 6.)]), "gravel");
    }
}
