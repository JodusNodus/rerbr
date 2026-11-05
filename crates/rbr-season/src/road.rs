//! The road's wear under the player.

/// The road's wear for a starter: the first three starters drive a new
/// road (0), places 4-11 a normal one (1), the rest a worn one (2); 1 when
/// the player is not in the start list.
pub fn road_wear(start_place: Option<usize>) -> u32 {
    match start_place {
        Some(p) if p <= 2 => 0,
        Some(p) => 1 + u32::from(p > 10),
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_road_wears_with_the_start_place() {
        assert_eq!([0, 2, 3, 10, 11, 13].map(|p| road_wear(Some(p))), [0, 0, 1, 1, 2, 2]);
        assert_eq!(road_wear(None), 1);
    }
}
