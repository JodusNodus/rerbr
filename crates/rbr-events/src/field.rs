//! The AI field in the player's class (ours): entrants with a skill and a consistency, and their stage times
//! around the stage's reference time.

/// An AI entrant.
#[derive(Debug, Clone, PartialEq)]
pub struct Entrant {
    pub name: String,
    /// The catalog car key it drives (shown in the standings), a car of the player's class.
    pub car: String,
    /// 1 drives the reference pace; lower is slower.
    pub skill: f32,
    /// 1 never errs; lower spreads its times and risks time losses and retirements.
    pub consistency: f32,
}

/// An AI entrant's result on one stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiResult {
    Time(f32),
    Retired,
}

/// `size` entrants for `difficulty` (0 to 1), driving `cars` (catalog keys of the class; repeated as needed).
pub fn field(cars: &[String], size: usize, difficulty: f32, seed: u32) -> Vec<Entrant> {
    let _ = (cars, size, difficulty, seed);
    todo!("rbr-events worker")
}
