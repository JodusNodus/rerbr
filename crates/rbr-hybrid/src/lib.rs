//! The game's physics step on `rbr-physics` ports: the original's functions as bindings by address, run by the pure
//! host ([`pure`]) on the memory image and the typed state it holds. A binding gets the call's registers and stack
//! words, runs the port, makes the original callees its port leaves to its caller through the [`Host`], and says
//! what the original's `ret` leaves: eax, the x87 result and the bytes it pops.

mod call_memo;
pub mod car_body;
pub mod collision;
pub mod engine_driveline;
pub mod entities;
pub mod game_log;
pub mod graph;
pub mod ngp;
pub mod ntweak;
pub mod physics_core;
mod probe_cache;
pub mod pure;
pub mod rigid_body;
pub mod spring_damper;
mod stage;
pub mod timekeeper;
pub mod tyre;
pub mod vcu;
pub mod wheel;

use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::SignalHost;

/// The registers and the stack arguments at a function's entry (the words after the return address).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Regs {
    pub ecx: u32,
    pub edx: u32,
    /// The return address's slot. The stack below it is free: the original's frame was there, and a binding
    /// may put what the original passes by address there (`engine_driveline`'s gear-change payload).
    pub esp: u32,
    /// The entry values of the other registers (a binding that does not read them ignores them).
    pub eax: u32,
    pub ebx: u32,
    pub esi: u32,
    pub edi: u32,
    pub ebp: u32,
    pub stack: [u32; STACK_ARGS],
}

/// Stack words passed to a binding. The most an original takes is 17 (`CScratch::ComputeOutputs`).
pub const STACK_ARGS: usize = 20;

/// What the original's `ret` leaves to the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Return {
    pub eax: u32,
    /// The x87 result pushed on the FPU stack, for a float-returning function.
    pub st0: Option<f64>,
    /// Argument bytes the callee pops (stdcall and thiscall); 0 for cdecl.
    pub pop: u32,
}

impl Return {
    /// A function with no result that pops `pop` bytes. `eax` is the residue the original leaves, when known.
    pub fn void(eax: u32, pop: u32) -> Self {
        Return { eax, st0: None, pop }
    }

    pub fn float(st0: f64, pop: u32) -> Self {
        Return { eax: 0, st0: Some(st0), pop }
    }

    /// A float-returning function that also leaves `eax` (the residue of an argument or `this`, as in the methods of
    /// `E38Vec` that return `this`).
    pub fn float_with_eax(eax: u32, st0: f64, pop: u32) -> Self {
        Return { eax, st0: Some(st0), pop }
    }
}

/// Why a binding did not run: the pure host stops ([`pure::Stop`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decline {
    /// No binding at this address.
    NotBound,
    /// The inputs take a path the port does not cover (or the binding doesn't map yet).
    Unsupported(&'static str),
}

/// What an original function returned: eax and the x87 top of stack (meaningful only for a float function).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Called {
    pub eax: u32,
    pub st0: f64,
}

/// The bindings' host, the pure host ([`pure::Pure`]): runs the function at an address (a binding, or a stand-in for
/// an original with none; thiscall with `ecx`, or stdcall/cdecl with `ecx` ignored) on the stack arguments `args`, and
/// returns what it left. The tests' hosts implement `call_at` alone and log the calls.
pub trait Host {
    /// `call` with the callee's entry stack pointer `sp` (its return address slot). `sp == 0` leaves the placement to
    /// the host, which runs the callee below the binding's frame.
    fn call_at(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Called;
    fn call(&self, addr: u32, ecx: u32, args: &[u32]) -> Called {
        self.call_at(0, addr, ecx, args)
    }
    /// The bindings and stand-ins entered so far: a caller that skips a call knows nothing ran in between while the
    /// count stays.
    fn entered(&self) -> u64 {
        0
    }
    /// Counts a binding a caller ran itself in place of a call ([`Host::entered`] goes up as for a run).
    fn ran_direct(&self) {}
    /// A tyre step (`CScratch::Step` or NGP's wrapper) a caller ran itself on the tyre at `scratch` with the time step
    /// `dt`: the pure host keeps its term as after a run of the binding ([`pure::Pure::take_tyre_terms`]).
    fn tyre_stepped(&self, _scratch: u32, _dt: f32) {}
    /// `QueryPerformanceCounter(out)` without its call: the host's count into `out` and the eax.
    fn query_counter(&self, _out: u32) -> u32 {
        unimplemented!("a test host counts nothing")
    }
    /// [`Host::call_at`] of a callee whose binding reads its stack words only from `args` (none through `esp`), and
    /// whose caller reads none of the call's words after it: on the pure host the call runs as a plain call, without
    /// its words on the stack, when `args` are the words the callee pops.
    fn call_direct(&self, sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Called {
        self.call_at(sp, addr, ecx, args)
    }
    /// [`Host::call_at`] of the binding at `addr` run in place: entered as the call enters it, in a frame of its own
    /// (the calls it leaves the placement of land as through the host) with `args` as its stack words, but none of the
    /// call's words written, counted as a run ([`Host::ran_direct`]). For a caller on held state that reads none of the
    /// call's words after it. `None` (nothing ran) for an address with no binding and for a watched emit, where the
    /// caller calls.
    fn call_in_place(&self, _sp: u32, _addr: u32, _ecx: u32, _args: &[u32]) -> Option<Called> {
        None
    }
    /// The esp a call at `sp` enters its callee at (0: the host places it).
    fn call_esp(&self, sp: u32) -> u32 {
        sp
    }
    /// `run` on the image the bindings run on (as [`Host::call_in_place`] lends it), its result, for a caller on held
    /// state that runs a callee's port itself with values.
    fn with_image(&self, _run: &mut dyn FnMut(&mut Mem) -> Option<u32>) -> Option<u32> {
        unimplemented!("a test host has no image")
    }
    /// A damper-signal emit (`CSpringDamper::ResetOutputs`): the strut and the payload the reset emitted. The pure
    /// host keeps the typed payload for the game, which reads it instead of the signal manager's words.
    fn emit_damper(&self, _strut: u32, _signal: rbr_physics::spring_damper::DamperSignal) {}
    /// The typed storage the bindings run on: the car's, its solids', the physics core's, the stage's and the
    /// definitions' (the pure host lifts it on demand, [`pure::Pure::construct`]).
    fn typed(&self) -> std::cell::Ref<'_, rbr_physics::store::typed::Typed> {
        unimplemented!("a test host holds no typed state")
    }
}

/// The value the game gives a stack word the original reads without anything having written it in the step (residue,
/// notes/physics.md "Stack residue is behaviour"): 0, which as a float lane is 0.0, so lanewise arithmetic on it makes
/// no NaN, infinity or denormal.
pub(crate) const RESIDUE: u32 = 0;

/// [`RESIDUE`] stored in the `words` words from `at`, the words of a payload or block the original never writes and a
/// listener may read.
pub(crate) fn fill_residue(mem: &mut Mem, at: u32, words: u32) {
    for k in 0..words {
        mem.set_u32(at + 4 * k, RESIDUE);
    }
}

/// A trivial callee's result, for a caller that then makes no call: `this` from the slot-12 identity getter
/// (`E43PairWalk::MFc00`, 0x519c00, `mov eax, ecx; ret`), the old frame `this + 0x1c` from the slot-13 getter
/// (`E43PairWalk::MFc10`, 0x519c10), 0 from the empty hook (`NTweak::CBase::Unk44`, 0x52f480, `ret`) and the `ret n`
/// stand-ins ([`pure::is_ret`]), whose eax no caller reads. The words the call would write are left as they are: no
/// later reader reads them (residue audit, notes/physics.md "Store seam"). `None` for another callee, which the caller
/// calls.
pub(crate) fn trivial(host: &dyn Host, target: u32, this: u32) -> Option<u32> {
    let eax = match target {
        entities::NODE_OF => this,
        entities::OLD_FRAME_OF => rbr_physics::entities::layout::old_frame_of(this),
        ntweak::CBASE_UNK44 => 0,
        _ if pure::is_ret(target) => 0,
        _ => return None,
    };
    host.ran_direct();
    Some(eax)
}

/// `host.call_at(sp, target, this, args)`'s eax, or [`trivial`]'s without the call.
pub(crate) fn call_or_trivial(host: &dyn Host, sp: u32, target: u32, this: u32, args: &[u32]) -> u32 {
    trivial(host, target, this).unwrap_or_else(|| host.call_at(sp, target, this, args).eax)
}

/// The frame base of a binding's original after `push ebp; mov ebp, esp; and esp, -16`, from its entry `esp` (the
/// return address's slot); the original's locals and its callees' pushes are at fixed offsets below it.
pub(crate) fn aligned_frame(esp: u32) -> u32 {
    esp.wrapping_sub(4) & !0xf
}

/// The `CMultiSignal1::Emit` instances, which differ only in their function name (the `%s` of their two log lines):
/// `(emit, name)`.
const EMIT_NAMES: [(u32, u32); 14] = [
    (0x48cd90, 0x72f948), // CSigCarControl
    (0x4a0390, 0x730b68), // CSigCheckPointTime
    (0x4e4490, 0x737f70), // CSigCarBrakeWallPenetration
    (0x4e8cb0, 0x738a40), // CSigMrRCarInit
    (0x4ef350, 0x739188), // CSigCarDamageHandlerInit
    (0x506f20, 0x73a238), // CSigMrRCarGearChange
    (0x50ef50, 0x73b778), // CSigEngineNoise
    (0x51f580, 0x73d3c0), // CSigDetachFromParent
    (0x51f700, 0x73d4b0), // CSigMrRBodyState
    (0x51f7c0, 0x73d528), // CSigMrRBodyEvent
    (0x523010, 0x73e8b0), // CIncarShake
    (0x52be70, 0x73fb80), // CSolid's damping collision
    (0x534f80, 0x740bf8), // CSigTyreNoise
    (0x53d0d0, 0x742a70), // CSigTireParticles
];

/// The function name the emit at `emit` logs with.
pub(crate) const fn emit_name(emit: u32) -> u32 {
    let mut k = 0;
    while EMIT_NAMES[k].0 != emit {
        k += 1;
    }
    EMIT_NAMES[k].1
}

/// The recursion's log line of a `CMultiSignal1::Emit` (`E01Log(6, 0, fmt, name)`). The count's (`E01Log(7, 0, ...)`)
/// is followed by `int3`: the bindings decline that path ([`decline_emit_trap`]).
const MSG_RECURSIVE: u32 = 0x730358;

/// Declines a `CMultiSignal1::Emit` on the signal at `signal` whose count is out of range: the original logs, then
/// traps (`physics_core::layout::emit_traps`).
pub(crate) fn decline_emit_trap(mem: &Mem, signal: u32) -> Result<(), Decline> {
    if rbr_physics::physics_core::layout::emit_traps(mem, signal) {
        return Err(Decline::Unsupported("the emit's count is out of range (the original logs, then traps at int3)"));
    }
    Ok(())
}

/// The connections of the `CMultiSignal1` instance named `.1` (see [`emit_name`]), invoked through the host: `Invoke`
/// is vtable slot 1, thiscall on the connection with the signal; the recursion's log is an `E01Log` host call.
pub(crate) struct SignalCallees<'a>(pub &'a dyn Host, pub u32);

impl SignalHost for SignalCallees<'_> {
    fn invoke(&mut self, mem: &mut Mem, conn: u32, sig: u32) {
        let target = mem.u32(mem.u32(conn) + 4);
        self.0.call(target, conn, &[sig]);
    }

    fn log_recursive(&mut self) {
        self.0.call(engine_driveline::E01LOG, 0, &[6, 0, MSG_RECURSIVE, self.1]);
    }

    fn log_range(&mut self, _count: u32) {
        unreachable!("the original traps after the count's log")
    }
}

/// [`SignalCallees`] with the calls at the original's stack: each `Invoke` and the recursion's log are entered at `sp`,
/// the return slot of the emit's own call to them (four pushes and the argument below the entry).
pub(crate) struct SignalCalleesAt<'a> {
    pub host: &'a dyn Host,
    pub sp: u32,
    pub name: u32,
}

impl SignalHost for SignalCalleesAt<'_> {
    fn invoke(&mut self, mem: &mut Mem, conn: u32, sig: u32) {
        let target = mem.u32(mem.u32(conn) + 4);
        self.host.call_at(self.sp, target, conn, &[sig]);
    }

    fn log_recursive(&mut self) {
        self.log_recursive_eax();
    }

    fn log_range(&mut self, _count: u32) {
        unreachable!("the binding declines a count out of range (the original traps)")
    }
}

impl SignalCalleesAt<'_> {
    /// [`SignalHost::log_recursive`], eax after it.
    pub(crate) fn log_recursive_eax(&self) -> u32 {
        self.host.call_at(self.sp, engine_driveline::E01LOG, 0, &[6, 0, MSG_RECURSIVE, self.name]).eax
    }
}

/// A binding's view of the game: its memory and the original callees.
pub struct Ctx<'a> {
    pub mem: Mem,
    pub host: &'a dyn Host,
}

impl Ctx<'_> {
    /// An original float accessor (`fld` and `ret`), rounded to f32 as the callers store it.
    pub fn get_f32(&self, addr: u32, this: u32) -> f32 {
        self.host.call(addr, this, &[]).st0 as f32
    }
}

pub type BindingFn = fn(&mut Ctx, &Regs) -> Result<Return, Decline>;

/// An original function and the binding that runs its port.
pub struct Binding {
    pub addr: u32,
    pub name: &'static str,
    pub subsystem: &'static str,
    pub run: BindingFn,
}

/// One binding entry. The subsystem strings are what the pure host matches on (`IN_PLACE`), so they stay
/// exactly as they are.
pub const fn bind(subsystem: &'static str, addr: u32, name: &'static str, run: BindingFn) -> Binding {
    Binding { addr, name, subsystem, run }
}

/// Every bound function.
pub fn bindings() -> impl Iterator<Item = &'static Binding> {
    vcu::BINDINGS
        .iter()
        .chain(&car_body::BINDINGS)
        .chain(&collision::BINDINGS)
        .chain(&spring_damper::BINDINGS)
        .chain(&tyre::BINDINGS)
        .chain(&engine_driveline::BINDINGS)
        .chain(&wheel::BINDINGS)
        .chain(&rigid_body::BINDINGS)
        .chain(&entities::BINDINGS)
        .chain(&ntweak::BINDINGS)
        .chain(&physics_core::BINDINGS)
        .chain(&timekeeper::BINDINGS)
        .chain(&ngp::BINDINGS)
        .chain(&game_log::BINDINGS)
        .chain(&graph::BINDINGS)
}

/// The binding of the original function at `addr`.
pub fn binding(addr: u32) -> Option<&'static Binding> {
    binding_slot(addr).map(|slot| bound()[slot])
}

/// The bindings by address, the first of each address kept (as `bindings().find` finds it).
pub fn bound() -> &'static [&'static Binding] {
    static BY_ADDR: std::sync::OnceLock<Vec<&'static Binding>> = std::sync::OnceLock::new();
    BY_ADDR.get_or_init(|| {
        let mut all: Vec<&'static Binding> = bindings().collect();
        all.sort_by_key(|b| b.addr);
        all.dedup_by_key(|b| b.addr);
        all
    })
}

/// The index in [`bound`] of the binding at `addr`, searched in a table of the addresses alone (one cache line holds
/// 16 of them; [`bound`]'s entries point elsewhere).
pub fn binding_slot(addr: u32) -> Option<usize> {
    static ADDRS: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    ADDRS.get_or_init(|| bound().iter().map(|b| b.addr).collect()).binary_search(&addr).ok()
}

/// Runs the binding of the original function at `addr`.
pub fn run(addr: u32, ctx: &mut Ctx, regs: &Regs) -> Result<Return, Decline> {
    match binding(addr) {
        Some(b) => (b.run)(ctx, regs),
        None => Err(Decline::NotBound),
    }
}

/// An f64 as the x87 holds it: the 64-bit significand with its explicit integer bit, and sign plus 15-bit exponent.
pub fn extended(x: f64) -> (u64, u16) {
    let bits = x.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exp = ((bits >> 52) & 0x7ff) as u16;
    let frac = bits & ((1 << 52) - 1);
    match exp {
        0 if frac == 0 => (0, sign),
        0 => {
            // Subnormal: frac * 2^-1074, normalised so the top bit is the integer bit.
            let shift = frac.leading_zeros();
            (frac << shift, sign | (15372 - shift) as u16)
        }
        0x7ff if frac == 0 => (1 << 63, sign | 0x7fff),
        // NaN keeps its payload and is quiet, as `fld` of a double gives.
        0x7ff => (1 << 63 | 1 << 62 | frac << 11, sign | 0x7fff),
        _ => (1 << 63 | frac << 11, sign | (exp + 16383 - 1023)),
    }
}

/// An x87 register's value as the nearest f64 (exact for every value an f32 or f64 load gives).
pub fn from_extended(significand: u64, exponent: u16) -> f64 {
    let sign = if exponent & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = (exponent & 0x7fff) as i32;
    if e == 0x7fff {
        return if significand << 1 == 0 { sign * f64::INFINITY } else { f64::NAN };
    }
    // Two exact scalings keep the exponent of each factor in range.
    let scale = e - 16383 - 63;
    sign * significand as f64 * 2f64.powi(scale / 2) * 2f64.powi(scale - scale / 2)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn extended_of_normals_zeros_and_specials() {
        assert_eq!(extended(1.0), (1 << 63, 0x3fff));
        assert_eq!(extended(-2.5), (0xa000_0000_0000_0000, 0xc000));
        assert_eq!(extended(0.0), (0, 0));
        assert_eq!(extended(-0.0), (0, 0x8000));
        assert_eq!(extended(f64::INFINITY), (1 << 63, 0x7fff));
        assert_eq!(extended(f64::NEG_INFINITY), (1 << 63, 0xffff));
        assert_eq!(extended(f64::NAN).1 & 0x7fff, 0x7fff);
        assert_eq!(extended(f64::NAN).0 >> 62, 0b11);
    }

    #[test]
    fn extended_of_subnormals_is_exact() {
        // 2^-1074: integer bit only, exponent -1074 + 16383.
        assert_eq!(extended(f64::from_bits(1)), (1 << 63, 16383 - 1074));
        // 3 * 2^-1060.
        let x = 3.0 * 2f64.powi(-1060);
        assert_eq!(extended(x), (0xc000_0000_0000_0000, 16383 - 1059));
    }

    #[test]
    fn from_extended_inverts_extended() {
        for x in [1.0, -2.5, 0.0, 1e-300, 3.0 * 2f64.powi(-1060), 0.1f32 as f64, -1e30, f64::INFINITY] {
            let (s, e) = extended(x);
            assert_eq!(from_extended(s, e).to_bits(), x.to_bits(), "{x}");
        }
        assert!(from_extended(1 << 63 | 1 << 62, 0x7fff).is_nan());
    }

    /// A host that answers float accessors from a table and logs every call.
    #[derive(Default)]
    pub struct Script {
        pub floats: Vec<(u32, u32, f32)>,
        pub log: RefCell<Vec<(u32, u32, Vec<u32>)>>,
    }

    impl Host for Script {
        fn call_at(&self, _sp: u32, addr: u32, ecx: u32, args: &[u32]) -> Called {
            self.log.borrow_mut().push((addr, ecx, args.to_vec()));
            let st0 = self.floats.iter().find(|f| f.0 == addr && f.1 == ecx).map_or(0.0, |f| f.2 as f64);
            Called { eax: 0, st0 }
        }
    }

    /// The bindings per subsystem: the pure step's in-place dispatch (`pure::IN_PLACE`) picks them by these names.
    #[test]
    fn bindings_per_subsystem() {
        let mut counts = std::collections::BTreeMap::new();
        for binding in bindings() {
            *counts.entry(binding.subsystem).or_insert(0) += 1;
        }
        let expected = [
            ("car_body", 99),
            ("collision", 68),
            ("engine_driveline", 32),
            ("entities", 89),
            ("game_log", 1),
            ("graph", 2),
            ("ngp", 21),
            ("ntweak", 4),
            ("physics_core", 38),
            ("rigid-body", 46),
            ("spring_damper", 4),
            ("timekeeper", 1),
            ("timing", 4),
            ("tyre", 11),
            ("vcu", 16),
            ("wheel", 26),
        ];
        assert_eq!(counts.into_iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn unbound_addresses_decline() {
        let host = Script::default();
        let mut ctx = Ctx { mem: Mem::new(), host: &host };
        assert_eq!(run(0x401000, &mut ctx, &Regs::default()), Err(Decline::NotBound));
    }
}
