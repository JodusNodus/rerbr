//! Rally-school animated replays: `Ghosts\rse%d.arp` in `Root/ghosts.rbz` (11 files: rse1, 2, 3, 5, 6,
//! 7, 8, 9, 11, 12, 15; `rse5.arp` is twice the size, the lesson 5 ride).
//!
//! The load site is `E59Host90::M00565c00` 0x565c00 (`src/rbr/RBR_E59_Gameflow.cpp`): for a timed lesson it loads
//! `Ghosts\rse%d.arp` through `E89Manager::LoadCar` 0x5fdfa0 (`src/rbr/RBR_E89_Mgr.cpp`), then `SetFlag(0, 0)` and
//! `StopCar(0)`, which arms the demo car; `E89Manager::Tick` 0x5fea00 advances its time and `E89_UpdateAll`
//! 0x5fdf30 plays it through `E88Car::Vf18` 0x5fd2c0 (`src/rbr/RBR_E88_Car.cpp`).
//!
//! Layout (little-endian; every retail file walks to exactly its last byte):
//!
//! - File: `LRPA` (`0x4152504c`), version 3, section count, trailing count (`LoadCar` checks the magic and the 3,
//!   then reads that many sections and that many trailing 8-byte snapshots). Every retail file has 1 section and 0
//!   trailing.
//! - Section: `tag`, `skip`, `size`, chunk count. `tag` is 0 in every retail file: `LoadCar` builds an `E88Car`,
//!   reads the head with `E88Car::LoadHead` 0x5fb980 and dispatches each chunk to `E88Car::Vf14` 0x5fc5a0 by its
//!   type. A non-zero `tag` section is skipped over `skip` bytes by the loader; `size` is the section's bytes after
//!   the file header (section header, head and chunks).
//! - Head (`0x290` bytes): `E88Car::WriteHead` 0x5fba00 writes one word and `E89Wheel::Write` 0x600600 four
//!   `0xa0`-byte wheel blocks. The word is read back as a float multiplier of the sampled steer (`Vf18`); the 12
//!   bytes after it are unwritten stack garbage in the save. Each wheel block holds the 37 words `Load` 0x5ff460
//!   scatters into the wheel (two ints, the time quantum at word 2, 34 floats) while its last 3 words are stack
//!   garbage `Write` never sets (zero in `rse5.arp`, stale floats elsewhere).
//! - Chunk: `type`, page `tick`, entry `count`, payload `size`, then the payload. Types (`Vf14`): 0 body, 1
//!   suspension, 2 steer (`C`), 3 engine noise, 4-7 per-wheel extra, 8-11 per-wheel tyre noise, 12 drivetrain
//!   (`rse5.arp` only). The payload is always one full buffer page (`E88*Buf::CopyFrom` copies the whole page:
//!   `0x8000` body, `0x2000` suspension/engine, `0x400` steer, `0xe00` drivetrain, `0x1800` extra/tyre); only the
//!   first `count` entries were recorded, the rest is stale buffer contents kept here as [`Chunk::filler`].
//!   The page `tick` is the chain tick-base (`CopyFrom` stores `info[1]`); the record path leaves it unset, so
//!   `rse5.arp` holds the debug fill `0x81...` and a few retail pages hold stale words. Playback (`Query`, the
//!   durations) never reads it.
//!
//! Entry formats (recorded by `E88Body::Record` 0x5f9560, `E88Car::Record` 0x5fd110 and friends):
//!
//! - Body (`0x20` bytes): position as three `f32`, a fourth `f32` the sampler carries but never uses (stale lane
//!   of the recorded `E01Vec4`: huge in some files, 0 in `rse5.arp`), the rotation quaternion as four `i16` times
//!   32767 (`E88_Quantize4` 0x5f8cc0), and 8 spare bytes (zero except in `rse5.arp` and part of `rse12.arp`: stack
//!   garbage).
//! - Suspension (8 bytes): per wheel one `ByteA` byte (`E89Wheel::ByteA` 0x5ff6d0: clamp to [0, 1], times 255
//!   with rounding bias) and one `ByteB` byte (0x5ff720: times `1/2pi`, truncated). Playback (`Vf18`) decodes
//!   them with `UnbyteA` 0x5ff770 (times the wheel quantum over 255) and `UnbyteB` 0x5ff790 (times `2pi` over
//!   255) into the wheel pose update (`E89Wheel::UpdatePose` 0x6007f0, whose solve is skeletal in the
//!   reconstruction, so the sampler stops at the decoded bytes' structure).
//! - Steer (1 byte): `(steer + 1) * 127.5 - 128` (`E88_Quant1` 0x5f9b00).
//! - Engine (8 bytes): `E88Car::EncodeExtra` 0x5f9ee0 scales seven lanes into bytes plus a flag byte; decoded by
//!   `E88Car::DecodeTire` 0x5fa0c0 for the engine-noise signal.
//! - Drivetrain (`0xe` bytes): `EncodeDrive` 0x5f9be0 / `DecodeDrive` 0x5f9df0.
//! - Extra / tyre (6 bytes each): per-wheel record bytes (`RecordExtra` 0x5fcc50, `RecordTire` 0x5fce40) blended
//!   by `E88_Delta6a/6b` 0x5fb010/0x5fb070 for the tyre signals.
//!
//! Playback ([`Section::sample_body`], [`Section::sample_steer`]) follows `Vf18` through `E88Body::Sample2`
//! 0x5f96f0 (`src/rbr/RBR_E88_Body.cpp`):
//!
//! - Time is the manager arg in seconds (`Tick` adds the frame `dt` after `StopCar`; `UpdateAll` passes it to
//!   `Vf18`; `IsDone` 0x5fe390 compares it with `Duration`).
//! - `Query` (position pages 0x5f8e80, small buffers `src/rbr/RBR_E88_Buffers.cpp`) walks the page chain with the
//!   quantum (`0.08` body from the `E88Body::Ctor` arg, `0.04` suspension/engine/drivetrain, `0.32` steer,
//!   `0.16` extra/tyre in `Vf18`): within a page `idx = t/q`, `f = frac`, clamping past the end; across the
//!   page seam the pair straddles the pages. A time at or below 0 yields the first entry.
//! - Position is `E11_VecLerp` 0x473c90 called as `(earlier, later, f)`. (The reconstruction transcribes both
//!   `Sample` 0x5f91d0 and `Sample2` with `a`/`b` swapped; the bytes show `a` is the earlier entry: at the
//!   0x5f9215 call `ecx` is reloaded with the `Query` `a` slot while the `b` slot goes on the stack, and at
//!   0x5f97a9 the same shape repeats. `Sample2`'s velocity, quats and slerp below confirm the slot mapping.)
//! - Rotation dequantises (`short * (1/32767)`, `E88_Dequantize` 0x5f8d90), normalises both ends, slerps
//!   (`E18_QuatSlerp` 0x49460, flip when the dot is negative, `acos`/`sin` path below the data word at
//!   `0x72fedc` = `0x3f7fff58`), and normalises again. `E18_QuatNormalize` 0x4921a0 uses `rsqrtss`, which has no
//!   portable exact form; the sampler divides by `sqrt`, good to ~1e-7 on unit quats.
//! - [`Section::duration`] (`E88_BodyDuration` 0x5f96a0, `E88Car::Duration` 0x5fc860) is the max over the body,
//!   suspension, steer and engine chains of `(head count - 1 + depth bonus) * quantum`; the drivetrain chain is
//!   sampled but never contributes. As in the original, a chained head page only spans its own entries.
//! - `sample_steer` is the `Vf18` steer signal input: the two bracketing steer bytes times `1/127.5` lerped,
//!   times the head's steer scale.
//!
//! Record moduli (why the counts differ: `Record` 0x5fd110 and its gates record the body every 8th call, the
//! suspension/engine/drivetrain every 4th, the steer every 32nd, the extra/tyre bytes every 16th) and the lane
//! scales live in the C++ only; entries stay raw here except the body pose and the steer byte the sampler reads.

// The sampler constants keep the C++ literals' exact decimal text (same nearest-f32 rounding as MSVC).
#![allow(clippy::excessive_precision)]

use std::fmt::{self, Display, Formatter};

/// File magic, the bytes `LRPA`.
pub const MAGIC: u32 = 0x4152504c;
/// File version (`LoadCar` reaches `int3` on anything else).
pub const VERSION: u32 = 3;
/// Head bytes after a tag-0 section header: one steer-scale word, 12 spare bytes, four wheel blocks.
pub const HEAD_LEN: usize = 0x290;
/// One wheel block's bytes (`E89Wheel::Write` 0x600600 writes `0xa0`).
pub const WHEEL_BLOCK_LEN: usize = 0xa0;
/// Body time quantum, the `E88Body::Ctor` arg both car paths pass (`0.0799...f`).
pub const BODY_QUANTUM: f32 = 0.08;
/// Suspension, engine-noise and drivetrain query quantum (`Vf18`).
pub const QUARTER_QUANTUM: f32 = 0.04;
/// Steer (`C`) query and duration quantum (`Vf18`, `E88Car::Duration`).
pub const STEER_QUANTUM: f32 = 0.32;
/// Extra/tyre query quantum (`Vf18`).
pub const EXTRA_QUANTUM: f32 = 0.16;
/// Quaternion dequantise scale (`E88_Dequantize` 0x5f8d90).
pub const QUAT_SCALE: f32 = 3.0518509447574615e-05;
/// Steer byte scale (`E88_Quant1` 0x5f9b00 encodes, `Vf18` multiplies by this).
pub const STEER_SCALE: f32 = 0.007843137718737125;
/// Slerp `acos`/`sin` path limit, the `f32` at `0x72fedc` (`E18_QuatSlerp` 0x49460).
pub const SLERP_LIMIT: f32 = f32::from_bits(0x3f7fff58);

/// Chunk types (`E88Car::Vf14` 0x5fc5a0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Body,
    Susp,
    Steer,
    Engine,
    Drive,
    Extra(u8),
    Tire(u8),
}

impl Kind {
    fn from_type(t: u32) -> Option<Kind> {
        match t {
            0 => Some(Kind::Body),
            1 => Some(Kind::Susp),
            2 => Some(Kind::Steer),
            3 => Some(Kind::Engine),
            12 => Some(Kind::Drive),
            4..=7 => Some(Kind::Extra((t - 4) as u8)),
            8..=11 => Some(Kind::Tire((t - 8) as u8)),
            _ => None,
        }
    }

    fn wire(self) -> u32 {
        match self {
            Kind::Body => 0,
            Kind::Susp => 1,
            Kind::Steer => 2,
            Kind::Engine => 3,
            Kind::Drive => 12,
            Kind::Extra(i) => 4 + u32::from(i),
            Kind::Tire(i) => 8 + u32::from(i),
        }
    }

    /// One entry's bytes.
    pub fn stride(self) -> usize {
        match self {
            Kind::Body => 0x20,
            Kind::Susp | Kind::Engine => 8,
            Kind::Steer => 1,
            Kind::Drive => 0xe,
            Kind::Extra(_) | Kind::Tire(_) => 6,
        }
    }

    /// One payload page's bytes (what `CopyFrom` copies and `Write` writes, whatever the count).
    pub fn capacity(self) -> usize {
        match self {
            Kind::Body => 0x8000,
            Kind::Susp | Kind::Engine => 0x2000,
            Kind::Steer => 0x400,
            Kind::Drive => 0xe00,
            Kind::Extra(_) | Kind::Tire(_) => 0x1800,
        }
    }

    /// Entries per full page (what `*Full`/`*Alloc` enforce).
    pub fn full(self) -> usize {
        self.capacity() / self.stride()
    }
}

/// One body entry (`0x20` bytes): the pose `Record` stores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyEntry {
    pub position: [f32; 3],
    /// The recorded `E01Vec4`'s fourth lane (stale in retail files: huge in some, 0 in `rse5.arp`).
    pub w: f32,
    /// Rotation quaternion `[x, y, z, w]` times 32767.
    pub rotation: [i16; 4],
    /// The entry's last 8 bytes (zero except in `rse5.arp` and part of `rse12.arp`: stack garbage).
    pub spare: [u8; 8],
}

/// A chunk's entries, in record order.
#[derive(Debug, Clone, PartialEq)]
pub enum Entries {
    Body(Vec<BodyEntry>),
    /// Suspension: four wheels of `[ByteA, ByteB]` per entry.
    Susp(Vec<[u8; 8]>),
    /// Steer: one `E88_Quant1` byte per entry.
    Steer(Vec<u8>),
    Engine(Vec<[u8; 8]>),
    Drive(Vec<[u8; 14]>),
    Extra(Vec<[u8; 6]>),
    Tire(Vec<[u8; 6]>),
}

impl Entries {
    pub fn len(&self) -> usize {
        match self {
            Entries::Body(e) => e.len(),
            Entries::Susp(e) => e.len(),
            Entries::Steer(e) => e.len(),
            Entries::Engine(e) => e.len(),
            Entries::Drive(e) => e.len(),
            Entries::Extra(e) => e.len(),
            Entries::Tire(e) => e.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One `0xa0`-byte wheel block of the head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelHead {
    /// File words 0-1 (`Load` copies them as ints into the wheel's +0/+4; `1,1,0,0` front to rear except in
    /// `rse5.arp`, whose `2,3,4,5` never saw `WheelInit` 0x5fa1f0).
    pub ids: [u32; 2],
    /// File word 2: the wheel's time quantum (body +8; `UnbyteA` scales by it). 1.0 in every retail file.
    pub quantum: f32,
    /// File words 3-36 (what `Load` scatters into the wheel and `Write` gathers back).
    pub fields: [f32; 34],
    /// File words 37-39: never written by `Write` (stack garbage; zero in `rse5.arp`).
    pub garbage: [u32; 3],
}

/// A tag-0 section's head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Head {
    /// The head's first word, read back as `f32`: `Vf18` multiplies the sampled steer by it.
    pub steer_scale: f32,
    /// `WriteHead`'s unset words 1-3 (stack garbage in the save).
    pub spare: [u8; 12],
    pub wheels: [WheelHead; 4],
}

/// One chunk: one buffer page of one channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub kind: Kind,
    /// The page tick-base (`CopyFrom` stores `info[1]`). The record path leaves it unset (`0x81...` fill in
    /// `rse5.arp`, stale words in a few retail pages); `Query` and the durations never read it.
    pub tick: u32,
    pub entries: Entries,
    /// The payload past `count * stride` up to the page capacity: stale record-buffer contents.
    pub filler: Vec<u8>,
}

/// One section: the head and the buffer pages of one car.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// `LoadCar` builds a car only for 0; anything else skips `skip` bytes.
    pub tag: u32,
    /// Ignored by the loader for `tag` 0 (`0x481` everywhere but `rse5.arp`'s `0x1`).
    pub skip: u32,
    /// The section's bytes after the file header (header, head and chunks).
    pub size: u32,
    /// The loader's chunk count (always the chunks read; kept for skipped sections).
    pub count: u32,
    pub head: Option<Head>,
    /// A non-zero `tag` section's skipped bytes.
    pub skipped: Vec<u8>,
    pub chunks: Vec<Chunk>,
}

/// One trailing snapshot (`E89Manager::Snapshot` 0x5feac0 appends the manager arg and a tag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trailing {
    pub time_bits: u32,
    pub tag: u32,
}

/// A parsed `.arp` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub sections: Vec<Section>,
    /// The trailing table (empty in every retail file: their trailing count is 0).
    pub trailing: Vec<Trailing>,
}

/// The demo car's pose at one time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyPose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Short {
        len: usize,
    },
    Magic,
    Version(u32),
    Kind(u32),
    /// A chunk with no entries (the loader would `int3` in `Query`).
    Empty {
        kind: u32,
    },
    /// The entry count overflows the page.
    Count {
        kind: u32,
        count: usize,
        capacity: usize,
    },
    /// The payload is not one full page.
    Size {
        kind: u32,
        size: usize,
        want: usize,
    },
    Section {
        stored: usize,
        actual: usize,
    },
    Trailing {
        stored: usize,
        actual: usize,
    },
    /// Bytes left over after the sections and the trailing table.
    Overlong {
        extra: usize,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Short { len } => write!(f, "{len} bytes is shorter than the header"),
            Error::Magic => f.write_str("not an animated replay (bad magic)"),
            Error::Version(v) => write!(f, "animated replay version {v}, want {VERSION}"),
            Error::Kind(t) => write!(f, "animated replay chunk type {t}"),
            Error::Empty { kind } => write!(f, "animated replay chunk type {kind} has no entries"),
            Error::Count { kind, count, capacity } => {
                write!(f, "animated replay chunk type {kind} holds {count} entries, page fits {capacity}")
            }
            Error::Size { kind, size, want } => {
                write!(f, "animated replay chunk type {kind} payload is {size} bytes, page is {want}")
            }
            Error::Section { stored, actual } => {
                write!(f, "animated replay section of {stored} bytes, holds {actual}")
            }
            Error::Trailing { stored, actual } => {
                write!(f, "animated replay trailing table of {stored} entries, file holds {actual}")
            }
            Error::Overlong { extra } => write!(f, "animated replay has {extra} bytes past its end"),
        }
    }
}

impl std::error::Error for Error {}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn parse_wheel(block: &[u8]) -> WheelHead {
    let words = |i: usize| word(block, i * 4);
    WheelHead {
        ids: [words(0), words(1)],
        quantum: f32::from_le_bytes(block[8..12].try_into().unwrap()),
        fields: std::array::from_fn(|i| f32::from_le_bytes(block[(i + 3) * 4..(i + 4) * 4].try_into().unwrap())),
        garbage: [words(37), words(38), words(39)],
    }
}

fn write_wheel(out: &mut Vec<u8>, wheel: &WheelHead) {
    for id in wheel.ids {
        out.extend_from_slice(&id.to_le_bytes());
    }
    out.extend_from_slice(&wheel.quantum.to_le_bytes());
    for field in wheel.fields {
        out.extend_from_slice(&field.to_le_bytes());
    }
    for garbage in wheel.garbage {
        out.extend_from_slice(&garbage.to_le_bytes());
    }
}

fn parse_body(entry: &[u8]) -> BodyEntry {
    BodyEntry {
        position: std::array::from_fn(|i| f32::from_le_bytes(entry[i * 4..i * 4 + 4].try_into().unwrap())),
        w: f32::from_le_bytes(entry[12..16].try_into().unwrap()),
        rotation: std::array::from_fn(|i| i16::from_le_bytes(entry[16 + i * 2..18 + i * 2].try_into().unwrap())),
        spare: entry[24..32].try_into().unwrap(),
    }
}

fn write_body(out: &mut Vec<u8>, entry: &BodyEntry) {
    for p in entry.position {
        out.extend_from_slice(&p.to_le_bytes());
    }
    out.extend_from_slice(&entry.w.to_le_bytes());
    for q in entry.rotation {
        out.extend_from_slice(&q.to_le_bytes());
    }
    out.extend_from_slice(&entry.spare);
}

fn parse_entries(kind: Kind, payload: &[u8], count: usize) -> Entries {
    let stride = kind.stride();
    let body = &payload[..count * stride];
    match kind {
        Kind::Body => Entries::Body(body.chunks_exact(stride).map(parse_body).collect()),
        Kind::Susp => Entries::Susp(body.chunks_exact(stride).map(|e| e.try_into().unwrap()).collect()),
        Kind::Steer => Entries::Steer(body.to_vec()),
        Kind::Engine => Entries::Engine(body.chunks_exact(stride).map(|e| e.try_into().unwrap()).collect()),
        Kind::Drive => Entries::Drive(body.chunks_exact(stride).map(|e| e.try_into().unwrap()).collect()),
        Kind::Extra(_) => Entries::Extra(body.chunks_exact(stride).map(|e| e.try_into().unwrap()).collect()),
        Kind::Tire(_) => Entries::Tire(body.chunks_exact(stride).map(|e| e.try_into().unwrap()).collect()),
    }
}

fn write_entries(out: &mut Vec<u8>, entries: &Entries) {
    match entries {
        Entries::Body(e) => {
            for entry in e {
                write_body(out, entry);
            }
        }
        Entries::Susp(e) | Entries::Engine(e) => {
            for entry in e {
                out.extend_from_slice(entry);
            }
        }
        Entries::Steer(e) => out.extend_from_slice(e),
        Entries::Drive(e) => {
            for entry in e {
                out.extend_from_slice(entry);
            }
        }
        Entries::Extra(e) | Entries::Tire(e) => {
            for entry in e {
                out.extend_from_slice(entry);
            }
        }
    }
}

impl Section {
    fn parse(bytes: &[u8], at: &mut usize) -> Result<Section, Error> {
        if *at + 16 > bytes.len() {
            return Err(Error::Short { len: bytes.len() });
        }
        let (tag, skip, size, count) =
            (word(bytes, *at), word(bytes, *at + 4), word(bytes, *at + 8), word(bytes, *at + 12));
        let start = *at;
        *at += 16;
        if tag != 0 {
            if *at + skip as usize > bytes.len() {
                return Err(Error::Short { len: bytes.len() });
            }
            let skipped = bytes[*at..*at + skip as usize].to_vec();
            *at += skip as usize;
            if *at - start != size as usize {
                return Err(Error::Section { stored: size as usize, actual: *at - start });
            }
            return Ok(Section { tag, skip, size, count, head: None, skipped, chunks: Vec::new() });
        }
        if *at + HEAD_LEN > bytes.len() {
            return Err(Error::Short { len: bytes.len() });
        }
        let head = Head {
            steer_scale: f32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap()),
            spare: bytes[*at + 4..*at + 16].try_into().unwrap(),
            wheels: std::array::from_fn(|i| {
                parse_wheel(&bytes[*at + 16 + i * WHEEL_BLOCK_LEN..*at + 16 + (i + 1) * WHEEL_BLOCK_LEN])
            }),
        };
        *at += HEAD_LEN;
        let mut chunks = Vec::with_capacity(count as usize);
        for _ in 0..count {
            if *at + 16 > bytes.len() {
                return Err(Error::Short { len: bytes.len() });
            }
            let (t, tick, entries, size) =
                (word(bytes, *at), word(bytes, *at + 4), word(bytes, *at + 8) as usize, word(bytes, *at + 12) as usize);
            let Some(kind) = Kind::from_type(t) else { return Err(Error::Kind(t)) };
            if size != kind.capacity() {
                return Err(Error::Size { kind: t, size, want: kind.capacity() });
            }
            if entries == 0 {
                return Err(Error::Empty { kind: t });
            }
            if entries > kind.full() {
                return Err(Error::Count { kind: t, count: entries, capacity: kind.full() });
            }
            *at += 16;
            if *at + size > bytes.len() {
                return Err(Error::Short { len: bytes.len() });
            }
            let payload = &bytes[*at..*at + size];
            let parsed = parse_entries(kind, payload, entries);
            let filler = payload[entries * kind.stride()..].to_vec();
            *at += size;
            chunks.push(Chunk { kind, tick, entries: parsed, filler });
        }
        if *at - start != size as usize {
            return Err(Error::Section { stored: size as usize, actual: *at - start });
        }
        Ok(Section { tag, skip, size, count, head: Some(head), skipped: Vec::new(), chunks })
    }

    /// The section's bytes after the file header (what `size` stores).
    fn body_len(&self) -> usize {
        if self.tag != 0 {
            return 16 + self.skipped.len();
        }
        16 + HEAD_LEN + self.chunks.iter().map(|c| 16 + c.kind.capacity()).sum::<usize>()
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.tag.to_le_bytes());
        out.extend_from_slice(&self.skip.to_le_bytes());
        out.extend_from_slice(&(self.body_len() as u32).to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        if self.tag != 0 {
            out.extend_from_slice(&self.skipped);
            return;
        }
        let head = self.head.as_ref().expect("tag-0 section without a head");
        out.extend_from_slice(&head.steer_scale.to_le_bytes());
        out.extend_from_slice(&head.spare);
        for wheel in &head.wheels {
            write_wheel(out, wheel);
        }
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.kind.wire().to_le_bytes());
            out.extend_from_slice(&chunk.tick.to_le_bytes());
            out.extend_from_slice(&(chunk.entries.len() as u32).to_le_bytes());
            out.extend_from_slice(&(chunk.kind.capacity() as u32).to_le_bytes());
            write_entries(out, &chunk.entries);
            out.extend_from_slice(&chunk.filler);
        }
    }
}

impl Replay {
    pub fn parse(bytes: &[u8]) -> Result<Replay, Error> {
        if bytes.len() < 16 {
            return Err(Error::Short { len: bytes.len() });
        }
        if word(bytes, 0) != MAGIC {
            return Err(Error::Magic);
        }
        let version = word(bytes, 4);
        if version != VERSION {
            return Err(Error::Version(version));
        }
        let (sections, trailing_count) = (word(bytes, 8) as usize, word(bytes, 12) as usize);
        let mut at = 16;
        let mut parsed = Vec::with_capacity(sections);
        for _ in 0..sections {
            parsed.push(Section::parse(bytes, &mut at)?);
        }
        if bytes.len() - at != trailing_count * 8 {
            return Err(Error::Trailing { stored: trailing_count, actual: (bytes.len() - at) / 8 });
        }
        let mut trailing = Vec::with_capacity(trailing_count);
        for _ in 0..trailing_count {
            trailing.push(Trailing { time_bits: word(bytes, at), tag: word(bytes, at + 4) });
            at += 8;
        }
        if at != bytes.len() {
            return Err(Error::Overlong { extra: bytes.len() - at });
        }
        Ok(Replay { sections: parsed, trailing })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.sections.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.trailing.len() as u32).to_le_bytes());
        for section in &self.sections {
            section.write(&mut out);
        }
        for snapshot in &self.trailing {
            out.extend_from_slice(&snapshot.time_bits.to_le_bytes());
            out.extend_from_slice(&snapshot.tag.to_le_bytes());
        }
        out
    }
}

/// One page chain's entry counts, in file order.
fn chain_counts(section: &Section, want: Kind) -> Vec<usize> {
    section.chunks.iter().filter(|c| c.kind == want).map(|c| c.entries.len()).collect()
}

/// The `Query` entry pair (`E88PosBuf::Query` 0x5f8e80 and its small-buffer twins): global entry indices into
/// the chain and the fraction between them.
fn query_index(counts: &[usize], full: usize, t: f32, quantum: f32) -> Option<(usize, usize, f32)> {
    if counts.is_empty() || counts.contains(&0) {
        return None;
    }
    let mut base = 0;
    let mut time = t;
    for (page, &count) in counts.iter().enumerate() {
        if time <= 0.0 {
            return Some((base, base, 0.0));
        }
        if time <= quantum * 1023.0 {
            let d = f64::from(time) / f64::from(quantum);
            let idx = d as usize;
            let frac = (d - idx as f64) as f32;
            if count - 1 <= idx {
                return Some((base + count - 1, base + count - 1, 0.0));
            }
            return Some((base + idx, base + idx + 1, frac));
        }
        if count != full || page + 1 == counts.len() {
            return Some((base + count - 1, base + count - 1, 0.0));
        }
        if time < quantum * 1024.0 {
            let d = f64::from(time) / f64::from(quantum);
            let idx = d as usize;
            let frac = (d - idx as f64) as f32;
            return Some((base + count - 1, base + count, frac));
        }
        time -= quantum * 1024.0;
        base += count;
    }
    None
}

/// Chain depth bonus of `Duration`: 0 for one page, 1 for two, the tail depth plus 1 after
/// (`E88_BodyDuration` 0x5f96a0 counts the head page and one quantum per further page).
fn depth_bonus(pages: usize) -> usize {
    match pages {
        0 | 1 => 0,
        n => n - 1,
    }
}

fn chain_duration(counts: &[usize], quantum: f32) -> Option<f32> {
    let (&head, _) = counts.split_first()?;
    Some((head + depth_bonus(counts.len()) - 1) as f32 * quantum)
}

fn normalize(q: [f32; 4]) -> [f32; 4] {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / len, q[1] / len, q[2] / len, q[3] / len]
}

/// `E18_QuatSlerp` 0x49460.
fn slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let mut s = 1.0f32;
    if dot < 0.0 {
        dot = -dot;
        s = -1.0;
    }
    let (f4, f5) = if dot < SLERP_LIMIT {
        let angle = f64::from(dot).acos();
        let sine = angle.sin();
        (
            ((f64::from(1.0f32 - t) * angle).sin() / sine) as f32,
            ((f64::from(t) * angle).sin() / sine * f64::from(s)) as f32,
        )
    } else {
        (1.0 - t, s * t)
    };
    [a[0] * f4 + b[0] * f5, a[1] * f4 + b[1] * f5, a[2] * f4 + b[2] * f5, a[3] * f4 + b[3] * f5]
}

impl Section {
    fn body_entries(&self) -> Vec<&BodyEntry> {
        self.chunks
            .iter()
            .filter(|c| c.kind == Kind::Body)
            .filter_map(|c| match &c.entries {
                Entries::Body(e) => Some(e.iter()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn steer_bytes(&self) -> Vec<u8> {
        self.chunks
            .iter()
            .filter(|c| c.kind == Kind::Steer)
            .filter_map(|c| match &c.entries {
                Entries::Steer(e) => Some(e.iter()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect()
    }

    /// `E88Car::Duration` 0x5fc860: the max over the body, suspension, steer and engine chains. 0.0 with none
    /// (the loader would crash in `Query`; retail sections hold all four).
    pub fn duration(&self) -> f32 {
        let mut best = 0.0f32;
        for (kind, quantum) in [
            (Kind::Body, BODY_QUANTUM),
            (Kind::Susp, QUARTER_QUANTUM),
            (Kind::Steer, STEER_QUANTUM),
            (Kind::Engine, QUARTER_QUANTUM),
        ] {
            if let Some(d) = chain_duration(&chain_counts(self, kind), quantum) {
                best = best.max(d);
            }
        }
        best
    }

    /// The demo car's body pose at `t` seconds (`Sample2` 0x5f96f0 without the signals).
    pub fn sample_body(&self, t: f32) -> Option<BodyPose> {
        let counts = chain_counts(self, Kind::Body);
        let (ia, ib, frac) = query_index(&counts, Kind::Body.full(), t, BODY_QUANTUM)?;
        let entries = self.body_entries();
        let (a, b) = (entries[ia], entries[ib]);
        let position = std::array::from_fn(|i| (b.position[i] - a.position[i]) * frac + a.position[i]);
        let qa = normalize(std::array::from_fn(|i| f32::from(a.rotation[i]) * QUAT_SCALE));
        let qb = normalize(std::array::from_fn(|i| f32::from(b.rotation[i]) * QUAT_SCALE));
        let rotation = normalize(slerp(qa, qb, frac));
        Some(BodyPose { position, rotation })
    }

    /// The steer signal input at `t` seconds (`Vf18`: the bracketing steer bytes through `1/127.5` lerped,
    /// times the head's steer scale).
    pub fn sample_steer(&self, t: f32) -> Option<f32> {
        let counts = chain_counts(self, Kind::Steer);
        let (ia, ib, frac) = query_index(&counts, Kind::Steer.full(), t, STEER_QUANTUM)?;
        let bytes = self.steer_bytes();
        let (a, b) = (bytes[ia] as i8 as f32, bytes[ib] as i8 as f32);
        let steer = (a - b) * STEER_SCALE * frac + b * STEER_SCALE;
        Some(steer * self.head.as_ref()?.steer_scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_chunk(count: usize, tick: u32) -> Chunk {
        Chunk {
            kind: Kind::Body,
            tick,
            entries: Entries::Body(
                (0..count)
                    .map(|i| BodyEntry {
                        position: [i as f32, 0.0, 0.0],
                        w: 0.0,
                        rotation: [0, 0, 0, 32767],
                        spare: [0; 8],
                    })
                    .collect(),
            ),
            filler: vec![0xcc; Kind::Body.capacity() - count * Kind::Body.stride()],
        }
    }

    fn test_head() -> Head {
        Head {
            steer_scale: 1.0,
            spare: [0; 12],
            wheels: [WheelHead { ids: [0; 2], quantum: 1.0, fields: [0.0; 34], garbage: [0; 3] }; 4],
        }
    }

    fn test_replay(chunks: Vec<Chunk>) -> Replay {
        Replay {
            sections: vec![Section {
                tag: 0,
                skip: 0x481,
                size: 0,
                count: chunks.len() as u32,
                head: Some(test_head()),
                skipped: Vec::new(),
                chunks,
            }],
            trailing: Vec::new(),
        }
    }

    #[test]
    fn chunk_round_trip() {
        let bytes = test_replay(vec![body_chunk(3, 0)]).write();
        let back = Replay::parse(&bytes).unwrap();
        assert_eq!(back.write(), bytes);
        assert_eq!(back.sections.len(), 1);
        assert_eq!(back.sections[0].chunks.len(), 1);
    }

    #[test]
    fn header_errors() {
        assert_eq!(Replay::parse(&[]), Err(Error::Short { len: 0 }));
        let mut bytes = test_replay(vec![body_chunk(2, 0)]).write();
        bytes[0] ^= 1;
        assert_eq!(Replay::parse(&bytes), Err(Error::Magic));
        bytes[0] ^= 1;
        bytes[4] = 4;
        assert_eq!(Replay::parse(&bytes), Err(Error::Version(4)));
    }

    #[test]
    fn unknown_chunk_type_errors() {
        let mut bytes = test_replay(vec![body_chunk(2, 0)]).write();
        let at = 16 + 16 + HEAD_LEN;
        bytes[at] = 13;
        assert_eq!(Replay::parse(&bytes), Err(Error::Kind(13)));
    }

    #[test]
    fn query_clamps_and_straddles() {
        assert_eq!(query_index(&[], 0x400, 1.0, 0.08), None);
        assert_eq!(query_index(&[0], 0x400, 1.0, 0.08), None);
        assert_eq!(query_index(&[3], 0x400, -1.0, 0.08), Some((0, 0, 0.0)));
        assert_eq!(query_index(&[3], 0x400, 0.0, 0.08), Some((0, 0, 0.0)));
        assert_eq!(query_index(&[3], 0x400, 100.0, 0.08), Some((2, 2, 0.0)));
        // t = 1.5 quanta into one page: pair (1, 2), f = 0.5.
        let (a, b, f) = query_index(&[8], 0x400, 0.12, 0.08).unwrap();
        assert_eq!((a, b), (1, 2));
        assert!((f - 0.5).abs() < 1e-6);
    }

    #[test]
    fn slerp_flips_and_falls_back() {
        // Opposite quats take the short way round.
        let q = slerp([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, -1.0], 0.5);
        assert!((q[3] - 1.0).abs() < 1e-6, "{q:?}");
        // Near-identical quats use the linear fallback.
        let q = slerp([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0], 0.25);
        assert!((q[3] - 1.0).abs() < 1e-6, "{q:?}");
    }
}
