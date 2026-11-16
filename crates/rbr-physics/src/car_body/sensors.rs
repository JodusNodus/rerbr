//! The sensor sampler, called by the publish step. It fills the 16-word sensor payload from the engine and drive
//! getters, the drive's sensor block, the filtered steering torque, the timer gap and the speed sum, then emits it on
//! the sensor signal. The order is the original's: the getters, the timer gap, the emit; the getters and the timer gap
//! are answered by `SensorCalls` (from the original's records in the test).

/// The calls the function makes out of its own body. The test answers them from the original's records.
pub trait SensorCalls {
    /// The engine's speed (payload word 0).
    fn engine_speed(&mut self, engine: u32) -> f32;
    /// The drive's gear word (payload word 1).
    fn drive_gear(&mut self, drive: u32) -> u32;
    /// The drive's speed, times the wheel radius for payload word 2.
    fn drive_speed(&mut self, drive: u32) -> f32;
    /// The engine's float for payload word 3 (a constant when the engine has no source for it). Named by use.
    fn engine_word_3(&mut self, engine: u32) -> f32;
    /// The engine's float for payload word 5. Named by use.
    fn engine_word_5(&mut self, engine: u32) -> f32;
    /// The engine's float for payload word 10 (a constant when the engine has no source for it). Named by use.
    fn engine_word_10(&mut self, engine: u32) -> f32;
    /// Payload word 14: 1 when the engine's speed is above a fixed limit, else 0.
    fn engine_word_14(&mut self, engine: u32) -> u32;
    /// The timer gap as a double (`obj` is the object it is called on, `at` the address of the timer's double).
    fn timer_gap(&mut self, obj: u32, at: u32) -> f64;
    /// Emits the payload on the sensor signal.
    fn emit_sensor(&mut self, payload: &[u32; 16]);
}
