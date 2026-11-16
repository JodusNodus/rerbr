//! The NSIS container: the first header, the (possibly solid) compressed stream and the file chunks in it. Nothing
//! here knows what the script means; [`super::script`] reads the header this returns.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::Fail;

const MAGIC: &[u8; 12] = b"NullsoftInst";
const SIGNATURE: [u8; 4] = [0xEF, 0xBE, 0xAD, 0xDE];
/// The first header: flags, signature, magic, the header's inflated size and the archive's size.
const FIRST_HEADER: u64 = 28;
const COMPRESSED: u32 = 0x8000_0000;

/// How a stream is packed. NSIS writes raw streams, with no zlib or bzip2 file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    Deflate,
    /// Five bytes of properties, then the stream; `flag` is a filter flag byte before them (0: none, 1: x86 BCJ).
    Lzma {
        flag: Option<u8>,
    },
    Bzip2,
}

/// An opened installer: where its data is and the inflated header.
pub struct Container {
    file: std::path::PathBuf,
    len: u64,
    /// Whether the header and all files are one compressed stream (else each is a block of its own).
    pub solid: bool,
    /// Solid: the stream's offset in the file. Else: where the first file's block would be (after the header).
    data: u64,
    pub header: Vec<u8>,
}

/// The file offset of the first header, at a 512-byte boundary, or `None` when the file has none.
fn find(file: &mut File) -> io::Result<Option<u64>> {
    let mut buf = vec![0u8; 1 << 20];
    let mut base = 0u64;
    loop {
        let mut got = 0;
        while got < buf.len() {
            match file.read(&mut buf[got..])? {
                0 => break,
                n => got += n,
            }
        }
        for at in (0..got.saturating_sub(19)).step_by(512) {
            if buf[at + 4..at + 8] == SIGNATURE && buf[at + 8..at + 20] == *MAGIC {
                return Ok(Some(base + at as u64));
            }
        }
        if got < buf.len() {
            return Ok(None);
        }
        base += got as u64;
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Whether `peek` starts an LZMA stream as NSIS writes it: the usual properties byte and a dictionary size that is
/// 2^k, 3*2^k or whole MiB (a deflate stream starting 0x5D passes by chance only about once in a million).
fn lzma_start(peek: &[u8]) -> bool {
    let Some(dict) = u32_at(peek, 1) else { return false };
    let shape = dict.is_power_of_two() || (dict / 3).is_power_of_two() && dict % 3 == 0 || dict % (1 << 20) == 0;
    peek[0] == 0x5D && dict >= 4096 && shape
}

fn codec(peek: &[u8]) -> Codec {
    if peek.len() >= 5 && lzma_start(peek) {
        Codec::Lzma { flag: None }
    } else if peek.len() >= 6 && peek[0] <= 1 && lzma_start(&peek[1..]) {
        Codec::Lzma { flag: Some(peek[0]) }
    } else if peek.len() >= 2 && peek[0] == b'1' && peek[1] < 14 {
        Codec::Bzip2
    } else {
        Codec::Deflate
    }
}

pub const UNSUPPORTED: &str = "this installer's format isn't supported";

/// The decompressed stream of the packed bytes in `r`, its codec read from the bytes themselves.
fn unpack<R: Read + 'static>(mut r: BufReader<R>) -> Result<Box<dyn Read>, Fail> {
    let peek = r.fill_buf().map_err(|e| e.to_string())?;
    match codec(peek) {
        Codec::Deflate => Ok(Box::new(super::inflate::Inflater::new(r))),
        Codec::Bzip2 => Err(format!("{UNSUPPORTED}: its data is bzip2-compressed")),
        Codec::Lzma { flag } => {
            if flag == Some(1) {
                return Err(format!("{UNSUPPORTED}: its LZMA data uses the BCJ filter"));
            }
            if flag.is_some() {
                r.consume(1);
            }
            let mut p = [0u8; 5];
            r.read_exact(&mut p).map_err(|e| e.to_string())?;
            let dict = u32_at(&p, 1).unwrap_or(0);
            // Unknown size: NSIS ends its LZMA streams with an end marker, and the headers are read by length.
            let lzma = lzma_rust2::LzmaReader::new_with_props(r, u64::MAX, p[0], dict, None)
                .map_err(|e| format!("{UNSUPPORTED}: LZMA: {e}"))?;
            Ok(Box::new(lzma))
        }
    }
}

fn read_u32(r: &mut dyn Read) -> Result<u32, Fail> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(b))
}

impl Container {
    /// Opens the installer at `path`: `Ok(None)` when it is no NSIS installer.
    pub fn open(path: &Path) -> Result<Option<Container>, Fail> {
        let error = |e: io::Error| format!("{}: {e}", path.display());
        let mut file = File::open(path).map_err(error)?;
        let len = file.metadata().map_err(error)?.len();
        let Some(base) = find(&mut file).map_err(error)? else { return Ok(None) };
        let mut first = [0u8; FIRST_HEADER as usize + 6];
        file.seek(SeekFrom::Start(base)).map_err(error)?;
        let got = file.read(&mut first).map_err(error)?;
        let inflated = u32_at(&first[..got], 20).ok_or_else(|| format!("{UNSUPPORTED}: its header is cut off"))?;
        let pos = base + FIRST_HEADER;
        let attempt = |solid: bool| {
            let mut container = Container { file: path.to_owned(), len, solid, data: pos, header: Vec::new() };
            container.header =
                if solid { container.solid_header(inflated)? } else { container.block_header(inflated)? };
            Ok(container)
        };
        // A stream starting like LZMA or bzip2 is solid; else the u32 there tells a header block of its own, and a
        // solid deflate stream is what is left.
        let solid_first = !matches!(codec(&first[FIRST_HEADER as usize..got]), Codec::Deflate);
        let order = if solid_first { [true, false] } else { [false, true] };
        let first_try = attempt(order[0]);
        let container = first_try.or_else(|first_error: Fail| attempt(order[1]).map_err(|_| first_error))?;
        Ok(Some(container))
    }

    /// The header of a non-solid installer: a block of its own, packed or (when packing didn't shrink it) stored.
    fn block_header(&mut self, inflated: u32) -> Result<Vec<u8>, Fail> {
        let mut file = File::open(&self.file).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(self.data)).map_err(|e| e.to_string())?;
        let n = read_u32(&mut file)?;
        let stored = u64::from(n & !COMPRESSED);
        if self.data + 4 + stored > self.len || (n & COMPRESSED == 0 && n != inflated) {
            return Err(format!("{UNSUPPORTED}: its header block is not where it should be"));
        }
        let block = BufReader::new(file.take(stored));
        let raw: Box<dyn Read> = if n & COMPRESSED != 0 { unpack(block)? } else { Box::new(block) };
        let header = Self::whole(raw, inflated)?;
        self.data += 4 + stored;
        Ok(header)
    }

    /// The header of a solid installer: the stream's first bytes.
    fn solid_header(&self, inflated: u32) -> Result<Vec<u8>, Fail> {
        let mut stream = self.stream()?;
        if read_u32(&mut stream)? != inflated {
            return Err(format!("{UNSUPPORTED}: its header doesn't unpack to its stated size"));
        }
        Self::whole(stream, inflated)
    }

    /// Exactly `size` bytes of `r`.
    fn whole(r: Box<dyn Read>, size: u32) -> Result<Vec<u8>, Fail> {
        let mut bytes = Vec::new();
        r.take(u64::from(size)).read_to_end(&mut bytes).map_err(|e| format!("{UNSUPPORTED}: {e}"))?;
        (bytes.len() == size as usize).then_some(bytes).ok_or_else(|| format!("{UNSUPPORTED}: its header is cut off"))
    }

    /// The solid stream from its start.
    fn stream(&self) -> Result<Box<dyn Read>, Fail> {
        let mut file = File::open(&self.file).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(self.data)).map_err(|e| e.to_string())?;
        unpack(BufReader::new(file))
    }

    /// The solid stream positioned after the header, at the first file.
    fn solid_stream(&self) -> Result<Box<dyn Read>, Fail> {
        let mut stream = self.stream()?;
        let size = read_u32(&mut stream)?;
        io::copy(&mut (&mut stream).take(u64::from(size)), &mut io::sink()).map_err(|e| e.to_string())?;
        Ok(stream)
    }

    /// One file of a non-solid installer: its block's stored size and its unpacked bytes.
    fn block(&self, offset: u32) -> Result<(u32, Box<dyn Read>), Fail> {
        let at = self.data + u64::from(offset);
        let mut file = File::open(&self.file).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        let n = read_u32(&mut file)?;
        let stored = n & !COMPRESSED;
        if at + 4 + u64::from(stored) > self.len {
            return Err(format!("{UNSUPPORTED}: a file at {offset} runs past the end"));
        }
        let block = BufReader::new(file.take(u64::from(stored)));
        Ok((stored, if n & COMPRESSED != 0 { unpack(block)? } else { Box::new(block) }))
    }

    /// The stored size of the file at `offset` when it can be told without unpacking (non-solid: the block's size,
    /// which is the packed size when packed); `None` for a solid installer.
    pub fn stored_size(&self, offset: u32) -> Result<Option<u64>, Fail> {
        if self.solid {
            return Ok(None);
        }
        Ok(Some(u64::from(self.block(offset)?.0)))
    }

    /// Calls `each` with the offset and the unpacked bytes of each file at `offsets` (sorted, without repeats), the
    /// reader ending at the file's end. A solid installer is one pass over its stream.
    pub fn files(
        &self,
        offsets: &[u32],
        each: &mut dyn FnMut(u32, &mut dyn Read) -> Result<(), Fail>,
    ) -> Result<(), Fail> {
        if !self.solid {
            for &offset in offsets {
                each(offset, &mut self.block(offset)?.1)?;
            }
            return Ok(());
        }
        let mut stream = self.solid_stream()?;
        let mut at = 0u64;
        for &offset in offsets {
            let skip = u64::from(offset).checked_sub(at).ok_or("this installer's files overlap")?;
            let skipped = io::copy(&mut (&mut stream).take(skip), &mut io::sink()).map_err(|e| e.to_string())?;
            if skipped != skip {
                return Err(format!("{UNSUPPORTED}: a file runs past the end"));
            }
            let size = u64::from(read_u32(&mut stream)?);
            let mut file = (&mut stream).take(size);
            each(offset, &mut file)?;
            io::copy(&mut file, &mut io::sink()).map_err(|e| e.to_string())?;
            if file.limit() != 0 {
                return Err(format!("{UNSUPPORTED}: a file runs past the end"));
            }
            at = u64::from(offset) + 4 + size;
        }
        Ok(())
    }
}
