//! The rally school's speech ids (`RBR_E25_School.cpp`): `E25School::M80c0x` (0x4b80c0) over the digit strings in the
//! lesson `.dls`'s table-7 string block (`rbr_formats::dls::DlsFile::string_bytes`), the briefing table
//! `E25School::M8160` (0x4b8160) and the end pages' tables (`E25School::OnSignal` 0x4b8200).
//!
//! Nothing here reads game files: the host keeps the string block with the run and resolves each `Trigger` pair's
//! code (the string's offset, the pair-code block base 0) through [`speech_ids`]. The briefing and end-page ids are
//! the viewer's (the record carries only `Trigger`'s ids).

/// `E25School::M80c0x` (0x4b80c0) over the NUL-terminated string at `offset`: each 4-character slot `atoi`'d to an
/// id, `digits[3] == 0` ending the scan. An empty string or an offset past the block gives no ids (ours: the
/// original would read past its buffer).
pub fn speech_ids(strings: &[u8], offset: u32) -> Vec<u32> {
    let offset = offset as usize;
    if offset >= strings.len() || strings[offset] == 0 {
        return Vec::new();
    }
    let end = strings[offset..].iter().position(|&b| b == 0).map_or(strings.len(), |n| offset + n);
    let mut ids = Vec::new();
    let mut at = offset;
    loop {
        ids.push(c_atoi(&strings[at..end]));
        // `digits[3]`: the NUL itself when the string ends here, 0 past it too (clamped, ours).
        let c = if at + 3 < end { strings[at + 3] } else { 0 };
        at += 4;
        if c == 0 || at >= end {
            break;
        }
    }
    ids
}

/// The C `atoi` (`M80c0x` reads each slot through it): leading whitespace skipped, an optional sign, then the
/// decimal digits (no digits gives 0).
fn c_atoi(text: &[u8]) -> u32 {
    let mut text = text.iter().skip_while(|&&b| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')).peekable();
    let mut sign: i32 = 1;
    match text.peek() {
        Some(b'+') => {
            text.next();
        }
        Some(b'-') => {
            sign = -1;
            text.next();
        }
        _ => {}
    }
    let mut id: i32 = 0;
    for &b in text {
        let Some(digit) = b.checked_sub(b'0').filter(|&d| d < 10) else { break };
        id = id.wrapping_mul(10).wrapping_add(i32::from(digit));
    }
    (id * sign) as u32
}

/// `E25School::M8160` (0x4b8160): the briefing id (`OnSignal` kind 1) by lesson.
pub const BRIEFING: [u32; 17] = [
    0x126, 0x130, 0x140, 0x149, 0x156, 0x160, 0x175, 0x179, 0x181, 0x18b, 400, 0x199, 0x19a, 0x19e, 0x19f, 0x1a1, 0x1a4,
];

/// The briefing id of `lesson`, if there is one.
pub fn briefing(lesson: u32) -> Option<u32> {
    BRIEFING.get(lesson as usize).copied()
}

/// The end pages' single-id tables (`OnSignal` kinds 4, 5 and 6, indexed by `sub = rand & 3`).
pub const FAILED: [u32; 4] = [499, 242, 236, 500];
pub const GRADED: [u32; 4] = [468, 235, 495, 496];
pub const PERFECT: [u32; 4] = [317, 467, 497, 498];

/// The end page's id (`OnSignal`): failed (verdict 0) takes kind 4, passed (1-3) kind 5, perfect (4) kind 6.
pub fn end_page(verdict: u32, sub: u32) -> u32 {
    let table = match verdict {
        0 => FAILED,
        4 => PERFECT,
        _ => GRADED,
    };
    table[(sub & 3) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_scan_four_chars_with_a_nul_end() {
        assert_eq!(speech_ids(b"294 295\0", 0), [294, 295]);
        assert_eq!(speech_ids(b"294\0", 0), [294]);
        assert_eq!(speech_ids(b"\0", 0), Vec::<u32>::new());
        assert_eq!(speech_ids(b"294\0", 4), Vec::<u32>::new());
        assert_eq!(speech_ids(b"294\0", 99), Vec::<u32>::new());
        // A later string in the same block.
        let block = [b"294\0".as_slice(), b"311 312\0"].concat();
        assert_eq!(speech_ids(&block, 4), [311, 312]);
    }

    #[test]
    fn atoi_skips_space_and_takes_the_sign() {
        assert_eq!(c_atoi(b" 12"), 12);
        assert_eq!(c_atoi(b"+34 rest"), 34);
        assert_eq!(c_atoi(b"-5"), (-5i32) as u32);
        assert_eq!(c_atoi(b""), 0);
        assert_eq!(c_atoi(b"ab12"), 0);
    }

    #[test]
    fn tables_cover_every_lesson_and_verdict() {
        assert_eq!(BRIEFING.len(), super::super::Lesson::COUNT as usize);
        assert_eq!(briefing(0), Some(0x126));
        assert_eq!(briefing(10), Some(400));
        assert_eq!(briefing(16), Some(0x1a4));
        assert_eq!(briefing(17), None);
        assert_eq!(end_page(0, 0), 499);
        assert_eq!(end_page(2, 3), 496);
        assert_eq!(end_page(4, 1), 467);
        assert_eq!(end_page(9, 2), 495, "any nonzero non-4 verdict is graded");
    }

    /// Every `Trigger` pair's offset lands on a string that yields ids, and every id is an English RS file
    /// (`Audio\RS\ENG\<id>.ogg` in `audio.dat`; `RBR_DATA`).
    #[test]
    fn retail_pairs_resolve_to_retail_speech() {
        let Some(data) = rbr_formats::data::Data::from_env() else { return };
        let dat_path = data.audio_dat().expect("no Root/audio.dat");
        let dat = rbr_formats::audio::AudioDat::open(&dat_path).expect("audio.dat");
        let eng: std::collections::HashSet<u32> = dat
            .files()
            .filter_map(|(path, _)| {
                let path = path.to_lowercase();
                path.strip_prefix("rs/eng/").and_then(|file| file.strip_suffix(".ogg")).and_then(|id| id.parse().ok())
            })
            .collect();
        assert!(!eng.is_empty(), "audio.dat has no RS/Eng oggs");
        for lesson in super::super::Lesson::all() {
            let map = lesson.map();
            let stage = format!("track-{map}_M");
            let path = rbr_formats::stage_files::file(&data, &stage, "dls").expect("lesson .dls");
            let bytes = std::fs::read(&path).expect("lesson .dls");
            let file = rbr_formats::dls::parse(&bytes).expect("lesson .dls");
            let replay = rbr_formats::markers::driveline_entry(&file).expect("Driveline set").replay.clone();
            let bank = super::super::Bank::from_stream(&super::super::Bank::fresh(), &replay, 0);
            if bank.pairs.is_empty() {
                continue; // the ride (lesson 5) has no sound pairs.
            }
            for pair in &bank.pairs {
                let ids = speech_ids(&file.string_bytes, pair.code as u32);
                assert!(!ids.is_empty(), "lesson {} pair {} resolves to nothing", lesson.index(), pair.code);
                for id in ids {
                    assert!(eng.contains(&id), "lesson {} id {id} is no RS/Eng file", lesson.index());
                }
            }
        }
    }
}
