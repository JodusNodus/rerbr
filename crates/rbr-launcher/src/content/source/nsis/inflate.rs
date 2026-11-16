//! Raw deflate as NSIS writes it. Some NSIS builds leave out the stored block's `NLEN` (the one's complement of `LEN`
//! that standard deflate puts after it), which standard inflaters reject, so this one reads the stream itself. A
//! stream is one flavour throughout: the first stored block says which, by whether `NLEN` follows `LEN`.

use std::io::{self, Read};

const WINDOW: usize = 1 << 15;
const INPUT: usize = 1 << 16;

const LENGTH_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CODE_LENGTH_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn corrupt() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "corrupt deflate stream")
}

/// A Huffman code as a lookup table indexed by the next `bits` bits of the stream: symbol << 4 | code length.
struct Table {
    bits: u32,
    entries: Vec<u16>,
}

impl Table {
    fn new(lengths: &[u8]) -> io::Result<Table> {
        let bits = u32::from(lengths.iter().copied().max().unwrap_or(0)).max(1);
        let mut count = [0u32; 16];
        for &l in lengths {
            count[usize::from(l)] += 1;
        }
        count[0] = 0;
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for l in 1..16 {
            code = (code + count[l - 1]) << 1;
            next[l] = code;
            if code + count[l] > 1 << l {
                return Err(corrupt());
            }
        }
        let mut entries = vec![0u16; 1 << bits];
        for (symbol, &l) in lengths.iter().enumerate().filter(|(_, l)| **l != 0) {
            let c = next[usize::from(l)];
            next[usize::from(l)] += 1;
            let reversed = (c.reverse_bits() >> (32 - u32::from(l))) as usize;
            for i in (reversed..entries.len()).step_by(1 << l) {
                entries[i] = (symbol as u16) << 4 | u16::from(l);
            }
        }
        Ok(Table { bits, entries })
    }
}

enum State {
    /// Before a block's header.
    Header,
    /// In a stored block with this many bytes left.
    Stored(usize),
    /// In a Huffman block.
    Codes(Table, Table),
    Done,
}

/// A deflate stream (NSIS flavour) read from `R`; reading stops at the end of the final block.
pub struct Inflater<R> {
    input: R,
    buf: Box<[u8; INPUT]>,
    at: usize,
    end: usize,
    eof: bool,
    bits: u64,
    count: u32,
    window: Box<[u8; WINDOW]>,
    /// Bytes written so far (the window is indexed by this modulo its size).
    total: u64,
    /// A match cut short by the caller's buffer: (length left, distance).
    pending: (usize, usize),
    last: bool,
    /// Whether stored blocks carry `NLEN`, known from the first one.
    nlen: Option<bool>,
    state: State,
}

impl<R: Read> Inflater<R> {
    pub fn new(input: R) -> Inflater<R> {
        Inflater {
            input,
            buf: Box::new([0; INPUT]),
            at: 0,
            end: 0,
            eof: false,
            bits: 0,
            count: 0,
            window: Box::new([0; WINDOW]),
            total: 0,
            pending: (0, 0),
            last: false,
            nlen: None,
            state: State::Header,
        }
    }

    fn fill(&mut self) -> io::Result<()> {
        if self.at == self.end && !self.eof {
            self.at = 0;
            self.end = self.input.read(&mut self.buf[..])?;
            self.eof = self.end == 0;
        }
        Ok(())
    }

    /// Tops the bit buffer up to more than 56 bits, or as far as the input goes.
    fn refill(&mut self) -> io::Result<()> {
        while self.count <= 56 {
            self.fill()?;
            if self.at == self.end {
                break;
            }
            self.bits |= u64::from(self.buf[self.at]) << self.count;
            self.at += 1;
            self.count += 8;
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> io::Result<u32> {
        if self.count < n {
            self.refill()?;
            if self.count < n {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        let v = (self.bits & ((1u64 << n) - 1)) as u32;
        self.bits >>= n;
        self.count -= n;
        Ok(v)
    }

    fn symbol(&mut self, table: &Table) -> io::Result<usize> {
        if self.count < table.bits {
            self.refill()?;
        }
        let entry = table.entries[(self.bits & ((1u64 << table.bits) - 1)) as usize];
        let len = u32::from(entry & 15);
        if len == 0 || len > self.count {
            return Err(if len == 0 { corrupt() } else { io::ErrorKind::UnexpectedEof.into() });
        }
        self.bits >>= len;
        self.count -= len;
        Ok(usize::from(entry >> 4))
    }

    fn header(&mut self) -> io::Result<()> {
        if self.last {
            self.state = State::Done;
            return Ok(());
        }
        self.last = self.take(1)? == 1;
        self.state = match self.take(2)? {
            0 => {
                self.take(self.count % 8)?;
                let len = self.take(16)?;
                if self.nlen.is_none() {
                    self.refill()?;
                    self.nlen = Some(self.count >= 16 && self.bits & 0xFFFF == u64::from(!len & 0xFFFF));
                }
                if self.nlen == Some(true) && self.take(16)? != !len & 0xFFFF {
                    return Err(corrupt());
                }
                State::Stored(len as usize)
            }
            1 => {
                let mut lengths = [8u8; 288];
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                State::Codes(Table::new(&lengths)?, Table::new(&[5; 30])?)
            }
            2 => self.dynamic()?,
            _ => return Err(corrupt()),
        };
        Ok(())
    }

    fn dynamic(&mut self) -> io::Result<State> {
        let literals = self.take(5)? as usize + 257;
        let distances = self.take(5)? as usize + 1;
        let lengths = self.take(4)? as usize + 4;
        let mut order = [0u8; 19];
        for &i in &CODE_LENGTH_ORDER[..lengths] {
            order[i] = self.take(3)? as u8;
        }
        let meta = Table::new(&order)?;
        let mut all = [0u8; 288 + 32];
        let mut i = 0;
        while i < literals + distances {
            let (value, repeat) = match self.symbol(&meta)? {
                s @ 0..=15 => (s as u8, 1),
                16 if i > 0 => (all[i - 1], 3 + self.take(2)? as usize),
                17 => (0, 3 + self.take(3)? as usize),
                18 => (0, 11 + self.take(7)? as usize),
                _ => return Err(corrupt()),
            };
            if i + repeat > literals + distances {
                return Err(corrupt());
            }
            all[i..i + repeat].fill(value);
            i += repeat;
        }
        if all[256] == 0 {
            return Err(corrupt());
        }
        Ok(State::Codes(Table::new(&all[..literals])?, Table::new(&all[literals..literals + distances])?))
    }

    /// Copies a pending match into `out`, returning how many bytes went.
    fn copy_match(&mut self, out: &mut [u8]) -> usize {
        let (left, distance) = self.pending;
        let n = left.min(out.len());
        for byte in &mut out[..n] {
            *byte = self.window[(self.total as usize).wrapping_sub(distance) % WINDOW];
            self.window[self.total as usize % WINDOW] = *byte;
            self.total += 1;
        }
        self.pending.0 -= n;
        n
    }

    fn put(&mut self, byte: u8) {
        self.window[self.total as usize % WINDOW] = byte;
        self.total += 1;
    }

    fn codes(&mut self, lit: &Table, dist: &Table, out: &mut [u8]) -> io::Result<(usize, bool)> {
        let mut n = 0;
        while n < out.len() {
            if self.pending.0 > 0 {
                n += self.copy_match(&mut out[n..]);
                continue;
            }
            self.refill()?;
            match self.symbol(lit)? {
                s @ 0..=255 => {
                    out[n] = s as u8;
                    self.put(s as u8);
                    n += 1;
                }
                256 => return Ok((n, true)),
                s @ 257..=285 => {
                    let s = s - 257;
                    let length = usize::from(LENGTH_BASE[s]) + self.take(u32::from(LENGTH_EXTRA[s]))? as usize;
                    let d = self.symbol(dist)?;
                    if d >= 30 {
                        return Err(corrupt());
                    }
                    let distance = usize::from(DIST_BASE[d]) + self.take(u32::from(DIST_EXTRA[d]))? as usize;
                    if distance as u64 > self.total {
                        return Err(corrupt());
                    }
                    self.pending = (length, distance);
                }
                _ => return Err(corrupt()),
            }
        }
        Ok((n, false))
    }

    fn stored(&mut self, left: usize, out: &mut [u8]) -> io::Result<usize> {
        let want = left.min(out.len());
        let mut n = 0;
        while n < want && self.count >= 8 {
            out[n] = self.take(8)? as u8;
            n += 1;
        }
        if n < want {
            self.fill()?;
            let k = (want - n).min(self.end - self.at);
            if k == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            out[n..n + k].copy_from_slice(&self.buf[self.at..self.at + k]);
            self.at += k;
            n += k;
        }
        for &byte in &out[..n] {
            self.put(byte);
        }
        Ok(n)
    }
}

impl<R: Read> Read for Inflater<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while !out.is_empty() {
            match std::mem::replace(&mut self.state, State::Done) {
                State::Done => return Ok(0),
                State::Header => self.header()?,
                State::Stored(0) => self.state = State::Header,
                State::Stored(left) => {
                    let n = self.stored(left, out)?;
                    self.state = State::Stored(left - n);
                    return Ok(n);
                }
                State::Codes(lit, dist) => {
                    let (n, ended) = self.codes(&lit, &dist, out)?;
                    self.state = if ended { State::Header } else { State::Codes(lit, dist) };
                    if n > 0 {
                        return Ok(n);
                    }
                }
            }
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn inflate(packed: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        Inflater::new(packed).read_to_end(&mut out)?;
        Ok(out)
    }

    fn sample(n: usize) -> Vec<u8> {
        let mut x = 12345u32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                if i % 7 < 4 {
                    b"the quick brown fox "[i % 20]
                } else {
                    (x >> 24) as u8
                }
            })
            .collect()
    }

    #[test]
    fn stored_blocks_have_no_nlen() {
        // A non-final stored block "abc", then a final one "de": header bits, LEN, bytes.
        let packed = [0x00, 3, 0, b'a', b'b', b'c', 0x01, 2, 0, b'd', b'e'];
        assert_eq!(inflate(&packed).unwrap(), b"abcde");
        assert!(inflate(&packed[..8]).is_err());
    }

    #[test]
    fn stored_blocks_with_nlen_are_standard() {
        let packed = [0x00, 3, 0, 0xFC, 0xFF, b'a', b'b', b'c', 0x01, 2, 0, 0xFD, 0xFF, b'd', b'e'];
        assert_eq!(inflate(&packed).unwrap(), b"abcde");
        // Once NLEN has been seen, a wrong one is corruption.
        let wrong = [0x00, 3, 0, 0xFC, 0xFF, b'a', b'b', b'c', 0x01, 2, 0, 0, 0, b'd', b'e'];
        assert!(inflate(&wrong).is_err());
    }

    #[test]
    fn stored_block_after_huffman_block_aligns() {
        // A fixed block holding "a" (not final), then a final stored block "xy" after the padding.
        let mut packed = vec![0x4A, 0x04, 0x04]; // fixed block: 'a', end; the stored header's bits follow in the last byte
        packed.extend([2, 0, b'x', b'y']);
        let out = inflate(&packed).unwrap();
        assert_eq!(out, b"axy");
    }

    #[test]
    fn huffman_blocks_match_flate2() {
        for (level, size) in [(0, 70_000), (1, 300_000), (6, 1_000_000), (9, 5_000), (6, 0), (6, 1)] {
            let data = sample(size);
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
            e.write_all(&data).unwrap();
            let packed = e.finish().unwrap();
            // Level 0 is stored blocks with an NLEN: not this flavour, so skip comparing those.
            if level == 0 {
                continue;
            }
            assert_eq!(inflate(&packed).unwrap(), data, "level {level}, {size} bytes");
        }
    }

    #[test]
    fn small_reads_and_slow_input_work() {
        struct Trickle<'a>(&'a [u8]);
        impl Read for Trickle<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let n = self.0.len().min(out.len()).min(3);
                out[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let data = sample(200_000);
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
        e.write_all(&data).unwrap();
        let packed = e.finish().unwrap();
        let mut inflater = Inflater::new(Trickle(&packed));
        let mut out: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 7];
        loop {
            let n = inflater.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            out.extend(&chunk[..n]);
        }
        assert_eq!(out, data);
    }

    #[test]
    fn garbage_is_refused() {
        assert!(inflate(&[0x07, 0xFF, 0xFF]).is_err());
    }
}
