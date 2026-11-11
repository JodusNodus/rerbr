//! The four-wheel-drive drivetrain's loss (ours, experimental) of the runs on this thread: with one set, NGP's drive
//! wrapper ([`super::steps`]) takes it on a four-wheel-drive car whose file's `DriveTrainLoss_NGP` is smaller; with
//! none (the default) the loss is the file's.

use std::cell::Cell;

thread_local! {
    static LOSS: Cell<Option<u32>> = const { Cell::new(None) };
}

/// The four-wheel-drive cars' drivetrain loss of the runs on this thread from now on, percent (`None`: the car
/// file's).
pub fn set_four_wheel_drive_loss(loss: Option<u32>) {
    LOSS.set(loss);
}

/// The loss, percent, of a car of drivetrain type `kind` (`rbr_physics::ngp::diff::drive_type`: 6 and 7 drive all
/// four wheels) whose file gives `file`.
pub(super) fn loss(kind: u32, file: u32) -> u32 {
    match LOSS.get() {
        Some(set) if matches!(kind, 6 | 7) => file.max(set),
        _ => file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_four_wheel_drive_car_takes_the_set_loss() {
        assert_eq!([4, 5, 6, 7].map(|kind| loss(kind, 12)), [12; 4]);
        set_four_wheel_drive_loss(Some(17));
        assert_eq!([4, 5, 6, 7].map(|kind| loss(kind, 12)), [12, 12, 17, 17]);
        assert_eq!(loss(6, 20), 20);
        set_four_wheel_drive_loss(None);
        assert_eq!(loss(7, 12), 12);
    }
}
