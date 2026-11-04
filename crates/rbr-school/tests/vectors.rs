//! Replays the original's direct calls of the rally-school judging core through
//! `rbr_school`. The records come from `harness/direct_calls.py --set gameplay_school`, built in the
//! emulator's stub heap so every record is self-contained (the bank, its arrays and the gameflow/app/message/signal
//! objects it reads).
//!
//! Needs `RBR_SCHOOL_VECTORS=<file>[:<file>...]`, the record files, or `RBR_VECTOR_STORE` (its `gameplay_school/`);
//! skipped without either. The record format is the one in harness/vcu_vectors.py: each call's arguments (`this`
//! first), the windows before and after, the x87 return, and the external calls it made (emits with their payload
//! windows, the class gate, the driveline sample and the 256 repair-part reads) as inner records.

use rbr_formats::dls::DlsReplayStream;
use rbr_school::{
    checkpoint, end_race, eval_filters, max_abs, school_incident, school_tick, severity_max, tracker, trigger, Bank,
    Check, FilterSub, IncidentInput, Lead, Pair, SchoolEvent,
};
use std::collections::BTreeMap;

/// Bytes of hex text.
fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

const MAX_ABS: u32 = 0x497b10;
const IS_ARMED: u32 = 0x497da0;
const TRIP: u32 = 0x497dd0;
const WINDOW_HIT: u32 = 0x497f90;
const SEVERITY_MAX: u32 = 0x498240;
const EVAL_FILTERS: u32 = 0x4982e0;
const READ_STREAM: u32 = 0x498840;
const SCHOOL_TICK: u32 = 0x498bb0;
const SCHOOL_INCIDENT: u32 = 0x498e10;
const END_RACE: u32 = 0x499110;
const CHECKPOINT: u32 = 0x4991c0;
const TRIGGER: u32 = 0x4992a0;
const TRACKER: u32 = 0x499360;
const CHK_CLEAR: u32 = 0x498630;
const SUB_INIT: u32 = 0x498660;
const UPDATE: u32 = 0x4986a0;
const FLAG_MAP: u32 = 0x4980a0;
const GET20: u32 = 0x497e40;
const GET24: u32 = 0x497e50;
const GET28: u32 = 0x497e60;
const SOUND_EMIT: u32 = 0x498c70;
const INCIDENT_EMIT: u32 = 0x498d30;
const ENDRACE_EMIT: u32 = 0x477920;
const CLS_GATE: u32 = 0x49a0f0;
const SAMPLE: u32 = 0x50b180;
const GET_DAMAGE: u32 = 0x4f2b40;

const SUB_STRIDE: usize = 0x2c;
const CHK_STRIDE: usize = 0x20;

#[derive(Default, Clone)]
struct Window {
    address: u32,
    pre: Vec<u8>,
    post: Vec<u8>,
}

#[derive(Default, Clone)]
struct Inner {
    address: u32,
    eax: u32,
    args: Vec<u32>,
    windows: BTreeMap<usize, Window>,
    after: Vec<(u32, Vec<u8>)>,
}

#[derive(Default, Clone)]
struct Call {
    address: u32,
    args: Vec<u32>,
    windows: BTreeMap<usize, Window>,
    after: Vec<(u32, Vec<u8>)>,
    eax: u32,
    st0: f64,
    inner: Vec<Inner>,
}

fn parse(text: &str) -> Vec<Call> {
    let mut calls: Vec<Call> = Vec::new();
    let mut inner_index = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.first().copied() {
            Some("call") => {
                inner_index = None;
                calls.push(Call {
                    address: u32::from_str_radix(f[2].trim_start_matches("0x"), 16).unwrap(),
                    ..Call::default()
                });
            }
            Some("args") => {
                calls.last_mut().unwrap().args =
                    f[1..].iter().map(|word| u32::from_str_radix(word, 16).unwrap()).collect();
            }
            Some("win") => {
                let index = f[1].parse().unwrap();
                let address = u32::from_str_radix(f[2].trim_start_matches("0x"), 16).unwrap();
                let window = calls.last_mut().unwrap().windows.entry(index).or_default();
                window.address = address;
                if f[4] == "pre" {
                    window.pre = f.get(5).map_or_else(Vec::new, |data| hex(data));
                } else {
                    window.post = hex(f[5]);
                }
            }
            Some("awin") => {
                let call = calls.last_mut().unwrap();
                call.after.push((u32::from_str_radix(f[1].trim_start_matches("0x"), 16).unwrap(), hex(f[3])));
            }
            Some("ret") => {
                let call = calls.last_mut().unwrap();
                call.eax = u32::from_str_radix(f[1], 16).unwrap();
                call.st0 = f64::from_bits(u64::from_str_radix(f[2], 16).unwrap());
            }
            Some("inner") => {
                let inner = Inner {
                    address: u32::from_str_radix(f[1].trim_start_matches("0x"), 16).unwrap(),
                    eax: u32::from_str_radix(f[2], 16).unwrap(),
                    args: f[4..].iter().map(|word| u32::from_str_radix(word, 16).unwrap()).collect(),
                    ..Inner::default()
                };
                let call = calls.last_mut().unwrap();
                call.inner.push(inner);
                inner_index = Some(call.inner.len() - 1);
            }
            Some("iwin") => {
                let index = f[1].parse().unwrap();
                let window = calls.last_mut().unwrap().inner[inner_index.expect("iwin without inner")]
                    .windows
                    .entry(index)
                    .or_default();
                window.address = u32::from_str_radix(f[2].trim_start_matches("0x"), 16).unwrap();
                window.pre = hex(f[4]);
                window.post = hex(f[5]);
            }
            Some("iawin") => {
                let call = calls.last_mut().unwrap();
                call.inner[inner_index.expect("iawin without inner")]
                    .after
                    .push((u32::from_str_radix(f[1].trim_start_matches("0x"), 16).unwrap(), hex(f[3])));
            }
            _ => {}
        }
    }
    calls
}

fn records() -> Option<Vec<Call>> {
    let mut calls = Vec::new();
    for path in record_paths("RBR_SCHOOL_VECTORS", "gameplay_school")? {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        calls.extend(parse(&text));
    }
    Some(calls)
}

fn record_paths(var: &str, dir: &str) -> Option<Vec<std::path::PathBuf>> {
    if let Ok(paths) = std::env::var(var) {
        return Some(paths.split(':').map(Into::into).collect());
    }
    let dir = std::path::Path::new(&std::env::var_os("RBR_VECTOR_STORE")?).join(dir);
    let mut paths: Vec<_> = std::fs::read_dir(dir).ok()?.map(|e| e.unwrap().path()).collect();
    paths.retain(|p| p.extension().is_some_and(|x| x == "txt"));
    paths.sort();
    Some(paths)
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn ifloat(bytes: &[u8], at: usize) -> f32 {
    f32::from_bits(word(bytes, at))
}

fn sub_of(bytes: &[u8]) -> FilterSub {
    assert_eq!(bytes.len(), SUB_STRIDE);
    FilterSub {
        tag: word(bytes, 0) as i32,
        value: word(bytes, 4) as i32,
        class: word(bytes, 8) as i32,
        flags: word(bytes, 12) as i32,
        center: ifloat(bytes, 16),
        hi: ifloat(bytes, 20),
        lo: ifloat(bytes, 24),
        near_bits: word(bytes, 28),
        lim0: word(bytes, 32) as i32,
        lim1: word(bytes, 36) as i32,
        lim2: word(bytes, 40) as i32,
    }
}

fn check_of(bytes: &[u8]) -> Check {
    assert_eq!(bytes.len(), CHK_STRIDE);
    Check {
        pos0: ifloat(bytes, 0),
        pos: ifloat(bytes, 4),
        span: ifloat(bytes, 8),
        lane: word(bytes, 12) as i32,
        code: word(bytes, 16) as i32,
        hi: ifloat(bytes, 20),
        lo: ifloat(bytes, 24),
        state: word(bytes, 28) as i32,
    }
}

fn win_bytes(call: &Call, index: usize, post: bool) -> &[u8] {
    let w = &call.windows[&index];
    if post {
        &w.post
    } else {
        &w.pre
    }
}

/// The scalar words of one 0x60-byte bank window (array contents, slots and lead need their own windows).
fn bank_scalars(bytes: &[u8]) -> Bank {
    assert_eq!(bytes.len(), 0x60);
    Bank {
        origin: word(bytes, 0) as i32,
        speed: ifloat(bytes, 4),
        time_limit: ifloat(bytes, 8),
        pass_time: ifloat(bytes, 12),
        perfect_time: ifloat(bytes, 16),
        subs: Vec::new(),
        slots: [None; 4],
        checks: Vec::new(),
        lead: None,
        pairs: Vec::new(),
        ack: word(bytes, 0x40),
        latched_a: word(bytes, 0x44) != 0,
        latched_b: word(bytes, 0x48) != 0,
        timer: ifloat(bytes, 0x50),
        sounded: word(bytes, 0x54) != 0,
        live: word(bytes, 0x58) != 0,
    }
}

/// The bank a bank-root record's windows hold (pre or post selected by `post`).
fn bank_of(call: &Call, post: bool) -> Bank {
    let bank_bytes = win_bytes(call, 0, post);
    let subs_win = win_bytes(call, 1, post);
    let chks_win = win_bytes(call, 2, post);
    let pairs_win = win_bytes(call, 3, post);
    assert_eq!(bank_bytes.len(), 0x60);
    let subs_addr = call.windows[&1].address;
    let chks_addr = call.windows[&2].address;
    let pairs_addr = call.windows[&3].address;
    let nsub = word(bank_bytes, 0x14) as usize;
    let nchk = word(bank_bytes, 0x2c) as usize;
    let npair = word(bank_bytes, 0x38) as usize;
    let subs: Vec<FilterSub> = (0..nsub)
        .map(|i| {
            let base = word(bank_bytes, 0x18).wrapping_sub(subs_addr) as usize + i * SUB_STRIDE;
            sub_of(&subs_win[base..base + SUB_STRIDE])
        })
        .collect();
    let slot = |ptr: u32| {
        ptr.checked_sub(subs_addr).and_then(|off| {
            (off as usize).is_multiple_of(SUB_STRIDE).then_some(off as usize / SUB_STRIDE).filter(|&i| i < 6)
        })
    };
    let lead = match word(bank_bytes, 0x34) {
        0 => None,
        ptr => {
            // The check array follows the sub array, so a check lead can fall in the sub range
            // numerically; alignment disambiguates (the fuzz only writes aligned valid leads).
            if let Some(i) = slot(ptr) {
                Some(Lead::Sub(i))
            } else {
                let off = ptr.checked_sub(chks_addr).expect("lead outside the arrays") as usize;
                assert_eq!(off % CHK_STRIDE, 0);
                Some(Lead::Check(off / CHK_STRIDE))
            }
        }
    };
    let mut bank = bank_scalars(bank_bytes);
    bank.slots =
        [word(bank_bytes, 0x1c), word(bank_bytes, 0x20), word(bank_bytes, 0x24), word(bank_bytes, 0x28)].map(slot);
    bank.subs = subs;
    bank.checks = (0..nchk)
        .map(|i| {
            let base = word(bank_bytes, 0x30).wrapping_sub(chks_addr) as usize + i * CHK_STRIDE;
            check_of(&chks_win[base..base + CHK_STRIDE])
        })
        .collect();
    bank.lead = lead;
    bank.pairs = (0..npair)
        .map(|i| {
            let base = word(bank_bytes, 0x3c).wrapping_sub(pairs_addr) as usize + i * 8;
            Pair { threshold: ifloat(pairs_win, base), code: word(pairs_win, base + 4) as i32 }
        })
        .collect();
    bank
}

/// The arrays a `ReadStream` record's `awin` snapshots hold, in the recorder's subs/checks/pairs order (the
/// freed bank's own windows carry no arrays). The lengths must match the post bank's counts exactly.
fn stream_arrays(call: &Call, nsub: usize, nchk: usize, npair: usize) -> (Vec<FilterSub>, Vec<Check>, Vec<Pair>) {
    let mut after = call.after.iter();
    let mut take = |len: usize| -> Vec<u8> {
        if len == 0 {
            return Vec::new();
        }
        let (_, bytes) = after.next().expect("array snapshot missing");
        assert_eq!(bytes.len(), len, "array snapshot length");
        bytes.clone()
    };
    let subs = take(nsub * SUB_STRIDE);
    let checks = take(nchk * CHK_STRIDE);
    let pairs = take(npair * 8);
    assert!(after.next().is_none(), "extra array snapshot");
    (
        subs.chunks_exact(SUB_STRIDE).map(sub_of).collect(),
        checks.chunks_exact(CHK_STRIDE).map(check_of).collect(),
        pairs.chunks_exact(8).map(|w| Pair { threshold: ifloat(w, 0), code: word(w, 4) as i32 }).collect(),
    )
}

/// The gameflow/app/message words of a bank root's node windows: (mode, lesson, substate, cb4, app, time).
fn globals_of(call: &Call, first: usize) -> (i32, u32, u32, i32, u32, u32) {
    let gameflow = &call.windows[&first].pre;
    let app = &call.windows[&(first + 1)].pre;
    let msg = &call.windows[&(first + 2)].pre;
    (
        word(gameflow, 0) as i32,
        word(gameflow, 0xca4),
        word(gameflow, 0xca8),
        word(gameflow, 0xcb4) as i32,
        word(app, 0),
        word(msg, 0),
    )
}

/// The emit events of one call's inner records, in order.
fn events_of(call: &Call) -> Vec<SchoolEvent> {
    let mut events = Vec::new();
    for inner in &call.inner {
        match inner.address {
            INCIDENT_EMIT => events.push(SchoolEvent::Incident { code: word(&inner.windows[&0].pre, 0) as i32 }),
            SOUND_EMIT => {
                let p = &inner.windows[&0].pre;
                events.push(SchoolEvent::Sound {
                    a: word(p, 0) as i32,
                    b: word(p, 4) as i32,
                    c: word(p, 8) as i32,
                    d: word(p, 12) as i32,
                });
            }
            ENDRACE_EMIT => events.push(SchoolEvent::EndRace { time: word(&inner.windows[&0].pre, 0) }),
            _ => {}
        }
    }
    events
}

/// The 256 part severities of an `EvalFilters`/`SeverityMax` record, by the `GetDamage` inner index.
/// Latched banks and lessons 4/16 return before the damage loop, so those records read no parts.
fn damages_of(call: &Call) -> ([u32; 256], bool) {
    let mut damage = [0u32; 256];
    let mut seen = [false; 256];
    for inner in &call.inner {
        if inner.address != GET_DAMAGE {
            continue;
        }
        let idx = inner.args[1] as usize;
        assert!(idx < 256);
        damage[idx] = inner.eax;
        seen[idx] = true;
    }
    (damage, seen.iter().all(|&s| s))
}

fn st0(call: &Call) -> f32 {
    call.st0 as f32
}

/// An x87 return compare. The recorder's `ext_to_f64` keeps no NaN payload (`math.nan`), so NaN returns match
/// by NaN-ness (the named-site rule in notes/physics.md); ordered returns match bit for bit. All six are pure
/// loads/selects (`flds`, `fcomip`+`movss`), which preserve payloads by moving bits, so NaN-ness plus the
/// bitwise ordered cases verify them.
fn assert_st0(got: f32, call: &Call, what: &str, n: usize) {
    let want = st0(call);
    if want.is_nan() || got.is_nan() {
        assert!(want.is_nan() && got.is_nan(), "{what} {n}: want {want:?}, got {got:?}");
    } else {
        assert_eq!(got.to_bits(), want.to_bits(), "{what} {n}");
    }
}

#[test]
fn sub_primitives_match_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = BTreeMap::new();
    for (n, call) in calls.iter().enumerate() {
        let sub = || sub_of(&call.windows[&0].pre);
        let arg = |i: usize| call.args[i + 1];
        let argf = |i: usize| f32::from_bits(arg(i));
        match call.address {
            MAX_ABS => {
                let v = [
                    ifloat(&call.windows[&0].pre, 0),
                    ifloat(&call.windows[&0].pre, 4),
                    ifloat(&call.windows[&0].pre, 8),
                ];
                assert_st0(max_abs(v), call, "MaxAbs", n);
            }
            IS_ARMED => assert_eq!(sub().is_armed() as u32, call.eax, "IsArmed {n}"),
            TRIP => {
                let mut s = sub();
                s.trip(argf(0));
                assert_eq!(sub_bits(&s), sub_bits(&sub_of(&call.windows[&0].post)), "Trip {n}");
            }
            WINDOW_HIT => assert_eq!(sub().window_hit(argf(0)) as u32, call.eax, "WindowHit {n}"),
            CHK_CLEAR => {
                assert!(check_bits(&Check::default()) == check_bits(&check_of(&call.windows[&0].post)), "ChkClear {n}");
                assert_eq!(call.eax, call.args[0], "ChkClear returns this");
            }
            SUB_INIT => {
                assert_eq!(sub_bits(&FilterSub::init_sub()), sub_bits(&sub_of(&call.windows[&0].post)), "SubInit {n}");
                assert_eq!(call.eax, call.args[0], "SubInit returns this");
            }
            UPDATE => {
                let mut s = sub();
                let status = s.update(argf(0));
                assert_eq!(sub_bits(&s), sub_bits(&sub_of(&call.windows[&0].post)), "Update {n} state");
                assert_eq!(status, call.eax as i32, "Update {n} status");
            }
            FLAG_MAP => assert_eq!(sub().flag_map(), call.eax as i32, "FlagMap {n}"),
            GET20 => assert_eq!(sub().get20(), call.eax as i32, "Get20 {n}"),
            GET24 => assert_eq!(sub().get24(), call.eax as i32, "Get24 {n}"),
            GET28 => assert_eq!(sub().get28(), call.eax as i32, "Get28 {n}"),
            _ => continue,
        }
        *checked.entry(call.address).or_insert(0) += 1;
    }
    for addr in [MAX_ABS, IS_ARMED, TRIP, WINDOW_HIT, CHK_CLEAR, SUB_INIT, UPDATE, FLAG_MAP, GET20, GET24, GET28] {
        assert!(checked.get(&addr).is_some_and(|&n| n > 0), "no records for {addr:#x}");
    }
    eprintln!("sub primitives: {checked:?}");
}

/// Bitwise equality: derived `PartialEq` treats NaN as unequal, but the port must reproduce the original's NaN
/// payloads bit for bit (SSE ops preserve them), so records compare every word.
fn sub_bits(s: &FilterSub) -> [u32; 11] {
    [
        s.tag as u32,
        s.value as u32,
        s.class as u32,
        s.flags as u32,
        s.center.to_bits(),
        s.hi.to_bits(),
        s.lo.to_bits(),
        s.near_bits,
        s.lim0 as u32,
        s.lim1 as u32,
        s.lim2 as u32,
    ]
}

fn check_bits(c: &Check) -> [u32; 8] {
    [
        c.pos0.to_bits(),
        c.pos.to_bits(),
        c.span.to_bits(),
        c.lane as u32,
        c.code as u32,
        c.hi.to_bits(),
        c.lo.to_bits(),
        c.state as u32,
    ]
}

fn banks_match(want: &Bank, got: &Bank) -> bool {
    want.origin == got.origin
        && want.speed.to_bits() == got.speed.to_bits()
        && want.time_limit.to_bits() == got.time_limit.to_bits()
        && want.pass_time.to_bits() == got.pass_time.to_bits()
        && want.perfect_time.to_bits() == got.perfect_time.to_bits()
        && want.slots == got.slots
        && want.subs.len() == got.subs.len()
        && want.subs.iter().zip(got.subs.iter()).all(|(a, b)| sub_bits(a) == sub_bits(b))
        && want.checks.len() == got.checks.len()
        && want.checks.iter().zip(got.checks.iter()).all(|(a, b)| check_bits(a) == check_bits(b))
        && want.lead == got.lead
        && want.pairs.len() == got.pairs.len()
        && want
            .pairs
            .iter()
            .zip(got.pairs.iter())
            .all(|(a, b)| a.threshold.to_bits() == b.threshold.to_bits() && a.code == b.code)
        && want.ack == got.ack
        && want.latched_a == got.latched_a
        && want.latched_b == got.latched_b
        && want.timer.to_bits() == got.timer.to_bits()
        && want.sounded == got.sounded
        && want.live == got.live
}

/// A record outside flow mode 7, which the ports do not run: the original left the bank as it was and emitted
/// nothing.
fn outside_school(call: &Call, bank: &Bank, mode: i32, n: usize) -> bool {
    if mode == 7 {
        return false;
    }
    assert_bank(call, bank, n);
    assert!(events_of(call).is_empty(), "record {n} emits outside the school");
    true
}

fn assert_bank(call: &Call, bank: &Bank, n: usize) {
    let want = bank_of(call, true);
    if !banks_match(&want, bank) {
        for (name, a, b) in [
            ("subs", want.subs.len(), bank.subs.len()),
            ("checks", want.checks.len(), bank.checks.len()),
            ("pairs", want.pairs.len(), bank.pairs.len()),
        ] {
            if a != b {
                eprintln!("record {n} {name} len: want {a} got {b}");
            }
        }
        for (i, (a, b)) in want.subs.iter().zip(bank.subs.iter()).enumerate() {
            if sub_bits(a) != sub_bits(b) {
                eprintln!("record {n} sub {i}:\n  want {a:?}\n  got  {b:?}");
            }
        }
        for (i, (a, b)) in want.checks.iter().zip(bank.checks.iter()).enumerate() {
            if check_bits(a) != check_bits(b) {
                eprintln!("record {n} check {i}:\n  want {a:?}\n  got  {b:?}");
            }
        }
        if want.slots != bank.slots {
            eprintln!("record {n} slots: want {:?} got {:?}", want.slots, bank.slots);
        }
        if want.lead != bank.lead {
            eprintln!("record {n} lead: want {:?} got {:?}", want.lead, bank.lead);
        }
        if want.pairs.len() != bank.pairs.len()
            || want
                .pairs
                .iter()
                .zip(bank.pairs.iter())
                .any(|(a, b)| a.threshold.to_bits() != b.threshold.to_bits() || a.code != b.code)
        {
            eprintln!("record {n} pairs: want {:?} got {:?}", want.pairs, bank.pairs);
        }
        for (name, a, b) in [
            ("origin", want.origin as u32, bank.origin as u32),
            ("speed", want.speed.to_bits(), bank.speed.to_bits()),
            ("time_limit", want.time_limit.to_bits(), bank.time_limit.to_bits()),
            ("pass_time", want.pass_time.to_bits(), bank.pass_time.to_bits()),
            ("perfect_time", want.perfect_time.to_bits(), bank.perfect_time.to_bits()),
            ("ack", want.ack, bank.ack),
            ("latched_a", want.latched_a as u32, bank.latched_a as u32),
            ("latched_b", want.latched_b as u32, bank.latched_b as u32),
            ("timer", want.timer.to_bits(), bank.timer.to_bits()),
            ("sounded", want.sounded as u32, bank.sounded as u32),
            ("live", want.live as u32, bank.live as u32),
        ] {
            if a != b {
                eprintln!("record {n} {name}: want {a:#x} got {b:#x}");
            }
        }
    }
    assert!(banks_match(&want, bank), "record {n} bank post state");
}

#[test]
fn read_stream_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == READ_STREAM) {
        let stream = &call.windows[&2].pre;
        let header: [u32; 7] = std::array::from_fn(|i| word(stream, i * 4));
        let (ten, six, pairs) = if header[0] == 6 {
            let ten: Vec<[u32; 10]> =
                (0..header[1] as usize).map(|i| std::array::from_fn(|k| word(stream, 28 + i * 40 + k * 4))).collect();
            let six: Vec<[u32; 6]> = (0..header[2] as usize)
                .map(|i| std::array::from_fn(|k| word(stream, 28 + header[1] as usize * 40 + i * 24 + k * 4)))
                .collect();
            let pair_base = 28 + header[1] as usize * 40 + header[2] as usize * 24;
            let pairs: Vec<[u32; 2]> = (0..header[3] as usize)
                .map(|i| [word(stream, pair_base + i * 8), word(stream, pair_base + i * 8 + 4)])
                .collect();
            (ten, six, pairs)
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
        let replay = DlsReplayStream { offset: 0, header, ten_word_records: ten, six_word_records: six, pairs };
        let block_base = word(&call.windows[&4].pre, 0);
        let pre = bank_scalars(&call.windows[&0].pre);
        let bank = Bank::from_stream(&pre, &replay, block_base);
        // The freed bank's scalars come from its own window (no array windows exist); the arrays come
        // from the fill snapshots. `FreeState` also clears the slots and the lead.
        let post_bytes = &call.windows[&0].post;
        assert_eq!(&post_bytes[0x1c..0x2c], &[0; 16], "record {n} slots cleared");
        assert_eq!(word(post_bytes, 0x34), 0, "record {n} lead cleared");
        assert_eq!(
            (word(post_bytes, 0x14) as usize, word(post_bytes, 0x2c) as usize, word(post_bytes, 0x38) as usize),
            (bank.subs.len(), bank.checks.len(), bank.pairs.len()),
            "record {n} counts"
        );
        let (subs, checks, pairs) = stream_arrays(call, bank.subs.len(), bank.checks.len(), bank.pairs.len());
        let mut post_full = bank_scalars(post_bytes);
        post_full.subs = subs;
        post_full.checks = checks;
        post_full.pairs = pairs;
        assert!(banks_match(&post_full, &bank), "record {n} full post bank");
        // The 7-word header copy advances the stream past 28 bytes before the magic check.
        let consumed = if header[0] == 6 {
            28 + header[1] as usize * 40 + header[2] as usize * 24 + header[3] as usize * 8
        } else {
            28
        };
        let cursor = word(&call.windows[&1].pre, 4);
        assert_eq!(word(&call.windows[&1].post, 4), cursor + consumed as u32, "record {n} cursor");
        checked += 1;
    }
    assert!(checked > 0, "no ReadStream records");
    eprintln!("ReadStream: {checked} records equal");
}

#[test]
fn school_tick_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == SCHOOL_TICK) {
        let mut bank = bank_of(call, false);
        let v = &call.windows[&4].pre;
        let input = [ifloat(v, 0), ifloat(v, 4), ifloat(v, 8)];
        let (mode, _, substate, _, _, _) = globals_of(call, 5);
        if outside_school(call, &bank, mode, n) {
            continue;
        }
        school_tick(&mut bank, input, substate);
        assert_bank(call, &bank, n);
        assert!(events_of(call).is_empty(), "record {n} emits");
        checked += 1;
    }
    assert!(checked > 0, "no SchoolTick records");
    eprintln!("SchoolTick: {checked} records equal");
}

#[test]
fn school_incident_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == SCHOOL_INCIDENT) {
        let mut bank = bank_of(call, false);
        let v = &call.windows[&4].pre;
        let (mode, _, substate, _, app, _) = globals_of(call, 5);
        let sample = call.inner.iter().find(|i| i.address == SAMPLE).and_then(|i| {
            i.after.iter().find_map(|&(at, ref bytes)| {
                (at == i.args[2]).then_some([ifloat(bytes, 0), ifloat(bytes, 4), ifloat(bytes, 8)])
            })
        });
        let gate = call.inner.iter().find(|i| i.address == CLS_GATE).is_some_and(|i| i.eax != 0);
        if outside_school(call, &bank, mode, n) {
            continue;
        }
        let input = IncidentInput {
            speed_vec: [ifloat(v, 0x30), ifloat(v, 0x34), ifloat(v, 0x38)],
            car_pos: [ifloat(v, 0x10), ifloat(v, 0x14), ifloat(v, 0x18)],
            app_state: app,
            substate,
            driveline: sample,
            cls_gate: gate,
        };
        let events = school_incident(&mut bank, &input);
        assert_bank(call, &bank, n);
        assert_eq!(events, events_of(call), "record {n} events");
        checked += 1;
    }
    assert!(checked > 0, "no SchoolIncident records");
    eprintln!("SchoolIncident: {checked} records equal");
}

#[test]
fn end_race_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == END_RACE) {
        let mut bank = bank_of(call, false);
        let (mode, _, _, _, _, host_time) = globals_of(call, 4);
        if outside_school(call, &bank, mode, n) {
            continue;
        }
        let events = end_race(&mut bank, f32::from_bits(call.args[1]), host_time);
        assert_bank(call, &bank, n);
        assert_eq!(events, events_of(call), "record {n} events");
        checked += 1;
    }
    assert!(checked > 0, "no EndRace records");
    eprintln!("EndRace: {checked} records equal");
}

#[test]
fn checkpoint_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == CHECKPOINT) {
        let mut bank = bank_of(call, false);
        let flags = call.windows[&4].pre[0x18];
        let (mode, _, _, _, app, _) = globals_of(call, 5);
        if outside_school(call, &bank, mode, n) {
            continue;
        }
        let events = checkpoint(&mut bank, flags, app);
        assert_bank(call, &bank, n);
        assert_eq!(events, events_of(call), "record {n} events");
        checked += 1;
    }
    assert!(checked > 0, "no Checkpoint records");
    eprintln!("Checkpoint: {checked} records equal");
}

#[test]
fn trigger_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == TRIGGER && c.windows.contains_key(&0)) {
        let mut bank = bank_of(call, false);
        let (_, _, substate, cb4, _, _) = globals_of(call, 4);
        let events = trigger(&mut bank, f32::from_bits(call.args[1]), cb4, substate);
        assert_bank(call, &bank, n);
        assert_eq!(events, events_of(call), "record {n} events");
        checked += 1;
    }
    assert!(checked > 0, "no Trigger records");
    eprintln!("Trigger: {checked} records equal");
}

#[test]
fn tracker_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == TRACKER) {
        let mut bank = bank_of(call, false);
        let (mode, _, substate, cb4, _, _) = globals_of(call, 4);
        let p_bits = word(&call.windows[&7].pre, 0);
        if outside_school(call, &bank, mode, n) {
            continue;
        }
        let events = tracker(&mut bank, p_bits, substate, cb4);
        assert_bank(call, &bank, n);
        assert_eq!(events, events_of(call), "record {n} events");
        checked += 1;
    }
    assert!(checked > 0, "no Tracker records");
    eprintln!("Tracker: {checked} records equal");
}

#[test]
fn eval_filters_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == EVAL_FILTERS) {
        let bank = bank_of(call, false);
        let (_, lesson, _, _, _, _) = globals_of(call, 4);
        let (damages, full) = damages_of(call);
        let latched = bank.latched_a || bank.latched_b;
        assert_eq!(full, !latched && lesson != 4 && lesson != 16, "record {n} damage loop ran");
        let verdict = eval_filters(&bank, f32::from_bits(call.args[1]), lesson as i32, &damages);
        assert_eq!(verdict, call.eax, "record {n} verdict");
        assert_bank(call, &bank, n);
        checked += 1;
    }
    assert!(checked > 0, "no EvalFilters records");
    eprintln!("EvalFilters: {checked} records equal");
}

#[test]
fn severity_max_matches_the_original() {
    let Some(calls) = records() else {
        eprintln!("RBR_SCHOOL_VECTORS and RBR_VECTOR_STORE unset; skipped");
        return;
    };
    let mut checked = 0;
    for (n, call) in calls.iter().enumerate().filter(|(_, c)| c.address == SEVERITY_MAX) {
        let bank = bank_of(call, false);
        let (_, lesson, _, _, _, _) = globals_of(call, 4);
        let (damages, full) = damages_of(call);
        assert_eq!(full, lesson != 4 && lesson != 16, "record {n} damage loop ran");
        let max = severity_max(lesson as i32, &damages);
        assert_eq!(max, call.eax, "record {n} severity");
        assert_bank(call, &bank, n);
        checked += 1;
    }
    assert!(checked > 0, "no SeverityMax records");
    eprintln!("SeverityMax: {checked} records equal");
}

/// Every retail lesson stream builds a bank whose arrays hold the header's counts (`RBR_DATA`; skipped without it).
#[test]
fn retail_dls_streams_build_banks() {
    let Some(data) = std::env::var_os("RBR_DATA") else {
        eprintln!("RBR_DATA unset; skipped");
        return;
    };
    let (mut files, mut v6) = (0, 0);
    for entry in std::fs::read_dir(std::path::Path::new(&data).join("Maps")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "dls") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let file = rbr_formats::dls::parse(&bytes).unwrap();
        files += 1;
        for animation in &file.animation_entries {
            let stream = &animation.replay;
            let bank = Bank::from_stream(&Bank::fresh(), stream, 0);
            if stream.header[0] == 6 {
                assert_eq!(bank.subs.len(), stream.ten_word_records.len(), "{path:?}");
                assert_eq!(bank.checks.len(), stream.six_word_records.len(), "{path:?}");
                assert_eq!(bank.pairs.len(), stream.pairs.len(), "{path:?}");
            } else {
                assert!(bank.subs.is_empty() && bank.checks.is_empty() && bank.pairs.is_empty(), "{path:?}");
            }
            v6 += usize::from(stream.header[0] == 6);
        }
    }
    eprintln!("{files} dls files, {v6} version-6 streams");
    assert!(files > 0);
}
