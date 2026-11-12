//! The keys, gamepad presses and the pause menu.

use super::App;
use crate::args::Args;
use rbr_gameplay::pause_menu::{Item, Page};
use rbr_render::bindings::Action;
use rbr_render::hud::camera_page::{CameraPage, Fov};
use rbr_render::hud::ffb_page::FfbPage;
use rbr_render::hud::hud_page::HudPage;
use rbr_render::hud::pause::{Nav, Outcome, PauseMenu};
use rbr_render::live::{self, Controls, Live};
use rbr_render::playback::{ChaseLook, Drive};
use rbr_render::shifter::Shifter;
use std::collections::HashSet;
use std::time::Instant;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;

impl App {
    /// A live-mode press of a gear, restart, call-for-help or pause key or gamepad input, or in either mode of the
    /// wipers, headlights, camera or gauge mode ones. While the pause menu is open only the pause goes through: it
    /// closes the menu. While a message box is up the pause accepts it.
    pub fn press(&mut self, action: Action) {
        if self.results.is_some() {
            match action {
                Action::Pause => self.results_nav(Nav::Back),
                Action::Restart
                    if self
                        .scene
                        .live
                        .as_ref()
                        .is_some_and(|p| Page::of(rbr_gameplay::flow::Mode(p.menu.mode)) == Page::Race) =>
                {
                    self.restart_live()
                }
                _ => {}
            }
            return;
        }
        if action == Action::Pause && self.accept_message() {
            return;
        }
        // A replay pauses with no menu: the pause key (or P, Space) pauses and goes on.
        if action == Action::Pause && self.replay {
            if self.paused_at.is_some() {
                self.resume_replay();
            } else {
                self.pause_replay();
            }
            return;
        }
        if action == Action::Pause && self.scene.live.is_some() {
            match self.menu {
                Some(_) => self.menu_nav(Nav::Back),
                None => self.open_menu(),
            }
            return;
        }
        if self.menu.is_some() {
            return;
        }
        if let (Action::Wipers, Some(glass)) = (action, &mut self.scene.glass) {
            glass.switch = glass.switch.next();
            eprintln!("viewer: wipers {}", glass.switch.name());
        }
        if let (Action::Headlights, Some(playback)) = (action, &mut self.scene.playback) {
            playback.headlights = playback.headlights.next();
            eprintln!("viewer: headlights {}", playback.headlights.name());
        }
        if action == Action::Camera && self.scene.playback.is_some() && self.shown.may_cycle() {
            self.view = self.view.next();
            self.scene.cameras.chose(self.view);
        }
        if let (Action::GaugeMode, Some(dials)) = (action, &mut self.scene.dials) {
            dials.next_mode();
        }
        let Some(process) = &self.scene.live else {
            return;
        };
        match action {
            // The poll gives the shifts and the direct gear actions (`rbr_render::devices`).
            Action::GearUp | Action::GearDown => {}
            a if a.gear().is_some() => {}
            Action::CallForHelp => self.controls.flags |= live::CALL_FOR_HELP,
            // The game restarts from its pause page 1 only: a championship stage has no restart.
            Action::Restart if Page::of(rbr_gameplay::flow::Mode(process.menu.mode)) == Page::Race => {
                self.restart_live()
            }
            _ => {}
        }
    }

    /// Accepts the race's message box (a retire's) if one is up, or confirms the start of a championship stage
    /// or a rally-school lesson, whose countdown waits for it (`rbr_gameplay::flow::Mode::start_waits`); whether
    /// either was. The game's box keeps the menu's input until it is accepted, so the pause, Enter, the gamepad's
    /// South and a click all accept it.
    pub fn accept_message(&mut self) -> bool {
        // A school demo playing is skipped first: the brief shows instead (a ride sends its confirm, ending
        // the lesson), and the Enter never reaches the standalone as the brief's confirm.
        if self.skip_demo() {
            return true;
        }
        let up = self.scene.live.as_ref().is_some_and(|live| {
            let waits = rbr_gameplay::flow::Mode(live.menu.mode).start_waits() && !live.menu.driving && !live.menu.over;
            live.hud_model.view().message_box.is_some() || waits
        });
        if up {
            self.controls.flags |= live::ACCEPT_MESSAGE;
        }
        up
    }

    /// Starts the stage over in the live process (the stage's start).
    pub fn restart_live(&mut self) {
        // A restart brings the brief, and the school demo, back.
        self.demo = None;
        // The drive so far ends: its samples go into the car's reference.
        self.scene.ffb.finish();
        let (Some(process), Some(playback)) = (&mut self.scene.live, &mut self.scene.playback) else {
            return;
        };
        match restart(process) {
            Ok(drive) => {
                playback.reset(drive);
                self.running.renderer.reset_history();
                if self.results.take().is_some() {
                    // The stage result paused them.
                    self.pause_outputs(false);
                }
                self.ended = None;
                self.controls = Controls::default();
                if let Some(shifter) = &mut self.shifter {
                    *shifter = Shifter::new(shifter.gearbox, shifter.clutch_threshold, shifter.auto_neutral);
                }
            }
            Err(e) => eprintln!("viewer: {e}"),
        }
    }

    /// A key press while watching a replay: P or Space pauses the replay and goes on, R starts it over, V
    /// resets the mouse look, the camera key cycles the views as in live, Escape closes.
    pub fn replay_key(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
        match code {
            KeyCode::Escape => {
                self.finish_ghost();
                event_loop.exit();
            }
            KeyCode::KeyP | KeyCode::Space => {
                if self.paused_at.is_some() {
                    self.resume_replay();
                } else {
                    self.pause_replay();
                }
            }
            KeyCode::KeyR => self.restart_live(),
            KeyCode::KeyV => self.look = ChaseLook::default(),
            _ => {
                if let Some(action) = self.bindings.action_for_key(code) {
                    self.press(action);
                }
            }
        }
        self.held.insert(code);
    }

    /// Pauses the replay: the process's clock stopped (`Live::pause`), the sound stopped where it is, and the
    /// viewer's clocks (the characters, the renderer's time) held, as with the pause menu but with no menu.
    pub fn pause_replay(&mut self) {
        if self.paused_at.is_some() {
            return;
        }
        if let Some(process) = &mut self.scene.live {
            process.pause();
        }
        self.pause_outputs(true);
        self.paused_at = Some(Instant::now());
    }

    /// Goes on after pausing the replay, from where the clock stopped, with no catch-up.
    pub fn resume_replay(&mut self) {
        let Some(at) = self.paused_at.take() else {
            return;
        };
        self.paused += at.elapsed();
        if let Some(process) = &mut self.scene.live {
            process.resume();
        }
        self.pause_outputs(false);
    }

    /// Stops or restarts the drive's sound where it is and holds the wheel's force at 0 (the pause menu, a paused
    /// replay, the stage result).
    pub fn pause_outputs(&self, paused: bool) {
        if let Some(sound) = &self.sound {
            sound.set_paused(paused);
        }
        if let Some(wheel) = &self.wheel {
            wheel.set_paused(paused);
        }
    }

    /// Opens the pause menu and pauses the drive: the process's clock stopped (`Live::pause`), the sound stopped
    /// where it is, the wheel's force at 0, and the viewer's clocks (the characters, the renderer's time) held.
    pub fn open_menu(&mut self) {
        let fov = self.drawn_fov();
        let Some(process) = &mut self.scene.live else {
            return;
        };
        // The Force feedback page with the wheel's force on, once the live process said the car's surface.
        let ffb = (process.hud.ffb_surface.zip(self.wheel.as_ref()))
            .map(|(surface, _)| FfbPage::of(&self.scene.ffb, surface));
        self.menu = Some(
            PauseMenu::open(
                process.menu,
                CameraPage::new(self.view, self.scene.cameras.nudges(), self.scene.cameras.cockpit(), fov),
                ffb,
            )
            .with_hud(HudPage::new(self.scene.hud.parts, self.scene.hud.layout)),
        );
        process.pause();
        self.pause_outputs(true);
        self.paused_at = Some(Instant::now());
    }

    /// One move in the open menu, and what the chosen item does or the Camera page changed.
    pub fn menu_nav(&mut self, nav: Nav) {
        let item = match self.menu.as_mut().and_then(|menu| menu.nav(nav)) {
            None => return,
            Some(Outcome::Camera) => return self.camera_changed(),
            Some(Outcome::ForceFeedback) => return self.ffb_changed(),
            Some(Outcome::Hud) => return self.hud_changed(),
            Some(Outcome::Chosen(item)) => item,
        };
        self.menu = None;
        if let Some(process) = &mut self.scene.live {
            process.resume();
        }
        self.pause_outputs(false);
        if let Some(at) = self.paused_at.take() {
            self.paused += at.elapsed();
        }
        self.choose_item(item);
    }

    /// Both the pause menu and stage results use the same live actions.
    pub fn choose_item(&mut self, item: Item) {
        match item {
            Item::Resume => {}
            Item::CallForHelp => self.controls.flags |= live::CALL_FOR_HELP,
            Item::Retire => self.controls.flags |= live::RETIRE,
            Item::Restart => self.restart_live(),
            Item::Exit | Item::Continue => self.exit = true,
        }
    }

    /// Keeps what the Force feedback page set: the strength, the smoothing, and the car's sensitivity (or drops the
    /// car's own), and writes them to the file; the wheel takes the first two at once and the sensitivity with the
    /// next frame ([`Self::ffb_frame`]).
    fn ffb_changed(&mut self) {
        let Some(page) = self.menu.as_ref().and_then(PauseMenu::ffb) else {
            return;
        };
        let (surface, session) = (page.surface, &mut self.scene.ffb);
        // The page shows the strength in whole percent: changed only when the player moved it.
        if page.strength != (session.strength * 100.0).round() as i32 {
            let strength = page.strength as f32 / 100.0;
            session.set_strength(strength);
            if let Some(wheel) = &self.wheel {
                wheel.set_strength(strength);
            }
        }
        if page.smoothing != session.smoothing {
            session.set_smoothing(page.smoothing);
            if let Some(wheel) = &self.wheel {
                wheel.set_smoothing(page.smoothing);
            }
        }
        if (page.own, page.value) != (session.own(), session.now()[surface]) {
            session.set(surface, page.own.then_some(page.value));
        }
    }

    /// The frame's force feedback: the drive's steering-load sample (kept while racing a live NGP drive above
    /// `load::MIN_SPEED`, not paused, not a replay), the drive's end at the stage result, and the wheel's
    /// force pointed at the car's sensitivity now on the drive's surface, from the one NGP's car started with
    /// (`ffb::reshape`).
    pub fn ffb_frame(&mut self, dt: f32) {
        let Some(process) = &self.scene.live else { return };
        let Some(surface) = process.hud.ffb_surface.map(|s| s.min(2) as usize) else { return };
        let paused = self.menu.is_some() || self.results.is_some();
        let racing = process.hud.race.is_some_and(|race| race.running);
        let kept = !self.replay && !paused && racing && process.hud.speed > rbr_render::ffb::load::MIN_SPEED;
        let session = &mut self.scene.ffb;
        if let Some(force) = process.hud.force.filter(|_| kept) {
            session.frame(surface, force, dt, true);
        }
        if self.results.is_some() || process.splits.as_ref().is_some_and(|s| s.over) {
            session.finish();
        }
        if let Some(wheel) = &self.wheel {
            wheel.set_sensitivity(session.started[surface], session.now()[surface]);
        }
    }

    /// On the HUD page the pointer takes the part under it (`pressed`), or lets go of the one it held, which keeps
    /// its place; whether it holds one.
    pub fn hud_hold(&mut self, pressed: bool) -> bool {
        if !pressed {
            if self.grab.take().is_some() {
                self.hud_changed();
            }
            return false;
        }
        let (pointer, hud) = (self.results_pointer(), self.running.overlay.hud());
        self.grab = match (self.menu.as_mut().and_then(PauseMenu::hud_mut), pointer) {
            (Some(page), Some((screen, pointer))) => page.grab(hud, screen, pointer),
            _ => None,
        };
        self.grab.is_some()
    }

    /// The held part follows the pointer.
    pub fn hud_drag(&mut self) {
        let (grab, pointer) = (self.grab, self.results_pointer());
        let page = self.menu.as_mut().and_then(PauseMenu::hud_mut);
        if let (Some(grab), Some(page), Some((screen, pointer))) = (grab, page, pointer) {
            page.drag(&grab, screen, pointer);
        }
    }

    /// Shows the parts, places and size the HUD page set, and writes them to the file as the launcher's keys, which
    /// it passes the next drive.
    fn hud_changed(&mut self) {
        let Some(page) = self.menu.as_ref().and_then(PauseMenu::hud) else {
            return;
        };
        let (hud, cameras) = (&mut self.scene.hud, &self.scene.cameras);
        if page.parts != hud.parts {
            hud.parts = page.parts;
            cameras.save_setting("hud_hide", &page.parts.hidden().unwrap_or_default());
            cameras.save_setting("hud_show", &page.parts.shown().unwrap_or_default());
        }
        if page.layout != hud.layout {
            hud.layout = page.layout;
            cameras.save_setting("hud_scale", &page.layout.scale.to_string());
            cameras.save_setting("hud_layout", &page.layout.list().unwrap_or_default());
        }
    }

    /// The fields of view drawn now: the fixed views' and the chase's.
    fn drawn_fov(&self) -> Fov {
        let chase = self.scene.playback.as_ref().map_or(Fov::default().chase, |playback| playback.chase_fov);
        Fov { views: self.fov, chase }
    }

    /// Shows the view, the car's nudges, the head and the fields of view the Camera page set, and writes them to the
    /// file.
    fn camera_changed(&mut self) {
        let Some(page) = self.menu.as_ref().and_then(PauseMenu::camera) else {
            return;
        };
        let (now, fov) = (self.drawn_fov(), page.fov);
        if fov.views != now.views {
            self.fov = fov.views;
            self.scene.cameras.save_fov("fov", fov.views);
        }
        if let Some(playback) = self.scene.playback.as_mut().filter(|_| fov.chase != now.chase) {
            playback.chase_fov = fov.chase;
            self.scene.cameras.save_fov("chase_fov", fov.chase);
        }
        self.view = page.view;
        self.scene.cameras.set(page.view, page.nudges, page.cockpit);
        self.scene.cameras.save();
        if let (Some(mounts), Some(playback)) = (&mut self.scene.mounts, &mut self.scene.playback) {
            self.scene.cameras.apply(mounts, playback);
        }
    }
}

/// The advanced H-shifter of a live drive, when the bindings turn it on and the car's gearbox (`--car-gearbox`) is
/// not sequential (a synchro box without one).
pub fn shifter(args: &Args, live: bool) -> Option<Shifter> {
    let settings = args.bindings.shifter;
    let gearbox = args.car_gearbox.unwrap_or_default();
    (live && settings.advanced && !gearbox.sequential).then(|| {
        eprintln!("viewer: advanced H-shifter, {}", gearbox.arg());
        Shifter::new(gearbox, settings.clutch_threshold, args.bindings.neutral_on_release())
    })
}

/// Sends `gear` with the next step (`CDrive::M6840`).
pub fn select_gear(controls: &mut Controls, gear: u8) {
    controls.flags |= live::SELECT_GEAR;
    controls.gear = gear;
}

/// The movement the held keys ask for, in the camera's axes.
pub fn movement(held: &HashSet<KeyCode>) -> rbr_render::camera::Movement {
    let axis = |plus: KeyCode, minus: KeyCode| (held.contains(&plus) as i32 - held.contains(&minus) as i32) as f32;
    rbr_render::camera::Movement {
        forward: axis(KeyCode::KeyW, KeyCode::KeyS),
        right: axis(KeyCode::KeyD, KeyCode::KeyA),
        up: axis(KeyCode::Space, KeyCode::KeyC),
    }
}

/// The live process started over, with its first frame (a replay's too: `Live::restart` serves its first frame
/// again).
fn restart(process: &mut Live) -> Result<Drive, String> {
    let first = process.restart()?;
    Drive::parse_with(&first, process.clock)
}
