//! Milestone B0: the four struts (`CSpringDamper`, wheel+0xec4) run alone in
//! Rust over the tape's steps, from their bytes at the tape's start. Each call gets only the tape's inputs; its
//! outputs and, after every step, every byte of the struts' objects must be the original's. The state is never
//! re-seeded from the tape, and the memory holds only the struts' objects and their owners' ids, so a port that reads
//! anything else fails. The tape: `harness/subtree_tape.py` (its header names the run).
//!
//! The tape runs on both storages of the struts' state: the image between calls (each call lifts its strut and
//! lowers it) and the typed storage over each step (lifted at the step's start, lowered at its end); the images must
//! be equal byte for byte after every step.

use rbr_physics::mem::Mem;
use rbr_physics::spring_damper::layout::{self, CarStruts};
use rbr_physics::spring_damper::store::StrutStore;
use rbr_physics::spring_damper::{strut, DamperSignal, NgpStep, StepInput, StepOutput};

const TAPE: &str = include_str!("vectors/struts-snap-300.txt");
/// Where the pure run's `Step` writes its three outputs (the original's slots are in the wheel's frame).
const OUT: u32 = 0x1000_0000;
/// 1.02's fade words (`0x74025c`, `0x740258`), which the tape's car has.
const FADE: NgpStep = NgpStep { fade_threshold: 27_000.0, fade_scale: f32::from_bits(0x3869_0453) };

struct Strut {
    controller: u32,
    def: u32,
    state: u32,
    fade: u32,
    owner_id: u32,
}

impl Strut {
    /// The objects compared after every step, as the tape's `end` lines give them.
    fn objects(&self) -> [(&'static str, u32); 3] {
        [("controller", self.controller), ("state", self.state), ("fade", self.fade)]
    }
}

/// Where the struts' state lives while the tape runs.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Storage {
    /// The image between calls: each call runs on its strut lifted into a store of its own.
    PerCall,
    /// The typed storage over each step.
    Typed,
}

fn word(hex: &str) -> u32 {
    u32::from_str_radix(hex, 16).unwrap()
}

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

/// Panics at the first byte where `mem` differs from `want`, naming the object and its offset.
fn compare(mem: &Mem, at: u32, want: &[u8], what: &str) {
    let got = mem.read(at, want.len());
    if let Some(i) = (0..want.len()).find(|&i| got[i] != want[i]) {
        let w = i & !3;
        panic!("{what} +{w:#x}: {:02x?}, the original's {:02x?}", &got[w..w + 4], &want[w..w + 4]);
    }
}

/// A call the tape makes on a strut, and what it returns.
enum Op {
    Step(StepInput),
    Reset,
    Update,
}

enum Out {
    Step(StepOutput),
    Reset(DamperSignal),
    Done,
}

fn run_op<S: StrutStore>(store: &mut S, id: S::Strut, op: Op) -> Out {
    match op {
        Op::Step(input) => Out::Step(strut::step(store, id, input, FADE)),
        Op::Reset => Out::Reset(strut::reset_outputs(store, id)),
        Op::Update => {
            strut::update_forces(store, id);
            Out::Done
        }
    }
}

/// `op` on the strut whose controller is at `controller`, in `storage` (`typed` the struts lifted at the step).
fn call(storage: Storage, mem: &mut Mem, typed: Option<&mut CarStruts>, controller: u32, op: Op) -> Out {
    match storage {
        Storage::PerCall => layout::with_strut(mem, controller, |view, id| run_op(view, id, op)),
        Storage::Typed => {
            let typed = typed.expect("lifted at the step");
            let id = typed.id_of(controller).expect("one of the car's struts");
            run_op(&mut typed.view(mem), id, op)
        }
    }
}

/// Runs the tape on `storage`, checking it against the original's bytes; the bytes of every object the memory holds
/// after each step.
fn run(storage: Storage) -> Vec<Vec<u8>> {
    let mut mem = Mem::new();
    mem.map(OUT, &[0; 12]);
    let mut struts: Vec<Strut> = Vec::new();
    let (mut step, mut steps, mut calls) = (None, 0, 0);
    let mut typed: Option<CarStruts> = None;
    let mut images = Vec::new();
    for line in TAPE.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let place = |step: Option<u32>| format!("{storage:?}: step {}, wheel {}", step.unwrap_or(0), f[1]);
        match f[0] {
            "strut" => struts.push(Strut {
                controller: word(f[2]),
                def: word(f[3]),
                state: word(f[4]),
                fade: word(f[5]),
                owner_id: word(f[6]),
            }),
            "start" => {
                let s = &struts[word(f[1]) as usize];
                for (hex, at) in f[2..].iter().zip([s.controller, s.state, s.fade, s.def]) {
                    mem.map(at, &bytes(hex));
                }
                let owner = mem.u32(s.state + 0x18);
                mem.map(owner + 0xeb4, &s.owner_id.to_le_bytes());
            }
            "step" => {
                step = Some(f[1].parse::<u32>().unwrap());
                if storage == Storage::Typed {
                    let controllers = std::array::from_fn(|k| struts.get(k).map(|s| s.controller));
                    typed = Some(CarStruts::lift_controllers(&mem, controllers));
                }
            }
            "call" => {
                let s = &struts[word(f[1]) as usize];
                let [travel, coordinate, velocity] = [f[2], f[3], f[4]].map(word);
                let input = StepInput {
                    travel: f32::from_bits(travel),
                    coordinate: f32::from_bits(coordinate),
                    velocity: f32::from_bits(velocity),
                };
                let Out::Step(out) = call(storage, &mut mem, typed.as_mut(), s.controller, Op::Step(input)) else {
                    unreachable!()
                };
                mem.set_f32(OUT, out.spring_stop_force);
                mem.set_f32(OUT + 4, out.breakpoint_offset);
                mem.set_f32(OUT + 8, out.damping_coefficient);
                let want: Vec<u8> = f[6..9].iter().flat_map(|w| word(w).to_le_bytes()).collect();
                compare(&mem, OUT, &want, &format!("{}: Step outputs", place(step)));
                calls += 1;
            }
            "reset" => {
                let s = &struts[word(f[1]) as usize];
                let Out::Reset(emitted) = call(storage, &mut mem, typed.as_mut(), s.controller, Op::Reset) else {
                    unreachable!()
                };
                let got = [
                    emitted.owner_id,
                    emitted.peak_velocity.to_bits(),
                    emitted.bump_velocity.to_bits(),
                    emitted.droop_velocity.to_bits(),
                    emitted.valid as u32,
                ];
                assert_eq!(got, [f[3], f[4], f[5], f[6], f[7]].map(word), "{}: emitted payload", place(step));
                calls += 1;
            }
            "update" => {
                let s = &struts[word(f[1]) as usize];
                call(storage, &mut mem, typed.as_mut(), s.controller, Op::Update);
                call(storage, &mut mem, typed.as_mut(), s.controller, Op::Reset);
                calls += 1;
            }
            "end" => {
                if let Some(typed) = typed.take() {
                    typed.lower(&mut mem);
                }
                let s = &struts[word(f[1]) as usize];
                for ((name, at), hex) in s.objects().into_iter().zip(&f[2..5]) {
                    compare(&mem, at, &bytes(hex), &format!("{}: {name}", place(step)));
                }
                if f[1] == "3" {
                    steps += 1;
                    let image = struts.iter().flat_map(|s| {
                        [(s.controller, 0x30), (s.state, 0x3c), (s.fade, 8), (s.def, 0x11c)]
                            .into_iter()
                            .flat_map(|(at, len)| mem.read(at, len))
                    });
                    images.push(image.chain(mem.read(OUT, 12)).collect());
                }
            }
            "def" => {
                let s = &struts[word(f[1]) as usize];
                compare(&mem, s.def, &bytes(f[2]), &format!("after the last step, wheel {}: def", f[1]));
            }
            "foreign" => panic!("the tape has a write from outside the struts' calls: {line}"),
            other => panic!("unknown tape line {other}"),
        }
    }
    assert_eq!(struts.len(), 4);
    assert!(steps > 0 && calls > 0);
    eprintln!("{storage:?}: {steps} steps, {calls} calls: bit-exact");
    images
}

#[test]
fn struts_run_alone_bit_exact() {
    run(Storage::PerCall);
}

#[test]
fn the_typed_storage_over_a_step_keeps_the_per_call_bytes() {
    let (per_call, typed) = (run(Storage::PerCall), run(Storage::Typed));
    assert_eq!(per_call.len(), typed.len());
    for (step, (a, b)) in per_call.iter().zip(&typed).enumerate() {
        let at = a.iter().zip(b).position(|(x, y)| x != y);
        assert!(at.is_none(), "step {step} differs from the per-call path at byte {at:?}");
    }
}
