// HDR post pass (renderer addition, `post.rs`; not the game's look). The scene arrives as linear colour in a float
// target (`graded` in `shader.wgsl`). A glare chain halves it (`fs_glare_first`, `fs_glare_down`, the 13-tap filter of
// Jimenez 2014, each level weighted by a camera's diffraction) and adds the levels back up (`fs_glare_up`, a 3x3 tent);
// `cs_histogram` bins the scene's light and `cs_meter` sets the exposure from the histogram, eased over time as an eye
// adapts; `fs_composite` adds the glare, exposes, and applies Gran Turismo 7's tone mapping (`gt7`; sharpening first
// under TAA, `sharpened`).

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(1) @binding(0) var bloom: texture_2d<f32>;

struct Exposure {
    // The exposure the composite applies.
    value: f32,
    // The luminance the eye is adapted to, cd/m2, eased as the exposure.
    adapting: f32,
    // In the in-car view the cabin's own exposure (`cs_meter`), eased as `value`; elsewhere `value`'s.
    cabin: f32,
    // The frame's terms of `adapting`, which `cs_meter` works out once for the composite's pixels: the rods' white
    // (`rod_vision`) and the glare's levels' weights summed (`glare_weight`).
    white: f32,
    spread: vec3<f32>,
}

// `cs_meter`'s settings (`post::Meter`).
struct Meter {
    // Seconds since the last frame's metering; 0 to set the exposure outright (the first frame, a jump in time).
    step: f32,
    snap: f32,
    // In the in-car view the depth (reversed) nearer than which the frame is the cabin; 0 outside the car.
    cabin_depth: f32,
    // The renderer's units per cd/m2 (`post::Conditions::pre_exposure`).
    pre_exposure: f32,
    // The weather's grade (`post::Conditions`): `x` the metered exposure's scale, `y` the saturation's; `z` the
    // sharpening (`sharpened`); `w` an HDR display's headroom (`HDR`).
    grade: vec4<f32>,
}

@group(1) @binding(1) var<storage, read> exposure: Exposure;
// The light shafts (`light_shafts.wgsl`), added with `SHAFTS` (a pipeline override, so the composite without them is
// built as before).
@group(1) @binding(2) var shafts_map: texture_2d<f32>;
// The frame's depth (reversed), which the shafts' depth (their alpha) is matched against (`shafts_at`).
@group(1) @binding(3) var composite_depth: texture_depth_2d;
override SHAFTS: bool = false;
// The in-car view's cabin exposure (`Exposure::cabin`, `CABIN_STOPS`): a pipeline override, so the composite outside
// the car is built as before.
override CABIN: bool = false;
// What `viewer --debug-view` passes through (`renderer::DebugView`, `cabin_debug` in `cabin.wgsl`): 0 the shaded
// scene, else the cabin's debug grey, already in display grey. A pipeline override, so the composite without it is
// built as before.
override DEBUG_VIEW: u32 = 0u;
// An HDR display (`present::surface_colour`, `viewer --hdr`): the target takes extended-range sRGB, 1 for the display's
// SDR white and above it up to its headroom (`Meter::headroom`, `grade.w`), which the tone curve's peak rises to
// (`display`). A pipeline override, so the composite for an SDR display is built as before.
override HDR: bool = false;

// `cs_histogram`'s and `cs_meter`'s, with the scene in `source`. `cs_meter` empties the histogram for the next frame.
// `meter` is the composite's too.
@group(1) @binding(2) var meter_depth: texture_depth_2d;
@group(1) @binding(3) var<storage, read_write> meter_exposure: Exposure;
@group(1) @binding(4) var<uniform> meter: Meter;
@group(1) @binding(5) var<storage, read_write> meter_histogram: array<atomic<u32>, ZONE_BINS>;

// The glare chain's levels (`post::LEVELS`); `fs_glare_up` sums them.
const LEVELS: f32 = 6.0;
// The metering (`cs_histogram`, `cs_meter`): the scene's luminance (cd/m2, its light over the pre-exposure) on a grid
// of `METER_GRID` points binned by its log2 in `BINS` bins from `EV_LOW` over `EV_SPAN` stops (starlit ground to the
// sunlit sky); the mean of the bins between the `PERCENTILES` (the darkest shadows and the
// brightest highlights dropped) is the frame's light, each point counting `WEIGHT`.
// In the in-car view the cabin (nearer than `Meter::cabin_depth`) is metered apart too, as the eye adapts to the dash
// when it looks there (user's call, 2026-10-31; was one exposure with the cabin at half the outside's weight): the
// outside's light counts the outside's points `IN_CAR_WEIGHT.x` and the cabin's `.y` (a fifth: the outside exposed
// nearly as seen from outside the car, the cabin's dark still holding it back a little), the cabin's light the cabin's
// points alone, in the next `BINS` bins. The cabin takes its own exposure, at least the outside's and at most
// `CABIN_STOPS` over it (4 until 2026-11-01, with a toe lifting its darkest values a stop: with the cabin's light
// weighed toward the windows, `cabin.wgsl`, the user's pick of the cabin looks kept its blacks).
const METER_GRID: vec2<u32> = vec2<u32>(128u, 64u);
const BINS: u32 = 128u;
const ZONE_BINS: u32 = 2u * BINS;
const EV_LOW: f32 = -14.0;
const EV_SPAN: f32 = 32.0;
const PERCENTILES: vec2<f32> = vec2<f32>(0.1, 0.9);
const WEIGHT: u32 = 2u;
const IN_CAR_WEIGHT: vec2<u32> = vec2<u32>(5u, 1u);
const CABIN_STOPS: f32 = 2.5;
// The cabin's lift tapers off over what is this share of `KEY` at the outside's exposure (ours, 2026-11-13): the sun on
// a cabin face is the outside's light, and lifted in full it burnt black door cards and dashes white (911 GT3 RS 2007,
// a door card of black texels 244 of 255; 203 tapered, 138 at the outside's exposure). 0.1: the user's pick of 0.25,
// 0.1 and the lift off.
const CABIN_TAPER: f32 = 0.1;
// The exposure, as Unreal's and BeamNG's auto exposure: the metered light, back in luminance (over the pre-exposure),
// as EV100 (ISO 100, the calibration constant `CALIBRATION`), held within `EV100_RANGE`, is exposed to `KEY`, middle
// grey in GT7's frame buffer units (`gt7`), shifted by the exposure compensation curve (`compensation`). The curve is
// Krawczyk, Myszkowski and Seidel's (2005) key value against the adapting luminance, relative to a sunny scene's
// (EV100 15): dim scenes are drawn darker, as a dim scene looks darker to the adapted eye (twilight 1 to 3 EV down, a
// moonlit night about 4.5). `KEY` 0.3681 is where GT7's curve shows middle grey at 0.415 of white, as AgX, the curve
// before it, showed a sunny scene's (2026-11-05; GT's own convention, 0.18, draws the mid-tones about 1.3 stops darker).
const KEY: f32 = 0.3681;
const CALIBRATION: f32 = 12.5;
const EV100_RANGE: vec2<f32> = vec2<f32>(-8.0, 18.0);
const SUNNY_LUMINANCE: f32 = 4096.0;
// Rod vision in the mesopic range: Cao, Pokorny, Smith and Zele's (2008) model of the rods' signal added to the cones'
// opponent channels, as Kirk and O'Brien (2011) tone map with it and Filament and Ghost of Tsushima (Patry 2021) apply
// it. `LMSR` takes linear Rec.709 light to the L, M and S cones' and the rods' responses (each receptor's curve over
// D65 and the Rec.709 primaries' reflectance spectra); `CONE_MAX` is the cones' maximal sensitivity (Cao et al.,
// Table 1); `ROD_INPUT` the rods' input strength to each (Kirk and O'Brien's k1, k1, k2); `ROD_SHIFT` the rods' signal
// through the KC pathway's opponent channels (scale 45, static saturation 10, surround 0.6, white ratio 0.139, L
// weight 0.6189) back to RGB. `ROD_RETINA` takes cd/m2 to the model's units: Filament's 380 (Patry's +11.4 EV) over
// exposed light, which at a moonlit night's exposure (EV100 about -3.5) is about 3000 over cd/m2.
const LMSR: mat4x3<f32> = mat4x3<f32>(
    vec3<f32>(7.696847, 18.424824, 2.068096),
    vec3<f32>(2.431137, 18.697937, 3.012463),
    vec3<f32>(0.289117, 1.401833, 13.792292),
    vec3<f32>(0.466386, 15.564362, 10.059963),
);
const CONE_MAX: vec3<f32> = vec3<f32>(0.63721, 0.39242, 1.6064);
const ROD_INPUT: vec3<f32> = vec3<f32>(0.2, 0.2, 0.3);
const ROD_SHIFT: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.319458, 0.058098, 0.659214),
    vec3<f32>(-0.342068, 0.239183, 0.654598),
    vec3<f32>(0.010507, -0.011333, 0.061863),
);
const ROD_RETINA: f32 = 3000.0;
// Local exposure (`fs_composite`; as Unreal's local exposure lowers the highlights' contrast): the eye adapts to each
// part of what it sees, a single exposure cannot, and the lamps' beam on the road at night is several stops over the
// dark the meter exposes for. Where the frame's broad light, the glare chain's sum of blurs, is exposed more than
// `LOCAL_FROM` stops over `KEY`, `LOCAL_GIVE` of the excess is taken off the exposure there; the detail within it stays.
// A day's sky seldom reaches it.
const LOCAL_FROM: f32 = 2.0;
const LOCAL_GIVE: f32 = 0.6;
// The glare (after Gran Turismo 7, SIGGRAPH 2025 Physically Based Shading course; until 2026-11-05 an even blur of 6%
// of the frame): the frame's light spread by a camera's diffraction, the Airy pattern of a round aperture, whose light
// beyond a radius r is J0(x)^2 + J1(x)^2 with x = pi r / (lambda N), about 2 / (pi x) past the first ring. The glare
// chain's level j takes the light between `GLARE_EDGE` of its texels and half that (1.5 and 3 pixels for the first,
// doubling each level), so its weight halves from level to level (`glare_weight`; least squares of the edge against the
// pattern's light outside 1 to 120 pixels, through the chain's own blurs: 27% rms in log, the outer levels short of the
// pattern's far tail). The radii are on a full-frame camera's 24 mm high frame (`GLARE_SENSOR`), the wavelengths the
// Rec.709 primaries' dominant ones (`GLARE_WAVELENGTH`: the red glare reaches further, the dispersion), and the
// F-number the aperture a camera at 1/125 s and ISO 100 takes for the adapting luminance's EV100 (`GLARE_SHUTTER`, the
// sunny 16 rule: f/16 in sun), within `GLARE_APERTURE`: the light spread grows with the F-number, so a bright scene
// glares and a dim one hardly does. At 1920x1080 it spreads 5.4% of the light in sun (f/16), 0.5% at f/1.4 (dusk and
// night): the light sources at their real luminances (`lamp_shown`, `sun_disc` in `shader.wgsl`) glare at night, where
// the even blur spread 6% of them into a red fog over the frame. GT7 spreads only the light past the display's white:
// with the game's lamps and low sun a few stops over it (and the chain's clamp at 32), that glare did not show, even 16
// times as strong.
const GLARE_EDGE: f32 = 1.5;
const GLARE_SENSOR: f32 = 24.0;
const GLARE_WAVELENGTH: vec3<f32> = vec3<f32>(611.0e-6, 549.0e-6, 464.0e-6);
const GLARE_SHUTTER: f32 = 1.0 / 125.0;
const GLARE_APERTURE: vec2<f32> = vec2<f32>(1.4, 22.0);
const PI: f32 = 3.14159265;
// The glare chain: the most a tap reads, renderer units (`LIGHT_CAP` in `shader.wgsl`, the most a light source is drawn
// at; only against a stray infinity). The local exposure's broad light takes the frame's light at most
// `BROAD_OVER_WHITE` times the display's white, exposed (`GLARE_WHITE`, where `gt7` reaches white, 3.3 stops over
// `KEY`): with the light sources' full light the local exposure darkened all about them (the car's rear and the glare
// under its tail lamps), and at white the night beam on the road burnt out (45O chase 23:00: 3.7% of the frame clipped,
// 2.4% at 8).
const CHAIN_CAP: f32 = 30000.0;
const GLARE_WHITE: f32 = 3.65;
const BROAD_OVER_WHITE: f32 = 8.0;
// How fast the exposure follows the frame, per second: faster down into a bright frame than up into a dark one.
const ADAPT_SPEED: vec2<f32> = vec2<f32>(3.0, 1.2);

// Gran Turismo 7's tone mapping (Polyphony Digital, SIGGRAPH 2025 Physically Based Shading course, and their MIT sample
// `gt7_tone_mapping.cpp` 1.0, ICtCp mode; `notes/references/gt7-tone-mapping`). Its curve (`gt7_curve`: a toe up
// to `GT7_MID`, linear to `GT7_LINEAR` of the peak, then a shoulder converging on the peak) maps each channel; the
// mapped colour's intensity in ICtCp with the input's chroma, faded out between `GT7_FADE` of the peak's intensity,
// goes back to RGB, and the two blend by `GT7_BLEND`. It works in linear Rec.2020 in GT's frame buffer units, 1 for
// 100 cd/m2 (`GT7_UNIT`), and an SDR display's white is its 250 cd/m2 paper white (`GT7_PEAK`); the exposure (`KEY`) puts
// light in those units. It reaches white about 3.3 stops over middle grey (AgX 0.92 of white at 4 stops); its highlights
// keep their hue and lose their chroma only near white. Chosen over AgX on the lighting sheet (user, 2026-11-05).
const GT7_PEAK: f32 = 2.5;
const GT7_UNIT: f32 = 100.0;
const GT7_ALPHA: f32 = 0.25;
const GT7_MID: f32 = 0.538;
const GT7_LINEAR: f32 = 0.444;
const GT7_TOE: f32 = 1.28;
const GT7_BLEND: f32 = 0.6;
const GT7_FADE: vec2<f32> = vec2<f32>(0.98, 1.16);
// The peak's ICtCp intensity: a grey's L, M and S are its own, so its PQ code, `pq(GT7_PEAK)`.
const GT7_PEAK_INTENSITY: f32 = 0.6025592;
// SMPTE ST 2084 (PQ): m1, m2; c1, c2, c3.
const PQ_M: vec2<f32> = vec2<f32>(0.1593017578125, 78.84375);
const PQ_C: vec3<f32> = vec3<f32>(0.8359375, 18.8515625, 18.6875);
// The matrices below are written by rows, as published, and applied as `v * m`. Rec.709 to Rec.2020 and back
// (ITU-R BT.2087, from the primaries); Rec.2020 to LMS, PQ-coded LMS to ICtCp, and back (ITU-T T.302; the sample's).
const REC709_TO_2020: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.6274039, 0.3292830, 0.0433131),
    vec3<f32>(0.0690973, 0.9195404, 0.0113623),
    vec3<f32>(0.0163914, 0.0880133, 0.8955953),
);
const REC2020_TO_709: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.6604910, -0.5876411, -0.0728499),
    vec3<f32>(-0.1245505, 1.1328999, -0.0083494),
    vec3<f32>(-0.0181508, -0.1005789, 1.1187297),
);
const TO_LMS: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.412109375, 0.52392578125, 0.06396484375),
    vec3<f32>(0.166748046875, 0.720458984375, 0.11279296875),
    vec3<f32>(0.024169921875, 0.075439453125, 0.900390625),
);
const TO_ICTCP: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.5, 0.5, 0.0),
    vec3<f32>(1.61376953125, -3.323486328125, 1.709716796875),
    vec3<f32>(4.378173828125, -4.24560546875, -0.132568359375),
);
const FROM_ICTCP: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.0, 0.00860904, 0.11103),
    vec3<f32>(1.0, -0.00860904, -0.11103),
    vec3<f32>(1.0, 0.560031, -0.320627),
);
const FROM_LMS: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(3.43661, -2.50645, 0.0698454),
    vec3<f32>(-0.79133, 1.9836, -0.192271),
    vec3<f32>(-0.0259499, -0.0989137, 1.12486),
);

// GT's frame buffer light to PQ code values.
fn pq(v: vec3<f32>) -> vec3<f32> {
    let y = pow(max(v, vec3<f32>(0.0)) * (GT7_UNIT / 10000.0), vec3<f32>(PQ_M.x));
    return pow((PQ_C.x + PQ_C.y * y) / (1.0 + PQ_C.z * y), vec3<f32>(PQ_M.y));
}

// PQ code values to GT's frame buffer light.
fn pq_light(n: vec3<f32>) -> vec3<f32> {
    let p = pow(clamp(n, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / PQ_M.y));
    let l = pow(max(p - PQ_C.x, vec3<f32>(0.0)) / (PQ_C.y - PQ_C.z * p), vec3<f32>(1.0 / PQ_M.x));
    return l * (10000.0 / GT7_UNIT);
}

fn ictcp(rgb: vec3<f32>) -> vec3<f32> {
    return pq(rgb * TO_LMS) * TO_ICTCP;
}

fn from_ictcp(i: vec3<f32>) -> vec3<f32> {
    return max(pq_light(i * FROM_ICTCP) * FROM_LMS, vec3<f32>(0.0));
}

fn gt7_curve(x: vec3<f32>, peak: f32) -> vec3<f32> {
    let k = (GT7_LINEAR - 1.0) / (GT7_ALPHA - 1.0);
    let v = max(x, vec3<f32>(0.0));
    let toe = GT7_MID * pow(v / GT7_MID, vec3<f32>(GT7_TOE));
    let below = mix(toe, v, smoothstep(vec3<f32>(0.0), vec3<f32>(GT7_MID), v));
    let shoulder = peak * (GT7_LINEAR + k) - peak * k * exp((GT7_LINEAR - v / peak) / k);
    return select(shoulder, below, v < vec3<f32>(GT7_LINEAR * peak));
}

// What the display shows (`display`): the tone curve's peak, frame buffer units, and its ICtCp intensity; the brightest
// display value over the SDR white, and sRGB-coded.
struct Display {
    peak: f32,
    intensity: f32,
    headroom: f32,
    top: f32,
}

// An SDR display shows up to the paper white. An HDR display (`HDR`) shows the paper white at its SDR white, as GT7's
// SDR mode, and its highlights up to its headroom: the curve peaks there, as GT7's HDR mode peaks at the display's
// luminance, so the mid-tones stay as on an SDR display.
fn display() -> Display {
    if (!HDR) {
        return Display(GT7_PEAK, GT7_PEAK_INTENSITY, 1.0, 1.0);
    }
    let headroom = max(meter.grade.w, 1.0);
    let peak = GT7_PEAK * headroom;
    return Display(peak, pq(vec3<f32>(peak)).x, headroom, srgb(vec3<f32>(headroom)).x);
}

// The sRGB transfer function, continued past 1 for an HDR display's extended-range sRGB.
fn srgb(shown: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(shown, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * shown, shown <= vec3<f32>(0.0031308));
}

// A near-primary light far past white whitens straight from its hue (ours): the curve maps each channel, so a red
// lamp's core, whose Rec.2020 green (0.07 of its red) clips well before its blue (0.02), went orange. Its least channel
// rises to its middle one from the curve's linear part (`GT7_LINEAR` of the peak) to `WHITEN_FULL` peaks, so the
// lamp stays red to some 4 times white, then shades to pink and white. A hue with a strong second channel (an amber
// lamp's, `WHITEN_PRIMARY`) keeps the curve's.
const WHITEN_FULL: f32 = 4.0;
const WHITEN_PRIMARY: vec2<f32> = vec2<f32>(0.15, 0.4);

fn whitened(rgb: vec3<f32>, peak: f32) -> vec3<f32> {
    let high = max(rgb.r, max(rgb.g, rgb.b));
    let low = min(rgb.r, min(rgb.g, rgb.b));
    let mid = rgb.r + rgb.g + rgb.b - high - low;
    let primary = 1.0 - smoothstep(WHITEN_PRIMARY.x, WHITEN_PRIMARY.y, mid / max(high, 1e-6));
    let over = smoothstep(GT7_LINEAR * peak, WHITEN_FULL * peak, high);
    return max(rgb, vec3<f32>(mix(low, mid, primary * over)));
}

// Display values for the exposed linear Rec.709 light: the mapped light over the paper white, sRGB-coded.
fn gt7(linear: vec3<f32>, d: Display) -> vec3<f32> {
    let rgb = whitened(linear * REC709_TO_2020, d.peak);
    let skewed = gt7_curve(rgb, d.peak);
    let ucs = ictcp(rgb);
    let chroma = 1.0 - smoothstep(GT7_FADE.x, GT7_FADE.y, ucs.x / d.intensity);
    let scaled = from_ictcp(vec3<f32>(ictcp(skewed).x, ucs.yz * chroma));
    let mapped = min(mix(skewed, scaled, GT7_BLEND), vec3<f32>(d.peak)) / GT7_PEAK;
    return srgb(clamp(mapped * REC2020_TO_709, vec3<f32>(0.0), vec3<f32>(d.headroom)));
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_full(@builtin(vertex_index) index: u32) -> Out {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The glare's F-number (`GLARE_SHUTTER`).
fn aperture() -> f32 {
    return aperture_at(exposure.adapting);
}

fn aperture_at(adapting: f32) -> f32 {
    let ev100 = log2(max(adapting, 1e-6) * 100.0 / CALIBRATION);
    return clamp(sqrt(exp2(ev100) * GLARE_SHUTTER), GLARE_APERTURE.x, GLARE_APERTURE.y);
}

// Each channel's share of the light the Airy pattern at F-number `n` puts beyond `r` texels of a level `height` texels
// high (`GLARE_SENSOR`).
fn airy_beyond(r: f32, height: f32, n: f32) -> vec3<f32> {
    return min(2.0 * GLARE_WAVELENGTH * n / (PI * PI * r / height * GLARE_SENSOR), vec3<f32>(1.0));
}

// The glare's weight of the level `height` texels high (`GLARE_EDGE`).
fn glare_weight(height: f32, n: f32) -> vec3<f32> {
    return airy_beyond(GLARE_EDGE * 0.5, height, n) - airy_beyond(GLARE_EDGE, height, n);
}

// The 13 taps of Jimenez's filter of `source` about `uv`, each by `glare_tap`.
fn glare_filter(uv: vec2<f32>, first: bool) -> vec4<f32> {
    let a = glare_tap(uv, vec2<f32>(-2.0, -2.0), first);
    let b = glare_tap(uv, vec2<f32>(0.0, -2.0), first);
    let c = glare_tap(uv, vec2<f32>(2.0, -2.0), first);
    let d = glare_tap(uv, vec2<f32>(-2.0, 0.0), first);
    let e = glare_tap(uv, vec2<f32>(0.0, 0.0), first);
    let f = glare_tap(uv, vec2<f32>(2.0, 0.0), first);
    let g = glare_tap(uv, vec2<f32>(-2.0, 2.0), first);
    let h = glare_tap(uv, vec2<f32>(0.0, 2.0), first);
    let i = glare_tap(uv, vec2<f32>(2.0, 2.0), first);
    let j = glare_tap(uv, vec2<f32>(-1.0, -1.0), first);
    let k = glare_tap(uv, vec2<f32>(1.0, -1.0), first);
    let l = glare_tap(uv, vec2<f32>(-1.0, 1.0), first);
    let m = glare_tap(uv, vec2<f32>(1.0, 1.0), first);
    return e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
}

// A tap of the glare chain: from the frame (`first`), exposed (held under `CHAIN_CAP`), and its luminance (held under
// `BROAD_OVER_WHITE`); from a level, as it is.
fn glare_tap(uv: vec2<f32>, offset: vec2<f32>, first: bool) -> vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let s = textureSampleLevel(source, linear_sampler, uv + offset * texel, 0.0);
    if (!first) {
        return s;
    }
    let broad = min(s.rgb, vec3<f32>(BROAD_OVER_WHITE * GLARE_WHITE / exposure.value));
    return vec4<f32>(clamp(s.rgb, vec3<f32>(0.0), vec3<f32>(CHAIN_CAP)) * exposure.value, luminance(broad));
}

// The glare chain's first level: in `rgb` the exposed light weighted by the level's glare (`glare_weight`; exposed, so
// the dim nights' weighted light stays in the half floats' range), in `a` the frame's luminance, which the local
// exposure reads.
@fragment
fn fs_glare_first(v: Out) -> @location(0) vec4<f32> {
    let sum = glare_filter(v.uv, true);
    let height = f32(textureDimensions(source).y / 2u);
    return vec4<f32>(sum.rgb * glare_weight(height, aperture()), sum.a);
}

// The glare chain's next levels (`fs_glare_first`): the level above's light, its weight exchanged for this level's.
@fragment
fn fs_glare_down(v: Out) -> @location(0) vec4<f32> {
    let sum = glare_filter(v.uv, false);
    let above = f32(textureDimensions(source).y);
    let n = aperture();
    return vec4<f32>(sum.rgb * glare_weight(floor(above * 0.5), n) / glare_weight(above, n), sum.a);
}

// Adds a level to the one above, a tent filter of 3x3 taps, every channel.
@fragment
fn fs_glare_up(v: Out) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    var sum = vec4<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let w = f32((2 - abs(x)) * (2 - abs(y)));
            sum += textureSampleLevel(source, linear_sampler, v.uv + vec2<f32>(f32(x), f32(y)) * texel, 0.0) * w;
        }
    }
    return sum / 16.0;
}

var<workgroup> histogram: array<atomic<u32>, ZONE_BINS>;

// Bins the scene's light (`source`) into `meter_histogram`: a workgroup for each 16x16 of the grid's points, each
// binning its own before adding them in.
@compute @workgroup_size(16, 16)
fn cs_histogram(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) index: u32) {
    atomicStore(&histogram[index], 0u);
    workgroupBarrier();
    if (all(id.xy < METER_GRID)) {
        let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(METER_GRID);
        let l = luminance(textureSampleLevel(source, linear_sampler, uv, 0.0).rgb) / max(meter.pre_exposure, 1e-12);
        let bin = u32(clamp((log2(max(l, 1e-6)) - EV_LOW) / EV_SPAN * f32(BINS), 0.0, f32(BINS - 1u)));
        let depth = textureLoad(meter_depth, vec2<i32>(uv * vec2<f32>(textureDimensions(meter_depth))), 0);
        let in_car = meter.cabin_depth > 0.0;
        let cabin = in_car && depth > meter.cabin_depth;
        atomicAdd(&histogram[bin], select(WEIGHT, select(IN_CAR_WEIGHT.x, IN_CAR_WEIGHT.y, cabin), in_car));
        if (cabin) {
            atomicAdd(&histogram[BINS + bin], WEIGHT);
        }
    }
    workgroupBarrier();
    let count = atomicLoad(&histogram[index]);
    if (count != 0u) {
        atomicAdd(&meter_histogram[index], count);
    }
}

var<workgroup> counts: array<u32, ZONE_BINS>;

// The light `c` (cd/m2) with the rods' signal added (`LMSR`).
fn rod_vision(c: vec3<f32>) -> vec3<f32> {
    let q = max(c * ROD_RETINA, vec3<f32>(0.0)) * LMSR;
    let gain = inverseSqrt(1.0 + 0.33 / CONE_MAX * (q.xyz + ROD_INPUT * q.w));
    return c + ROD_SHIFT * (gain * q.w) / ROD_RETINA;
}

// The exposure compensation curve (`KEY`'s scale) for an adapting luminance `l`, cd/m2.
fn compensation(l: f32) -> f32 {
    return krawczyk_key(l) / krawczyk_key(SUNNY_LUMINANCE);
}

fn krawczyk_key(l: f32) -> f32 {
    return 1.03 - 2.0 / (2.0 + log2(l + 1.0) * 0.30103);
}

// The mean log2 light of the weight between the percentiles of the bins from `first`, and the weight binned.
fn metered(first: u32) -> vec2<f32> {
    var total = 0.0;
    for (var bin = 0u; bin < BINS; bin++) {
        total += f32(counts[first + bin]);
    }
    let range = PERCENTILES * total;
    var below = 0.0;
    var sum = 0.0;
    var counted = 0.0;
    for (var bin = 0u; bin < BINS; bin++) {
        let w = f32(counts[first + bin]);
        let kept = max(min(below + w, range.y) - max(below, range.x), 0.0);
        sum += kept * (EV_LOW + (f32(bin) + 0.5) / f32(BINS) * EV_SPAN);
        counted += kept;
        below += w;
    }
    return vec2<f32>(sum / max(counted, 1.0), total);
}

// The adapting luminance (held within `EV100_RANGE`) and the exposure aimed at for a metered log2 light.
fn aimed(light: f32) -> vec2<f32> {
    let ev100 = clamp(log2(exp2(light) * 100.0 / CALIBRATION), EV100_RANGE.x, EV100_RANGE.y);
    let held = exp2(ev100) * CALIBRATION / 100.0;
    return vec2<f32>(held, KEY * compensation(held) / (held * meter.pre_exposure) * meter.grade.x);
}

// The exposure from `cs_histogram`'s histogram, eased from the last frame's (`meter_exposure`), and the cabin's.
@compute @workgroup_size(128)
fn cs_meter(@builtin(local_invocation_index) index: u32) {
    counts[index] = atomicExchange(&meter_histogram[index], 0u);
    counts[BINS + index] = atomicExchange(&meter_histogram[BINS + index], 0u);
    workgroupBarrier();
    if (index != 0u) {
        return;
    }
    let outside = aimed(metered(0u).x);
    let held = outside.x;
    let aim = outside.y;
    // Without the cabin in view (or outside the car) the cabin's follows the outside's.
    let inside = metered(BINS);
    let cabin = select(aim, aimed(inside.x).y, inside.y > 0.0);
    let last = meter_exposure.value;
    if (meter.snap != 0.0 || !(last > 0.0) || !(meter_exposure.adapting > 0.0) || !(meter_exposure.cabin > 0.0)) {
        meter_exposure.value = aim;
        meter_exposure.adapting = held;
        meter_exposure.cabin = cabin;
    } else {
        let speed = select(ADAPT_SPEED.y, ADAPT_SPEED.x, aim < last);
        let eased = 1.0 - exp(-meter.step * speed);
        meter_exposure.value = exp2(mix(log2(last), log2(aim), eased));
        meter_exposure.adapting = exp2(mix(log2(meter_exposure.adapting), log2(held), eased));
        let cabin_speed = select(ADAPT_SPEED.y, ADAPT_SPEED.x, cabin < meter_exposure.cabin);
        let cabin_eased = 1.0 - exp(-meter.step * cabin_speed);
        meter_exposure.cabin = exp2(mix(log2(meter_exposure.cabin), log2(cabin), cabin_eased));
    }
    let adapting = max(meter_exposure.adapting, 1e-6);
    meter_exposure.white = luminance(rod_vision(vec3<f32>(adapting))) / adapting;
    let n = aperture_at(meter_exposure.adapting);
    var spread = vec3<f32>(0.0);
    for (var level = 1u; level <= u32(LEVELS); level++) {
        spread += glare_weight(f32(textureDimensions(source).y >> level), n);
    }
    meter_exposure.spread = spread;
}

// Whether the pixel at `uv` takes the cabin's exposure: it and its 8 neighbours are the cabin's. The edge's pixels,
// whose colour the resolve mixed with the outside's, keep the outside's (a softer blend drew a dark band inside the
// pillars, or a bright halo outside them).
fn in_cabin(uv: vec2<f32>) -> bool {
    let size = vec2<i32>(textureDimensions(composite_depth));
    let p = vec2<i32>(uv * vec2<f32>(size));
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            if (textureLoad(composite_depth, clamp(p + vec2<i32>(x, y), vec2<i32>(0), size - 1), 0) <= meter.cabin_depth) {
                return false;
            }
        }
    }
    return true;
}

// The shafts' map (a quarter of the frame's size) at `uv`: its four texels around `uv` weighted bilinearly and by how
// near their depth (alpha: each texel's, `fs_beams`' and `fs_march`'s) is to the frame's here, so that the lamps' beams
// and the shaded air stop at the edges of what stands in front of them.
fn shafts_at(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(shafts_map));
    let p = uv * vec2<f32>(size) - 0.5;
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    let a = textureLoad(shafts_map, clamp(base, vec2<i32>(0), size - 1), 0);
    let b = textureLoad(shafts_map, clamp(base + vec2<i32>(1, 0), vec2<i32>(0), size - 1), 0);
    let c = textureLoad(shafts_map, clamp(base + vec2<i32>(0, 1), vec2<i32>(0), size - 1), 0);
    let d = textureLoad(shafts_map, clamp(base + vec2<i32>(1, 1), vec2<i32>(0), size - 1), 0);
    var w = vec4<f32>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    // Four texels of one depth (open sky, a cleared map) share the depth weight, which cancels: the plain
    // bilinear sum, without the frame's depth, the costly read.
    let texel_depth = vec4<f32>(a.a, b.a, c.a, d.a);
    if (any(texel_depth != vec4<f32>(a.a))) {
        let depth_size = vec2<i32>(textureDimensions(composite_depth));
        let depth = textureLoad(composite_depth, clamp(vec2<i32>(uv * vec2<f32>(depth_size)), vec2<i32>(0), depth_size - 1), 0);
        let off = abs(texel_depth - depth) / max(max(texel_depth, vec4<f32>(depth)), vec4<f32>(1e-6));
        w *= exp(-off * off * 100.0) + 1e-3;
    }
    let sum = a.rgb * w.x + b.rgb * w.y + c.rgb * w.z + d.rgb * w.w;
    return sum / max(w.x + w.y + w.z + w.w, 1e-6);
}

fn tone_map(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + luminance(c));
}

// The scene at `uv` sharpened by `meter.grade.z`: the pixel's difference from its four edge neighbours added back, held
// within their range (no ringing past an edge), in Karis's tone-mapped space (c / (1 + luma)) so that a bright highlight
// does not carry the contrast.
fn sharpened(uv: vec2<f32>) -> vec3<f32> {
    let c = max(textureSampleLevel(source, linear_sampler, uv, 0.0).rgb, vec3<f32>(0.0));
    if (meter.grade.z <= 0.0) {
        return c;
    }
    let d = 1.0 / vec2<f32>(textureDimensions(source));
    let n = tone_map(max(textureSampleLevel(source, linear_sampler, uv + vec2<f32>(0.0, -d.y), 0.0).rgb, vec3<f32>(0.0)));
    let s = tone_map(max(textureSampleLevel(source, linear_sampler, uv + vec2<f32>(0.0, d.y), 0.0).rgb, vec3<f32>(0.0)));
    let e = tone_map(max(textureSampleLevel(source, linear_sampler, uv + vec2<f32>(d.x, 0.0), 0.0).rgb, vec3<f32>(0.0)));
    let w = tone_map(max(textureSampleLevel(source, linear_sampler, uv + vec2<f32>(-d.x, 0.0), 0.0).rgb, vec3<f32>(0.0)));
    let t = tone_map(c);
    let low = min(min(min(n, s), min(e, w)), t);
    let high = max(max(max(n, s), max(e, w)), t);
    let sharp = clamp(t + meter.grade.z * (t - (n + s + e + w) * 0.25), low, high);
    return sharp / max(1.0 - luminance(sharp), 1.0e-4);
}

@fragment
fn fs_composite(v: Out) -> @location(0) vec4<f32> {
    // The cabin debug view (`viewer --debug-view`): the cabin's pixels carry their debug grey (`cabin_debug` in
    // `cabin.wgsl`), already in display grey, so they bypass the sharpening, the glare, the shafts, the exposure,
    // the rod vision, the local exposure and the tone curve, and read as drawn. Every other pixel draws as normal.
    if (DEBUG_VIEW != 0u && in_cabin(v.uv)) {
        return vec4<f32>(max(textureSampleLevel(source, linear_sampler, v.uv, 0.0).rgb, vec3<f32>(0.0)), 1.0);
    }
    let scene = sharpened(v.uv);
    let bloomed = textureSampleLevel(bloom, linear_sampler, v.uv, 0.0);
    // The cabin's exposure (`CABIN_STOPS`). The glare and the shafts (the light scattered on the way, mostly the
    // outside's) keep the outside's: at the cabin's they lit the pillars with a green haze.
    let cabin = CABIN && in_cabin(v.uv);
    var exposed = exposure.value;
    var veil = 1.0;
    if (cabin) {
        exposed = clamp(exposure.cabin, exposure.value, exposure.value * exp2(CABIN_STOPS));
        // Tapered (`CABIN_TAPER`): the lift over the outside's exposure in full on the cabin's dark, less on what is
        // bright at the outside's (the sun on it), `x + (g - 1) k x / (k + x)`, which keeps the order of the values.
        // `x` is the darker of the pixel and the frame about it (the glare chain's luminance), so a dial's white
        // figures keep the lift of the dark face they are on.
        let x = min(luminance(scene) / exposure.white, bloomed.a / LEVELS) * exposure.value;
        let k = CABIN_TAPER * KEY;
        exposed = exposure.value * (1.0 + (exposed / exposure.value - 1.0) * k / (k + x));
        veil = exposure.value / exposed;
    }
    // The chain's levels carry the glare (exposed) and the frame's luminance; the glare takes its light from the scene
    // (the levels' weights, `glare_weight`, summed: `Exposure::spread`).
    var lit = scene * (1.0 - exposure.spread) + bloomed.rgb / exposure.value * veil;
    if (SHAFTS) {
        lit += shafts_at(v.uv) * veil;
    }
    // Rod vision (`rod_vision`) on the light in cd/m2, over what it adds to white at the adapting luminance: the
    // exposure meters the cones' light, the rods shift the hues and the light between colours.
    let pre = max(meter.pre_exposure, 1e-12);
    lit = rod_vision(lit / pre) * pre / exposure.white;
    // Local exposure (`LOCAL_FROM`).
    let broad = log2(max(bloomed.a / LEVELS * exposed, 1e-9) / KEY);
    let linear = max(lit * exposed * exp2(-LOCAL_GIVE * max(broad - LOCAL_FROM, 0.0)), vec3<f32>(0.0));
    let d = display();
    let c = gt7(linear, d);
    let grey = dot(c, vec3<f32>(0.299, 0.587, 0.114));
    return vec4<f32>(clamp(mix(vec3<f32>(grey), c, meter.grade.y), vec3<f32>(0.0), vec3<f32>(d.top)), 1.0);
}
