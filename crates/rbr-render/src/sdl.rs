//! SDL3's joystick layer as the devices (`crate::devices`, off Linux) and the menu's gamepads (`crate::gamepads`) use
//! it: polled (`SDL_UpdateJoysticks`), its events off, as nothing reads SDL's event queue and they would fill it.
//!
//! macOS: SDL's IOKit driver hears of devices plugged in or out on the run loop of the thread that first started the
//! subsystem, when that thread updates the joysticks; the launcher does both on its main thread, the viewer starts
//! its gamepads there before its other threads start SDL (`Gamepads::open`, then the wheel's and the input's).

use sdl3_sys::gamepad::SDL_SetGamepadEventsEnabled;
use sdl3_sys::init::{SDL_InitFlags, SDL_InitSubSystem, SDL_QuitSubSystem};
use sdl3_sys::joystick::SDL_SetJoystickEventsEnabled;

/// SDL subsystems, started (SDL counts the starts) and quit at the drop.
pub struct Subsystem(SDL_InitFlags);

impl Subsystem {
    pub fn init(flags: SDL_InitFlags) -> Result<Subsystem, String> {
        // SAFETY: SDL's init is safe from any thread; the event switches are plain flags.
        unsafe {
            if !SDL_InitSubSystem(flags) {
                return Err(crate::ffb::sdl_error());
            }
            SDL_SetJoystickEventsEnabled(false);
            SDL_SetGamepadEventsEnabled(false);
        }
        Ok(Subsystem(flags))
    }
}

impl Drop for Subsystem {
    fn drop(&mut self) {
        // SAFETY: pairs the init above.
        unsafe { SDL_QuitSubSystem(self.0) }
    }
}
