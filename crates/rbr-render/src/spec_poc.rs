//! Proof of concept (ours, presentation only): Lagarde's specular occlusion on the car's reflections and the wet road
//! under the car, and multiple-scattering energy compensation on the car's GGX environment reflection. Each is off by
//! default, not in [`Enhance::NAMES`](crate::renderer::Enhance) and `all`; set the environment variable to anything but
//! `0` to turn it on: `RERBR_SPEC_OCC` (`SPEC_OCC` in `shader.wgsl`, `water.wgsl`), `RERBR_SPEC_ENERGY` (`SPEC_ENERGY`).

/// Whether the environment variable `name` is set and not `0`.
fn on(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v != "0")
}

/// The pipelines' `(SPEC_OCC, SPEC_ENERGY)` overrides.
pub(crate) fn overrides() -> [(&'static str, f64); 2] {
    [
        ("SPEC_OCC", f64::from(u8::from(on("RERBR_SPEC_OCC")))),
        ("SPEC_ENERGY", f64::from(u8::from(on("RERBR_SPEC_ENERGY")))),
    ]
}
