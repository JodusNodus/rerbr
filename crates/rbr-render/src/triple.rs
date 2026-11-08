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

use crate::coords;
use glam::{Mat4, Quat, Vec3};

/// Millimetres per inch.
const INCH: f32 = 25.4;
/// The largest side angle taken, degrees: the side screens at right angles to the centre one.
pub const MAX_ANGLE: f32 = 90.0;

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
        self.diagonal * INCH * aspect / (1.0 + aspect * aspect).sqrt()
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

    #[test]
    fn a_turned_screen_looks_along_its_yaw() {
        let screen = Screen { yaw: std::f32::consts::FRAC_PI_2, left: -1.0, right: 1.0, bottom: -1.0, top: 1.0 };
        let forward = screen.forward(Vec3::Z, Vec3::Y);
        assert!((forward - Vec3::X).length() < 1e-6, "{forward}");
    }
}
