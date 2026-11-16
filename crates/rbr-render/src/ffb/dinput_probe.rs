//! The force-feedback wheels as DirectInput itself answers, when SDL's haptic made no constant force (issue #7: a G29
//! under Windows, "Unable to create effect", SDL's error without DirectInput's result). Per device: its force-feedback
//! driver, capabilities, axes, effects, gain and autocentre, the exclusive acquire, then each of `ffb::SHAPES` as SDL
//! builds it for DirectInput and NGP's own (`notes/ngp.md` "NGP 7.5.779's device calls"), created and started at
//! level 0 and released. Only read for the drive's log: the device is free again before the drive's joystick opens it.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null_mut, without_provenance};

use windows::core::{w, Interface, BOOL, GUID};
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW, HWND_MESSAGE, WINDOW_EX_STYLE,
    WNDCLASSW, WS_OVERLAPPED,
};

/// `DIDFT_OPTIONAL`: an object of the data format the device may not have.
const OPTIONAL: u32 = 0x8000_0000;

/// Our data format: the axes SDL names by offset (`SDL_c_dfDIJoystick2`'s first six, `DIJOFS_X` to `DIJOFS_RZ`).
static AXES: [GUID; 6] = [GUID_XAxis, GUID_YAxis, GUID_ZAxis, GUID_RxAxis, GUID_RyAxis, GUID_RzAxis];

/// One line per attached force-feedback game controller and what it answered, or why there is none.
pub fn probe() -> Vec<String> {
    // SAFETY: DirectInput's objects and the window live within this call, on this thread.
    unsafe { devices().unwrap_or_else(|e| vec![format!("DirectInput: {}", error(&e))]) }
}

unsafe fn devices() -> windows::core::Result<Vec<String>> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    let mut raw = null_mut();
    DirectInput8Create(instance, DIRECTINPUT_VERSION, &IDirectInput8W::IID, &mut raw, None)?;
    let input = IDirectInput8W::from_raw(raw);
    let mut list: Vec<DIDEVICEINSTANCEW> = Vec::new();
    input.EnumDevices(DI8DEVCLASS_GAMECTRL, Some(push_device), (&raw mut list).cast(), DIEDFL_ATTACHEDONLY)?;
    if list.is_empty() {
        return Ok(vec!["no attached game controller".into()]);
    }
    // Exclusive access needs a window: a message-only one, as SDL's helper window is.
    let class = w!("ReRBR force feedback probe");
    let wc =
        WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance, lpszClassName: class, ..Default::default() };
    RegisterClassW(&wc);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!(""),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(instance),
        None,
    );
    let lines = match window {
        Ok(window) => {
            let lines = list.iter().flat_map(|d| device(&input, d, window)).collect();
            let _ = DestroyWindow(window);
            lines
        }
        Err(e) => vec![format!("no window: {}", error(&e))],
    };
    let _ = UnregisterClassW(class, Some(instance));
    Ok(lines)
}

/// An axis as EnumObjects gives it.
struct Axis {
    name: String,
    guid: GUID,
    kind: u32,
    flags: u32,
}

unsafe fn device(input: &IDirectInput8W, d: &DIDEVICEINSTANCEW, window: HWND) -> Vec<String> {
    let name = text(&d.tszProductName);
    let mut lines = vec![format!(
        "{name}: instance {}, type {:#x}, usage {:#x}/{:#x}, force-feedback driver {:?}",
        text(&d.tszInstanceName),
        d.dwDevType,
        d.wUsagePage,
        d.wUsage,
        d.guidFFDriver
    )];
    let mut line = |text: String| lines.push(format!("{name}: {text}"));
    let mut device = None;
    if let Err(e) = input.CreateDevice(&d.guidInstance, &mut device, None) {
        line(format!("create device {}", error(&e)));
        return lines;
    }
    let Some(device) = device else {
        line("no device".into());
        return lines;
    };
    let mut caps = DIDEVCAPS { dwSize: size_of::<DIDEVCAPS>() as u32, ..Default::default() };
    match device.GetCapabilities(&mut caps) {
        Ok(()) => line(format!(
            "flags {:#x} (force feedback {}), {} axes, {} buttons, sample period {} us, min time resolution {} us, \
             driver version {:#x}, firmware {:#x}, hardware {:#x}",
            caps.dwFlags,
            caps.dwFlags & DIDC_FORCEFEEDBACK != 0,
            caps.dwAxes,
            caps.dwButtons,
            caps.dwFFSamplePeriod,
            caps.dwFFMinTimeResolution,
            caps.dwFFDriverVersion,
            caps.dwFirmwareRevision,
            caps.dwHardwareRevision
        )),
        Err(e) => line(format!("capabilities {}", error(&e))),
    }
    let mut axes: Vec<Axis> = Vec::new();
    let _ = device.EnumObjects(Some(push_axis), (&raw mut axes).cast(), DIDFT_AXIS);
    let shown: Vec<String> =
        axes.iter().map(|a| format!("{} (type {:#x}, flags {:#x})", a.name, a.kind, a.flags)).collect();
    line(format!("axes {}", shown.join(", ")));
    let mut effects: Vec<String> = Vec::new();
    let _ = device.EnumEffects(Some(push_effect), (&raw mut effects).cast(), DIEFT_ALL);
    line(format!("effects {}", effects.join(", ")));
    if caps.dwFlags & DIDC_FORCEFEEDBACK == 0 {
        return lines;
    }
    let mut objects: Vec<DIOBJECTDATAFORMAT> = AXES
        .iter()
        .enumerate()
        .map(|(k, guid)| DIOBJECTDATAFORMAT {
            pguid: guid,
            dwOfs: 4 * k as u32,
            dwType: DIDFT_AXIS | DIDFT_ANYINSTANCE | OPTIONAL,
            dwFlags: 0,
        })
        .collect();
    let mut format = DIDATAFORMAT {
        dwSize: size_of::<DIDATAFORMAT>() as u32,
        dwObjSize: size_of::<DIOBJECTDATAFORMAT>() as u32,
        dwFlags: DIDF_ABSAXIS,
        dwDataSize: 4 * AXES.len() as u32,
        dwNumObjs: AXES.len() as u32,
        rgodf: objects.as_mut_ptr(),
    };
    let set = device
        .SetDataFormat(&mut format)
        .map_err(|e| format!("data format {}", error(&e)))
        .and_then(|()| {
            device
                .SetCooperativeLevel(window, DISCL_EXCLUSIVE | DISCL_BACKGROUND)
                .map_err(|e| format!("exclusive level {}", error(&e)))
        })
        .and_then(|()| device.Acquire().map_err(|e| format!("acquire {}", error(&e))));
    if let Err(e) = set {
        line(e);
        return lines;
    }
    let mut state = 0;
    let state = match device.GetForceFeedbackState(&mut state) {
        Ok(()) => format!("{state:#x}"),
        Err(e) => error(&e),
    };
    line(format!(
        "acquired exclusively; gain {}, autocentre {}, force-feedback state {state}",
        property(&device, DIPROP_FFGAIN),
        property(&device, DIPROP_AUTOCENTER)
    ));
    let reset = device.SendForceFeedbackCommand(DISFFC_RESET).map_err(|e| error(&e));
    let on = device.SendForceFeedbackCommand(DISFFC_SETACTUATORSON).map_err(|e| error(&e));
    line(format!("reset {}, actuators on {}", shown_result(reset), shown_result(on)));
    // The force axes as SDL takes them: by offset in its data format, X to Rz, the first three.
    let force: Vec<&Axis> = axes.iter().filter(|a| a.flags & DIDOI_FFACTUATOR != 0).collect();
    let mut offsets: Vec<u32> =
        force.iter().filter_map(|a| AXES.iter().position(|g| *g == a.guid)).map(|k| 4 * k as u32).take(3).collect();
    let mut ids: Vec<u32> = force.iter().map(|a| a.kind).take(2).collect();
    let wrapped = u32::MAX.wrapping_mul(1000);
    let hour = 3_600_000_000;
    let by_offset = DIEFF_OBJECTOFFSETS | DIEFF_CARTESIAN;
    for (shape, result) in [
        ("SDL's steering axis", effect(&device, by_offset, &mut [0], &mut [0], wrapped)),
        ("SDL's steering axis for an hour", effect(&device, by_offset, &mut [0], &mut [0], hour)),
        (
            "SDL's cartesian {1, 0} for an hour",
            match offsets.len() {
                0 => "no force axis SDL takes".into(),
                n => effect(&device, by_offset, &mut offsets, &mut [1, 0, 0][..n], hour),
            },
        ),
        (
            "NGP's",
            match ids.len() {
                0 => "no force axis".into(),
                n => effect(&device, DIEFF_OBJECTIDS | DIEFF_CARTESIAN, &mut ids, &mut [1, 0][..n], u32::MAX),
            },
        ),
    ] {
        line(format!("{shape}: {result}"));
    }
    let _ = device.Unacquire();
    lines
}

/// A DWORD property of the device, or its error.
unsafe fn property(device: &IDirectInputDevice8W, which: GUID) -> String {
    let mut value = DIPROPDWORD {
        diph: DIPROPHEADER {
            dwSize: size_of::<DIPROPDWORD>() as u32,
            dwHeaderSize: size_of::<DIPROPHEADER>() as u32,
            dwObj: 0,
            dwHow: DIPH_DEVICE,
        },
        dwData: 0,
    };
    // DirectInput's own properties are small integers passed as the GUID pointer (`MAKEDIPROP`).
    let id = without_provenance::<GUID>(which.to_u128() as usize);
    match device.GetProperty(id, &mut value.diph) {
        Ok(()) => value.dwData.to_string(),
        Err(e) => error(&e),
    }
}

/// A constant force at level 0 created and started, then stopped and released: what each step answered.
unsafe fn effect(
    device: &IDirectInputDevice8W,
    flags: u32,
    axes: &mut [u32],
    direction: &mut [i32],
    duration: u32,
) -> String {
    let mut force = DICONSTANTFORCE { lMagnitude: 0 };
    let mut params = DIEFFECT {
        dwSize: size_of::<DIEFFECT>() as u32,
        dwFlags: flags,
        dwDuration: duration,
        dwSamplePeriod: 0,
        dwGain: 10000,
        dwTriggerButton: DIEB_NOTRIGGER,
        dwTriggerRepeatInterval: 0,
        cAxes: axes.len() as u32,
        rgdwAxes: axes.as_mut_ptr(),
        rglDirection: direction.as_mut_ptr(),
        lpEnvelope: null_mut(),
        cbTypeSpecificParams: size_of::<DICONSTANTFORCE>() as u32,
        lpvTypeSpecificParams: (&raw mut force).cast(),
        dwStartDelay: 0,
    };
    let what = format!("axes {axes:#x?}, direction {direction:?}, duration {duration} us");
    let mut made = None;
    match device.CreateEffect(&GUID_ConstantForce, &mut params, &mut made, None) {
        Err(e) => format!("{what}: create {}", error(&e)),
        Ok(()) => match made {
            None => format!("{what}: created none"),
            Some(made) => {
                let started = made.Start(1, 0).map_err(|e| error(&e));
                let mut status = 0;
                let status = match made.GetEffectStatus(&mut status) {
                    Ok(()) => format!("{status:#x}"),
                    Err(e) => error(&e),
                };
                let _ = made.Stop();
                let _ = made.Unload();
                format!("{what}: created, start {}, status {status}", shown_result(started))
            }
        },
    }
}

unsafe extern "system" fn push_device(d: *mut DIDEVICEINSTANCEW, list: *mut c_void) -> BOOL {
    (*list.cast::<Vec<DIDEVICEINSTANCEW>>()).push(*d);
    BOOL(DIENUM_CONTINUE as i32)
}

unsafe extern "system" fn push_axis(o: *mut DIDEVICEOBJECTINSTANCEW, axes: *mut c_void) -> BOOL {
    let o = &*o;
    let axis = Axis { name: text(&o.tszName), guid: o.guidType, kind: o.dwType, flags: o.dwFlags };
    (*axes.cast::<Vec<Axis>>()).push(axis);
    BOOL(DIENUM_CONTINUE as i32)
}

unsafe extern "system" fn push_effect(e: *mut DIEFFECTINFOW, effects: *mut c_void) -> BOOL {
    let e = &*e;
    (*effects.cast::<Vec<String>>()).push(format!("{} (type {:#x})", text(&e.tszName), e.dwEffType));
    BOOL(DIENUM_CONTINUE as i32)
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(window, message, w, l)
}

fn text(wide: &[u16]) -> String {
    String::from_utf16_lossy(&wide[..wide.iter().position(|&c| c == 0).unwrap_or(wide.len())])
}

fn shown_result(result: Result<(), String>) -> String {
    result.map_or_else(|e| e, |()| "ok".into())
}

/// The HRESULT, DirectInput's name for it, and the system's text.
fn error(e: &windows::core::Error) -> String {
    let code = e.code().0 as u32;
    let name = match code {
        0x8007_0057 => "DIERR_INVALIDPARAM",
        0x8007_0005 => "DIERR_OTHERAPPHASPRIO or E_ACCESSDENIED",
        0x8007_000c => "DIERR_NOTACQUIRED",
        0x8007_001e => "DIERR_INPUTLOST",
        0x8007_0015 => "DIERR_NOTINITIALIZED",
        0x8004_0154 => "DIERR_DEVICENOTREG",
        0x8004_0201 => "DIERR_DEVICEFULL",
        0x8004_0202 => "DIERR_MOREDATA",
        0x8004_0203 => "DIERR_NOTDOWNLOADED",
        0x8004_0204 => "DIERR_HASEFFECTS",
        0x8004_0205 => "DIERR_NOTEXCLUSIVEACQUIRED",
        0x8004_0206 => "DIERR_INCOMPLETEEFFECT",
        0x8004_0207 => "DIERR_NOTBUFFERED",
        0x8004_0208 => "DIERR_EFFECTPLAYING",
        0x8004_0209 => "DIERR_UNPLUGGED",
        0x8000_4001 => "DIERR_UNSUPPORTED",
        0x8000_4005 => "DIERR_GENERIC",
        _ => "",
    };
    format!("{code:#010x} {name} {}", e.message()).trim_end().to_owned()
}
