//! The gamepads' moves in the menus ([`Nav`]): the d-pad, South (A, cross) and East (B, circle) of every gamepad SDL
//! knows, polled once a frame; Start is the pause binding's. A held d-pad repeats its move, as a held key does.

use std::time::{Duration, Instant};

use sdl3_sys::gamepad::*;
use sdl3_sys::init::SDL_INIT_GAMEPAD;
use sdl3_sys::joystick::SDL_UpdateJoysticks;

use crate::hud::pause::Nav;
use crate::sdl::Subsystem;

/// A held button repeats its move after this and then this often.
const REPEAT_AFTER: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(80);
/// How often gamepads plugged in or out are looked for.
const SEARCH: Duration = Duration::from_secs(1);

const BUTTONS: [(SDL_GamepadButton, Nav); 6] = [
    (SDL_GAMEPAD_BUTTON_DPAD_UP, Nav::Up),
    (SDL_GAMEPAD_BUTTON_DPAD_DOWN, Nav::Down),
    (SDL_GAMEPAD_BUTTON_DPAD_LEFT, Nav::Left),
    (SDL_GAMEPAD_BUTTON_DPAD_RIGHT, Nav::Right),
    (SDL_GAMEPAD_BUTTON_SOUTH, Nav::Select),
    (SDL_GAMEPAD_BUTTON_EAST, Nav::Back),
];

/// The gamepads open, and each button's next repeat while one of them holds it.
pub struct Gamepads {
    pads: Vec<*mut SDL_Gamepad>,
    held: [Option<Instant>; BUTTONS.len()],
    searched: Option<Instant>,
    _sdl: Subsystem,
}

impl Gamepads {
    pub fn open() -> Result<Gamepads, String> {
        let sdl = Subsystem::init(SDL_INIT_GAMEPAD)?;
        Ok(Gamepads { pads: Vec::new(), held: [None; BUTTONS.len()], searched: None, _sdl: sdl })
    }

    /// The moves since the last poll.
    pub fn poll(&mut self, now: Instant) -> Vec<Nav> {
        // SAFETY: the pads are SDL's open gamepads, closed in `search` or at the drop only.
        unsafe { SDL_UpdateJoysticks() };
        if self.searched.is_none_or(|at| now - at >= SEARCH) {
            self.search();
            self.searched = Some(now);
        }
        let mut navs = Vec::new();
        for ((button, nav), held) in BUTTONS.into_iter().zip(&mut self.held) {
            // SAFETY: as above.
            let down = self.pads.iter().any(|&pad| unsafe { SDL_GetGamepadButton(pad, button) });
            let repeats = !matches!(nav, Nav::Select | Nav::Back);
            if press(held, down, repeats, now) {
                navs.push(nav);
            }
        }
        navs
    }

    /// Closes the gamepads unplugged and opens the ones not open yet.
    fn search(&mut self) {
        // SAFETY: `ids` is SDL's array of `count` ids, freed here; a closed pad leaves the list.
        unsafe {
            self.pads.retain(|&pad| {
                let connected = SDL_GamepadConnected(pad);
                if !connected {
                    SDL_CloseGamepad(pad);
                }
                connected
            });
            let mut count = 0;
            let ids = SDL_GetGamepads(&mut count);
            if ids.is_null() {
                return;
            }
            for &id in std::slice::from_raw_parts(ids, usize::try_from(count).unwrap_or(0)) {
                if self.pads.iter().all(|&pad| SDL_GetGamepadID(pad) != id) {
                    let pad = SDL_OpenGamepad(id);
                    if !pad.is_null() {
                        self.pads.push(pad);
                    }
                }
            }
            sdl3_sys::stdinc::SDL_free(ids.cast());
        }
    }
}

impl Drop for Gamepads {
    fn drop(&mut self) {
        // SAFETY: each pad was opened in `search` and is closed once.
        self.pads.drain(..).for_each(|pad| unsafe { SDL_CloseGamepad(pad) });
    }
}

/// Whether a button `down` or not at `now` makes its move: at the press, and with `repeats` again while it is held;
/// `held` is its next repeat.
fn press(held: &mut Option<Instant>, down: bool, repeats: bool, now: Instant) -> bool {
    match *held {
        _ if !down => {
            *held = None;
            false
        }
        None => {
            *held = Some(now + REPEAT_AFTER);
            true
        }
        Some(next) if repeats && now >= next => {
            *held = Some(next + REPEAT_EVERY);
            true
        }
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_button_repeats() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut held = None;
        let moves = |held: &mut Option<Instant>, repeats: bool| {
            [0, 100, 399, 400, 450, 480, 560, 600].map(|ms| press(held, ms != 600, repeats, at(ms)))
        };
        assert_eq!(moves(&mut held, true), [true, false, false, true, false, true, true, false]);
        assert_eq!(held, None);
        // Select and Back move once a press.
        assert_eq!(moves(&mut held, false), [true, false, false, false, false, false, false, false]);
        assert!(press(&mut held, true, false, at(700)));
    }
}
