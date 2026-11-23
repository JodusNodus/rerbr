//! Triple screens (ours): three like monitors side by side, the side ones turned towards the player, each drawn as
//! its own view from one eye (`viewer --triple`).
//!
//! The eye looks down the centre screen's middle. Each side screen hinges on the centre's outer frame edge, turned
//! towards the player by the side angle; its image starts past its own frame. A screen's view looks along its normal
//! and is off axis: its image's edges as tangents in its own frame ([`Screen`]), so the scene runs on across the
//! frames as a window frame would cut it. The field of view the cameras set is the centre screen's: it places the
//! eye at the distance that sees the centre image at that angle, and the side screens follow at their angle and
//! frame ([`Rig::screens`]). At the seat's own distance's field of view ([`Rig::true_fov`]) the screens show the
//! world at its true size.
//!
//! A curved panel ([`Curved`], `viewer --curved`) is drawn as three flat segments along its arc: a triple rig without
//! frames whose side screens turn by the angle a segment spans ([`Curved::rig`]). There the field of view the cameras
//! set is the whole panel's; [`Screens::screens`] turns it into the centre segment's.

use crate::coords;
use glam::{Mat4, Quat, Vec3};

/// Millimetres per inch.
const INCH: f32 = 25.4;
/// The largest side angle taken, degrees: the side screens at right angles to the centre one.
pub const MAX_ANGLE: f32 = 90.0;

/// The views a frame is drawn as: three like screens, or a curved panel's three segments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screens {
    Triple(Rig),
    Curved(Curved),
}

impl Screens {
    /// The left, centre and right views at horizontal field of view `fov` (radians), each `aspect` wide per high: a
    /// triple rig's centre screen shows `fov`, a curved panel's whole width does.
    pub fn screens(&self, fov: f32, aspect: f32) -> [Screen; 3] {
        match self {
            Screens::Triple(rig) => rig.screens(fov, aspect),
            Screens::Curved(panel) => {
                let whole = aspect * SEGMENTS;
                panel.rig(whole).screens(panel.centre_fov(fov, whole), aspect)
            }
        }
    }
}

/// The players' setup: three like screens, the side ones turned in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rig {
    /// Each screen's diagonal, inches.
    pub diagonal: f32,
    /// The frame around each screen's image, millimetres: two of them part neighbouring images.
    pub bezel: f32,
    /// From the eye to the centre screen, centimetres.
    pub distance: f32,
    /// How far each side screen is turned towards the player from the centre screen's plane, degrees; `None` turns
    /// them to face the eye ([`Rig::side_angle`]).
    pub angle: Option<f32>,
}

/// One screen's view: turned `yaw` radians to the right of the centre one, its image's edges as tangents of the angle
/// off its own axis (right and up positive).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub yaw: f32,
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
}

impl Rig {
    /// Parses `viewer --triple`'s `DIAGONAL,BEZEL,DISTANCE[,ANGLE]`: inches, millimetres, centimetres and degrees
    /// (`auto` or left out: facing the eye).
    pub fn parse(text: &str) -> Option<Rig> {
        let mut parts = text.split(',').map(str::trim);
        let mut number = || parts.next().and_then(|p| p.parse::<f32>().ok()).filter(|v| v.is_finite());
        let (diagonal, bezel, distance) = (number()?, number()?, number()?);
        let angle = match parts.next() {
            None | Some("auto") => None,
            Some(degrees) => Some(degrees.parse::<f32>().ok().filter(|a| (0.0..=MAX_ANGLE).contains(a))?),
        };
        let rig = Rig { diagonal, bezel, distance, angle };
        (parts.next().is_none() && diagonal > 0.0 && bezel >= 0.0 && distance > 0.0).then_some(rig)
    }

    /// `viewer --triple`'s text for the rig ([`Rig::parse`]).
    pub fn arg(&self) -> String {
        let angle = self.angle.map_or_else(|| "auto".to_owned(), |a| a.to_string());
        format!("{},{},{},{angle}", self.diagonal, self.bezel, self.distance)
    }

    /// Each screen's image width, millimetres, for screens `aspect` wide per high.
    pub fn width(&self, aspect: f32) -> f32 {
        width(self.diagonal, aspect)
    }

    /// The side screens' turn towards the player, radians: the rig's, or the one that faces their images' middles at
    /// the eye (`tan(angle / 2)` is the half image and frame over the distance).
    pub fn side_angle(&self, aspect: f32) -> f32 {
        match self.angle {
            Some(degrees) => degrees.clamp(0.0, MAX_ANGLE).to_radians(),
            None => 2.0 * ((self.width(aspect) * 0.5 + self.bezel) / (self.distance * 10.0)).atan(),
        }
    }

    /// The centre screen's horizontal field of view at the rig's distance, radians: the one that shows the world at
    /// its true size.
    pub fn true_fov(&self, aspect: f32) -> f32 {
        2.0 * (self.width(aspect) * 0.5 / (self.distance * 10.0)).atan()
    }

    /// The left, centre and right screens' views when the centre one shows horizontal field of view `fov` (radians)
    /// on screens `aspect` wide per high.
    pub fn screens(&self, fov: f32, aspect: f32) -> [Screen; 3] {
        // In image widths: the eye's distance for `fov`, and the frame.
        let distance = 0.5 / (fov * 0.5).tan();
        let bezel = self.bezel / self.width(aspect);
        let half_height = 0.5 / aspect;
        let centre = Screen {
            yaw: 0.0,
            left: -0.5 / distance,
            right: 0.5 / distance,
            bottom: -half_height / distance,
            top: half_height / distance,
        };
        // The right screen hinges on the centre's outer frame edge, turned by `angle` towards the eye.
        let angle = self.side_angle(aspect);
        let (sin, cos) = angle.sin_cos();
        let hinge = 0.5 + bezel;
        let depth = hinge * sin + distance * cos;
        let across = hinge * cos - distance * sin + bezel;
        let right = Screen {
            yaw: angle,
            left: across / depth,
            right: (across + 1.0) / depth,
            bottom: -half_height / depth,
            top: half_height / depth,
        };
        let left = Screen { yaw: -angle, left: -right.right, right: -right.left, ..right };
        [left, centre, right]
    }
}

/// The width, millimetres, of an image `diagonal` inches across and `aspect` wide per high.
fn width(diagonal: f32, aspect: f32) -> f32 {
    diagonal * INCH * aspect / (1.0 + aspect * aspect).sqrt()
}

/// The flat segments a curved panel is drawn as.
const SEGMENTS: f32 = 3.0;

/// A curved panel (ours): one screen bent along a circle towards the player, drawn as [`SEGMENTS`] flat segments, the
/// chords of equal arcs. The segments' heights take the arc's pixels' aspect, about 0.2 % short at 1800R.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curved {
    /// The image's diagonal, inches.
    pub diagonal: f32,
    /// The curve's radius, millimetres (1800R: 1800).
    pub radius: f32,
    /// From the eye to the panel's middle, centimetres.
    pub distance: f32,
}

impl Curved {
    /// Parses `viewer --curved`'s `DIAGONAL,RADIUS,DISTANCE`: inches, millimetres and centimetres. A panel whose
    /// diagonal is half the circle or more is refused: at some window shape it would span a half circle.
    pub fn parse(text: &str) -> Option<Curved> {
        let numbers: Option<Vec<f32>> =
            text.split(',').map(|p| p.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0)).collect();
        let [diagonal, radius, distance] = numbers?[..] else { return None };
        (diagonal * INCH < std::f32::consts::PI * radius).then_some(Curved { diagonal, radius, distance })
    }

    /// `viewer --curved`'s text for the panel ([`Curved::parse`]).
    pub fn arg(&self) -> String {
        format!("{},{},{}", self.diagonal, self.radius, self.distance)
    }

    /// The image's width along the arc, millimetres, for a panel `aspect` wide per high.
    pub fn width(&self, aspect: f32) -> f32 {
        width(self.diagonal, aspect)
    }

    /// The angle each segment spans about the curve's centre, radians.
    fn segment_angle(&self, aspect: f32) -> f32 {
        self.width(aspect) / (SEGMENTS * self.radius)
    }

    /// The segments as a triple rig for a panel `aspect` wide per high: no frames, each side segment turned by the
    /// segment angle, each segment's chord as its screen's width and the eye at the centre chord.
    pub fn rig(&self, aspect: f32) -> Rig {
        let angle = self.segment_angle(aspect);
        let chord = 2.0 * self.radius * (angle * 0.5).sin();
        let sagitta = self.radius * (1.0 - (angle * 0.5).cos());
        Rig {
            diagonal: chord / width(1.0, aspect / SEGMENTS),
            bezel: 0.0,
            distance: self.distance - sagitta / 10.0,
            angle: Some(angle.to_degrees()),
        }
    }

    /// The centre segment's horizontal field of view (radians) when the whole panel, `aspect` wide per high, shows
    /// `fov`: the eye's distance in chords is where the outer segments' far edges are `fov` apart.
    pub fn centre_fov(&self, fov: f32, aspect: f32) -> f32 {
        let (sin, cos) = self.segment_angle(aspect).sin_cos();
        let distance = sin + (0.5 + cos) / (fov * 0.5).tan();
        2.0 * (0.5 / distance).atan()
    }

    /// The whole panel's horizontal field of view at the panel's distance, radians: the one that shows the world at
    /// its true size.
    pub fn true_fov(&self, aspect: f32) -> f32 {
        let half = self.width(aspect) / (2.0 * self.radius);
        let depth = self.distance * 10.0 - self.radius * (1.0 - half.cos());
        2.0 * (self.radius * half.sin()).atan2(depth)
    }
}

impl Screen {
    /// The view-to-clip matrix: [`coords::off_axis`] with near plane `near`.
    pub fn projection(&self, near: f32) -> Mat4 {
        coords::off_axis(self.left, self.right, self.bottom, self.top, near)
    }

    /// `forward` turned by the screen's yaw about `up`.
    pub fn forward(&self, forward: Vec3, up: Vec3) -> Vec3 {
        Quat::from_axis_angle(up.normalize(), self.yaw) * forward
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: f32 = 16.0 / 9.0;

    fn rig() -> Rig {
        Rig { diagonal: 27.0, bezel: 10.0, distance: 70.0, angle: None }
    }

    #[test]
    fn parses_the_viewer_argument_and_writes_it_back() {
        assert_eq!(Rig::parse("27,10,70"), Some(rig()));
        assert_eq!(Rig::parse("27, 10, 70, auto"), Some(rig()));
        let turned = Rig { angle: Some(55.0), ..rig() };
        assert_eq!(Rig::parse("27,10,70,55"), Some(turned));
        assert_eq!(Rig::parse(&turned.arg()), Some(turned));
        assert_eq!(Rig::parse(&rig().arg()), Some(rig()));
        for bad in ["", "27,10", "27,10,70,91", "0,10,70", "27,-1,70", "27,10,0", "27,10,70,auto,1", "a,10,70"] {
            assert_eq!(Rig::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_27_inch_screen_at_70_cm_shows_about_46_degrees() {
        let width = rig().width(WIDE);
        assert!((width - 597.7).abs() < 0.1, "{width}");
        let fov = rig().true_fov(WIDE).to_degrees();
        assert!((fov - 46.2).abs() < 0.1, "{fov}");
    }

    #[test]
    fn the_centre_screen_is_the_single_view() {
        let fov = 60.0f32.to_radians();
        let [_, centre, _] = rig().screens(fov, WIDE);
        assert_eq!(centre.yaw, 0.0);
        let single = coords::perspective(fov, WIDE, coords::NEAR);
        let diff = (centre.projection(coords::NEAR) - single).abs().to_cols_array();
        assert!(diff.iter().all(|d| *d < 1e-5), "{diff:?}");
    }

    #[test]
    fn the_side_screens_face_the_eye_by_default_and_mirror_each_other() {
        let [left, centre, right] = rig().screens(rig().true_fov(WIDE), WIDE);
        // Facing the eye at the true field of view: the image's middle lies on the screen's axis.
        assert!((right.left + right.right).abs() < 1e-5, "{right:?}");
        assert_eq!((left.yaw, left.left, left.right), (-right.yaw, -right.right, -right.left));
        // Each side image is as wide as the centre one, at the same distance from the eye.
        assert!(((right.right - right.left) - (centre.right - centre.left)).abs() < 1e-5);
    }

    #[test]
    fn without_frames_or_angle_the_views_meet() {
        let flat = Rig { bezel: 0.0, angle: Some(0.0), ..rig() };
        let [left, centre, right] = flat.screens(50.0f32.to_radians(), WIDE);
        assert!((right.left - centre.right).abs() < 1e-6 && (left.right - centre.left).abs() < 1e-6);
        // Turned, the right screen's left edge is the centre's right edge seen from its own axis.
        let turned = Rig { bezel: 0.0, angle: Some(40.0), ..rig() };
        let [_, centre, right] = turned.screens(50.0f32.to_radians(), WIDE);
        let edge = centre.right.atan();
        assert!((right.yaw + right.left.atan() - edge).abs() < 1e-5);
    }

    #[test]
    fn the_frames_hide_a_wedge_between_the_views() {
        let [_, centre, right] = Rig { angle: Some(0.0), ..rig() }.screens(rig().true_fov(WIDE), WIDE);
        // Two 10 mm frames at 700 mm hide about 1.4 degrees.
        let gap = (right.left.atan() - centre.right.atan()).to_degrees();
        assert!((gap - 1.36).abs() < 0.05, "{gap}");
    }

    const ULTRAWIDE: f32 = 32.0 / 9.0;

    fn panel() -> Curved {
        Curved { diagonal: 49.0, radius: 1800.0, distance: 70.0 }
    }

    /// The right segment's far edge, as the angle off the centre segment's axis.
    fn far_edge(screens: &[Screen; 3]) -> f32 {
        screens[2].yaw + screens[2].right.atan()
    }

    #[test]
    fn parses_the_curved_panel_and_refuses_a_half_circle() {
        assert_eq!(Curved::parse("49,1800,70"), Some(panel()));
        assert_eq!(Curved::parse(" 49, 1800 ,70"), Some(panel()));
        assert_eq!(Curved::parse(&panel().arg()), Some(panel()));
        // 49 inches is 1244.6 mm, half a circle of 396.2 mm radius.
        assert!(Curved::parse("49,397,70").is_some());
        for bad in ["", "49,1800", "49,1800,70,1", "0,1800,70", "49,0,70", "49,1800,-1", "49,396,70", "a,1800,70"] {
            assert_eq!(Curved::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_49_inch_1800r_panel_at_70_cm() {
        let close = |value: f32, expected: f32, what: &str| assert!((value - expected).abs() < 0.05, "{what} {value}");
        close(panel().width(ULTRAWIDE), 1198.1, "arc width");
        let rig = panel().rig(ULTRAWIDE);
        close(rig.side_angle(ULTRAWIDE / 3.0).to_degrees(), 12.71, "segment angle");
        close(rig.width(ULTRAWIDE / 3.0), 398.6, "chord");
        close((70.0 - rig.distance) * 10.0, 11.1, "sagitta");
        let fov = panel().true_fov(ULTRAWIDE);
        close(fov.to_degrees(), 88.7, "true field of view");
        close(panel().centre_fov(fov, ULTRAWIDE).to_degrees(), 32.3, "centre segment's");
        let flat = Rig { diagonal: 49.0, bezel: 0.0, distance: 70.0, angle: None };
        close(flat.true_fov(ULTRAWIDE).to_degrees(), 81.1, "flat");
    }

    #[test]
    fn the_segments_end_on_the_arc() {
        let rig = panel().rig(ULTRAWIDE);
        let (chord, angle) = (rig.width(ULTRAWIDE / 3.0), rig.side_angle(ULTRAWIDE / 3.0));
        // From the eye, millimetres: across and ahead. The curve's centre is the radius behind the panel's middle.
        let centre = (0.0, panel().distance * 10.0 - panel().radius);
        let joint = (chord * 0.5, rig.distance * 10.0);
        let end = (joint.0 + chord * angle.cos(), joint.1 - chord * angle.sin());
        for (x, z) in [joint, end] {
            let off = (x - centre.0).hypot(z - centre.1) - panel().radius;
            assert!(off.abs() < 0.01, "{off} mm off the arc at {x}, {z}");
        }
    }

    #[test]
    fn the_whole_panel_shows_the_set_field_of_view_and_the_segments_meet() {
        for degrees in [30.0f32, 65.0, 88.7, 120.0] {
            let fov = degrees.to_radians();
            let screens = Screens::Curved(panel()).screens(fov, ULTRAWIDE / 3.0);
            let [left, centre, right] = screens;
            assert!((2.0 * far_edge(&screens) - fov).abs() < 1e-5, "{degrees}");
            assert!((right.yaw + right.left.atan() - centre.right.atan()).abs() < 1e-5);
            assert!((left.yaw + left.right.atan() - centre.left.atan()).abs() < 1e-5);
        }
        // At the true field of view the eye sits at the seat's distance from the centre chord.
        let fov = panel().true_fov(ULTRAWIDE);
        let [_, centre, _] = Screens::Curved(panel()).screens(fov, ULTRAWIDE / 3.0);
        let rig = panel().rig(ULTRAWIDE);
        let eye = rig.width(ULTRAWIDE / 3.0) * 0.5 / centre.right;
        assert!((eye - rig.distance * 10.0).abs() < 0.05, "{eye}");
    }

    #[test]
    fn a_flat_panel_is_one_view_in_thirds() {
        let flat = Curved { radius: 1.0e6, ..panel() };
        let fov = 81.1f32.to_radians();
        let screens = Screens::Curved(flat).screens(fov, ULTRAWIDE / 3.0);
        let thirds = (fov * 0.5).tan() / 1.5;
        let [left, centre, right] = screens.map(|s| [s.yaw + s.left.atan(), s.yaw + s.right.atan()]);
        for (edges, expected) in [(left, [-3.0, -1.0]), (centre, [-1.0, 1.0]), (right, [1.0, 3.0])] {
            let expected = expected.map(|t: f32| (t * thirds * 0.5).atan());
            assert!(edges.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-4), "{edges:?} {expected:?}");
        }
    }

    #[test]
    fn a_turned_screen_looks_along_its_yaw() {
        let screen = Screen { yaw: std::f32::consts::FRAC_PI_2, left: -1.0, right: 1.0, bottom: -1.0, top: 1.0 };
        let forward = screen.forward(Vec3::Z, Vec3::Y);
        assert!((forward - Vec3::X).length() < 1e-6, "{forward}");
    }
}
