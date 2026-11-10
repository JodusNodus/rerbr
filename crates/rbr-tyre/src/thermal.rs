//! The tyre's temperatures under the brush (ours, experimental): per wheel a thin surface layer and the tread under it
//! (the bulk), driven by the patch's friction power, in place of NGP's heat step (`rbr_physics::tyre::heat`), which
//! sends 81 % of the slip power past the surface (into the tread, the carcass or nowhere), so a slide moves the
//! surface a few kelvin (`notes/tyre-model.md`, "Tyre temperatures"). The capacities, the tread-carcass conduction and
//! the road's contact conductance are NGP's; the grip reads the temperatures as NGP's does ([`rubber_grip`]).
//!
//! Flows, W: into the surface the tyre's share [`TYRE_SHARE`] of the friction power; from it conduction into the
//! bulk over the layer's own thickness ([`RUBBER_CONDUCTIVITY`]), forced convection to the air ([`convection`]) and
//! conduction to the road in the patch ([`road_conductance`]); into the bulk the rolling
//! resistance's power, from it conduction to NGP's carcass. Explicit steps at the physics' 1/720 s: the fastest time
//! constant, the surface's, is about 0.7 s.

/// The share of the patch's friction power that goes into the tyre (the rest into the road): 0.5, as Kobayashi,
/// Weber & Gerdes assume for a drifting car's rear tyre (arXiv 2407.12989, 2024, Table I); West & Limebeer's fit to
/// a race car's tread temperatures gives 0.49-0.68 per tyre (VSD 2022, Table 3).
pub const TYRE_SHARE: f64 = 0.5;

/// The tread rubber's thermal conductivity, W/(m·K): elastomers 0.2-0.3, fillers changing it little (Gent & Walter,
/// The Pneumatic Tire, NHTSA DOT HS 810 561, 2006, ch. 1); Fina's racing tread model 0.33 (PhD Surrey 2016, §5.4).
/// The surface layer conducts into the rubber under it across its own thickness, so its time constant is its
/// thickness squared over the diffusivity (NGP's layer, 0.5 mm of a 10 mm tread: about 1.5 s).
pub const RUBBER_CONDUCTIVITY: f64 = 0.25;

/// Air at 20 °C: conductivity, W/(m·K), and kinematic viscosity, m²/s (standard property values, ours).
pub const AIR_CONDUCTIVITY: f64 = 0.0257;
pub const AIR_VISCOSITY: f64 = 1.51e-5;

/// The tread's forced convection coefficient at the speed `v` (m/s), W/(m²·K): Nu = 0.0239 Re^0.805 over the length
/// 1/(1/D + 1/W) of the tyre's outer diameter D and tread width W (West & Limebeer, VSD 2022, eq. 10; the same form in
/// Farroni et al.'s thermoRIDE, Appl. Sci. 2020, eq. 13). The Fiesta Rally2's tyre: 44 / 75 / 104 at 10 / 20 / 30 m/s
/// (NGP's sectors 45 from 10 m/s on).
pub fn convection(v: f64, diameter: f64, width: f64) -> f64 {
    let length = 1.0 / (1.0 / diameter + 1.0 / width);
    let re = v.abs() * length / AIR_VISCOSITY;
    AIR_CONDUCTIVITY / length * 0.0239 * libm::pow(re, 0.805)
}

/// The road's temperature over the air's, K: NGP's heat step's road, 303 K at its reference air of 293.14 K.
pub const ROAD_OVER_AIR: f64 = 303.0 - 293.14;

/// The contact's conductance to the road, W/K: NGP's heat step's, kept as it is: its contact conductance factor
/// `conductance` (rising with the contact pressure) times 1500 W/(m²·K), raised to 6000 as the tyre's mean temperature
/// goes from 40 to 120 °C, times 1 + 3 W on a wet road, over the whole contact `area` (m²). West & Limebeer's race
/// tyre takes 12 kW/(m²·K), from Kelly & Sharp, over the non-sliding part only (VSD 2022, Table 2, eq. 12); over the
/// brush's patch, which slides whole at its peak, that leaves a tyre at the limit nothing but the air to cool it (lab:
/// 375 K after 10 min at 7°). No published coefficient for a wet road; tyres run 6-7 K cooler on one (Ejsmont et al.,
/// J. Civ. Eng. Archit. 9 (2015) 1302).
pub fn road_conductance(conductance: f64, mean_temp: f64, wetness: f64, area: f64) -> f64 {
    let hot = ((mean_temp - 273.15 - 40.0) * 0.0125).clamp(0.0, 1.0);
    conductance * (1500.0 + 4500.0 * hot) * (1.0 + 3.0 * wetness) * area
}

/// One wheel's temperatures, K.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temperatures {
    pub surface: f64,
    pub bulk: f64,
}

/// What a step reads (SI units).
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub dt: f64,
    /// The patch's friction power, W: the force the tyre holds times the slip velocity, each axis.
    pub friction_power: f64,
    /// The rolling resistance's power, W.
    pub rolling_power: f64,
    /// The road's wetness (0-1) and the speed over the road, m/s.
    pub wetness: f64,
    pub speed: f64,
    /// The air's and the carcass's temperatures, K.
    pub air: f64,
    pub carcass: f64,
    /// Heat capacities of the surface layer and the bulk, J/K; the tread's depth and width and the outer diameter, m;
    /// the tread's and the contact's areas, m²; the bulk-carcass conductance, W/K, and NGP's contact conductance factor.
    pub surface_capacity: f64,
    pub bulk_capacity: f64,
    pub tread_depth: f64,
    pub tread_width: f64,
    pub diameter: f64,
    pub tread_area: f64,
    pub contact_area: f64,
    pub carcass_conductance: f64,
    pub contact_conductance: f64,
}

/// The heat flows at `t`, W: into the surface, into the bulk, and the parts of them that leave the tyre (to the
/// air, the road and the carcass).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flows {
    pub surface: f64,
    pub bulk: f64,
    pub to_air: f64,
    pub to_road: f64,
    pub to_carcass: f64,
}

/// The surface layer's conductance to the bulk, W/K: the rubber's conductivity over the layer's thickness (its share
/// of the capacity times the tread depth), times the tread's area.
pub fn surface_conductance(x: &Input) -> f64 {
    let thickness = x.tread_depth * x.surface_capacity / (x.surface_capacity + x.bulk_capacity);
    RUBBER_CONDUCTIVITY * x.tread_area / thickness
}

/// The heat flows at `t`.
pub fn flows(t: Temperatures, x: &Input) -> Flows {
    let into_bulk = surface_conductance(x) * (t.surface - t.bulk);
    let to_air = convection(x.speed, x.diameter, x.tread_width) * x.tread_area * (t.surface - x.air);
    let road = road_conductance(x.contact_conductance, 0.5 * (t.surface + t.bulk), x.wetness, x.contact_area);
    let to_road = road * (t.surface - (x.air + ROAD_OVER_AIR));
    let to_carcass = x.carcass_conductance * (t.bulk - x.carcass);
    Flows {
        surface: TYRE_SHARE * x.friction_power - into_bulk - to_air - to_road,
        bulk: x.rolling_power + into_bulk - to_carcass,
        to_air,
        to_road,
        to_carcass,
    }
}

/// The temperatures after one step from `t`.
pub fn step(t: Temperatures, x: &Input) -> Temperatures {
    let q = flows(t, x);
    Temperatures {
        surface: t.surface + q.surface * x.dt / x.surface_capacity,
        bulk: t.bulk + q.bulk * x.dt / x.bulk_capacity,
    }
}

/// NGP's rubber grip at the temperatures `t`: `RubMyFct` (`friction` over the temperature `axis`, held at its ends)
/// at the surface's, 0.6, and the bulk's, 0.4 (`rbr_physics::tyre::step`).
pub fn rubber_grip(axis: &[f32], friction: &[f32], t: Temperatures) -> f64 {
    let at = |temp: f64| {
        let n = axis.len().min(friction.len());
        if n == 0 {
            return 1.0;
        }
        let (a, f) = (|i: usize| f64::from(axis[i]), |i: usize| f64::from(friction[i]));
        if temp <= a(0) || n == 1 {
            return f(0);
        }
        (1..n).find(|&i| temp <= a(i)).map_or(f(n - 1), |i| {
            let w = (temp - a(i - 1)) / (a(i) - a(i - 1));
            f(i - 1) + (f(i) - f(i - 1)) * w
        })
    };
    0.6 * at(t.surface) + 0.4 * at(t.bulk)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Fiesta Rally2's front tyre as NGP's car init leaves it (`ngp_transient` image, map 85): 0.5 mm surface
    /// layer of 307 J/K over a 5836 J/K tread, 0.2 m wide, 0.63 m across, contact 0.0305 m², carcass 613 W/K.
    fn fiesta(friction_power: f64, speed: f64) -> Input {
        Input {
            dt: 1.0 / 720.0,
            friction_power,
            rolling_power: 0.0,
            wetness: 0.0,
            speed,
            air: 293.13,
            carcass: 337.0,
            surface_capacity: 307.2,
            bulk_capacity: 5836.1,
            tread_depth: 0.01,
            tread_width: 0.2,
            diameter: 0.63,
            tread_area: 0.396,
            contact_area: 0.0305,
            carcass_conductance: 613.0,
            contact_conductance: 1.1,
        }
    }

    fn run(mut t: Temperatures, x: &Input, secs: f64) -> Temperatures {
        for _ in 0..(secs / x.dt) as usize {
            t = step(t, x);
        }
        t
    }

    #[test]
    fn without_heat_it_settles_at_the_surroundings() {
        // No friction or rolling, the carcass at the air's temperature, no road contact: everything goes to the air.
        let x = Input { carcass: 293.13, contact_area: 0.0, ..fiesta(0.0, 20.0) };
        let t = run(Temperatures { surface: 360.0, bulk: 350.0 }, &x, 900.0);
        assert!((t.surface - 293.13).abs() < 0.5 && (t.bulk - 293.13).abs() < 0.5, "{t:?}");
        // Rolling free on the road, the surface sits between the air and the road.
        let x = Input { carcass_conductance: 0.0, ..fiesta(0.0, 20.0) };
        let t = run(Temperatures { surface: 360.0, bulk: 360.0 }, &x, 3600.0);
        assert!(t.surface > 293.13 && t.surface < 293.13 + ROAD_OVER_AIR, "{t:?}");
        assert!((t.bulk - t.surface).abs() < 0.1, "{t:?}");
    }

    #[test]
    fn the_energy_balances() {
        // Over a step the stored heat equals what came in less what left.
        let x = Input { rolling_power: 850.0, ..fiesta(9800.0, 20.0) };
        let t = Temperatures { surface: 340.0, bulk: 336.0 };
        let q = flows(t, &x);
        let next = step(t, &x);
        let stored = (next.surface - t.surface) * x.surface_capacity + (next.bulk - t.bulk) * x.bulk_capacity;
        let net = (TYRE_SHARE * x.friction_power + x.rolling_power - q.to_air - q.to_road - q.to_carcass) * x.dt;
        assert!((stored - net).abs() < 1e-9 * net.abs().max(1.0), "{stored} {net}");
    }

    #[test]
    fn a_slide_heats_the_surface_within_a_second_and_it_recovers_in_seconds() {
        // 30° at 20 m/s: about 42 kW of friction power, the patch sliding whole; then rolling free.
        let start = Temperatures { surface: 337.0, bulk: 337.0 };
        let slide = run(start, &fiesta(42_000.0, 20.0), 1.0);
        assert!(slide.surface - start.surface > 30.0 && slide.bulk - start.bulk < 2.0, "{slide:?}");
        let after = run(slide, &fiesta(0.0, 20.0), 3.0);
        assert!(after.surface - after.bulk < 0.2 * (slide.surface - slide.bulk), "{after:?}");
    }

    #[test]
    fn the_skidpad_s_steady_state_keeps_ngp_s_grip() {
        // Steady at the limit on map 85's circle (1.2 g, 21.7 m/s, about 7°): 12 kW per front tyre, the
        // rolling resistance's 0.0106 Fz at 4 kN. The carcass as NGP's relaxation steps it: 16165 J/K, its
        // sidewalls losing (6 v^0.78 + 20) W/(m²·K) over 0.266 m² to the air plus 5 K.
        let curve = (
            [293.15, 313.15, 323.15, 333.15, 363.15, 373.15, 383.15, 403.15],
            [0.838, 0.938, 0.975, 1.0, 1.0, 0.975, 0.925, 0.85],
        );
        let v = 21.7;
        let mut x = Input { rolling_power: 0.0106 * 4000.0 * v, ..fiesta(12_000.0, v) };
        let sidewalls = (6.0 * libm::pow(v, 0.78) + 20.0) * 0.266;
        let mut t = Temperatures { surface: 337.0, bulk: 337.0 };
        for _ in 0..(900.0 / x.dt) as usize {
            let to_carcass = flows(t, &x).to_carcass;
            x.carcass += (to_carcass - sidewalls * (x.carcass - x.air - 5.0)) * x.dt / 16165.0;
            t = step(t, &x);
        }
        let ngp = rubber_grip(&curve.0, &curve.1, Temperatures { surface: 331.3, bulk: 337.2 });
        let model = rubber_grip(&curve.0, &curve.1, t);
        assert!((model / ngp - 1.0).abs() < 0.01, "{t:?} {model} {ngp}");
    }

    #[test]
    fn the_grip_is_ngp_s_blend_held_at_the_table_s_ends() {
        let (axis, friction) = ([300.0, 340.0, 380.0], [0.8, 1.0, 0.9]);
        let at = |s, b| rubber_grip(&axis, &friction, Temperatures { surface: s, bulk: b });
        assert!((at(320.0, 320.0) - 0.9).abs() < 1e-6);
        assert!((at(400.0, 280.0) - (0.6 * 0.9 + 0.4 * 0.8)).abs() < 1e-6);
        assert!((at(340.0, 360.0) - (0.6 + 0.4 * 0.95)).abs() < 1e-6);
    }
}
