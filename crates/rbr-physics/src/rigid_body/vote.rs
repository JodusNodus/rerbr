//! The contact vote: with no list the call returns 0 and writes nothing. With a list it clears the two outputs
//! and, for each record, adds the impulse pair of a contact hit to them, lanes x, y, z in that order. 1 when a
//! record hits. The contact-list word doubles as the sleep flag.

use super::contact::Impulse;
use super::host::HostUnavailable;
use super::layout::Vec4;
use crate::math::sse;

/// The vote's call of the contact worker on record `k` (0-based): the pair on a hit, `None` on a miss.
pub trait ContactVoteHost {
    fn contact(&mut self, k: u32) -> Result<Option<Impulse>, HostUnavailable>;
}

/// The two outputs of a vote over a list (cleared, then summed) and whether a record hit.
/// Named by use: the force pass applies them as the force and the torque.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactVote {
    pub force: Vec4,
    pub torque: Vec4,
    pub hit: bool,
}

/// `base + delta` in lanes x, y, z; lane w is kept.
fn add3(base: Vec4, delta: Vec4) -> Vec4 {
    Vec4 { x: base.x + delta.x, y: base.y + delta.y, z: base.z + delta.z, w: base.w }
}

/// The vote over the list of `count` records, `None` when the body has no list (no call, no write).
pub fn vote(count: Option<u32>, host: &mut impl ContactVoteHost) -> Result<Option<ContactVote>, HostUnavailable> {
    let Some(count) = count else {
        return Ok(None);
    };
    let mut out = ContactVote::default();
    for k in 0..count {
        if let Some(i) = host.contact(k)? {
            out.force = add3(out.force, i.force);
            out.torque = add3(out.torque, i.torque);
            out.hit = true;
        }
    }
    Ok(Some(out))
}

/// The vote's answer when it votes: the index and the scale the caller takes from the output buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vote {
    pub index: u32,
    pub scale: f32,
}

/// The vote hook the table-vote worker calls: the position and the point (the position with its z lowered by the
/// argument). `None` when it does not vote.
pub trait VoteHost {
    fn vote(&mut self, pos: Vec4, point: Vec4) -> Result<Option<Vote>, HostUnavailable>;
}

/// What the ground probe returns and writes: the result, then the words it leaves at its two pointers, the index and
/// the drop (the wheel's names for them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundProbe {
    pub result: i32,
    pub index: u32,
    pub drop: f32,
}

/// Votes for the position `pos` with its z lowered by `drop`. When the vote is taken, the index is the vote's and the
/// drop is scaled by its scale; otherwise the index word is 1.0 and the drop is unchanged.
pub fn probe_ground(pos: Vec4, drop: f32, host: &mut impl VoteHost) -> Result<GroundProbe, HostUnavailable> {
    let point = Vec4 { z: sse::sub(pos.z, drop), ..pos };
    Ok(match host.vote(pos, point)? {
        Some(v) => GroundProbe { result: 1, index: v.index, drop: sse::mul(drop, v.scale) },
        None => GroundProbe { result: 0, index: 1.0f32.to_bits(), drop },
    })
}
