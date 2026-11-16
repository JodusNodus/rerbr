use std::io::Write;

use super::*;
use crate::content::source::open as open_source;

#[derive(Clone, Copy, Debug)]
enum Pack {
    Deflate,
    Lzma,
}

/// A string of the strings table: text, or a variable by its index.
enum Part {
    T(&'static str),
    V(u32),
}
use Part::{T, V};

/// The strings table of an installer, in the style of NSIS 3 (ANSI or Unicode).
struct Strings {
    unicode: bool,
    bytes: Vec<u8>,
}

impl Strings {
    fn new(unicode: bool) -> Strings {
        Strings { unicode, bytes: if unicode { vec![0, 0] } else { vec![0] } }
    }

    /// Adds a string and returns its index (in bytes, or in units for Unicode).
    fn add(&mut self, parts: &[Part]) -> u32 {
        let at = if self.unicode { self.bytes.len() / 2 } else { self.bytes.len() } as u32;
        let unit = |bytes: &mut Vec<u8>, u: u16| {
            if self.unicode {
                bytes.extend(u.to_le_bytes());
            } else {
                bytes.push(u as u8);
            }
        };
        let mut bytes = std::mem::take(&mut self.bytes);
        for part in parts {
            match part {
                T(text) => text.encode_utf16().for_each(|u| unit(&mut bytes, u)),
                V(n) => {
                    unit(&mut bytes, 3);
                    let (lo, hi) = ((*n & 0x7F) as u16 | 0x80, (*n >> 7) as u16 | 0x80);
                    if self.unicode {
                        bytes.extend((lo | hi << 8).to_le_bytes());
                    } else {
                        bytes.extend([lo as u8, hi as u8]);
                    }
                }
            }
        }
        unit(&mut bytes, 0);
        self.bytes = bytes;
        at
    }
}

fn pack(pack: Pack, bytes: &[u8]) -> Vec<u8> {
    match pack {
        Pack::Deflate => {
            let mut w = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            w.write_all(bytes).unwrap();
            w.finish().unwrap()
        }
        Pack::Lzma => {
            let mut options = lzma_rust2::LzmaOptions::with_preset(1);
            options.dict_size = 1 << 20;
            let mut out = vec![options.get_props()];
            out.extend((1u32 << 20).to_le_bytes());
            let mut w = lzma_rust2::LzmaWriter::new(out, &options, false, true, None).unwrap();
            w.write_all(bytes).unwrap();
            w.finish().unwrap()
        }
    }
}

fn nested_7z(dir: &Path) -> Vec<u8> {
    let path = dir.join("pack.7z");
    let mut out = sevenz_rust2::ArchiveWriter::create(&path).unwrap();
    for (name, bytes) in [("x/y.txt", b"inner".as_slice()), ("z.ini", b"[a]\n".as_slice())] {
        out.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(bytes)).unwrap();
    }
    out.finish().unwrap();
    fs::read(path).unwrap()
}

const A: &[u8] = b"file A: maps";
const B: &[u8] = b"file B: textures";

/// An installer exe: a stub, then the first header, header and files, as a script would have it:
/// SetOutPath $INSTDIR\Maps; File a.txt; StrCpy $5 8994; File $PLUGINSDIR\p.dll; File pack.7z; Push $OUTDIR\pack.7z;
/// Nsis7z::Extract; WriteINIStr $INSTDIR\Maps\Tracks.ini Map$5 Key v$5; SetOutPath $INSTDIR\Textures; File b.bin;
/// File ..\..\evil.txt.
fn installer(unicode: bool, solid: bool, pack_with: Pack, seven: &[u8]) -> Vec<u8> {
    let mut s = Strings::new(unicode);
    let files: [(&[u8], u32); 5] = [(A, 0), (b"plugin", 0), (seven, 0), (B, 0), (b"evil", 0)];
    // File offsets: each file is a block of 4 bytes of size and its (packed) bytes.
    let mut blocks = Vec::new();
    let mut offsets = Vec::new();
    for (bytes, _) in files {
        offsets.push(blocks.len() as u32);
        if solid {
            blocks.extend((bytes.len() as u32).to_le_bytes());
            blocks.extend(bytes);
        } else {
            let packed = pack(pack_with, bytes);
            blocks.extend((packed.len() as u32 | 0x8000_0000).to_le_bytes());
            blocks.extend(packed);
        }
    }
    let (maps, textures) = (s.add(&[V(21), T("\\Maps")]), s.add(&[V(21), T("\\Textures")]));
    let (a, plugin, pack7, b, evil) = (
        s.add(&[T("a.txt")]),
        s.add(&[V(26), T("\\p.dll")]),
        s.add(&[T("pack.7z")]),
        s.add(&[T("b.bin")]),
        s.add(&[T("..\\..\\evil.txt")]),
    );
    let (num, push, dll, func) = (
        s.add(&[T("8994")]),
        s.add(&[V(22), T("\\pack.7z")]),
        s.add(&[V(26), T("\\nsis7z.dll")]),
        s.add(&[T("Extract")]),
    );
    let (ini, section, key, value) = (
        s.add(&[V(21), T("\\Maps\\Tracks.ini")]),
        s.add(&[T("Map"), V(5)]),
        s.add(&[T("Key")]),
        s.add(&[T("v"), V(5)]),
    );
    let entries: Vec<[u32; 7]> = vec![
        [11, maps, 1, 0, 0, 0, 0],
        [20, 0, a, offsets[0], 0, 0, 0],
        [25, 5, num, 0, 0, 0, 0],
        [20, 0, plugin, offsets[1], 0, 0, 0],
        [20, 0, pack7, offsets[2], 0, 0, 0],
        [31, push, 0, 0, 0, 0, 0],
        [44, dll, func, 0, 0, 0, 0],
        [48, section, key, value, ini, 0, 0],
        [11, textures, 1, 0, 0, 0, 0],
        [20, 0, b, offsets[3], 0, 0, 0],
        [20, 0, evil, offsets[4], 0, 0, 0],
    ];
    // The header: flags, the eight blocks, then the entries and the strings.
    let table = 4 + 8 * 8;
    let strings_at = table + entries.len() * 28;
    let langs_at = strings_at + s.bytes.len();
    let mut header = vec![0u8; table];
    let block = |header: &mut Vec<u8>, n: usize, at: usize, count: usize| {
        header[4 + 8 * n..8 + 8 * n].copy_from_slice(&(at as u32).to_le_bytes());
        header[8 + 8 * n..12 + 8 * n].copy_from_slice(&(count as u32).to_le_bytes());
    };
    block(&mut header, 2, table, entries.len());
    block(&mut header, 3, strings_at, 0);
    block(&mut header, 4, langs_at, 0);
    entries.iter().flatten().for_each(|w| header.extend(w.to_le_bytes()));
    header.extend(&s.bytes);

    let mut exe = vec![b'M'; 512];
    exe.extend([0, 0, 0, 0, 0xEF, 0xBE, 0xAD, 0xDE]);
    exe.extend(b"NullsoftInst");
    exe.extend((header.len() as u32).to_le_bytes());
    exe.extend(0u32.to_le_bytes());
    if solid {
        let mut stream = (header.len() as u32).to_le_bytes().to_vec();
        stream.extend(&header);
        stream.extend(&blocks);
        exe.extend(pack(pack_with, &stream));
    } else {
        let packed = pack(pack_with, &header);
        exe.extend((packed.len() as u32 | 0x8000_0000).to_le_bytes());
        exe.extend(packed);
        exe.extend(blocks);
    }
    exe
}

fn names(source: &dyn Source) -> Vec<String> {
    crate::content::source::names(source)
}

#[test]
fn a_synthetic_installer_lists_extracts_and_reads_in_every_flavour() {
    let dir = crate::test_dir("nsis-synthetic");
    let seven = nested_7z(&dir);
    for unicode in [false, true] {
        for solid in [false, true] {
            for packing in [Pack::Deflate, Pack::Lzma] {
                let what = format!("unicode {unicode}, solid {solid}, {packing:?}");
                let exe = dir.join("setup.exe");
                fs::write(&exe, installer(unicode, solid, packing, &seven)).unwrap();
                let source = open_source(&exe).unwrap_or_else(|e| panic!("{what}: {e}"));
                let mut listed = names(&*source);
                listed.sort();
                assert_eq!(listed, ["Maps/a.txt", "Maps/x/y.txt", "Maps/z.ini", "Textures/b.bin"], "{what}");
                let ini = source.ini_writes();
                assert_eq!(ini.len(), 1, "{what}");
                let expected = IniWrite {
                    file: vec!["Maps".into(), "Tracks.ini".into()],
                    section: "Map8994".into(),
                    key: "Key".into(),
                    value: "v8994".into(),
                };
                assert_eq!(ini[0], expected, "{what}");
                let entry = |name: &str| source.entries().iter().find(|e| e.path.join("/") == name).unwrap().clone();
                if solid {
                    assert_eq!(entry("Maps/a.txt").size, A.len() as u64, "{what}");
                }
                assert_eq!(source.read(&entry("Maps/a.txt")).unwrap(), A, "{what}");
                assert_eq!(source.read(&entry("Maps/x/y.txt")).unwrap(), b"inner", "{what}");
                let out = dir.join("out");
                let wanted: Vec<_> = source.entries().iter().map(|e| (e.path.clone(), e.path.clone())).collect();
                let mut bytes = 0;
                source
                    .extract(&wanted, &out, &mut |n| {
                        bytes += n;
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(fs::read(out.join("Textures/b.bin")).unwrap(), B, "{what}");
                assert_eq!(fs::read(out.join("Maps/z.ini")).unwrap(), b"[a]\n", "{what}");
                assert!(bytes > 0, "{what}");
                assert!(!out.join("evil.txt").exists() && !dir.join("evil.txt").exists(), "{what}");
                fs::remove_dir_all(&out).unwrap();
            }
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_exe_that_is_not_nsis_keeps_the_installer_refusal_and_bzip2_is_refused_by_name() {
    let dir = crate::test_dir("nsis-refusals");
    fs::write(dir.join("other.exe"), vec![b'M'; 2048]).unwrap();
    assert_eq!(open_source(&dir.join("other.exe")).err().as_deref(), Some(crate::content::source::refused::INSTALLER));
    // A first header whose header block starts like bzip2 ('1', then a small block size).
    let mut exe = vec![b'M'; 512];
    exe.extend([0, 0, 0, 0, 0xEF, 0xBE, 0xAD, 0xDE]);
    exe.extend(b"NullsoftInst");
    exe.extend(100u32.to_le_bytes());
    exe.extend(0u32.to_le_bytes());
    exe.extend([b'1', 9, 0, 0, 0, 0, 0, 0, 0, 0]);
    fs::write(dir.join("bz.exe"), exe).unwrap();
    let error = open_source(&dir.join("bz.exe")).err().unwrap();
    assert!(error.starts_with("this installer's format isn't supported") && error.contains("bzip2"), "{error}");
    fs::remove_dir_all(&dir).unwrap();
}

/// Opens every installer in `RBR_INSTALLERS` (a folder); skips without it.
#[test]
fn real_installers() {
    let Some(dir) = std::env::var_os("RBR_INSTALLERS") else { return };
    let mut exes: Vec<_> = fs::read_dir(dir).unwrap().filter_map(|e| Some(e.ok()?.path())).collect();
    exes.sort();
    let mut seen = 0;
    for exe in exes.iter().filter(|p| p.extension().is_some_and(|e| e == "exe")) {
        let started = std::time::Instant::now();
        let source = open_source(exe).unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        eprintln!(
            "{name}: {} entries, {} ini writes, {:?}",
            source.entries().len(),
            source.ini_writes().len(),
            started.elapsed()
        );
        let listed = names(&*source);
        let has = |n: &str| assert!(listed.iter().any(|l| l == n), "{name}: {n} missing from {listed:?}");
        let ini = source.ini_writes();
        let tracks: Vec<_> =
            ini.iter().filter(|w| w.file == ["Maps", "Tracks.ini"] && w.section.starts_with("Map")).collect();
        assert!(!tracks.is_empty(), "{name}: no Tracks.ini writes");
        let keys: Vec<&str> = tracks.iter().map(|w| w.key.as_str()).collect();
        for key in ["TrackName", "Particles", "StageName", "Surface"] {
            assert!(keys.contains(&key), "{name}: {key} missing from {keys:?}");
        }
        assert!(listed.iter().all(|l| !l.contains("${")), "{name}: unresolved names in {listed:?}");
        if name == "RBRManukau.exe" {
            for ext in ["ini", "col", "dls", "fnc", "lb2", "lbs", "mat", "trk"] {
                has(&format!("Maps/track-8994_O.{ext}"));
            }
            has("Maps/track-8994_O_textures.rbz");
            has("Maps/PS_Kahoo8.ini");
            assert!(listed.iter().all(|l| !l.ends_with(".7z") && !l.contains("nsis7z")), "{listed:?}");
            for key in ["Author", "CountryCode", "SplashScreen", "Key", "Length"] {
                assert!(keys.contains(&key), "{name}: {key} missing");
            }
            assert!(tracks.iter().all(|w| w.section == "Map8994"));
            let settings = ini.iter().filter(|w| w.file == ["Maps", "TrackSettings.ini"]);
            assert!(settings.clone().count() > 0 && settings.into_iter().all(|w| w.section.starts_with("8994O_")));
            // The nested 7z's file reads through the installer.
            let ini_entry = source.entries().iter().find(|e| e.path.join("/") == "Maps/track-8994_O.ini").unwrap();
            assert!(!source.read(ini_entry).unwrap().is_empty());
        }
        seen += 1;
    }
    assert!(seen > 0, "no .exe in RBR_INSTALLERS");
}
