//! Each frame of the drive or the free-fly camera ([`App::frame`]).

use super::feed::Feed;
use super::input::{movement, select_gear};
use crate::draw::{
    demo_hint, dial_values, draw_car, draw_demo, draw_particles, readout, scripted_camera, step_characters,
};
use crate::scene::take_damage;
use crate::shown::{self, Feedback};
use gilrs::ev::filter::Filter;
use rbr_recording::pose::CarPose;
use rbr_render::bindings::{self, Action};
use rbr_render::hud::pause::{self, Nav};
use rbr_render::hud::results::{self as result_actions};
use rbr_render::hud::school;
use rbr_render::hud::{self as hud_overlay};
use rbr_render::live::{self};
use rbr_render::perf_log;
use rbr_render::present::Acquired;
use rbr_render::shifter;
use rbr_render::views::Shown;
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

use super::{App, DemoPlay, FAST_FACTOR, HIDDEN_FRAME, MAX_FRAME_SECONDS, PAD_REPEAT, WALK_SPEED, ZOOM_RATE};

impl App {
    /// Whether the lesson stands on the brief (the SCH1 record's brief state, the countdown waiting for the
    /// confirm): the school demo plays then.
    fn school_brief(&self) -> bool {
        self.scene
            .live
            .as_ref()
            .is_some_and(|live| live.hud_model.view().school.is_some_and(|school| school.phase == school::Phase::Brief))
    }

    /// Plays the school demo on while the brief shows (viewer-only): the replay's time runs with the frame, and at
    /// its end the scene returns to the player's car at the start and the brief shows. A ride sends the brief's
    /// confirm at its end, so the lesson ends with its award and the verdict panel. Once the drive starts the demo
    /// is gone; a restart brings the brief, and the demo, back.
    fn advance_demo(&mut self, dt: f32) {
        let Some(demo) = &self.scene.school_demo else {
            self.demo = None;
            return;
        };
        if !self.school_brief() {
            self.demo = None;
            return;
        }
        let fresh = self.demo.is_none();
        let play = self.demo.get_or_insert(DemoPlay { t: 0.0, finished: false });
        if fresh {
            self.running.renderer.reset_history();
        }
        if play.finished {
            return;
        }
        play.t += dt;
        if play.t < demo.duration() {
            return;
        }
        play.finished = true;
        self.running.renderer.reset_history();
        if demo.ride() {
            // The brief's confirm ends the ride with its award.
            self.controls.flags |= live::ACCEPT_MESSAGE;
        }
    }

    /// The demo car playing now: its pose and whether it is the ride.
    fn demo_car(&self) -> Option<(CarPose, bool)> {
        let (Some(play), Some(demo)) = (&self.demo, &self.scene.school_demo) else { return None };
        if play.finished || !self.school_brief() {
            return None;
        }
        let pose = demo.pose(play.t)?;
        Some((CarPose { position: pose.position, orientation: pose.rotation, velocity: [0.0; 3] }, demo.ride()))
    }

    /// Skips the school demo playing (Enter): the brief shows instead, and a ride sends its confirm, ending the
    /// lesson. Whether one played (consumed: the Enter must not reach the standalone as the brief's confirm).
    pub(super) fn skip_demo(&mut self) -> bool {
        if !self.school_brief() {
            return false;
        }
        let (Some(play), Some(demo)) = (&mut self.demo, &self.scene.school_demo) else { return false };
        if play.finished {
            return false;
        }
        let ride = demo.ride();
        play.finished = true;
        self.running.renderer.reset_history();
        if ride {
            self.controls.flags |= live::ACCEPT_MESSAGE;
        }
        true
    }

    pub(super) fn frame(&mut self) {
        perf_log::frame();
        let mut span = perf_log::span("acquire surface");
        // The surface's texture first: acquiring it is where the loop waits (for a free image, the display's
        // refresh under fifo), so the input read and the live answers after it are the newest the frame can show.
        let acquire = Instant::now();
        let running = &mut self.running;
        if running.stale {
            running.stale = !rbr_render::present::configure(&running.surface, &running.gpu.device, &running.config);
            if running.stale {
                return;
            }
        }
        let frame = match rbr_render::present::acquire(&running.surface) {
            Acquired::Frame(frame) => Some(frame),
            // Hidden, the frame runs all the same (the input, the live process's steps, the sound) without being
            // drawn, at most once a `HIDDEN_FRAME`: nothing waits for a free image then.
            Acquired::Hidden => {
                if let Some(wait) = (self.last_frame + HIDDEN_FRAME).checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
                None
            }
            Acquired::Failed => {
                running.stale = !rbr_render::present::configure(&running.surface, &running.gpu.device, &running.config);
                return;
            }
        };
        let now = Instant::now();
        if let Some(latency) = &mut self.latency {
            latency.acquired(acquire, now);
        }
        let dt = (now - self.last_frame).as_secs_f32().min(MAX_FRAME_SECONDS);
        self.last_frame = now;
        span.next("input");
        if let Some(gilrs) = &mut self.gilrs {
            let mut navs = Vec::new();
            while let Some(event) = gilrs.next_event().filter_ev(&PAD_REPEAT, gilrs) {
                match event.event {
                    gilrs::EventType::ButtonPressed(button, _) => navs.extend(Nav::of_button(button)),
                    // A held d-pad repeats its move, as a held key does (the filter's events are not in the state).
                    gilrs::EventType::ButtonRepeated(button, _) => {
                        gilrs.update(&event);
                        navs.extend(Nav::of_button(button).filter(|nav| !matches!(nav, Nav::Select | Nav::Back)));
                    }
                    _ => {}
                }
            }
            if self.menu.is_some() {
                navs.into_iter().for_each(|nav| self.menu_nav(nav));
            } else if self.results.is_some() {
                navs.into_iter().for_each(|nav| self.results_nav(nav));
            } else if navs.contains(&Nav::Select) {
                self.accept_message();
            }
        }
        // The devices' presses, as the input thread last polled them.
        let polled = self.feed.as_ref().map(Feed::polled);
        if let Some((pads, _)) = polled {
            let before = std::mem::replace(&mut self.pads, pads);
            for action in bindings::pad_presses(before, pads) {
                self.press(action);
            }
        }
        if !self.replay {
            self.sync_results();
        }
        self.ffb_frame(dt);
        if self.menu.is_none() {
            self.advance_demo(dt);
        }
        let demo = self.demo_car();
        let demo_ride = demo.as_ref().map(|(_, ride)| *ride);
        let running = &mut self.running;
        let speed = WALK_SPEED * if self.held.contains(&KeyCode::ShiftLeft) { FAST_FACTOR } else { 1.0 };
        let movement = movement(&self.held);
        let (width, height) = (running.config.width, running.config.height);
        let aspect = width as f32 / height as f32;
        let mut live_failed = false;
        if self.menu.is_some() {
            // Paused: the stepping process's clock is stopped, so its game, clocks and car stand still.
        } else if let (Some(process), Some(playback)) = (&mut self.scene.live, &mut self.scene.playback) {
            let held = Action::ALL.map(|a| self.bindings.get(a).key.is_some_and(|k| self.held.contains(&k)));
            // The input thread polls the devices (and `RBR_LATENCY`'s script) over these: the frame gives the flags
            // and the gear it selects.
            if let Some(shifter) = &mut self.shifter {
                // The advanced H-shifter selects itself: NGP's poll sees no direct gear actions.
                let polled = polled.map_or(self.controls, |(_, c)| c);
                let held = |a: Action| held[a as usize] || self.pads[a as usize] > bindings::PRESS;
                let action = |word| Action::ALL.into_iter().find(|a| a.gear() == Some(word)).is_some_and(held);
                let inputs = shifter::Inputs {
                    lever: if held(Action::Neutral) { Some(shifter::NEUTRAL) } else { shifter::lever(action) },
                    engaged: u8::try_from(process.hud.gear).unwrap_or(shifter::NEUTRAL),
                    clutch: polled.clutch,
                    throttle: polled.throttle,
                    engine_speed: process.hud.engine_speed,
                    gearbox: process.hud.gearbox,
                };
                if let Some(gear) = shifter.update((now - self.started).as_secs_f32(), inputs) {
                    select_gear(&mut self.controls, gear);
                }
                if let Some(sound) = &mut self.sound {
                    sound.grind(shifter.grind());
                }
            }
            if let Some(feed) = &self.feed {
                feed.post(&mut self.controls, &self.held, self.bindings.shifter.advanced);
            }
            span.next("live link");
            let pumped = process.pump();
            if let (Some(latency), Some(feed)) = (&mut self.latency, &self.feed) {
                latency.input(feed.take_sends(), process.take_steps(), process.newest_step());
            }
            match pumped {
                Ok(frames) => {
                    for frame in frames {
                        match process.parse(&frame) {
                            Ok(drive) => playback.push(drive),
                            Err(e) => eprintln!("viewer: live frame: {e}"),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("viewer: {e}");
                    if self.replay {
                        self.replay_failure = Some(e);
                        self.exit = true;
                    }
                    live_failed = true;
                }
            }
            // The car drawn at the physics' newest step, between the answers (`playback::lead`).
            playback.lead_to(process.newest_step().filter(|_| !self.replay).map(|step| step.step));
            span.next("live state and sound");
            running.renderer.set_interactive_poses(&process.interactive_poses);
            take_damage(playback, process);
            if !self.replay {
                if let Some(ghost) = &mut self.scene.ghost {
                    let penalties = process.hud.race.map_or(0.0, |race| race.penalties);
                    ghost.observe(process.splits.as_ref(), penalties, playback.sample().step);
                }
            }
            if self.went.is_none() && process.hud.race.is_some_and(|race| race.countdown <= 0.0) {
                self.went = Some(now);
                perf_log::mark("go");
                if let Some(latency) = &mut self.latency {
                    latency.go();
                }
            }
            if process.splits.as_ref().is_some_and(|s| s.over) {
                self.ended.get_or_insert(now);
            }
            playback.reverse = process.hud.gear == 0;
            if let Some(lamps) = process.lamps {
                playback.set_lamps(lamps);
            }
            // The race flow's retire codes 1 and 2 (and a rally incident with the gate shut) call the helicopter.
            if process.take_rescue_call() {
                if let Some(characters) = &mut self.scene.characters {
                    characters.call_rescue();
                }
            }
            let sounds = process.take_sounds();
            // The rally school's speech, from the live record's school state.
            let (school_stop, school_ids) = process.take_school_speech();
            let school = process.hud_view().school;
            if let Some(sound) = &mut self.sound {
                if let Some(mounts) = &self.scene.mounts {
                    let lens = self.shown.lens(playback, mounts, None, self.look, self.fov);
                    let interior = self.shown.car_view().interior();
                    sound.set_scene(rbr_render::sound::scene(
                        &lens,
                        &playback.sample().car,
                        interior,
                        self.replay,
                        self.scene.roadside.as_ref().filter(|_| self.scene.echoes),
                    ));
                }
                sound.set_water(self.scene.particles.as_ref().map(|p| rbr_render::sound::water(&p.particles)));
                sound.set_glass(self.scene.glass.as_mut().map(rbr_render::sound::glass));
                let particles = self.scene.particles.as_ref().map(|p| &p.particles);
                let car = &playback.sample().car;
                sound.set_verge(rbr_render::sound::verge(particles, self.scene.roadside.as_ref(), car));
                sounds.iter().for_each(|s| sound.step(s));
                self.school_frame = self.school_frame.wrapping_add(1);
                let frame = self.school_frame;
                self.school_speech.feed(sound, school.as_ref(), school_stop, &school_ids, frame);
            }
        } else if let Some(playback) = &mut self.scene.playback {
            playback.advance(dt as f64);
        }
        span.next("scene update");
        if live_failed {
            self.scene.live = None;
            // Dropping the wheel stops its force.
            self.wheel = None;
        }
        if self.menu.is_none() {
            self.clock += f64::from(dt);
        }
        if let Some(characters) = &mut self.scene.characters {
            let race = self.scene.live.as_ref().and_then(|l| l.hud.race);
            let events = step_characters(characters, self.scene.playback.as_ref(), self.clock, race);
            if let Some(sound) = &mut self.sound {
                sound.events(&rbr_render::sound::events(&events, characters.rescue_position()));
            }
        }
        // The game's start orbit replaces the player's view until the go, and the helicam while the helicopter runs.
        let camera_record = self.scene.live.as_ref().and_then(|live| live.hud.camera);
        let (heli, flip, _) = scripted_camera(self.scene.characters.as_ref());
        let before = self.shown;
        self.shown = Shown::of(self.view, camera_record, heli, flip);
        // The Camera page shows the view it sets, also over the start orbit and the helicam.
        if self.menu.as_ref().is_some_and(|menu| menu.camera().is_some()) {
            self.shown = Shown::Player(self.view);
        }
        // A cut to another camera: the temporal anti-aliasing's history is of the last one's view.
        if !before.same_camera(self.shown) {
            running.renderer.reset_history();
        }
        let looking_back = self.bindings.get(Action::LookBack).key.is_some_and(|k| self.held.contains(&k))
            || self.pads[Action::LookBack as usize] > bindings::PRESS;
        if let Some(playback) = &mut self.scene.playback {
            playback.follow_cameras(self.shown.swing(), looking_back);
        }
        let fov = self.fov;
        let car_lens = match &demo {
            // The school demo: the player's car at the replay's pose, the player's camera replaced.
            Some((car, ride)) => draw_demo(&mut self.scene, &mut running.renderer, *ride, car, self.look, fov),
            None => draw_car(&mut self.scene, &mut running.renderer, self.shown, self.look, fov),
        };
        let (view_projection, sky_view_projection) = match (&mut self.scene.orbit, car_lens) {
            (Some(orbit), _) => {
                orbit.zoom((movement.forward * -ZOOM_RATE * dt).exp());
                (orbit.view_projection(fov, aspect), orbit.sky_view_projection(fov, aspect))
            }
            (None, Some(lens)) => (lens.view_projection(aspect), lens.sky_view_projection(aspect)),
            (None, None) => {
                if self.scene.playback.is_none() {
                    running.camera.step(movement, speed * dt);
                }
                (running.camera.view_projection(fov, aspect), running.camera.sky_view_projection(fov, aspect))
            }
        };
        if let Some(characters) = &mut self.scene.characters {
            characters.apply(&mut running.renderer, view_projection);
        }
        if let (Some(ghost), Some(playback)) = (&mut self.scene.ghost, &self.scene.playback) {
            let hud = rbr_render::ghost::hud_time(self.scene.live.as_ref());
            ghost.apply(&mut running.renderer, playback, hud, view_projection);
        }
        draw_particles(&mut self.scene, &mut running.renderer, view_projection);
        if let (Some(drawn), Some(playback)) = (&mut self.scene.fences, &self.scene.playback) {
            drawn.draw(&mut running.renderer, playback, view_projection);
        }
        let Some(frame) = frame else {
            running.renderer.skip_frame();
            return;
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = running.gpu.device.create_command_encoder(&Default::default());
        let paused = self.paused + self.paused_at.map_or(Duration::ZERO, |at| at.elapsed());
        let elapsed = self.started.elapsed() - paused;
        running.renderer.set_time(elapsed.as_secs_f32());
        // The headroom moves with the brightness and the display the window is on (on the main thread, as macOS asks).
        if running.config.color_space.is_hdr() {
            let headroom = running.surface.display_hdr_info(&running.gpu.adapter).tone_map_headroom();
            running.renderer.set_headroom(headroom.unwrap_or(1.0));
        }
        // The school demo shows no HUD of the player's car (start lamps, dials, times), only its hint.
        let live = self.scene.live.as_ref().filter(|_| demo_ride.is_none());
        running.renderer.set_start_clock(live.and_then(|l| l.hud_model.view().start_clock));
        running.renderer.set_start_lamps(live.and_then(|l| l.hud.race.as_ref()));
        if let (Some(dials), Some(playback)) = (&mut self.scene.dials, &self.scene.playback) {
            dials.update(&dial_values(playback, self.scene.live.as_ref()), Some(dt), &mut running.renderer);
        }
        span.next("render");
        running.renderer.render(&mut encoder, &view, (width, height), view_projection, sky_view_projection);
        span.next("HUD");
        let readout =
            self.scene.playback.as_ref().filter(|_| self.scene.hud.show && demo_ride.is_none()).map(|playback| {
                let fps = if self.scene.hud.fps { self.meter.update(dt) } else { None };
                let (live, shown, bindings) = (self.scene.live.as_ref(), self.shown, &self.bindings);
                readout(playback, live, shown, bindings, self.scene.hud, fps)
            });
        if readout.is_some() || self.menu.is_some() || self.results.is_some() || demo_ride.is_some() {
            let (gpu, menu) = (&running.gpu, &self.menu);
            running.overlay.draw_with(&gpu.device, &gpu.queue, &mut encoder, &view, (width, height), |ctx, hud| {
                readout.iter().for_each(|r| hud_overlay::paint_readout(ctx, hud, r));
                menu.iter().for_each(|m| pause::paint(ctx, m));
                self.results.iter().for_each(|m| result_actions::paint(ctx, m));
                if let Some(ride) = demo_ride {
                    demo_hint(ctx, ride);
                }
            });
        }
        span.next("submit");
        running.gpu.queue.submit([encoder.finish()]);
        let submitted = Instant::now();
        if let Some(pacer) = &mut self.pacer {
            pacer.submitted(&running.gpu.queue, self.presented, self.last_frame, submitted);
        }
        span.next("GPU timing readback");
        running.renderer.end_frame();
        span.next("present");
        if let Some(feedback) = &mut self.feedback {
            feedback.ask(self.presented);
        }
        running.window.pre_present_notify();
        running.gpu.queue.present(frame);
        let feedback = self.feedback.as_mut().map(shown::Shown::take).unwrap_or_default();
        if let Some(pacer) = &mut self.pacer {
            for &(frame, shown) in &feedback {
                if let Feedback::Shown { at, .. } = shown {
                    pacer.shown(frame, at);
                }
            }
        }
        if let Some(latency) = &mut self.latency {
            let step = self.scene.playback.as_ref().map(|p| p.shown_step());
            if latency.drawn(submitted, step, &feedback) {
                latency.watch_gpu(&running.gpu.device, &running.gpu.queue);
            }
        }
        self.presented += 1;
    }

    /// Writes the live drive's ghost (`--save-ghost`); a replay saves none.
    pub(super) fn finish_ghost(&self) {
        if self.replay {
            return;
        }
        if let (Some(ghost), Some(playback)) = (&self.scene.ghost, &self.scene.playback) {
            ghost.finish(playback);
        }
    }
}
