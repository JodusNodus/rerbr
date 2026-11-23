//! DiRT Rally's frame to the stage's: DiRT Rally is right handed, y up, in metres (gate crossings lie on the drawn
//! terrain to 0.24 m, a gate's `left` is up × forward); the converted world is z up with DR's `(x, y, z)` at
//! `(x, -z, y)`, the frame `rbr_ac_tracks` builds a stage in (`assetto::ai::world`, the same for AC's frame). The map
//! is a rotation about x; a `btb::geometry::Triangle` holds the left-handed BTB mesh's winding, so a DR triangle
//! `(a, b, c)` becomes `(a, c, b)`, as the AC converter turns its faces.

/// A DiRT Rally position or direction in the stage's world.
pub fn world(p: [f32; 3]) -> [f64; 3] {
    [f64::from(p[0]), -f64::from(p[2]), f64::from(p[1])]
}

/// A direction (a normal) in the stage's world, single precision.
pub fn world_dir(n: [f32; 3]) -> [f32; 3] {
    [n[0], -n[2], n[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_is_z_and_the_rotation_keeps_handedness() {
        assert_eq!(world([0., 1., 0.]), [0., 0., 1.]);
        // x × y = z in DR maps to x × y = z in the world: a rotation, not a reflection.
        let (x, y) = (world([1., 0., 0.]), world([0., 1., 0.]));
        let z = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
        assert_eq!(z, world([0., 0., 1.]));
        assert_eq!(world_dir([0., 0., 1.]), [0., -1., 0.]);
    }
}
