//! The force-feedback wheels as DirectInput itself answers, when SDL's haptic made no constant force (issue #7: a G29
//! under Windows, "Unable to create effect", SDL's error without DirectInput's result). Per device: its force-feedback
//! driver, its force axes, the exclusive acquire, then SDL's effect and NGP's (`notes/ngp.md` "NGP 7.5.779's device
//! calls") each created and started at level 0, and released. Only read for the drive's log: the device is free again
//! before the drive's joystick opens it.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

use windows::core::{w, Interface, BOOL, GUID};
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW, HWND_MESSAGE, WINDOW_EX_STYLE,
    WNDCLASSW, WS_OVERLAPPED,
};

/// Our data format's one object, the X axis at offset 0 (`DIJOFS_X`, which SDL's effect names).
static X_AXIS: GUID = GUID_XAxis;

/// One line per attached force-feedback game controller, or why there is none.
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
    input.EnumDevices(
        DI8DEVCLASS_GAMECTRL,
        Some(push_device),
        (&raw mut list).cast(),
        DIEDFL_ATTACHEDONLY | DIEDFL_FORCEFEEDBACK,
    )?;
    if list.is_empty() {
        return Ok(vec!["no attached force-feedback game controller".into()]);
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
            let lines = list.iter().map(|d| device(&input, d, window)).collect();
            let _ = DestroyWindow(window);
            lines
        }
        Err(e) => vec![format!("no window: {}", error(&e))],
    };
    let _ = UnregisterClassW(class, Some(instance));
    Ok(lines)
}

unsafe fn device(input: &IDirectInput8W, d: &DIDEVICEINSTANCEW, window: HWND) -> String {
    let name = text(&d.tszProductName);
    let mut line = format!("{name}: force-feedback driver {:?}", d.guidFFDriver);
    let mut device = None;
    if let Err(e) = input.CreateDevice(&d.guidInstance, &mut device, None) {
        return format!("{line}, create device {}", error(&e));
    }
    let Some(device) = device else { return format!("{line}, no device") };
    let mut caps = DIDEVCAPS { dwSize: size_of::<DIDEVCAPS>() as u32, ..Default::default() };
    if device.GetCapabilities(&mut caps).is_ok() {
        line +=
            &format!(", flags {:#x}, {} axes, driver version {:#x}", caps.dwFlags, caps.dwAxes, caps.dwFFDriverVersion);
    }
    let mut axes: Vec<u32> = Vec::new();
    let _ = device.EnumObjects(Some(push_force_axis), (&raw mut axes).cast(), DIDFT_AXIS);
    line += &format!(", force axes {axes:#x?}");
    let mut objects =
        [DIOBJECTDATAFORMAT { pguid: &X_AXIS, dwOfs: 0, dwType: DIDFT_AXIS | DIDFT_ANYINSTANCE, dwFlags: 0 }];
    let mut format = DIDATAFORMAT {
        dwSize: size_of::<DIDATAFORMAT>() as u32,
        dwObjSize: size_of::<DIOBJECTDATAFORMAT>() as u32,
        dwFlags: DIDF_ABSAXIS,
        dwDataSize: 4,
        dwNumObjs: 1,
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
        return format!("{line}, {e}");
    }
    line += ", acquired";
    // SDL's: one axis by offset, the zero direction of its steering axis, its infinite length wrapped (`ffb::SHAPES`).
    line += &format!(
        ", SDL's effect {}",
        effect(&device, DIEFF_OBJECTOFFSETS | DIEFF_CARTESIAN, &mut [0], &mut [0], u32::MAX.wrapping_mul(1000))
    );
    // NGP's: its first two force axes by id, {1, 0}, infinite.
    let n = axes.len().min(2);
    line += &match n {
        0 => ", NGP's effect: no force axis".into(),
        _ => format!(
            ", NGP's effect {}",
            effect(&device, DIEFF_OBJECTIDS | DIEFF_CARTESIAN, &mut axes[..n], &mut [1, 0][..n], u32::MAX)
        ),
    };
    let _ = device.Unacquire();
    line
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
    let mut made = None;
    match device.CreateEffect(&GUID_ConstantForce, &mut params, &mut made, None) {
        Err(e) => format!("create {}", error(&e)),
        Ok(()) => match made {
            None => "created none".into(),
            Some(made) => {
                let started = made.Start(1, 0);
                let _ = made.Stop();
                let _ = made.Unload();
                match started {
                    Ok(()) => "created, started".into(),
                    Err(e) => format!("created, start {}", error(&e)),
                }
            }
        },
    }
}

unsafe extern "system" fn push_device(d: *mut DIDEVICEINSTANCEW, list: *mut c_void) -> BOOL {
    (*list.cast::<Vec<DIDEVICEINSTANCEW>>()).push(*d);
    BOOL(DIENUM_CONTINUE as i32)
}

unsafe extern "system" fn push_force_axis(o: *mut DIDEVICEOBJECTINSTANCEW, axes: *mut c_void) -> BOOL {
    if (*o).dwFlags & DIDOI_FFACTUATOR != 0 {
        (*axes.cast::<Vec<u32>>()).push((*o).dwType);
    }
    BOOL(DIENUM_CONTINUE as i32)
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(window, message, w, l)
}

fn text(wide: &[u16]) -> String {
    String::from_utf16_lossy(&wide[..wide.iter().position(|&c| c == 0).unwrap_or(wide.len())])
}

fn error(e: &windows::core::Error) -> String {
    format!("{:#010x} {}", e.code().0 as u32, e.message())
}
