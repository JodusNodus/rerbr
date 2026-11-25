//! The finish of a converted car's material as the shaders read it (ours): [`finish_uniform`].

use rbr_formats::car_parts::{Finish, FinishKind};

/// A converted car's material's finish (`rbr_formats::car_parts::Finish`, DiRT Rally's shaders' constants) as
/// `finish.wgsl` reads it (`mode.finish`): `[kind (1 plain, 2 flock, 3 carbon, 4 gated, 5 exterior), Fresnel min, max, power]`,
/// `[environment rgb, blur (-1: no reflection)]`, `[Phong power, Phong colour times its scale]`, `[carbon strength,
/// power, falloff power, flock noise size]` (not a carbon: `[specular gain, 0, 0, noise size]`); zero for a face with
/// none.
pub fn finish_uniform(finish: Option<&Finish>) -> [[f32; 4]; 4] {
    let Some(f) = finish else { return [[0.0; 4]; 4] };
    let kind = match f.kind {
        FinishKind::Plain => 1.0,
        FinishKind::Flock => 2.0,
        FinishKind::Carbon => 3.0,
        FinishKind::Gated => 4.0,
        FinishKind::Exterior => 5.0,
    };
    let [power, scale, r, g, b] = f.phong;
    // The last row: a carbon's weave, else the specular's gain; the flock's noise.
    let [x, y, z] = if f.kind == FinishKind::Carbon { f.carbon } else { [f.specular, 0.0, 0.0] };
    [
        [kind, f.fresnel[0], f.fresnel[1], f.fresnel[2]],
        f.environment.unwrap_or([0.0, 0.0, 0.0, -1.0]),
        [power, scale * r, scale * g, scale * b],
        [x, y, z, f.noise],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finish_packs_as_the_shader_reads_it() {
        let finish = Finish {
            kind: FinishKind::Flock,
            fresnel: [0.0, 1.0, 2.5],
            environment: None,
            phong: [15.0, 2.0, 1.0, 0.5, 0.25],
            noise: 10.0,
            carbon: [1.0, 20.0, 3.0],
            specular: 1.8,
        };
        let packed = finish_uniform(Some(&finish));
        assert_eq!(packed[0], [2.0, 0.0, 1.0, 2.5]);
        // No environment: no reflection.
        assert_eq!(packed[1][3], -1.0);
        assert_eq!(packed[2], [15.0, 2.0, 1.0, 0.5]);
        assert_eq!(packed[3], [1.8, 0.0, 0.0, 10.0]);
        let carbon = finish_uniform(Some(&Finish { kind: FinishKind::Carbon, ..finish }));
        assert_eq!(carbon[3], [1.0, 20.0, 3.0, 10.0]);
        assert_eq!(finish_uniform(None), [[0.0; 4]; 4]);
    }
}
