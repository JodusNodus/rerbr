#!/usr/bin/env python3
"""Regenerate crates/rbr-formats/src/exe_data.rs: the 1.02 executable's data words ReRBR reads.

The words are listed in data/exe-words.txt, one range a line:

    START END   # what reads it         the words START..END (hex, END excluded)
    strings START COUNT  # ...          COUNT string pointers from START, and the strings they point to

The boot's words come from runs of an `exe-reads` build of rbr-standalone, which writes the words a run reads before
writing them (`RBR_EXE_READS`, rbr_physics::mem::exe_reads); `--reads FILE...` merges such dumps into the list.
A run reads only the virtual-table slots it calls, so every table a listed word lies in is taken whole (`whole_vtables`):
a slot no run had called read zero, and the call through it went to 0 (CMountedRigidBody's 0x73e62c table when a
mounted part came off, 2026-11-03). Nothing is transcribed: every word comes from the binary, whose SHA-256 is checked.

Run from the repo root:

    python3 -I -B scripts/gen-exe-data.py [--exe PATH] [--reads DUMP...] [--write]

`--exe` defaults to $RBR_PATCH's exe. Without `--write` the script checks that its output matches the checked-in
file byte for byte (a differing output goes to exe_data.rs.regen); with it, it writes the file.
"""

import argparse
import hashlib
import os
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(os.environ.get("RBR_PATCH", ""), "RichardBurnsRally_SSE.exe")
EXE_SHA256 = "cd20966ed5654f983aa08c5d1960e0182bfe508e957af625540821f0705ef01c"
LIST = os.path.join(ROOT, "data", "exe-words.txt")
OUT = os.path.join(ROOT, "crates", "rbr-formats", "src", "exe_data.rs")

HEADER = """\
//! The data of `RichardBurnsRally_SSE.exe` 1.02 that ReRBR reads, so that the game runs on the retail disc's files
//! without the 1.02 patch: the words the standalone boot maps, the season table, the rally folders' names and the maps
//! without flip spectators. Generated from the exe by `scripts/gen-exe-data.py` over the ranges in
//! `data/exe-words.txt`; do not edit.
"""

ACCESSORS = """
/// The run holding the `len` bytes at `at`, and their offset in it.
fn run(at: u32, len: usize) -> Option<(&'static [u32], usize)> {
    let k = RUNS.partition_point(|&(start, _)| start <= at).checked_sub(1)?;
    let (start, words) = RUNS[k];
    let offset = (at - start) as usize;
    (offset + len <= 4 * words.len()).then_some((words, offset))
}

/// The `len` bytes at `at`, when one run holds them all.
pub fn bytes(at: u32, len: usize) -> Option<Vec<u8>> {
    let (words, offset) = run(at, len)?;
    Some(words.iter().flat_map(|w| w.to_le_bytes()).skip(offset).take(len).collect())
}

/// The little-endian word at `at`.
pub fn u32(at: u32) -> Option<u32> {
    bytes(at, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The NUL-terminated string at `at`.
pub fn c_str(at: u32) -> Option<String> {
    let (words, offset) = run(at, 1)?;
    let bytes = words.iter().flat_map(|w| w.to_le_bytes()).skip(offset);
    let bytes: Vec<u8> = bytes.take_while(|&b| b != 0).collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_are_sorted_and_apart() {
        for pair in RUNS.windows(2) {
            let ((a, words), (b, _)) = (pair[0], pair[1]);
            assert!(a + 4 * words.len() as u32 <= b, "{a:#x} overlaps {b:#x}");
        }
    }

    /// Every word equals the 1.02 exe's (`RBR_PATCH`).
    #[test]
    fn runs_match_the_exe() {
        let Some(exe) = std::env::var_os("RBR_PATCH").map(|p| std::path::Path::new(&p).join("RichardBurnsRally_SSE.exe"))
        else {
            eprintln!("skipping: RBR_PATCH not set");
            return;
        };
        let image = crate::pe::parse(&std::fs::read(exe).unwrap()).unwrap();
        for &(start, words) in RUNS {
            for (k, &word) in words.iter().enumerate() {
                let at = start + 4 * k as u32;
                assert_eq!(image.u32(at), Some(word), "{at:#x}");
            }
        }
    }
}
"""


def sections(exe):
    """The PE's sections as (name, virtual address, virtual size, raw bytes zero-filled to the virtual size)."""
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    count = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    base = struct.unpack_from("<I", exe, pe + 24 + 28)[0]
    out = []
    for k in range(count):
        at = pe + 24 + opt + 40 * k
        vsize, va, rsize, raw = struct.unpack_from("<IIII", exe, at + 8)
        data = exe[raw:raw + min(rsize, vsize)]
        out.append((exe[at:at + 8].rstrip(b"\0"), base + va, vsize, data + b"\0" * (vsize - len(data))))
    return out


def reader(exe):
    secs = sections(exe)

    def read(at, n):
        for _, va, _, data in secs:
            if va <= at and at + n <= va + len(data):
                return data[at - va:at - va + n]
        raise SystemExit("%#x+%d is outside the exe's sections" % (at, n))

    return read


def parse_list(path):
    """The list's lines as (kind, a, b, comment)."""
    entries = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            body, _, comment = line.partition("#")
            fields = body.split()
            if not fields:
                continue
            if fields[0] == "strings":
                entries.append(("strings", int(fields[1], 16), int(fields[2]), comment.strip()))
            else:
                entries.append(("words", int(fields[0], 16), int(fields[1], 16), comment.strip()))
    return entries


def merge_reads(entries, dumps):
    """`entries` with the words of the dumps added as ranges of their own (`boot`), the existing boot ranges merged."""
    words = set()
    for kind, a, b, comment in entries:
        if kind == "words" and comment == "boot":
            words.update(range(a, b, 4))
    for dump in dumps:
        with open(dump, encoding="utf-8") as f:
            words.update(int(line, 16) for line in f if line.strip())
    others = [e for e in entries if not (e[0] == "words" and e[3] == "boot")]
    runs = []
    for w in sorted(words):
        if runs and runs[-1][1] == w:
            runs[-1][1] = w + 4
        else:
            runs.append([w, w + 4])
    return others + [("words", a, b, "boot") for a, b in runs]


def write_list(path, entries):
    lines = [
        "# The 1.02 exe's words ReRBR reads (scripts/gen-exe-data.py): START END (hex, END excluded), or",
        "# strings START COUNT (COUNT string pointers and their strings). `boot` lines come from --reads.",
    ]
    for kind, a, b, comment in entries:
        if kind == "strings":
            lines.append("strings %08x %d  # %s" % (a, b, comment))
        else:
            lines.append("%08x %08x  # %s" % (a, b, comment))
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


def words_of(entries, read):
    """Every word address the list covers."""
    words = set()
    for kind, a, b, _ in entries:
        if kind == "words":
            words.update(range(a & ~3, b, 4))
            continue
        for k in range(b):
            words.update(range(a + 4 * k, a + 4 * k + 4))
            ptr = struct.unpack("<I", read(a + 4 * k, 4))[0]
            end = ptr
            while read(end, 1) != b"\0":
                end += 1
            words.update(range(ptr & ~3, end + 1, 4))
    return {w & ~3 for w in words}


def whole_vtables(words, exe, read):
    """`words` with each virtual table one of them lies in: the run of `.text` pointers around it, after the word
    pointing into `.rdata` (MSVC's RTTI locator) that comes before every table."""
    secs = {name: (va, va + size) for name, va, size, _ in sections(exe)}
    inside = lambda name, v: secs[name][0] <= v < secs[name][1]
    word = lambda at: struct.unpack("<I", read(at, 4))[0]
    out = set(words)
    for w in words:
        if not inside(b".rdata", w) or not inside(b".text", word(w)):
            continue
        start = w
        while inside(b".text", word(start - 4)):
            start -= 4
        if not inside(b".rdata", word(start - 4)):
            continue
        end = w + 4
        while inside(b".text", word(end)):
            end += 4
        out.update(range(start, end, 4))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--exe", default=EXE)
    ap.add_argument("--reads", nargs="*", default=[])
    ap.add_argument("--write", action="store_true")
    opts = ap.parse_args()

    with open(opts.exe, "rb") as f:
        exe = f.read()
    if hashlib.sha256(exe).hexdigest() != EXE_SHA256:
        raise SystemExit("%s is not the 1.02 RichardBurnsRally_SSE.exe" % opts.exe)
    read = reader(exe)

    entries = parse_list(LIST)
    if opts.reads:
        entries = merge_reads(entries, opts.reads)
        write_list(LIST, entries)

    runs = []
    for w in sorted(whole_vtables(words_of(entries, read), exe, read)):
        if runs and runs[-1][0] + 4 * len(runs[-1][1]) == w:
            runs[-1][1].append(w)
        else:
            runs.append((w, [w]))

    spans = [(va, size) for name, va, size, _ in sections(exe) if name in (b".rdata", b".data")]
    out = [HEADER, "/// `.rdata` and `.data` as the loader maps them: address and virtual size. What [`RUNS`] does not hold",
           "/// reads as zero (all of `.data` past its file data does).",
           "pub const SECTIONS: [(u32, u32); 2] = [%s];" % ", ".join("(%#010x, %#x)" % s for s in spans), "",
           "/// The words by address: each run's first address and its words.", "#[rustfmt::skip]",
           "pub const RUNS: &[(u32, &[u32])] = &["]
    for start, addrs in runs:
        words = ["%#010x" % struct.unpack("<I", read(a, 4))[0] for a in addrs]
        rows = [", ".join(words[k:k + 8]) + "," for k in range(0, len(words), 8)]
        out.append("    (%#010x, &[" % start)
        out.extend("        " + row for row in rows)
        out.append("    ]),")
    out.append("];")
    text = "\n".join(out) + "\n" + ACCESSORS
    text = subprocess.run(["rustfmt", "--edition", "2021", "--config-path", ROOT], input=text,
                          capture_output=True, text=True, check=True).stdout

    if opts.write:
        with open(OUT, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print("wrote %s: %d runs, %d words" % (OUT, len(runs), sum(len(a) for _, a in runs)))
        return 0
    current = open(OUT, encoding="utf-8").read() if os.path.exists(OUT) else ""
    if current != text:
        with open(OUT + ".regen", "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        sys.stderr.write("exe_data.rs differs from generator output; see %s.regen\n" % OUT)
        return 1
    print("exe_data.rs matches generator output (%d runs)" % len(runs))
    return 0


if __name__ == "__main__":
    sys.exit(main())
