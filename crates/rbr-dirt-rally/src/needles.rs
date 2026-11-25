//! A DiRT car's needle gauges as the model's gauges file (`rbr_formats::car_gauges`), from its interior files.
//!
//! The interior config's analog displays name each needle mesh and its range
//! (`<display value="tach" node="int_needle_tach" min_value="0.0" max_value="12.0" factor="0.00955" long_way="1"/>`:
//! the shown value is the game's own, rad/s, m/s or bar, times `factor`). The scene holds the needle at its rest (the
//! range's bottom) and a node `<needle>_max` at the top, so the turn between them, the long way round where
//! `long_way` is 1, is the needle's sweep, about its node's place.
use crate::mesh::{self, Mat};
use crate::pssg::Pssg;
use rbr_formats::car_gauges::{Needle, Reading};

/// What the config's `value` reads, and the reading in our units per the game's unit (rad/s to rpm, m/s to km/h).
const VALUES: [(&str, Reading, f32); 3] =
    [("tach", Reading::Rpm, 30.0 / std::f32::consts::PI), ("speed", Reading::Kmh, 3.6), ("boost", Reading::Bar, 1.0)];

/// The needles of interior scene `pssg` that its `config` turns, in DiRT's frame, named by their nodes (`int_needle_tach`).
pub fn read(pssg: &Pssg, config: &str) -> Vec<Needle> {
    let worlds = mesh::node_worlds(pssg);
    displays(config, "analog")
        .filter_map(|display| {
            let attr = |name: &str| attribute(display, name);
            let number = |name: &str| attr(name)?.trim().parse::<f32>().ok();
            let node = attr("node")?;
            let (_, reading, unit) = VALUES.iter().find(|(v, ..)| Some(*v) == attr("value"))?;
            let factor = number("factor").filter(|f| *f != 0.0)?;
            let (rest, top) = (worlds.get(node)?, worlds.get(&format!("{node}_max"))?);
            let (axis, angle) = turn(rest, top)?;
            let sweep = match attr("long_way").map(str::trim) {
                Some("1") => angle - 360.0,
                _ => angle,
            };
            Some(Needle {
                node: node.to_owned(),
                reading: *reading,
                from: number("min_value")? / factor * unit,
                to: number("max_value")? / factor * unit,
                pivot: [rest[3][0], rest[3][1], rest[3][2]],
                axis,
                sweep,
            })
        })
        .collect()
}

/// The `<display .../>` elements of the config's `displays` of type `kind` (`analog`, `digital`).
pub(crate) fn displays<'a>(config: &'a str, kind: &str) -> impl Iterator<Item = &'a str> {
    let marker = format!("type=\"{kind}\"");
    let blocks = config.split("<displays").filter(move |b| b.trim_start().starts_with(&marker));
    blocks.flat_map(|b| b.split("</displays>").next().unwrap_or("").split("<display ").skip(1))
}

/// The value of attribute `name` in element text `element`.
pub(crate) fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let element = element.split('>').next()?;
    element
        .split(&format!(" {name}=\""))
        .nth(1)
        .or_else(|| element.strip_prefix(&format!("{name}=\"")))?
        .split('"')
        .next()
}

/// The turn from placement `rest` to `top` (row vectors), as a unit axis and degrees (0 to 180) about it, right handed;
/// `None` for none.
fn turn(rest: &Mat, top: &Mat) -> Option<([f32; 3], f32)> {
    // Row vectors: a point at rest goes to `p * rest^T * top` (rotations, so the inverse is the transpose); `c` is that
    // turn on column vectors.
    let c: [[f32; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| top[k][i] * rest[k][j]).sum()));
    let cos = ((c[0][0] + c[1][1] + c[2][2] - 1.0) / 2.0).clamp(-1.0, 1.0);
    let axis = [c[2][1] - c[1][2], c[0][2] - c[2][0], c[1][0] - c[0][1]];
    let length = axis.iter().map(|a| a * a).sum::<f32>().sqrt();
    (length > 1e-6).then(|| (axis.map(|a| a / length), cos.acos().to_degrees()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A placement turned by `deg` about z, at `at`.
    fn about_z(deg: f32, at: [f32; 3]) -> Mat {
        let (s, c) = deg.to_radians().sin_cos();
        // Row vectors: rows are the images of the axes.
        [[c, s, 0., 0.], [-s, c, 0., 0.], [0., 0., 1., 0.], [at[0], at[1], at[2], 1.]]
    }

    #[test]
    fn the_turn_between_two_placements_is_their_axis_and_angle() {
        let (axis, angle) = turn(&about_z(10.0, [1., 2., 3.]), &about_z(50.0, [1., 2., 3.])).unwrap();
        assert!((angle - 40.0).abs() < 1e-3 && (axis[2] - 1.0).abs() < 1e-5, "{axis:?} {angle}");
        let (axis, angle) = turn(&about_z(50.0, [0.; 3]), &about_z(10.0, [0.; 3])).unwrap();
        assert!((angle - 40.0).abs() < 1e-3 && (axis[2] + 1.0).abs() < 1e-5, "{axis:?} {angle}");
        assert_eq!(turn(&about_z(5.0, [0.; 3]), &about_z(5.0, [0.; 3])), None);
    }

    #[test]
    fn the_analog_displays_are_read_and_the_others_left() {
        let config = r#"<displays type="analog">
        <display value="tach" node="int_needle_tach" min_value="0.0" max_value="12.0" factor="0.00955" long_way="1"/>
    </displays>
    <displays type="digital">
        <display value="gear" node="int_indicator_gear" font="int_gear" num_chars="1"/>
    </displays>
    <displays type="analog_dummy">
        <display node="int_needle_01" deviation="0.001" speed="0.5"/>
    </displays>"#;
        let displays: Vec<&str> = displays(config, "analog").collect();
        assert_eq!(displays.len(), 1);
        assert_eq!(attribute(displays[0], "node"), Some("int_needle_tach"));
        assert_eq!(attribute(displays[0], "value"), Some("tach"));
        assert_eq!(attribute(displays[0], "long_way"), Some("1"));
        assert_eq!(attribute(displays[0], "font"), None);
    }
}
