//! How the viewer's window presents its frames (`viewer --present`, the launcher's "Frame presentation"; ours).

/// The presentation the window asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Present {
    /// The newest frame at each refresh, with no tearing and no wait for it: the frame rate is not tied to the
    /// display's (the default, but on macOS).
    Mailbox,
    /// Vsync: each frame waits for a refresh, so a frame just over one refresh waits two.
    Fifo,
    /// Shown as soon as drawn, uncapped; may tear.
    Immediate,
}

/// Mailbox, and vsync on macOS, whose Metal has no mailbox.
impl Default for Present {
    fn default() -> Present {
        if cfg!(target_os = "macos") {
            Present::Fifo
        } else {
            Present::Mailbox
        }
    }
}

impl Present {
    pub const ALL: [Present; 3] = [Present::Mailbox, Present::Fifo, Present::Immediate];
    /// The modes the launcher offers: on macOS vsync or not, as Metal has them.
    pub const OFFERED: &[Present] =
        if cfg!(target_os = "macos") { &[Present::Fifo, Present::Immediate] } else { &Present::ALL };

    /// This mode where it is offered, else the default (a mailbox saved on macOS).
    pub fn offered(self) -> Present {
        if Present::OFFERED.contains(&self) {
            self
        } else {
            Present::default()
        }
    }

    /// The name in the launcher's settings and `viewer --present`.
    pub fn name(self) -> &'static str {
        match self {
            Present::Mailbox => "mailbox",
            Present::Fifo => "fifo",
            Present::Immediate => "immediate",
        }
    }

    pub fn parse(text: &str) -> Option<Present> {
        Present::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(text.trim()))
    }

    fn mode(self) -> wgpu::PresentMode {
        match self {
            Present::Mailbox => wgpu::PresentMode::Mailbox,
            Present::Fifo => wgpu::PresentMode::Fifo,
            Present::Immediate => wgpu::PresentMode::Immediate,
        }
    }

    /// The surface's mode: this one if `supported`, else mailbox, else fifo (which every surface has).
    pub fn pick(self, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
        [self.mode(), wgpu::PresentMode::Mailbox]
            .into_iter()
            .find(|m| supported.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo)
    }
}

/// The window's colour format among the surface's `formats`: 8-bit linear (not sRGB-encoding) when the surface has
/// one (the game's D3D9 output was not gamma-encoded, and egui blends in gamma space), else 10-bit, else the first
/// linear one, else the first. A Vulkan surface may also list formats the renderer cannot draw in (`Rgba8Snorm`,
/// `Rgba16Unorm` without its feature) or the extended-linear `Rgba16Float`, which would show the frame darker, and
/// they need not come after the 8-bit ones.
pub fn surface_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    use wgpu::TextureFormat::{Bgra8Unorm, Rgb10a2Unorm, Rgba8Unorm};
    [Bgra8Unorm, Rgba8Unorm, Rgb10a2Unorm]
        .into_iter()
        .find(|f| formats.contains(f))
        .or_else(|| formats.iter().copied().find(|f| !f.is_srgb()))
        .unwrap_or(formats[0])
}

/// An HDR display's format (`viewer --hdr`, [`surface_colour`]): a renderer drawing into it tone maps for the display's
/// headroom (`HDR` in `post.wgsl`, [`Renderer::set_headroom`](crate::renderer::Renderer::set_headroom)).
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Whether the launcher offers HDR: on macOS, where Metal has extended-range sRGB; DX12 has no such colour space,
/// Windows' Vulkan drivers only scRGB and PQ, and Linux's depends on the driver and the compositor (`viewer --hdr`
/// still tries it everywhere).
pub const HDR_OFFERED: bool = cfg!(target_os = "macos");

/// The window's format and colour space: with `hdr` [`HDR_FORMAT`] in extended-range sRGB when the surface has it (the
/// extended dynamic range on macOS; Vulkan where the driver and the compositor have it, DX12 not), else
/// [`surface_format`]'s. Extended-range sRGB takes the frame's sRGB-coded values as on an SDR display, with 1 the
/// display's SDR white and the highlights above it, so what else is drawn into the window (the HUD, the loading screen)
/// shows as on an SDR display.
pub fn surface_colour(
    capabilities: &wgpu::SurfaceCapabilities,
    hdr: bool,
) -> (wgpu::TextureFormat, wgpu::SurfaceColorSpace) {
    let extended = wgpu::SurfaceColorSpaces::EXTENDED_SRGB;
    if hdr && capabilities.color_spaces(HDR_FORMAT).contains(extended) {
        (HDR_FORMAT, wgpu::SurfaceColorSpace::ExtendedSrgb)
    } else {
        (surface_format(&capabilities.formats), wgpu::SurfaceColorSpace::Auto)
    }
}

/// Configures `surface`; whether it took. wgpu 30 waits for the queue before it reconfigures a surface and refuses
/// when another thread submitted meanwhile (the viewer's loading thread uploading the stage): the caller keeps the
/// surface marked stale and tries again on a later frame, instead of the device's error handler ending the program.
pub fn configure(surface: &wgpu::Surface<'_>, device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> bool {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(device, config);
    futures_executor::block_on(scope.pop()).is_none()
}

/// What [`acquire`] got for this frame.
pub enum Acquired {
    /// The surface's texture, also when it no longer matches the surface (`Suboptimal`), as wgpu before 30 gave it.
    Frame(wgpu::SurfaceTexture),
    /// The window is hidden (occluded, minimised, on another space, the display asleep): wgpu 30 on macOS no longer
    /// waits up to a second for a drawable then (wgpu #8309). The caller runs the frame without drawing it, at its
    /// own pace.
    Hidden,
    /// None (timed out, outdated, lost): the caller configures the surface again and skips the frame.
    Failed,
}

/// The surface's texture for this frame ([`Acquired`]).
pub fn acquire(surface: &wgpu::Surface<'_>) -> Acquired {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(texture) | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
            Acquired::Frame(texture)
        }
        wgpu::CurrentSurfaceTexture::Occluded => Acquired::Hidden,
        _ => Acquired::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::PresentMode::{Fifo, Immediate, Mailbox};

    #[test]
    fn the_default_is_offered_and_a_mode_not_offered_falls_back_to_it() {
        assert!(Present::OFFERED.contains(&Present::default()));
        assert!(Present::ALL.into_iter().all(|p| Present::OFFERED.contains(&p.offered())));
        assert_eq!(Present::Immediate.offered(), Present::Immediate);
    }

    #[test]
    fn names_round_trip() {
        for present in Present::ALL {
            assert_eq!(Present::parse(present.name()), Some(present));
        }
        assert_eq!(Present::parse("vsync"), None);
    }

    #[test]
    fn unsupported_modes_fall_back_to_mailbox_then_fifo() {
        assert_eq!(Present::Immediate.pick(&[Fifo, Immediate, Mailbox]), Immediate);
        assert_eq!(Present::Immediate.pick(&[Fifo, Mailbox]), Mailbox);
        // Metal: no mailbox.
        assert_eq!(Present::Mailbox.pick(&[Fifo, Immediate]), Fifo);
        assert_eq!(Present::Fifo.pick(&[Fifo, Immediate, Mailbox]), Fifo);
        assert_eq!(Present::Mailbox.pick(&[]), Fifo);
    }

    #[test]
    fn surface_format_prefers_8_bit_linear() {
        use wgpu::TextureFormat::*;
        // Metal.
        assert_eq!(surface_format(&[Bgra8Unorm, Bgra8UnormSrgb, Rgba16Float, Rgb10a2Unorm]), Bgra8Unorm);
        // Vulkan surfaces listing an HDR or unusable format first.
        assert_eq!(surface_format(&[Rgba16Float, Bgra8UnormSrgb, Bgra8Unorm]), Bgra8Unorm);
        assert_eq!(surface_format(&[Bgra8UnormSrgb, Rgba8Snorm, Rgba8UnormSrgb, Rgba8Unorm]), Rgba8Unorm);
        assert_eq!(surface_format(&[Rgba16Unorm, Rgb10a2Unorm, Bgra8UnormSrgb]), Rgb10a2Unorm);
        assert_eq!(surface_format(&[Bgra8UnormSrgb]), Bgra8UnormSrgb);
    }

    #[test]
    fn hdr_takes_extended_srgb_where_the_surface_has_it() {
        use wgpu::{SurfaceColorSpace as Space, SurfaceColorSpaces as Spaces, TextureFormat::*};
        let surface = |extended: Spaces| wgpu::SurfaceCapabilities {
            formats: vec![Bgra8Unorm, Rgba16Float],
            format_capabilities: vec![
                wgpu::SurfaceFormatCapabilities { format: Bgra8Unorm, color_spaces: Spaces::SRGB },
                wgpu::SurfaceFormatCapabilities { format: Rgba16Float, color_spaces: Spaces::SRGB | extended },
            ],
            ..Default::default()
        };
        // Metal.
        let metal = surface(Spaces::EXTENDED_SRGB_LINEAR | Spaces::EXTENDED_SRGB);
        assert_eq!(surface_colour(&metal, true), (Rgba16Float, Space::ExtendedSrgb));
        assert_eq!(surface_colour(&metal, false), (Bgra8Unorm, Space::Auto));
        // DX12, and Vulkan without HDR.
        assert_eq!(surface_colour(&surface(Spaces::EXTENDED_SRGB_LINEAR), true), (Bgra8Unorm, Space::Auto));
        assert_eq!(surface_colour(&surface(Spaces::empty()), true), (Bgra8Unorm, Space::Auto));
    }
}
