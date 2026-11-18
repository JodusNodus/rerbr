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
