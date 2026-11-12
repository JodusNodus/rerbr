//! The joysticks the live drive polls (`crate::bindings::Device`), each giving the device state the game's data format
//! would (`rbr_gameplay::poll::wine::objects`), and the poll over our bindings ([`Input`]).
//!
//! On Linux the devices are read from evdev, as Proton's winebus reads them through SDL's Linux joystick for the game
//! (`rbr_gameplay::poll::wine`): a device's index counts the joystick nodes with its vendor and product ids
//! (ABS_X and ABS_Y, or a joystick button, as SDL and udev take a node for one), in `/dev/input/event*` order.
//! Elsewhere they come from SDL3's joysticks (`sdl` below; on macOS its HIDAPI, Game Controller and IOKit drivers):
//! the axes and buttons in its order (unverified on macOS and Windows).

use rbr_gameplay::control::NGP_GEAR_PRESS;
use rbr_gameplay::poll::{keyboard, Device as PollDevice, Poll, DEFAULT_RANGE};
use rbr_recording::input::CELL_BUTTON;

use crate::bindings::{dik, Action, Bindings, Device, ACTIONS};
use crate::live::Controls;
use rbr_formats::saves::action_map::Object as Target;

/// The joysticks: each opened one, its state as last read, and whether its last read failed.
pub struct Joysticks {
    list: Vec<(Device, imp::Joystick)>,
    /// Per joystick: the read failed (unplugged); the state stays the last one read, as the game keeps a device's
    /// entries when `GetDeviceState` fails, until [`Joysticks::reopen`] finds it again.
    pub lost: Vec<bool>,
    context: imp::Context,
}

impl Joysticks {
    /// The joysticks `want` names, as the game sees them, and why each one is not open.
    pub fn open(want: &[Device]) -> (Joysticks, Vec<String>) {
        Joysticks::open_in(imp::Context::new(), want)
    }

    fn open_in(mut context: imp::Context, want: &[Device]) -> (Joysticks, Vec<String>) {
        let mut found = imp::Joystick::all(&mut context, false);
        let mut missing = Vec::new();
        let list: Vec<_> = want
            .iter()
            .filter_map(|d| match found.iter().position(|(f, _)| f == d) {
                Some(k) => Some(found.swap_remove(k)),
                None => {
                    missing.push(format!("{}: not connected (or not readable)", d.format()));
                    None
                }
            })
            .collect();
        let lost = vec![false; list.len()];
        (Joysticks { list, lost, context }, missing)
    }

    /// Every joystick connected, its axes at their values from the start (not the game's centre until they move):
    /// the launcher's capture and its bars.
    pub fn connected() -> Joysticks {
        let mut context = imp::Context::new();
        let list = imp::Joystick::all(&mut context, true);
        Joysticks { lost: vec![false; list.len()], list, context }
    }

    /// Looks for joysticks plugged in or out, in the same context (off Linux SDL stays started, on the thread that
    /// hears of them on macOS: `crate::sdl`). The list, with its states, is kept while the same devices are connected
    /// and none was lost.
    pub fn rescan(&mut self) {
        self.context.update(&mut self.list);
        let found = imp::Joystick::devices(&mut self.context);
        if self.lost.contains(&true) || !found.iter().eq(self.list.iter().map(|(d, _)| d)) {
            self.list = imp::Joystick::all(&mut self.context, true);
            self.lost = vec![false; self.list.len()];
        }
    }

    /// Whether one of `want` that is not open is connected now (plugged in after the open).
    fn found(&mut self, want: &[Device]) -> bool {
        self.context.update(&mut self.list);
        let open: Vec<_> = self.list.iter().map(|(d, _)| *d).collect();
        imp::Joystick::devices(&mut self.context).iter().any(|d| want.contains(d) && !open.contains(d))
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn device(&self, k: usize) -> Device {
        self.list[k].0
    }

    pub fn name(&self, k: usize) -> &str {
        self.list[k].1.name()
    }

    /// The axes and buttons the game's data format has for joystick `k`.
    pub fn objects(&self, k: usize) -> (u32, u32) {
        let j = &self.list[k].1;
        (j.axes(), j.buttons())
    }

    /// Reads every joystick; a failed read keeps its state and marks it [`Joysticks::lost`].
    pub fn read(&mut self) {
        self.context.update(&mut self.list);
        for ((_, j), lost) in self.list.iter_mut().zip(&mut self.lost) {
            *lost = !j.read(&self.context);
        }
    }

    /// Joystick `k`'s state as last read.
    pub fn state(&self, k: usize) -> &[u8] {
        self.list[k].1.state()
    }

    /// Opens the lost joysticks again if they are back (fresh, as a device plugged in again is).
    pub fn reopen(&mut self) {
        if !self.lost.contains(&true) {
            return;
        }
        let mut found = imp::Joystick::all(&mut self.context, false);
        for ((device, j), lost) in self.list.iter_mut().zip(&mut self.lost) {
            if let Some(k) = found.iter().position(|(f, _)| f == device).filter(|_| *lost) {
                *j = found.swap_remove(k).1;
                *lost = false;
            }
        }
    }
}

/// The live poll: the keyboard and the bound joysticks, bound as our bindings say, through `Tic`.
pub struct Input {
    poll: Poll,
    joysticks: Joysticks,
    /// Each action's device input: its poll device and entry.
    pads: Vec<(Action, usize, usize)>,
    /// The bound devices not open, why.
    pub missing: Vec<String>,
}

impl Input {
    /// The keyboard and the joysticks the bindings name that are connected.
    pub fn open(bindings: &Bindings) -> Input {
        Input::open_in(imp::Context::new(), bindings)
    }

    fn open_in(context: imp::Context, bindings: &Bindings) -> Input {
        let (joysticks, missing) = Joysticks::open_in(context, &bindings.devices());
        let mut devices = vec![keyboard()];
        for k in 0..joysticks.len() {
            let (axes, buttons) = joysticks.objects(k);
            let objects = rbr_gameplay::poll::wine::objects(axes, buttons);
            devices.push(PollDevice::new([0; 16], &objects, |_| DEFAULT_RANGE));
        }
        let index = |d: Option<Device>| match d {
            None => Some(0),
            Some(d) => (0..joysticks.len()).position(|k| joysticks.device(k) == d).map(|k| k + 1),
        };
        let slots: Vec<_> =
            bindings.slots().into_iter().filter_map(|(slot, d, object)| Some((slot, index(d)?, object))).collect();
        let mut poll = Poll::new(devices);
        poll.bind(&slots);
        poll.invert = bindings.invert;
        let pads = bindings
            .pads()
            .filter_map(|(a, p)| {
                let d = index(Some(p.device))?;
                Some((a, d, poll.devices[d].find(p.object)?))
            })
            .collect();
        Input { poll, joysticks, pads, missing }
    }

    /// The joysticks open and not answering, by name.
    pub fn lost(&self) -> Vec<String> {
        let names = self.names();
        names.into_iter().zip(&self.joysticks.lost).filter(|(_, lost)| **lost).map(|(n, _)| n).collect()
    }

    /// The joysticks open, by name.
    pub fn names(&self) -> Vec<String> {
        (0..self.joysticks.len())
            .map(|k| format!("{} ({})", self.joysticks.name(k), self.joysticks.device(k).format()))
            .collect()
    }

    /// One poll, the keyboard's state (`c_dfDIKeyboard`, [`keyboard_state`]) and the joysticks as they are now: the
    /// controls `last` had with the poll's channels, cells, ignition and direct gear actions, and its gear up or down
    /// presses added to the flags; and in `pads` each action's device input now (0..1).
    pub fn controls(&mut self, keys: &[u8; 256], last: Controls, pads: &mut [f32; ACTIONS]) -> Controls {
        self.joysticks.read();
        self.poll.devices[0].update(keys, 0.0);
        for k in 0..self.joysticks.len() {
            // A lost device keeps its entries: `Poll` skips the update on a failed `GetDeviceState`.
            if !self.joysticks.lost[k] {
                self.poll.devices[k + 1].update(self.joysticks.state(k), 0.0);
            }
        }
        *pads = [0.0; ACTIONS];
        for &(a, d, e) in &self.pads {
            let entry = &self.poll.devices[d].entries[e];
            let v = entry.value();
            pads[a as usize] = if entry.is_axis() { (v + 1.0) * 0.5 } else { v };
        }
        let raw = self.poll.tic();
        let gears = self.poll.gear_actions();
        let bits = |test: &dyn Fn(Option<f32>) -> bool| (0..8).filter(|&k| test(gears[k])).fold(0, |b, k| b | 1 << k);
        let [steering, throttle, brake, handbrake, clutch] = raw.channels;
        let cells = raw.select.map(|c| c as u8);
        Controls {
            steering,
            throttle,
            brake,
            handbrake,
            clutch,
            cells: Some(cells),
            keys_steer: cells[0] == CELL_BUTTON,
            ignition: raw.aux != 0,
            flags: last.flags | raw.flags,
            gears_held: bits(&|v| v.is_some_and(|v| v > NGP_GEAR_PRESS)),
            gears_bound: bits(&|v| v.is_some()),
            ..last
        }
    }

    /// Opens everything again, in the same context, when a bound joystick that was not connected is now; whether it
    /// did. The poll starts fresh, as at the open.
    pub fn find_missing(&mut self, bindings: &Bindings) -> bool {
        if self.missing.is_empty() || !self.joysticks.found(&bindings.devices()) {
            return false;
        }
        *self = Input::open_in(std::mem::take(&mut self.joysticks.context), bindings);
        true
    }

    /// Opens the lost joysticks again if they are back (the keepalive's).
    pub fn reopen(&mut self) {
        let lost = self.joysticks.lost.clone();
        self.joysticks.reopen();
        // A device opened again is a fresh one to the game: its axes settle again.
        for (k, was) in lost.iter().enumerate() {
            if *was && !self.joysticks.lost[k] {
                let (axes, buttons) = self.joysticks.objects(k);
                let fresh =
                    PollDevice::new([0; 16], &rbr_gameplay::poll::wine::objects(axes, buttons), |_| DEFAULT_RANGE);
                if fresh.entries.len() == self.poll.devices[k + 1].entries.len() {
                    self.poll.devices[k + 1] = fresh;
                }
            }
        }
    }
}

/// The keyboard's state (`c_dfDIKeyboard`): 0x80 at each held key's DIK code.
pub fn keyboard_state<'a>(held: impl IntoIterator<Item = &'a winit::keyboard::KeyCode>, state: &mut [u8; 256]) {
    state.fill(0);
    for dik in held.into_iter().filter_map(|&k| dik(k)) {
        state[usize::from(dik)] = 0x80;
    }
}

/// A move of a joystick's object from where it rested: an axis's value (-1..1) or a button pressed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moved {
    pub device: Device,
    pub object: Target,
    pub rest: f32,
    pub value: f32,
}

/// The objects' values of a joystick's state, -1..1 per axis (dinput's 0..65535), then 0 or 1 per button.
pub fn values(state: &[u8], axes: u32, buttons: u32) -> impl Iterator<Item = (Target, f32)> + '_ {
    let axis = |k: u32| {
        let at = 4 * k as usize;
        let raw = state.get(at..at + 4).map_or(32768, |b| i32::from_le_bytes(b.try_into().unwrap()));
        (raw as f32 / 65535.0) * 2.0 - 1.0
    };
    let button = move |k: u32| if state.get((4 * axes + k) as usize).is_some_and(|&b| b != 0) { 1.0 } else { 0.0 };
    (0..axes).map(move |k| (Target::Axis(k), axis(k))).chain((0..buttons).map(move |k| (Target::Button(k), button(k))))
}

#[cfg(target_os = "linux")]
mod imp {
    pub use super::evdev::Joystick;

    /// evdev needs no context: each joystick is its file.
    #[derive(Default)]
    pub struct Context;

    impl Context {
        pub fn new() -> Context {
            Context
        }

        pub fn update(&mut self, _: &mut [(crate::bindings::Device, Joystick)]) {}
    }
}

/// SDL2's Linux joystick (`SDL_sysjoystick.c`, `ConfigJoystick` and `AxisCorrect`) over an evdev device, and
/// winebus's report of it: an axis reads winebus's 0 (dinput 32768) until it first changes, and the whole device
/// dinput's centre until winebus has a report (the T500 RS's untouched axes: 32767, then 32768).
#[cfg(target_os = "linux")]
mod evdev {
    use std::fs::File;
    use std::os::fd::AsRawFd;

    use crate::bindings::Device;
    use rbr_gameplay::poll::wine;

    const ABS_MAX: u32 = 0x3f;
    const KEY_MAX: u32 = 0x2ff;
    const BTN_JOYSTICK: u32 = 0x120;
    const BTN_DIGI: u32 = 0x140;
    const BTN_TRIGGER_HAPPY: u32 = 0x2c0;
    const ABS_X: u32 = 0x00;
    const ABS_Y: u32 = 0x01;
    const ABS_HAT0X: u32 = 0x10;
    const ABS_HAT3Y: u32 = 0x17;
    const EV_KEY: u32 = 1;
    const EV_ABS: u32 = 3;
    const KEY_BYTES: usize = (KEY_MAX as usize + 1) / 8;

    /// `struct input_absinfo`.
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct AbsInfo {
        value: i32,
        minimum: i32,
        maximum: i32,
        fuzz: i32,
        flat: i32,
        resolution: i32,
    }

    /// `_IOC(_IOC_READ, 'E', nr, size)`.
    fn read_request(nr: u32, size: usize) -> libc::c_ulong {
        (2 << 30 | (size as libc::c_ulong) << 16 | (b'E' as libc::c_ulong) << 8 | nr as libc::c_ulong) as _
    }

    fn ioctl<T>(file: &File, nr: u32, out: &mut T) -> bool {
        // SAFETY: the request's size is `T`'s, and `out` is a valid `T` the kernel writes at most that much into.
        unsafe { libc::ioctl(file.as_raw_fd(), read_request(nr, size_of::<T>()) as _, out as *mut T) >= 0 }
    }

    fn bit(bits: &[u8], k: u32) -> bool {
        bits.get((k / 8) as usize).is_some_and(|b| b >> (k % 8) & 1 != 0)
    }

    fn abs(file: &File, code: u32) -> Option<AbsInfo> {
        let mut info = AbsInfo::default();
        ioctl(file, 0x40 + code, &mut info).then_some(info)
    }

    pub struct Joystick {
        file: File,
        name: String,
        /// Per axis: its code, range, value when opened and whether it changed since.
        axes: Vec<(u32, i32, i32, i32, bool)>,
        keys: Vec<u32>,
        reported: bool,
        /// The state as last read ([`wine::objects`]'s layout).
        state: Vec<u8>,
    }

    impl Joystick {
        /// The connected joysticks' ids.
        pub fn devices(context: &mut super::imp::Context) -> Vec<Device> {
            Joystick::all(context, false).into_iter().map(|(device, _)| device).collect()
        }

        /// Every joystick node (in `/dev/input/event*` order) that can be opened, with its device id; with `actual`
        /// its axes read their values at once.
        pub fn all(_: &mut super::imp::Context, actual: bool) -> Vec<(Device, Joystick)> {
            let Ok(dir) = std::fs::read_dir("/dev/input") else { return Vec::new() };
            let mut paths: Vec<_> = dir
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")))
                .collect();
            paths.sort_by_key(|p| p.to_str().and_then(|n| n.rsplit("event").next()?.parse::<u32>().ok()));
            let mut out: Vec<(Device, Joystick)> = Vec::new();
            for file in paths.iter().filter_map(|p| File::open(p).ok()) {
                let mut id = [0u16; 4];
                if !ioctl(&file, 0x02, &mut id) {
                    continue;
                }
                let (mut absbits, mut keybits) = ([0u8; (ABS_MAX as usize + 1) / 8], [0u8; KEY_BYTES]);
                ioctl(&file, 0x20 + EV_ABS, &mut absbits);
                ioctl(&file, 0x20 + EV_KEY, &mut keybits);
                let buttons = (BTN_JOYSTICK..BTN_DIGI).chain(BTN_TRIGGER_HAPPY..BTN_TRIGGER_HAPPY + 40);
                let joystick =
                    bit(&absbits, ABS_X) && bit(&absbits, ABS_Y) || buttons.into_iter().any(|k| bit(&keybits, k));
                if !joystick {
                    continue;
                }
                let index = out.iter().filter(|(d, _)| (d.vendor, d.product) == (id[1], id[2])).count() as u32;
                let joystick = Joystick::new(file, &absbits, &keybits, actual);
                out.push((Device { vendor: id[1], product: id[2], index }, joystick));
            }
            out
        }

        fn new(file: File, absbits: &[u8], keybits: &[u8], actual: bool) -> Joystick {
            let mut name = [0u8; 256];
            ioctl(&file, 0x06, &mut name);
            let name = String::from_utf8_lossy(name.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
            let keys: Vec<u32> = (BTN_JOYSTICK..KEY_MAX).chain(0..BTN_JOYSTICK).filter(|&k| bit(keybits, k)).collect();
            // A pair of hat codes SDL takes for a digital hat (`GuessIfAxesAreDigitalHat`) is not an axis.
            let hat = |pair: u32| {
                let info = |k: u32| bit(absbits, k).then(|| abs(&file, k)).flatten();
                let (x, y) = (info(pair), info(pair + 1));
                let digital = |i: Option<AbsInfo>| i.is_none_or(|i| i.minimum == -1 && i.maximum == 1);
                let quiet = |i: Option<AbsInfo>| i.is_none_or(|i| i.fuzz == 0 && i.flat == 0 && i.resolution == 0);
                (x.is_some() || y.is_some()) && (digital(x) && digital(y) || quiet(x) && quiet(y))
            };
            let axes: Vec<_> = (0..ABS_MAX)
                .filter(|&k| bit(absbits, k))
                .filter(|&k| !((ABS_HAT0X..=ABS_HAT3Y).contains(&k) && hat(ABS_HAT0X + (k - ABS_HAT0X) / 2 * 2)))
                .filter_map(|k| abs(&file, k).map(|i| (k, i.minimum, i.maximum, i.value, actual)))
                .collect();
            let state = wine::state(&vec![CENTRE; axes.len()], &vec![false; keys.len()]);
            let mut joystick = Joystick { file, name, axes, keys, reported: false, state };
            // SDL sends the buttons held when it opens the device; the axes not.
            let mut keybits = [0u8; KEY_BYTES];
            joystick.reported =
                actual || ioctl(&joystick.file, 0x18, &mut keybits) && joystick.keys.iter().any(|&k| bit(&keybits, k));
            joystick
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn axes(&self) -> u32 {
            self.axes.len() as u32
        }

        pub fn buttons(&self) -> u32 {
            self.keys.len() as u32
        }

        pub fn state(&self) -> &[u8] {
            &self.state
        }

        /// Reads the state the game's data format gives now ([`wine::objects`]); `false`, the state kept, when the
        /// device does not answer (unplugged).
        pub fn read(&mut self, _: &super::imp::Context) -> bool {
            let mut keybits = [0u8; KEY_BYTES];
            if !ioctl(&self.file, 0x18, &mut keybits) {
                return false;
            }
            let mut values = [0i32; ABS_MAX as usize];
            for (k, (code, ..)) in self.axes.iter().enumerate() {
                match abs(&self.file, *code) {
                    Some(info) => values[k] = info.value,
                    None => return false,
                }
            }
            let axes = self.axes.len();
            for (k, (_, min, max, opened, moved)) in self.axes.iter_mut().enumerate() {
                *moved |= values[k] != *opened;
                self.reported |= *moved;
                let v = if *moved { wine::axis(wine::sdl_axis(values[k], *min, *max)) } else { wine::axis(0) };
                self.state[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
            }
            for (k, &code) in self.keys.iter().enumerate() {
                let down = bit(&keybits, code);
                self.reported |= down;
                self.state[4 * axes + k] = if down { 0x80 } else { 0 };
            }
            if !self.reported {
                for k in 0..axes {
                    self.state[4 * k..4 * k + 4].copy_from_slice(&CENTRE.to_le_bytes());
                }
            }
            true
        }
    }

    /// dinput's axis value before the device's first report: the range's centre.
    const CENTRE: i32 = 32767;
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub use super::sdl::{Context, Joystick};
}

/// The joysticks through SDL3 (off Linux; on Linux for the test against [`evdev`]): its axes and buttons in its order,
/// its hats left out as on Linux, an axis's -32768..32767 as dinput's 0..65535 ([`wine::axis`]). The axes read what
/// SDL has, `actual` or not: Windows' dinput has no report to wait for.
#[cfg(any(not(target_os = "linux"), test))]
mod sdl {
    use std::ffi::CStr;

    use rbr_gameplay::poll::wine;
    use sdl3_sys::init::SDL_INIT_JOYSTICK;
    use sdl3_sys::joystick::*;

    use crate::bindings::Device;
    use crate::sdl::Subsystem;

    /// SDL's joystick subsystem, while the joysticks are open; none when it did not start.
    #[derive(Default)]
    pub struct Context(Option<Subsystem>);

    impl Context {
        pub fn new() -> Context {
            Context(Subsystem::init(SDL_INIT_JOYSTICK).map_err(|e| eprintln!("devices: no joysticks: {e}")).ok())
        }

        /// SDL reads its devices and looks for new ones; each joystick takes its state at its [`Joystick::read`].
        pub fn update(&mut self, _: &mut [(Device, Joystick)]) {
            if self.0.is_some() {
                // SAFETY: the subsystem is started.
                unsafe { SDL_UpdateJoysticks() };
            }
        }

        /// SDL's joysticks with their ids (an index per vendor and product, in SDL's order).
        fn ids(&self) -> Vec<(Device, SDL_JoystickID)> {
            let mut out: Vec<(Device, SDL_JoystickID)> = Vec::new();
            if self.0.is_none() {
                return out;
            }
            // SAFETY: `ids` is SDL's array of `count` ids, freed here.
            unsafe {
                let mut count = 0;
                let ids = SDL_GetJoysticks(&mut count);
                if ids.is_null() {
                    return out;
                }
                for &id in std::slice::from_raw_parts(ids, usize::try_from(count).unwrap_or(0)) {
                    let (vendor, product) = (SDL_GetJoystickVendorForID(id), SDL_GetJoystickProductForID(id));
                    let index = out.iter().filter(|(d, _)| (d.vendor, d.product) == (vendor, product)).count() as u32;
                    out.push((Device { vendor, product, index }, id));
                }
                sdl3_sys::stdinc::SDL_free(ids.cast());
            }
            out
        }
    }

    /// An open SDL joystick, closed at the drop.
    struct Handle(*mut SDL_Joystick);

    // SAFETY: SDL's joystick calls take its lock, and a joystick is not tied to the thread that opened it.
    unsafe impl Send for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: opened in `Joystick::all`, closed once.
            unsafe { SDL_CloseJoystick(self.0) }
        }
    }

    pub struct Joystick {
        handle: Handle,
        name: String,
        axes: u32,
        buttons: u32,
        /// The state as last read ([`wine::objects`]'s layout).
        state: Vec<u8>,
    }

    impl Joystick {
        /// The connected joysticks' ids, none opened.
        pub fn devices(context: &mut Context) -> Vec<Device> {
            context.ids().into_iter().map(|(device, _)| device).collect()
        }

        /// The connected joysticks that can be opened, with their ids.
        pub fn all(context: &mut Context, _actual: bool) -> Vec<(Device, Joystick)> {
            let mut out = Vec::new();
            for (device, id) in context.ids() {
                // SAFETY: `id` is one of SDL's joysticks; the name is SDL's string, copied.
                let (handle, name, axes, buttons) = unsafe {
                    let handle = SDL_OpenJoystick(id);
                    if handle.is_null() {
                        eprintln!("devices: {}: {}", device.format(), crate::ffb::sdl_error());
                        continue;
                    }
                    let name = SDL_GetJoystickNameForID(id);
                    let name =
                        if name.is_null() { String::new() } else { CStr::from_ptr(name).to_string_lossy().into() };
                    let count = |n: i32| u32::try_from(n).unwrap_or(0);
                    (
                        Handle(handle),
                        name,
                        count(SDL_GetNumJoystickAxes(handle)),
                        count(SDL_GetNumJoystickButtons(handle)),
                    )
                };
                let state = wine::state(&vec![wine::axis(0); axes as usize], &vec![false; buttons as usize]);
                let mut joystick = Joystick { handle, name, axes, buttons, state };
                joystick.read(context);
                out.push((device, joystick));
            }
            out
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn axes(&self) -> u32 {
            self.axes
        }

        pub fn buttons(&self) -> u32 {
            self.buttons
        }

        pub fn state(&self) -> &[u8] {
            &self.state
        }

        /// Takes the state SDL has now; `false`, the state kept, when the joystick is unplugged.
        pub fn read(&mut self, _: &Context) -> bool {
            let joystick = self.handle.0;
            // SAFETY: the handle is open until the drop.
            unsafe {
                if !SDL_JoystickConnected(joystick) {
                    return false;
                }
                for k in 0..self.axes as usize {
                    let v = wine::axis(i32::from(SDL_GetJoystickAxis(joystick, k as i32)));
                    self.state[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
                }
                for k in 0..self.buttons as usize {
                    let down = SDL_GetJoystickButton(joystick, k as i32);
                    self.state[4 * self.axes as usize + k] = if down { 0x80 } else { 0 };
                }
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_s_arrows() {
        let mut state = [0xff; 256];
        keyboard_state(&[winit::keyboard::KeyCode::ArrowLeft, winit::keyboard::KeyCode::KeyW], &mut state);
        assert_eq!((state[0xcb], state[0x11], state[0xcd]), (0x80, 0x80, 0));
    }

    /// SDL's joysticks against evdev's on this machine: the same devices, objects and values (an axis not moved since
    /// SDL opened it reads SDL's 0 on Linux). For the joysticks SDL reads through evdev (a pad its HIDAPI takes has
    /// other objects).
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs joysticks connected"]
    fn sdl_reads_what_evdev_reads() {
        let mut context = sdl::Context::new();
        let mut sdl = sdl::Joystick::all(&mut context, true);
        assert!(!sdl.is_empty());
        assert_eq!(sdl::Joystick::devices(&mut context), sdl.iter().map(|(d, _)| *d).collect::<Vec<_>>());
        // SDL takes the axes' values at its first update.
        context.update(&mut sdl);
        let mut evdev = evdev::Joystick::all(&mut imp::Context, true);
        assert_eq!(sdl.len(), evdev.len());
        for (device, joystick) in &mut sdl {
            let (_, other) = evdev.iter_mut().find(|(d, _)| d == device).expect("evdev has SDL's joystick");
            assert!(joystick.read(&context) && other.read(&imp::Context));
            eprintln!(
                "{} ({}): {} axes, {} buttons",
                joystick.name(),
                device.format(),
                joystick.axes(),
                joystick.buttons()
            );
            // SDL has its own names for the pads it knows.
            assert_eq!((joystick.axes(), joystick.buttons()), (other.axes(), other.buttons()));
            let (ours, theirs) = (joystick.state(), other.state());
            let axes = 4 * joystick.axes() as usize;
            assert_eq!(ours[axes..], theirs[axes..], "{}", joystick.name());
            let rest = rbr_gameplay::poll::wine::axis(0).to_le_bytes();
            for (k, (ours, theirs)) in ours[..axes].chunks(4).zip(theirs[..axes].chunks(4)).enumerate() {
                assert!(ours == theirs || ours == rest, "{} axis {k}: {ours:?}, evdev {theirs:?}", joystick.name());
                eprintln!("  axis {k}: {}", if ours == theirs { "evdev's value" } else { "not moved" });
            }
        }
    }

    #[test]
    fn a_state_s_values() {
        let state = rbr_gameplay::poll::wine::state(&[0, 65535], &[false, true]);
        let v: Vec<_> = values(&state, 2, 2).collect();
        assert_eq!(
            v,
            [(Target::Axis(0), -1.0), (Target::Axis(1), 1.0), (Target::Button(0), 0.0), (Target::Button(1), 1.0)]
        );
    }
}
