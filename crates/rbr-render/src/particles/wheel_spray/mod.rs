//! Ours: the dust and water the tyres throw, in place of the game's tyre handlers, fake smoke, splash and wave when
//! the renderer draws its own particles (`--enhance particles`; not the game's look). Each wheel on the ground throws
//! by its surface ([`ground::material`]: kind, wetness, how loose), its slip, the car's speed and the standing water
//! under it:
//!
//! - dry loose ground: a dust trail that hangs and drifts, in the colour of the stage's loose road ([`dust::of_stage`]),
//!   more with slip;
//! - snow: powder clouds, as the dust;
//! - wet road: a grey mist plume and soft clumps of water off every tyre, more with speed; wet loose ground: the same
//!   in the ground's brown ([`MUD_SHADE`]), coarser and with less mist (a damp road: a third of the mist, no drops);
//! - standing water (the renderer's puddles with `--enhance wet`, the water surface, or the payload's +0x98): a splash
//!   out of both sides.
//!
//! Where the in-car views see it ([`front`]): a front tyre sliding sideways scrubs stones and a little dust out the way
//! it slides; in standing water a front tyre throws a short plume of water up at the front corner and partly over the
//! bonnet, whose splats land on the windscreen ([`WheelSpray::feed`]'s, `Thrown::bow`), and in snow a flurry; a braking
//! car's dust keeps its air's speed and rolls past it.
//!
//! The particles live in the stage's [`Simulation`] pools of their own, drawn lit and soft like the rest, with
//! sprites drawn here ([`sprites`]).

pub mod dust;
pub mod front;
pub mod ground;
pub mod sprites;

use glam::Vec3;
use rbr_recording::particles::TyreParticles;

use super::car::Pose;
use super::sim::{EmitterDef, Look, Rng, Simulation};
use ground::{smoothstep, Kind, Material};
use rbr_formats::pss::Shape;

/// The spray's pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pool {
    Dust,
    Powder,
    Mist,
    Drops,
    Splash,
    WetMist,
    MudMist,
    MudDrops,
    SplashMist,
    Debris,
    Haze,
    /// Ours, the front tyres': dust and stones scrubbed out sideways, snow kicked up, standing water thrown up at the
    /// front corner in clumps and mist.
    Kick,
    Fling,
    Flurry,
    Bow,
    BowMist,
}

const POOLS: [Pool; 16] = [
    Pool::Dust,
    Pool::Powder,
    Pool::Mist,
    Pool::Drops,
    Pool::Splash,
    Pool::WetMist,
    Pool::MudMist,
    Pool::MudDrops,
    Pool::SplashMist,
    Pool::Debris,
    Pool::Haze,
    Pool::Kick,
    Pool::Fling,
    Pool::Flurry,
    Pool::Bow,
    Pool::BowMist,
];

/// The share of the airborne dust's brightness that loose ground's spray has: wet earth is dark.
const MUD_SHADE: f32 = 0.6;
/// Wet clods are the ground's colour squared (darker, more saturated) times this.
const CLOD_SHADE: f32 = 0.45;
/// The dust in the air against the ground's colour ([`airborne`]).
const DUST_GREY: f32 = 0.35;
/// A young dust's brightness against the airborne dust's: dense and dark at the tyre.
const DUST_YOUNG: f32 = 1.0;
/// The plume's tan against the airborne dust's.
const DUST_PALE: f32 = 1.25;
const DUST_LIFT: f32 = 1.1;
/// Particles a second from one wheel at full strength.
const DUST_RATE: f32 = 55.0;
/// The stones and grit a dry wheel throws a second at full strength.
const DEBRIS_RATE: f32 = 150.0;
/// The faint dust left hanging on the road behind, as a share of the plume's rate.
const HAZE_SHARE: f32 = 0.5;
const POWDER_RATE: f32 = 70.0;
const MIST_RATE: f32 = 190.0;
/// How many times `MIST_RATE` a wheel on wet ground throws at full strength (the pools are sized for three times it).
const MIST_GAIN: f32 = 1.5;
/// A wet road's mist is fewer, larger, denser puffs than the damp haze: they stand out as lumps.
const WET_MIST_SHARE: f32 = 0.3;
/// A damp road's mist, as a share of the wet road's (barely a haze).
const DAMP_MIST: f32 = 1.0 / 3.0;
/// The mist off a damp loose ground (gravel, dirt, grass) as a share of a damp road's, and off a wet one as a share of
/// a wet road's: the water soaks into the mud, and the wheels throw it as drops more than as a fine spray.
const LOOSE_MIST: f32 = 0.25;
const MUD_MIST: f32 = 0.4;
const DROP_RATE: f32 = 600.0;
/// Loose wet ground throws this many times a road's drops: it is mud and clods, not film.
const MUD_DROP_GAIN: f32 = 1.0;
/// The most of the tread's speed a clod of mud keeps.
const MUD_KEPT: f32 = 0.3;
const SPLASH_RATE: f32 = 2500.0;
/// The white spray hanging in a splash's sheet, a second.
const SPLASH_MIST_RATE: f32 = 150.0;
/// The car speeds, m/s, over which the trailing spray grows to full; the slip speeds over which a sliding or
/// spinning tyre's does; the speeds over which standing water splashes.
const MOTION: [f32; 2] = [3.0, 25.0];
/// The speeds, m/s, over which the mist grows from nothing (about 20 km/h) to the dense tail (100 km/h).
const MIST_MOTION: [f32; 2] = [5.0, 28.0];
const SPIN: [f32; 2] = [1.0, 8.0];
const SPLASH_SPEED: [f32; 2] = [2.0, 15.0];
/// The wetness over which the ground sprays water: a damp road binds the dust and throws a faint mist, no drops.
const SPRAYS: [f32; 2] = [0.5, 1.0];
/// The water level of +0x98 that counts as a full puddle.
const FULL_WATER: f32 = 0.5;
/// The standing water of wet tarmac's film (`water.wgsl`'s `TARMAC_SHARE`: an even film, no puddles): shallow, so
/// half a puddle's splash everywhere.
const FILM_WATER: f32 = 0.5;
/// The tyre's radius, m (ours: the payload has none; rally tyres are 0.3 to 0.33).
const TYRE_RADIUS: f32 = 0.32;
/// Where the tread lets go of its water: from this angle behind the contact to this one, radians up the wheel's back.
const RELEASE: [f32; 2] = [0.2, 0.9];
/// The share of the tread's speed (relative to the hub) the water keeps when it lets go; the rest the arch, the mud
/// flap and the air take.
const KEPT: [f32; 2] = [0.06, 0.15];
/// A wheel entering standing water throws this many drops at once at full speed, when its water rises by at least
/// `BURST_RISE` in a frame.
const BURST: f32 = 60.0;
const BURST_RISE: f32 = 0.25;
/// The sideways slide speeds, m/s, over which a front tyre's scrub grows from nothing (a bend's slip angle) to full (a
/// slide).
const SCRUB: [f32; 2] = [0.8, 4.0];
/// A front tyre's scrub a second at full slide: the dust, the stones, the snow.
const KICK_RATE: f32 = 40.0;
const FLING_RATE: f32 = 120.0;
const FLURRY_RATE: f32 = 30.0;
/// The snow a front tyre ploughs up over the bonnet at speed, as a share of the full slide's flurry.
const PLOUGH: f32 = 0.3;
/// The clumps and mist of the plume a front tyre throws up in standing water, a second at full speed, and the share
/// of it that goes in over the bonnet rather than out at the corner.
const BOW_RATE: f32 = 90.0;
const BOW_MIST_RATE: f32 = 40.0;
const BOW_INWARD: f32 = 0.25;
/// The standing water over which a front tyre's plume grows from nothing (a wet rut's film) to full (a puddle).
const BOW_WATER: [f32; 2] = [0.15, 0.6];
/// The splats of it that land on the windscreen a second, a front tyre at full speed (`Thrown::bow`).
const BOW_SPLATS: f32 = 6.0;
/// A front tyre entering a puddle at speed throws this many clumps (and half as much mist) at once, and this many
/// splats at the windscreen: a puddle is crossed in a tenth of a second.
const BOW_BURST: f32 = 80.0;
const BOW_BURST_SPLATS: f32 = 3.0;
/// How far below the wheel a cloud (dust, powder, mist) may sink before it is dropped, m.
const CLOUD_FLOOR: f32 = 4.0;

/// The dust's colour on a stage with too little loose road to measure ([`dust::of_stage`]): a neutral grey-brown
/// (display colour, tinting the sprite's white).
pub const NEUTRAL_DUST: [f32; 3] = [0.7, 0.66, 0.6];

/// The spray's pools in the simulation and each wheel's carried fraction of a particle per pool.
#[derive(Debug, Clone)]
pub struct WheelSpray {
    pools: [usize; POOLS.len()],
    carry: [[f32; POOLS.len()]; 4],
    /// The air about the car ([`front::wake`]).
    air: Option<Vec3>,
    /// Each wheel's standing water at its last frame, for the burst on entering it.
    standing: [f32; 4],
    /// Whether the renderer draws puddles to splash through.
    puddles: bool,
    /// Whether wet tarmac carries the renderer's film (wet weather with `--enhance wet`).
    film: bool,
    rng: Rng,
    /// The front tyres' throws draw from their own, so the rest of the spray is as without them.
    front_rng: Rng,
}

/// One wheel's values for a frame.
struct Throw {
    edges: [Vec3; 2],
    velocity: Vec3,
    up: Vec3,
    out: Vec3,
    forward: Vec3,
    back: Vec3,
    slip: f32,
    speed: f32,
    /// The velocity the clouds are carried along with ([`front::carried`]).
    carried: Vec3,
    /// A front tyre: the way it slides sideways (outward when it does not) and how fast; a rear one: `None`.
    front: Option<(Vec3, f32)>,
}

impl WheelSpray {
    /// Adds the spray's pools to `sim`, the dust of `dust` colour; `puddles` when the renderer draws them, `film` its wet tarmac.
    pub fn new(sim: &mut Simulation, dust: [f32; 3], puddles: bool, film: bool) -> WheelSpray {
        let pools = POOLS.map(|pool| sim.add_pool(def(pool, dust)));
        WheelSpray {
            pools,
            carry: [[0.0; POOLS.len()]; 4],
            air: None,
            standing: [0.0; 4],
            puddles,
            film,
            rng: Rng::new(0x5eed_0006),
            front_rng: Rng::new(0x5eed_0007),
        }
    }

    /// Back to nothing carried and dry wheels.
    pub fn clear(&mut self) {
        self.carry = [[0.0; POOLS.len()]; 4];
        self.air = None;
        self.standing = [0.0; 4];
    }

    /// Throws `dt` seconds of each wheel's spray from its payload. `game` when the payloads are the game's: their
    /// rubber is 0 to 1, so the slip comes from the mass flow ([`super::slip_of_mass`]); playback's rubber is the slip.
    /// Gives the splats of standing water the front tyres threw over the bonnet ([`BOW_SPLATS`]).
    pub fn feed(&mut self, sim: &mut Simulation, payloads: &[TyreParticles], pose: &Pose, dt: f32, game: bool) -> f32 {
        let speed = pose.velocity.length();
        let forward = pose.velocity.try_normalize().unwrap_or(pose.side.cross(pose.up));
        let grade = pose.up.truncate().length() / pose.up.z.max(1e-3);
        let level = 1.0 - smoothstep(ground::PUDDLE_GRADE, grade);
        let air = front::wake(self.air, pose.velocity, dt);
        self.air = Some(air);
        let carried = front::carried(pose.velocity, air, forward);
        let mut standing = [0.0; 4];
        let mut splats = 0.0;
        for p in payloads.iter().filter(|p| p.contact && p.wheel < 4) {
            let m = ground::material(p.surface());
            if m.kind == Kind::None {
                continue;
            }
            let at = Vec3::from(p.centre());
            let out = if p.wheel % 2 == 0 { pose.side } else { -pose.side };
            let edges = p.edges.map(Vec3::from);
            let w = Throw {
                edges,
                velocity: pose.velocity,
                up: pose.up,
                out,
                forward,
                back: -forward,
                slip: if game { super::slip_of_mass(p.mass) } else { p.rubber },
                speed,
                carried,
                front: (p.wheel < 2)
                    .then(|| front::slide(edges, Vec3::from(p.velocity), pose.up).unwrap_or((out, 0.0))),
            };
            let puddle = if self.film && m.wet >= 1.0 && m.kind == Kind::Tarmac {
                FILM_WATER
            } else if self.puddles && m.wet >= 1.0 && matches!(m.kind, Kind::Gravel | Kind::Dirt) {
                let (share, tracks) = if self.film {
                    (ground::PUDDLE_SHARE, ground::RUT_COVER)
                } else {
                    (ground::PUDDLE_SHARE_DAMP, 0.0)
                };
                level * ground::puddle_cover(at.x, at.y, share).max(tracks)
            } else {
                0.0
            };
            let wheel = p.wheel as usize;
            standing[wheel] = if m.kind == Kind::Water { 1.0 } else { puddle.max(p.splash / FULL_WATER).min(1.0) };
            if standing[wheel] - self.standing[wheel] >= BURST_RISE {
                let k = POOLS.iter().position(|&p| p == Pool::Splash).unwrap();
                self.burst(sim, k, BURST * standing[wheel] * smoothstep(SPLASH_SPEED, speed), &w);
            }
            // A front tyre running into a puddle: the plume and its splats at once, by how much deeper it got.
            if w.front.is_some() {
                let deeper = smoothstep(BOW_WATER, standing[wheel]) - smoothstep(BOW_WATER, self.standing[wheel]);
                let hit = deeper.max(0.0) * smoothstep(SPLASH_SPEED, speed);
                for (pool, share) in [(Pool::Bow, 1.0), (Pool::BowMist, 0.35)] {
                    let k = POOLS.iter().position(|&p| p == pool).unwrap();
                    self.throw(sim, wheel, k, BOW_BURST * share * hit, &w);
                }
                splats += BOW_BURST_SPLATS * hit;
            }
            let rates = rates(&m, &w, standing[wheel]);
            let bow = POOLS.iter().position(|&p| p == Pool::Bow).unwrap();
            splats += rates[bow] / BOW_RATE * BOW_SPLATS * dt;
            for (k, rate) in rates.into_iter().enumerate() {
                self.throw(sim, wheel, k, rate * dt, &w);
            }
        }
        self.standing = standing;
        splats
    }

    /// A wheel's crown of `count` drops of pool `k` on entering standing water: out all round its front half and up.
    fn burst(&mut self, sim: &mut Simulation, k: usize, count: f32, w: &Throw) {
        let hard = (w.speed / 15.0).clamp(0.3, 1.0);
        for _ in 0..count as usize {
            let r = &mut self.rng;
            let at = w.edges[0].lerp(w.edges[1], r.unit());
            let angle = std::f32::consts::PI * r.unit();
            let around = w.out * angle.cos() + w.forward * angle.sin() * 0.6;
            let launch = w.velocity * 0.6 + (around * (1.5 + 2.0 * r.unit()) + w.up * (1.5 + 1.5 * r.unit())) * hard;
            sim.emit(self.pools[k], at, at, launch, 1.0, at.z - 0.2);
        }
    }

    /// Emits `count` (with the wheel's carried fraction) of pool `k`.
    fn throw(&mut self, sim: &mut Simulation, wheel: usize, k: usize, count: f32, w: &Throw) {
        let carry = &mut self.carry[wheel][k];
        *carry += count.max(0.0);
        let n = carry.floor();
        *carry -= n;
        let front = matches!(POOLS[k], Pool::Kick | Pool::Fling | Pool::Flurry | Pool::Bow | Pool::BowMist);
        for _ in 0..n as usize {
            let r = if front { &mut self.front_rng } else { &mut self.rng };
            let at = w.edges[0].lerp(w.edges[1], r.unit());
            let (at, launch, floor) = launch(POOLS[k], at, w, r);
            if front {
                sim.emit_drawing(self.pools[k], at, launch, floor, &mut self.front_rng);
            } else {
                sim.emit(self.pools[k], at, at, launch, 1.0, floor);
            }
        }
    }
}

/// Each pool's particles a second from a wheel on `m` with standing water `standing` (0 to 1), in [`POOLS`] order.
fn rates(m: &Material, w: &Throw, standing: f32) -> [f32; POOLS.len()] {
    let motion = smoothstep(MOTION, w.speed);
    let spin = smoothstep(SPIN, w.slip);
    let dry = 1.0 - m.wet;
    // Damp ground binds its dust: an eighth of a dry one's.
    let bound = dry * dry * dry;
    let trail = m.loose * (0.5 * motion + spin);
    let (dust, powder) = match m.kind {
        Kind::Gravel | Kind::Dirt | Kind::Tarmac => (trail * bound, 0.0),
        Kind::Grass => (0.3 * trail * bound, 0.0),
        Kind::Snow => (0.0, trail),
        Kind::Water | Kind::None => (0.0, 0.0),
    };
    let sprays = !matches!(m.kind, Kind::Snow | Kind::None);
    let wet = if sprays { smoothstep(SPRAYS, m.wet) } else { 0.0 };
    let cover = if !sprays {
        0.0
    } else if m.wet >= 1.0 {
        1.0
    } else if m.wet > 0.0 {
        DAMP_MIST
    } else {
        0.0
    };
    let mist = smoothstep(MIST_MOTION, w.speed);
    let hard = matches!(m.kind, Kind::Tarmac | Kind::Water);
    let splash = standing * smoothstep(SPLASH_SPEED, w.speed);
    let fine = (cover * mist + wet * 0.5 * spin) * MIST_RATE;
    let drops = wet * (motion + spin) * DROP_RATE;
    // Wet ground's water is grey on a road and the ground's brown, thrown lower and coarser, on loose ground: pools of
    // their own. The damp road's haze is the first mist pool.
    let (damp, road, loose) = if m.wet < 1.0 && m.kind != Kind::Water {
        (fine * if hard { 1.0 } else { LOOSE_MIST }, 0.0, 0.0)
    } else if hard {
        (0.0, 1.0, 0.0)
    } else {
        (0.0, 0.0, 1.0)
    };
    let debris = match m.kind {
        Kind::Gravel | Kind::Dirt | Kind::Tarmac | Kind::Grass => trail * bound,
        Kind::Snow | Kind::Water | Kind::None => 0.0,
    };
    // A front tyre scrubs out sideways what it slides over, ploughs snow up and throws standing water over the bonnet.
    let scrub = w.front.map_or(0.0, |(_, sideways)| m.loose * smoothstep(SCRUB, sideways));
    let front = if w.front.is_some() { 1.0 } else { 0.0 };
    let (kick, fling) = match m.kind {
        Kind::Gravel | Kind::Dirt | Kind::Tarmac | Kind::Grass => (scrub * bound, scrub * bound),
        Kind::Snow | Kind::Water | Kind::None => (0.0, 0.0),
    };
    let flurry = if m.kind == Kind::Snow { scrub + front * PLOUGH * m.loose * motion } else { 0.0 };
    let bow = front * smoothstep(BOW_WATER, standing) * smoothstep(SPLASH_SPEED, w.speed);
    [
        dust * DUST_RATE,
        powder * POWDER_RATE,
        damp,
        drops * road,
        splash * SPLASH_RATE,
        fine * road * MIST_GAIN * WET_MIST_SHARE,
        fine * loose * MUD_MIST * MIST_GAIN,
        drops * loose * MUD_DROP_GAIN,
        splash * SPLASH_MIST_RATE,
        debris * DEBRIS_RATE,
        dust * DUST_RATE * HAZE_SHARE,
        kick * KICK_RATE,
        fling * FLING_RATE,
        flurry * FLURRY_RATE,
        bow * BOW_RATE,
        bow * BOW_MIST_RATE,
    ]
}

/// Where a particle of `pool` starts (from `at` on the patch), its launch velocity and its floor (below which it
/// dies). The clouds' floor is well down: the wind's gusts carry them over the road's dips, and they fade where they
/// meet the ground (`fs_soft`).
fn launch(pool: Pool, at: Vec3, w: &Throw, r: &mut Rng) -> (Vec3, Vec3, f32) {
    let mut u = |from: f32, to: f32| from + (to - from) * r.unit();
    let ground = at.z;
    let cloud = ground - CLOUD_FLOOR;
    match pool {
        Pool::Dust => {
            // Kicked up by a squared draw: most puffs stay low and a few billow high, so the plume has no flat top.
            let high = u(0.0, 1.0);
            let launch = w.carried * 0.6
                + w.up * (0.3 + 1.7 * high * high)
                + w.out * u(-0.5, 2.0)
                + w.back * w.slip * u(0.0, 0.4);
            (at, launch, cloud)
        }
        Pool::Haze => {
            let launch = w.carried * 0.5 + w.up * u(0.1, 0.8) + w.out * u(-0.5, 1.5);
            (at, launch, cloud)
        }
        Pool::Debris => {
            // Stones and grit off the tread's back, up and out, keeping part of its speed.
            // A fan low and outward from the rear tyre.
            let (from, flung) = released(at, w, u(0.0, RELEASE[0] * 2.0), u(0.2, 0.5));
            (from, w.velocity * 0.7 + flung + w.out * u(0.5, 4.5) + w.up * u(0.2, 1.4), ground - 0.2)
        }
        Pool::Powder => (at, w.carried * 0.5 + w.up * u(0.3, 1.5) + w.out * u(0.0, 1.0), cloud),
        Pool::MudMist => {
            // Low out of the contact patch, mostly sideways and back, and it settles.
            let (from, _) = released(at, w, u(0.0, RELEASE[0]), 0.0);
            (from, w.velocity * 0.5 + w.up * u(0.2, 0.9) + w.out * u(-0.8, 1.6) + w.back * u(0.0, 1.0), ground - 1.0)
        }
        Pool::WetMist => {
            // A road's mist hangs tall behind the car: kicked well up, so it rises over a bush, and it lingers.
            let (from, _) = released(at, w, u(RELEASE[0], RELEASE[1]), 0.0);
            let kick = 0.5 + w.speed / 28.0;
            (
                from,
                w.velocity * 0.5
                    + (w.up * u(0.2, 2.2).powi(2) / 2.0 + w.out * u(-0.6, 1.2) + w.back * u(0.0, 2.0)) * kick,
                cloud,
            )
        }
        Pool::Mist => {
            // Left hanging behind the tread, kicked up and out harder the faster the car.
            let (from, _) = released(at, w, u(RELEASE[0], RELEASE[1]), 0.0);
            let kick = 0.5 + w.speed / 28.0;
            (from, w.velocity * 0.6 + (w.up * u(0.4, 1.2) + w.out * u(0.2, 1.0) + w.back * u(0.0, 1.5)) * kick, cloud)
        }
        Pool::MudDrops => {
            // A fan of muddy water and clods off the tread, back and out, landing within a few metres.
            let (from, flung) = released(at, w, u(0.0, RELEASE[1]), u(KEPT[0], MUD_KEPT));
            let side = if u(0.0, 1.0) < 0.5 { w.out } else { -w.out };
            (from, w.velocity * 0.85 + flung + side * u(0.2, 1.2) + w.up * u(0.0, 0.8), ground - 0.2)
        }
        Pool::Drops => {
            // The rooster tail: off the tread up the wheel's back, the higher the faster it turns.
            let (from, flung) = released(at, w, u(RELEASE[0], RELEASE[1]), u(KEPT[0], KEPT[1]));
            (from, w.velocity + flung + w.out * u(0.3, 1.5), ground - 0.2)
        }
        Pool::Splash => {
            // Out of both sides of the tyre, harder with speed.
            let side = if u(0.0, 1.0) < 0.7 { w.out } else { -w.out };
            let hard = (w.speed / 15.0).clamp(0.3, 1.0);
            let launch = w.velocity * 0.6 + side * u(2.0, 6.0) * hard + w.up * u(1.5, 5.5) * hard;
            (at, launch, ground - 0.2)
        }
        Pool::SplashMist => {
            let side = if u(0.0, 1.0) < 0.7 { w.out } else { -w.out };
            let hard = (w.speed / 15.0).clamp(0.3, 1.0);
            (at, w.velocity * 0.5 + side * u(1.0, 4.0) * hard + w.up * u(0.5, 3.0) * hard, ground - 0.5)
        }
        // The front tyres' throws keep most of the car's velocity, so they show beside and over the front of the car
        // before the air takes them back.
        Pool::Kick => {
            let (side, _) = w.front.unwrap_or((w.out, 0.0));
            let high = u(0.0, 1.0);
            (at, w.carried * u(0.92, 1.0) + side * u(0.4, 2.0) + w.up * (1.5 + 3.5 * high * high), cloud)
        }
        Pool::Fling => {
            let (side, _) = w.front.unwrap_or((w.out, 0.0));
            // A squared draw: most stones stay low, a few fly up past the bonnet.
            let high = u(0.0, 1.0);
            (at, w.velocity * u(0.9, 1.0) + side * u(1.5, 4.5) + w.up * (1.0 + 4.0 * high * high), ground - 0.2)
        }
        Pool::Flurry => {
            let (side, sideways) = w.front.unwrap_or((w.out, 0.0));
            let side = side * u(0.5, 1.5) * (1.0 + sideways / SCRUB[1]);
            let high = u(0.0, 1.0);
            (at, w.velocity * u(0.95, 1.02) + side + w.up * (2.0 + 4.0 * high * high), cloud)
        }
        Pool::Bow | Pool::BowMist => {
            // A short plume pushed off the tyre's front, mostly out and up at the corner, some in over the bonnet;
            // it keeps less than the car's speed, so the air takes it back past the wing.
            let hard = (w.speed / 15.0).clamp(0.3, 1.0);
            let side = if u(0.0, 1.0) < BOW_INWARD { -w.out * u(0.3, 1.2) } else { w.out * u(0.6, 3.0) };
            let launch = w.velocity * u(0.9, 0.98) + (side + w.up * u(2.0, 5.0)) * hard;
            (at + (w.forward * 0.7 + w.up * 0.5) * TYRE_RADIUS, launch, ground - 0.3)
        }
    }
}

/// Where water lets go of the tread `angle` radians up the wheel's back from the contact `at`, and the share `kept`
/// of the tread's velocity relative to the hub it leaves with: the tread turns at the car's speed plus the slip, back
/// along the ground and up the wheel's back.
fn released(at: Vec3, w: &Throw, angle: f32, kept: f32) -> (Vec3, Vec3) {
    let (sin, cos) = angle.sin_cos();
    let from = at + w.back * TYRE_RADIUS * sin + w.up * TYRE_RADIUS * (1.0 - cos);
    let tread = (w.speed + w.slip) * (w.back * cos + w.up * sin);
    (from, tread * kept)
}

/// How a pool's particles look and move.
struct Spec {
    sprite: u32,
    /// Start, middle and end colour (straight alpha), and where the middle is (0 to 1 of the life).
    colors: ([[f32; 4]; 3], f32),
    /// Start, middle and end size (width, height), and where the middle is.
    sizes: ([[f32; 2]; 3], f32),
    life: f32,
    /// Seconds of motion a particle is streaked along (0: a round sprite).
    trail: f32,
    /// The most a wheel throws a second, for the pool's size.
    rate: f32,
    /// Upward acceleration, m/s² (gravity -9.81; dust rises a little).
    rise: f32,
    friction: f32,
    wind: f32,
}

/// The colour of the dust in the air from the ground's `dust` colour: paler and greyer (`DUST_GREY` of the way to its
/// grey, lifted by `DUST_LIFT`), as fine dust scatters more than the soil it comes from.
fn airborne(dust: [f32; 3]) -> [f32; 3] {
    let grey = 0.299 * dust[0] + 0.587 * dust[1] + 0.114 * dust[2];
    dust.map(|c| ((c + (grey - c) * DUST_GREY) * DUST_LIFT).min(1.0))
}

/// Pool `pool`'s emitter, its dust of `dust` colour.
fn def(pool: Pool, dust: [f32; 3]) -> EmitterDef {
    let rgba = |[r, g, b]: [f32; 3], a: f32| [r, g, b, a];
    let fade = |c: [f32; 3], start: f32, most: f32, at: f32| ([rgba(c, start), rgba(c, most), rgba(c, 0.0)], at);
    let growing = |a: f32, b: f32, c: f32| ([[a, a * 0.85], [b, b * 0.8], [c, c * 0.75]], 0.35);
    let low = |a: f32, b: f32, c: f32| ([[a, a * 0.6], [b, b * 0.5], [c, c * 0.45]], 0.35);
    // Water and its mist in an overcast's grey: lit by the scene they come out no brighter than the sky.
    let water = [0.5, 0.53, 0.57];
    let splash_mist = [0.8, 0.83, 0.86];
    let wet_mist = [0.5, 0.53, 0.56];
    // The damp road's haze: a greyer white than the drops.
    let mist = [0.86, 0.88, 0.9];
    // Loose ground's water: the ground's colour, darker and browner than its dust.
    let mud = airborne(dust).map(|c| c * MUD_SHADE);
    // The clods and muddy water are the wet ground itself: dark and brown, not the dust's pale.
    let clod = dust.map(|c| c * c * CLOD_SHADE);
    // The road's plume is densest and palest young, at the tyre, and thins and greys as it rises and drifts back.
    let young_mist =
        ([[0.66, 0.69, 0.72, 0.16], [0.58, 0.61, 0.64, 0.22], [wet_mist[0], wet_mist[1], wet_mist[2], 0.0]], 0.1);
    let puff = |colors, sizes, life, rate, rise, friction, wind| Spec {
        sprite: match pool {
            Pool::WetMist | Pool::SplashMist | Pool::BowMist => sprites::WISP,
            Pool::Dust => sprites::DUST,
            _ => sprites::PUFF,
        },
        colors,
        sizes,
        life,
        trail: 0.0,
        rate,
        rise,
        friction,
        wind,
    };
    let drop = |colour, alpha: f32, size: f32, life, trail, rate, friction| Spec {
        sprite: sprites::DROP,
        colors: fade(colour, alpha, alpha * 0.8, 0.5),
        sizes: ([[size, size]; 3], 0.5),
        life,
        trail,
        rate,
        rise: -9.81,
        // Fine drops lose their speed to the air within a second: the tail curls back behind the car.
        friction,
        wind: 0.0,
    };
    let spec = match pool {
        // Spreading low and settling: no rise, so a bend's earlier dust does not hang as a ball above the road.
        Pool::Dust => {
            // A shade paler than the airborne dust, the sunlit plume's tan: dense at the tyre, thinning as it spreads.
            let pale = airborne(dust).map(|c| (c * DUST_PALE).min(1.0));
            puff(
                ([rgba(pale.map(|c| c * DUST_YOUNG), 0.9), rgba(pale, 0.55), rgba(pale, 0.0)], 0.2),
                ([[0.8, 0.9], [1.5, 1.7], [3.8, 4.2]], 0.3),
                3.6,
                DUST_RATE,
                0.15,
                0.5,
                0.6,
            )
        }
        // Stones are lit grey-brown (the dust's colour dimmed); `sim` caps their size on screen.
        Pool::Debris => {
            Spec { sprite: sprites::STONE, ..drop(dust.map(|c| c * 0.65), 0.95, 0.035, 0.7, 0.004, DEBRIS_RATE, 0.3) }
        }
        // A thin veil hanging a few seconds: soft, no lumps.
        Pool::Haze => Spec {
            sprite: sprites::PUFF,
            ..puff(
                fade(airborne(dust).map(|c| (c * DUST_PALE).min(1.0)), 0.0, 0.1, 0.2),
                low(1.5, 3.0, 4.5),
                5.0,
                DUST_RATE * HAZE_SHARE,
                0.05,
                0.6,
                0.6,
            )
        },
        Pool::Powder => {
            puff(fade([0.95, 0.97, 1.0], 0.4, 1.0, 0.1), growing(0.8, 3.0, 7.0), 3.5, POWDER_RATE, 0.15, 0.8, 0.5)
        }
        Pool::Mist => puff(fade(mist, 0.0, 0.2, 0.2), low(0.8, 2.2, 3.4), 2.5, MIST_RATE * 3.0, 0.2, 0.9, 0.5),
        Pool::WetMist => puff(young_mist, low(1.0, 2.4, 3.8), 3.2, MIST_RATE * 3.0, 0.3, 0.7, 0.5),
        Pool::MudMist => puff(fade(mud, 0.0, 0.4, 0.12), low(0.5, 1.5, 2.2), 2.0, MIST_RATE * 3.0, -0.4, 1.2, 0.3),
        // The water off the tread and out of a puddle: soft clumps that spread and thin as they fall, not streaks
        // (their life, fall and drag as the streaked drops' were, so the plume keeps its shape). The clods: small dark
        // lumps of mud, unstreaked.
        Pool::Drops => puff(fade(water, 0.32, 0.22, 0.4), growing(0.08, 0.25, 0.5), 0.7, DROP_RATE, -9.81, 1.2, 0.0),
        Pool::MudDrops => {
            Spec { sprite: sprites::STONE, ..drop(clod, 0.9, 0.03, 0.35, 0.0, DROP_RATE * MUD_DROP_GAIN, 0.7) }
        }
        Pool::Splash => {
            puff(fade(water, 0.25, 0.15, 0.4), growing(0.05, 0.18, 0.35), 0.9, SPLASH_RATE, -9.81, 1.0, 0.0)
        }
        Pool::SplashMist => {
            puff(fade(splash_mist, 0.0, 0.35, 0.15), low(0.8, 2.0, 3.0), 1.3, SPLASH_MIST_RATE, -2.0, 1.5, 0.3)
        }
        // Small and short-lived: a wisp at the windscreen's edge, never a wall in front of the driver.
        Pool::Kick => {
            let pale = airborne(dust).map(|c| (c * DUST_PALE).min(1.0));
            puff(fade(pale, 0.35, 0.3, 0.2), growing(0.3, 0.9, 1.6), 1.2, KICK_RATE, 0.0, 0.6, 0.3)
        }
        Pool::Fling => {
            Spec { sprite: sprites::STONE, ..drop(dust.map(|c| c * 0.65), 0.95, 0.035, 0.8, 0.004, FLING_RATE, 0.3) }
        }
        Pool::Flurry => {
            puff(fade([0.95, 0.97, 1.0], 0.3, 0.35, 0.2), growing(0.3, 1.0, 1.8), 1.0, FLURRY_RATE, 0.0, 0.6, 0.3)
        }
        // Translucent clumps of the road's muddy water breaking into a grey-white mist: short-lived, never streaks.
        Pool::Bow => {
            let muddy = std::array::from_fn(|k| (water[k] + mud[k]) * 0.5);
            puff(fade(muddy, 0.4, 0.3, 0.25), growing(0.3, 0.8, 1.3), 0.6, BOW_RATE, -4.0, 0.3, 0.2)
        }
        Pool::BowMist => puff(fade(splash_mist, 0.0, 0.2, 0.2), low(0.4, 1.1, 1.8), 0.9, BOW_MIST_RATE, -1.5, 0.6, 0.3),
    };
    let look = Look {
        sprite: spec.sprite,
        colors: spec.colors.0,
        color_at: spec.colors.1,
        sizes: spec.sizes.0,
        size_at: spec.sizes.1,
        size_variance: match pool {
            Pool::MudMist => 2.5,
            Pool::WetMist => 2.0,
            _ => 0.5,
        },
        life: spec.life,
        weight: 1.0,
        trail: spec.trail,
        additive: false,
        attenuation: [0.0, 0.0],
        offset_height: 0.5,
        stone: false,
        // The drops are drawn as small as they are, as the sparks: a few millimetres, streaked.
        spark: matches!(spec.sprite, sprites::DROP | sprites::STONE),
        wisp: matches!(pool, Pool::Mist | Pool::WetMist | Pool::MudMist | Pool::SplashMist | Pool::BowMist),
    };
    EmitterDef {
        name: format!("{pool:?} (wheel spray, ours)"),
        // Four wheels at full rate (the mist's at its splash), a quarter over, and four bursts.
        max: ((spec.rate * spec.life * 4.0 * 1.25).ceil() + 4.0 * BURST) as usize,
        emit_rate: 0.0,
        shape: Shape::Point,
        force_variance: 0.0,
        look,
        acceleration: Vec3::new(0.0, 0.0, spec.rise),
        friction: spec.friction,
        wind: spec.wind,
        lands: false,
        water: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn throw(speed: f32, slip: f32) -> Throw {
        Throw {
            edges: [Vec3::ZERO; 2],
            velocity: Vec3::new(0.0, -speed, 0.0),
            up: Vec3::Z,
            out: Vec3::X,
            forward: -Vec3::Y,
            back: Vec3::Y,
            slip,
            speed,
            carried: Vec3::new(0.0, -speed, 0.0),
            front: None,
        }
    }

    fn rate(name: &str, pool: Pool, speed: f32, slip: f32, standing: f32) -> f32 {
        let k = POOLS.iter().position(|&p| p == pool).unwrap();
        rates(&ground::by_name(name), &throw(speed, slip), standing)[k]
    }

    #[test]
    fn dry_gravel_dusts_wet_gravel_sprays() {
        assert!(rate("Gravel_Medium_Deep_Dry", Pool::Dust, 25.0, 0.0, 0.0) > 0.0);
        assert_eq!(rate("Gravel_Medium_Deep_Wet", Pool::Dust, 25.0, 0.0, 0.0), 0.0);
        assert!(rate("Gravel_Medium_Deep_Wet", Pool::MudMist, 25.0, 0.0, 0.0) > 0.0);
        assert!(rate("Gravel_Medium_Deep_Wet", Pool::MudDrops, 25.0, 0.0, 0.0) > 0.0);
        assert_eq!(rate("Gravel_Medium_Deep_Dry", Pool::Drops, 25.0, 0.0, 0.0), 0.0);
        assert_eq!(rate("Gravel_Medium_Deep_Damp", Pool::Drops, 25.0, 0.0, 0.0), 0.0, "a damp road throws no water");
        assert!(
            rate("Gravel_Medium_Deep_Dry", Pool::Dust, 10.0, 6.0, 0.0)
                > rate("Gravel_Medium_Deep_Dry", Pool::Dust, 10.0, 0.0, 0.0)
        );
        assert!(rate("SnowOnGravel_Deep", Pool::Powder, 25.0, 0.0, 0.0) > 0.0);
    }

    #[test]
    fn the_mist_grows_with_speed_and_a_damp_road_throws_a_third_of_it() {
        let wet = |speed| rate("Tarmac_Fine_Clean_Wet", Pool::WetMist, speed, 0.0, 0.0);
        let damp = |name, speed| rate(name, Pool::Mist, speed, 0.0, 0.0);
        assert_eq!(wet(3.0), 0.0);
        assert!(wet(15.0) < 0.6 * wet(28.0));
        let road = damp("Tarmac_Fine_Clean_Damp", 28.0);
        assert!((road * 3.0 * MIST_GAIN * WET_MIST_SHARE - wet(28.0)).abs() < 1e-3, "{road}");
        assert_eq!(damp("Tarmac_Fine_Clean_Wet", 28.0), 0.0);
        let mud = rate("Dirt_Hard_Wet", Pool::MudMist, 28.0, 0.0, 0.0);
        assert!((mud - MUD_MIST * wet(28.0) / WET_MIST_SHARE).abs() < 1e-3, "{mud}");
        let loose = damp("Dirt_Hard_Damp", 28.0);
        assert!((loose - LOOSE_MIST * road).abs() < 1e-3, "{loose}");
    }

    #[test]
    fn standing_water_splashes_with_speed() {
        assert_eq!(rate("Tarmac_Fine_Clean_Wet", Pool::Splash, 0.0, 0.0, 1.0), 0.0);
        assert_eq!(rate("Tarmac_Fine_Clean_Wet", Pool::Splash, 20.0, 0.0, 1.0), SPLASH_RATE);
        assert_eq!(rate("Tarmac_Fine_Clean_Wet", Pool::Splash, 20.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn the_tread_flings_water_up_its_back_faster_with_speed() {
        let (low, slow) = released(Vec3::ZERO, &throw(10.0, 0.0), 0.8, 0.3);
        let (_, fast) = released(Vec3::ZERO, &throw(30.0, 0.0), 0.8, 0.3);
        assert!(low.z > 0.0 && low.y > 0.0, "up the wheel's back: {low}");
        assert!(slow.z > 0.0 && fast.z > 2.5 * slow.z, "{slow} {fast}");
        let (_, at_ground) = released(Vec3::ZERO, &throw(30.0, 0.0), 0.0, 1.0);
        assert_eq!(at_ground.z, 0.0, "the tread at the contact runs back along the ground");
    }

    #[test]
    fn the_slip_comes_from_the_mass_flow() {
        use crate::particles::{slip_of_mass, tyres::table_lerp, SLIP_KEYS, SLIP_MASS};
        // El Condor live payloads: rubber at most 0.001, mass about 50 spinning off the start and sideways.
        assert!(slip_of_mass(47.0) >= SPIN[1]);
        assert!(slip_of_mass(0.26) < SPIN[0]);
        for slip in [0.0, 0.3, 2.5, 6.0] {
            let mass = table_lerp(&SLIP_KEYS, &SLIP_MASS, slip);
            assert!((slip_of_mass(mass) - slip).abs() < 1e-4, "playback's slip {slip} round trips");
        }
    }

    #[test]
    fn only_a_sliding_front_tyre_scrubs_out_sideways() {
        let gravel = ground::by_name("Gravel_Medium_Deep_Dry");
        let k = |pool: Pool| POOLS.iter().position(|&p| p == pool).unwrap();
        let at = |front: Option<(Vec3, f32)>, pool| rates(&gravel, &Throw { front, ..throw(20.0, 0.0) }, 0.0)[k(pool)];
        assert_eq!(at(None, Pool::Fling), 0.0, "a rear tyre");
        assert_eq!(at(Some((Vec3::X, 0.5)), Pool::Fling), 0.0, "rolling nearly straight");
        assert!(at(Some((Vec3::X, 6.0)), Pool::Fling) > 0.5 * FLING_RATE * gravel.loose);
        assert!(at(Some((Vec3::X, 6.0)), Pool::Kick) > 0.0);
        let wet = ground::by_name("Tarmac_Fine_Clean_Wet");
        let bow = |front| rates(&wet, &Throw { front, ..throw(20.0, 0.0) }, 1.0)[k(Pool::Bow)];
        assert_eq!(bow(None), 0.0);
        assert_eq!(bow(Some((Vec3::X, 0.0))), BOW_RATE, "a front tyre in standing water throws over the bonnet");
        let snow = ground::by_name("SnowOnGravel_Deep");
        assert!(rates(&snow, &Throw { front: Some((Vec3::X, 0.0)), ..throw(25.0, 0.0) }, 0.0)[k(Pool::Flurry)] > 0.0);
        assert_eq!(rates(&snow, &throw(25.0, 0.0), 0.0)[k(Pool::Flurry)], 0.0);
    }

    #[test]
    fn clean_dry_tarmac_throws_nothing() {
        let rates = rates(&ground::by_name("Tarmac_Fine_Clean_Dry"), &throw(25.0, 10.0), 0.0);
        assert!(rates.iter().all(|&r| r == 0.0), "{rates:?}");
    }

    const FILE: &str = "version3 1 0 0 1 0 1 0 1 0 0 1 1 1 1 1 1 10 \
            1 pc 13 1 1 1 1 0 1 1 1 1 1 1 1 1 0.1 0.1 0.1 0.1 0.1 0.1 0 1 2 0.41 0 0 0 0 BLEND_BLEND 0 0 0.5 0 \
            1 ec 2 0.2 0.2 -3 0.3 0 0 0 0 0 1 pc \
            1 em ec 500 0 0 1 0 0 -9.81 0 0 0 \
            10 0 \
            1 17 em 0 5 0 0 0 0 0.5 0 0 1 0 -10 1 8 \
            0 1 6 17 0 1 17 0 0 0 0 -7 0.02 0.02 em 0 $";

    /// The particles of `sprite` a driver sees through the windscreen after a car on `surface` ran 1.5 s (or braked to
    /// a stop at `brake` m/s² and stood) along -y from `speed` m/s, its tyres in `water` standing water: within `across`
    /// (the tangent: 0.77 the windscreen's 37 degrees, 1.5 the side windows too) and from 14 degrees below to 22 above
    /// the eye's level, outside the body ([`cabin`]'s mask for a
    /// 1.8 by 1.4 by 4 m car, the eye 1.1 m up, 0.35 m left of and 0.1 m behind the middle).
    fn seen_from_the_seat(surface: &str, speed: f32, water: f32, brake: f32, sprite: u32, across: f32) -> usize {
        use crate::particles::cabin::CabinMask;
        use crate::particles::sim::{to_stage, Facing};
        let mut sim = Simulation::new(&rbr_formats::pss::parse(FILE).unwrap(), 1);
        let mut spray = WheelSpray::new(&mut sim, NEUTRAL_DUST, false, false);
        let want = ground::by_name(surface);
        let id = (0..256).find(|&id| ground::material(id) == want).unwrap();
        let wheels = [(0.75, -1.3), (-0.75, -1.3), (0.75, 1.3), (-0.75, 1.3)].map(|(x, y)| Vec3::new(x, y, 0.0));
        let (dt, mut car, mut v) = (1.0 / 60.0, Vec3::ZERO, -speed);
        let frames = if brake > 0.0 { (speed / brake / dt) as usize + 30 } else { 90 };
        for _ in 0..frames {
            v = (v + brake * dt).min(0.0);
            let velocity = Vec3::new(0.0, v, 0.0);
            let pose = Pose { velocity, up: Vec3::Z, side: Vec3::X, engine: None, exhaust: None };
            // Braking hard the tyres slip, at rest they do not.
            let rubber = if brake > 0.0 && v < 0.0 {
                6.0
            } else if brake > 0.0 {
                0.0
            } else {
                2.0
            };
            let payloads = wheels.map(|w| {
                let at = car + w;
                let edges = [(at - Vec3::X * 0.1).to_array(), (at + Vec3::X * 0.1).to_array()];
                let wheel = wheels.iter().position(|&x| x == w).unwrap() as u32;
                let (velocity, splash, surfaces) = (velocity.to_array(), water, [id; 2]);
                TyreParticles { wheel, edges, velocity, rubber, splash, surfaces, contact: true, ..Default::default() }
            });
            spray.feed(&mut sim, &payloads, &pose, dt, false);
            sim.step(dt);
            car += velocity * dt;
        }
        let eye = Vec3::new(0.35, 0.1, 1.1);
        let facing = Facing { eye: to_stage(car + eye), right: to_stage(-Vec3::X), up: to_stage(Vec3::Z) };
        let mask = CabinMask::new(Vec3::new(-0.9, 0.0, -2.0), Vec3::new(0.9, 1.4, 2.0), Vec3::new(eye.x, eye.z, eye.y));
        let seen = |b: &crate::particles::sim::Billboard| {
            let p = Vec3::new(b.centre[0], b.centre[2], b.centre[1]) - car;
            let (d, ahead) = (p - eye, eye.y - p.y);
            let shown = mask.shown(Vec3::new(p.x, p.z, p.y)) > 0.5;
            shown && ahead > 0.2 && (d.x / ahead).abs() < across && d.z / ahead > -0.25 && d.z / ahead < 0.4
        };
        sim.billboards(facing, |s| (s == sprite).then_some(0)).iter().filter(|b| seen(b)).count()
    }

    #[test]
    fn a_braking_cars_dust_rolls_past_it_into_view() {
        let seen = seen_from_the_seat("Gravel_Medium_Deep_Dry", 20.0, 0.0, 9.0, sprites::DUST, 0.77);
        assert!(seen > 20, "{seen}");
        assert_eq!(
            seen_from_the_seat("Gravel_Medium_Deep_Dry", 20.0, 0.0, 0.0, sprites::DUST, 0.77),
            0,
            "not at speed"
        );
    }

    #[test]
    fn standing_water_splashes_up_at_the_front_corners() {
        // The plume's clumps rise at the front corners, seen out of the windscreen's edges and the side windows.
        let seen = |water, across| seen_from_the_seat("Tarmac_Fine_Clean_Wet", 20.0, water, 0.0, sprites::PUFF, across);
        let (wide, ahead) = (seen(0.5, 1.5), seen(0.5, 0.77));
        assert!(wide > 5 && ahead < wide, "{wide} {ahead}");
        assert_eq!(seen(0.0, 1.5), 0, "no standing water");
    }

    #[test]
    fn a_front_tyre_running_into_a_puddle_splats_the_windscreen() {
        let mut sim = Simulation::new(&rbr_formats::pss::parse(FILE).unwrap(), 1);
        let mut spray = WheelSpray::new(&mut sim, NEUTRAL_DUST, false, false);
        let wet = (0..256).find(|&id| ground::material(id) == ground::by_name("Tarmac_Fine_Clean_Wet")).unwrap();
        let pose =
            Pose { velocity: Vec3::new(0.0, -20.0, 0.0), up: Vec3::Z, side: Vec3::X, engine: None, exhaust: None };
        let mut feed = |wheel, splash| {
            let p = TyreParticles { wheel, splash, surfaces: [wet; 2], contact: true, ..TyreParticles::default() };
            spray.feed(&mut sim, &[p], &pose, 0.01, false)
        };
        assert_eq!(feed(0, 0.0), 0.0, "no standing water");
        let hit = feed(0, 0.5);
        assert!(hit >= BOW_BURST_SPLATS, "running in: {hit}");
        let through = feed(0, 0.5);
        assert!((through - BOW_SPLATS * 0.01).abs() < 1e-6, "running through: {through}");
        assert_eq!(feed(0, 0.04), 0.0, "a wet rut's film throws none over the bonnet");
        assert_eq!(feed(2, 0.0) + feed(2, 0.5), 0.0, "a rear tyre's splash stays behind");
    }

    #[test]
    fn a_wheel_throws_into_its_pools() {
        let text = "version3 1 0 0 1 0 1 0 1 0 0 1 1 1 1 1 1 10 \
            1 pc 13 1 1 1 1 0 1 1 1 1 1 1 1 1 0.1 0.1 0.1 0.1 0.1 0.1 0 1 2 0.41 0 0 0 0 BLEND_BLEND 0 0 0.5 0 \
            1 ec 2 0.2 0.2 -3 0.3 0 0 0 0 0 1 pc \
            1 em ec 500 0 0 1 0 0 -9.81 0 0 0 \
            10 0 \
            1 17 em 0 5 0 0 0 0 0.5 0 0 1 0 -10 1 8 \
            0 1 6 17 0 1 17 0 0 0 0 -7 0.02 0.02 em 0 $";
        let mut sim = Simulation::new(&rbr_formats::pss::parse(text).unwrap(), 1);
        let mut spray = WheelSpray::new(&mut sim, NEUTRAL_DUST, false, false);
        let wet_gravel =
            (0..256).find(|&id| ground::material(id).kind == Kind::Gravel && ground::material(id).wet == 1.0);
        let payload = TyreParticles { surfaces: [wet_gravel.unwrap(); 2], contact: true, ..TyreParticles::default() };
        let pose =
            Pose { velocity: Vec3::new(0.0, -25.0, 0.0), up: Vec3::Z, side: Vec3::X, engine: None, exhaust: None };
        for _ in 0..100 {
            spray.feed(&mut sim, &[payload], &pose, 0.01, false);
        }
        assert!(sim.count() > 100, "{}", sim.count());
    }
}
