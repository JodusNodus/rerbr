//! JPAK, the uncompressed container of a route's collision cells (`track.jpk`), little endian: `JPAK`, a zero word, the
//! entry count (one more than the entries), then at 0x20 one 32-byte record per entry: the absolute offset of its
//! NUL-terminated name, its size, its data's absolute offset and its size again. Layout as Ego Engine Modding's
//! `Archive/Jpk` reads it (cited in `notes/references/README.md`), checked on every route of the install.

/// One entry: its name (`qt_01_10_00.vcqtc`) and its bytes' range in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub data: std::ops::Range<usize>,
}

fn word(bytes: &[u8], at: usize) -> Result<usize, String> {
    let b = bytes.get(at..at + 4).ok_or("JPAK truncated")?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// The entries of a JPAK file.
pub fn entries(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if !bytes.starts_with(b"JPAK") {
        return Err("not a JPAK file".into());
    }
    let count = word(bytes, 8)?.saturating_sub(1);
    (0..count)
        .map(|k| {
            let at = 0x20 + 32 * k;
            let (name_at, size, data_at, again) =
                (word(bytes, at)?, word(bytes, at + 4)?, word(bytes, at + 8)?, word(bytes, at + 12)?);
            if size != again {
                return Err(format!("JPAK entry {k} is compressed ({size} of {again} bytes)"));
            }
            let name = bytes.get(name_at..).ok_or("JPAK name past the end")?;
            let name = String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())]);
            let data = data_at
                ..data_at.checked_add(size).filter(|&end| end <= bytes.len()).ok_or("JPAK entry past the end")?;
            Ok(Entry { name: name.into_owned(), data })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_named_ranges() {
        let mut file = b"JPAK".to_vec();
        file.extend([0u32, 2, 16, 0, 0x40, 0, 0].iter().flat_map(|w| w.to_le_bytes()));
        file.extend([0x40u32, 3, 0x46, 3, 0, 0, 0, 0].iter().flat_map(|w| w.to_le_bytes()));
        file.extend(b"a.bin\0xyz");
        assert_eq!(entries(&file).unwrap(), [Entry { name: "a.bin".into(), data: 0x46..0x49 }]);
        assert_eq!(&file[0x46..0x49], b"xyz");
        file.truncate(0x47);
        assert!(entries(&file).is_err());
        assert!(entries(b"PKZP").is_err());
    }
}
