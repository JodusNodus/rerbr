//! Every WGSL module the renderer builds, translated to SPIR-V the way wgpu's Vulkan backend does it (the user plays
//! on Linux/Vulkan; the renderer is mostly run on Metal): parsed, validated with the capabilities wgpu allows on a
//! device with no optional shader features, its overrides set per entry point, and written with the backend's SPIR-V
//! options (`wgpu-hal` `vulkan::adapter` and `vulkan::device::compile_stage`).

use naga::back::spv;
use naga::valid::{Capabilities, ValidationFlags, Validator};

/// The capabilities `wgpu-core`'s `create_validator` gives a Vulkan device with every downlevel flag and none of the
/// optional shader features ([`crate::renderer::request_gpu`] asks for BC compression and timestamps only).
fn capabilities() -> Capabilities {
    Capabilities::MULTISAMPLED_SHADING | Capabilities::CUBE_ARRAY_TEXTURES | Capabilities::SHADER_FLOAT16_IN_FLOAT32
}

/// The backend's writer options on a Vulkan 1.3 device without robust buffer or image access (the bounds checks
/// written in), its SPIR-V capabilities the ones every device has plus the downlevel ones.
fn options() -> spv::Options<'static> {
    spv::Options {
        lang_version: (1, 6),
        flags: spv::WriterFlags::LABEL_VARYINGS | spv::WriterFlags::FORCE_POINT_SIZE,
        capabilities: Some(
            [
                spv::Capability::Shader,
                spv::Capability::Matrix,
                spv::Capability::Sampled1D,
                spv::Capability::Image1D,
                spv::Capability::ImageQuery,
                spv::Capability::DerivativeControl,
                spv::Capability::StorageImageExtendedFormats,
                spv::Capability::SampledCubeArray,
                spv::Capability::SampleRateShading,
            ]
            .into_iter()
            .collect(),
        ),
        bounds_check_policies: naga::proc::BoundsCheckPolicies {
            index: naga::proc::BoundsCheckPolicy::Restrict,
            buffer: naga::proc::BoundsCheckPolicy::Restrict,
            image_load: naga::proc::BoundsCheckPolicy::Restrict,
            binding_array: naga::proc::BoundsCheckPolicy::Unchecked,
        },
        zero_initialize_workgroup_memory: spv::ZeroInitializeWorkgroupMemoryMode::Polyfill,
        force_loop_bounding: true,
        ..Default::default()
    }
}

/// Builds `source` for every entry point with each of `constants` (override name and value pairs), as pipelines do.
/// Returns the failures.
fn check(name: &str, source: &str, constants: &[Vec<(&str, f64)>]) -> Vec<String> {
    let module = match naga::front::wgsl::parse_str(source) {
        Ok(module) => module,
        Err(e) => return vec![format!("{name}: parse: {}", e.emit_to_string(source))],
    };
    let info = match Validator::new(ValidationFlags::all(), capabilities()).validate(&module) {
        Ok(info) => info,
        Err(e) => return vec![format!("{name}: validate: {}", e.emit_to_string(source))],
    };
    let options = options();
    let mut failures = Vec::new();
    // naga 30 refuses a constant the module does not declare; a flag set shared by several modules holds some of
    // those, which their pipelines never pass.
    let declared = |name: &str| module.overrides.iter().any(|(_, o)| o.name.as_deref() == Some(name));
    for set in constants {
        let set: naga::back::PipelineConstants =
            set.iter().filter(|&&(k, _)| declared(k)).map(|&(k, v)| (k.to_owned(), v)).collect();
        for entry in &module.entry_points {
            let at = format!("{name} {} {set:?}", entry.name);
            let (module, info) = match naga::back::pipeline_constants::process_overrides(
                &module,
                &info,
                Some((entry.stage, &entry.name)),
                &set,
            ) {
                Ok(done) => done,
                Err(e) => {
                    failures.push(format!("{at}: overrides: {e}"));
                    continue;
                }
            };
            let pipeline = spv::PipelineOptions { shader_stage: entry.stage, entry_point: entry.name.clone() };
            if let Err(e) = spv::write_vec(&module, &info, &options, Some(&pipeline)) {
                failures.push(format!("{at}: spv: {e}"));
            }
        }
    }
    failures
}

/// Every combination of `names` set to 0 or 1.
fn flags<'a>(names: &[&'a str]) -> Vec<Vec<(&'a str, f64)>> {
    (0..1u32 << names.len())
        .map(|bits| names.iter().enumerate().map(|(i, &n)| (n, f64::from(bits >> i & 1))).collect())
        .collect()
}

#[test]
fn every_shader_builds_for_vulkan() {
    let none = || vec![Vec::new()];
    let mut failures = Vec::new();
    failures.extend(check(
        "stage",
        crate::renderer::SHADER,
        &flags(&["ALPHA_TEST", "WATER_PLUS", "CABIN", "LIGHTING", "MIRRORS", "GRASS"]),
    ));
    let taps = [("SSR_RAYS", 1.0), ("SSR_ITERATIONS", 32.0)];
    failures.extend(check("stage quality", crate::renderer::SHADER, &[taps.to_vec()]));
    failures.extend(check("depth pyramid", crate::renderer::PYRAMID_SHADER, &none()));
    let rain = none().into_iter().map(|c| [c, vec![("NEAR_COUNT", 1.0)]].concat()).collect::<Vec<_>>();
    failures.extend(check("rain", &crate::effects::rain_source(), &rain));
    failures.extend(check("particles", &crate::particles::gpu::source(), &none()));
    failures.extend(check("particle target", crate::particle_target::SHADER, &none()));
    failures.extend(check("post", crate::post::SHADER, &flags(&["SHAFTS"])));
    failures.extend(check("light shafts", crate::light_shafts::SHADER, &none()));
    failures.extend(check("taa", crate::taa::SHADER, &none()));
    failures.extend(check("motion blur", crate::motion_blur::SHADER, &none()));
    failures.extend(check("skid marks", crate::skidmarks_pass::SHADER, &none()));
    failures.extend(check("tyre marks", crate::skidmarks_pass::TYRE_SHADER, &none()));
    failures.extend(check("glass", crate::glass::gpu::SHADER, &none()));
    failures.extend(check("glass sim", crate::glass::sim::SHADER, &none()));
    failures.extend(check("sun", crate::sun::SHADER, &none()));
    failures.extend(check("sky occlusion", crate::sky_occlusion::SHADER, &none()));
    failures.extend(check("bounce", crate::bounce::SHADER, &none()));
    failures.extend(check("bounce surfaces", crate::bounce::SURFACE_SHADER, &none()));
    failures.extend(check("skylight", crate::skylight::SHADER, &none()));
    failures.extend(check("dials", crate::dials::gpu::SHADER, &none()));
    failures.extend(check("shadow range", crate::renderer::RANGE_SHADER, &none()));
    failures.extend(check("mip blit", crate::renderer::BLIT, &none()));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Checks a hand-mirrored Rust GPU struct against its WGSL twin: the size and each member's offset, by order (the
/// names differ). `offsets` are the Rust fields' `offset_of!`, in declaration order.
pub(crate) fn check_layout(source: &str, name: &str, size: usize, offsets: &[(&str, usize)]) {
    let module =
        naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
    let found = module.types.iter().find_map(|(_, t)| match (&t.name, &t.inner) {
        (Some(n), naga::TypeInner::Struct { members, span }) if n == name => Some((members, *span as usize)),
        _ => None,
    });
    let (members, span) = found.unwrap_or_else(|| panic!("WGSL struct {name} not found"));
    assert_eq!(size, span, "{name}: Rust size {size}, WGSL size {span}");
    assert_eq!(offsets.len(), members.len(), "{name}: {} Rust fields, {} WGSL members", offsets.len(), members.len());
    for ((field, at), member) in offsets.iter().zip(members) {
        let wgsl = member.name.as_deref().unwrap_or("?");
        assert_eq!(
            *at as u32, member.offset,
            "{name}: Rust field {field} at {at}, WGSL member {wgsl} at {}",
            member.offset
        );
    }
}

/// A WGSL module's `const` of a literal value (a limit the Rust side copies).
pub(crate) fn wgsl_const(source: &str, name: &str) -> f64 {
    let module = naga::front::wgsl::parse_str(source).expect("parse");
    let (_, c) = module.constants.iter().find(|(_, c)| c.name.as_deref() == Some(name)).expect("const");
    match module.global_expressions[c.init] {
        naga::Expression::Literal(naga::Literal::U32(v)) => f64::from(v),
        naga::Expression::Literal(naga::Literal::F32(v)) => f64::from(v),
        ref other => panic!("{name} is not a literal: {other:?}"),
    }
}

/// A test that `$t`'s size and the offsets of its listed fields (in order, a nested field as `a.b`; those the WGSL
/// struct packs into one member, only the first) are those of the WGSL struct `$name` in `$source`.
macro_rules! wgsl_layout {
    ($test:ident, $source:expr, $name:literal, $t:ty { $($($field:ident).+),+ $(,)? }) => {
        #[test]
        fn $test() {
            crate::shader_check::check_layout(
                &$source,
                $name,
                std::mem::size_of::<$t>(),
                &[$((stringify!($($field).+), std::mem::offset_of!($t, $($field).+))),+],
            );
        }
    };
}
pub(crate) use wgsl_layout;
