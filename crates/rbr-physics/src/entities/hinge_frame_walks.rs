//! The frames the hinged body is carried through for its signals: the car's carries of the rows of `axes_rf` and of the
//! products of the hinge rotation's rows with it, the car's transform of the frame matrix's product with
//! `inverse_origin_bf` (plus the hinge point), then the carry of the face normal and of the three basis vectors on the
//! hinged body. The carries' outputs and the transform's output are scratch, so what the model decides is
//! the calls it makes and their input vectors ([`FrameWalkCalls`], answered by the caller).
//!
//! The sums keep the original's order, which changes the bits: `row_times_matrix` and `row_times_matrix_alt` (the
//! rotation rows times the definition matrix) and `vector_times_matrix`.

/// The inputs read from the hinged body and its definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameWalkInput {
    /// The identity frame (the root), as an object handle.
    pub identity: u32,
    /// The hinged body, as an object handle: the receiver of the axis and basis carries.
    pub body: u32,
    /// The body's car, as an object handle: the receiver of the matrix carries and the target of the transform.
    pub car: u32,
    /// The hinge rotation, one row per vector.
    pub hinge_rotation: [[f32; 3]; 3],
    /// The body's frame matrix ([`super::hinge_pose::PoseBody::frame_matrix`]): `vector_times_matrix`'s matrix.
    pub frame_matrix: [[f32; 3]; 3],
    /// The axes built from the hinge axis and the zero axis in the reference frame: its rows are carried, and the
    /// rotation rows are multiplied by it.
    pub axes_rf: [[f32; 3]; 3],
    /// The inverse origin of the body-frame axes build: `vector_times_matrix`'s vector.
    pub inverse_origin_bf: [f32; 3],
    /// The hinge point in the reference frame: added to the product before the transform.
    pub hinge_point_rf: [f32; 3],
    /// The face normal in the body frame: the vector carried on the body.
    pub face_normal_bf: [f32; 3],
}

/// What the frame walks need from the game: the carries and the car's transform, each with its input vector.
pub trait FrameWalkCalls {
    /// Carries `vector` from the identity frame into `receiver`'s frame; the output is not used.
    fn carry(&mut self, receiver: u32, vector: [f32; 3], identity: u32);
    /// The car's transform of `vector`; the output is not used.
    fn car_transform(&mut self, car: u32, vector: [f32; 3]);
}
