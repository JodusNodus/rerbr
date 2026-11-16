//! When a session ends.

/// How a session ends. Evaluated at the lap line: a race does not stop in the middle of a lap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EndRule {
    /// At the first pass of the line (a stage's finish, a one-lap hotlap).
    Finish,
    /// After this many laps.
    Laps(u32),
    /// At the first pass of the line once this many seconds have gone.
    Time(f32),
    /// Never: free practice, until the player leaves.
    Open,
}

impl EndRule {
    /// Whether the session ends at a pass of the line: `laps` laps done, `elapsed` seconds since the start.
    pub fn reached(self, laps: u32, elapsed: f32) -> bool {
        match self {
            EndRule::Finish => laps >= 1,
            EndRule::Laps(n) => laps >= n,
            EndRule::Time(seconds) => elapsed >= seconds,
            EndRule::Open => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_rule_ends_at_the_line() {
        assert!(!EndRule::Finish.reached(0, 0.0) && EndRule::Finish.reached(1, 90.0));
        assert!(!EndRule::Laps(3).reached(2, 600.0) && EndRule::Laps(3).reached(3, 0.0));
        assert!(!EndRule::Time(600.0).reached(9, 599.9) && EndRule::Time(600.0).reached(1, 600.0));
        assert!(!EndRule::Open.reached(99, 1e9));
    }
}
