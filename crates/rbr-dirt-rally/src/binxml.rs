//! Codemasters' binary XML (`"Rr`), as the route files hold it (`progress_track.xml`, `ai_track.xml`, the codriver
//! files). Layout, from the files themselves (the container is Codemasters'; EgoEngineLibrary's `Xml` in
//! EgoEngineModding, MIT, reads the same family and is the reference): three 8-byte records `{tag, 0x22 'R' 'r', u32
//! size}` (`0x1a` file, `0x17` string block, then the `0x1d` string blob's own header), then sections of the same
//! record form: `0x1d` blob of NUL-terminated latin-1 strings (names, texts and attribute values alike), `0x1e` u32
//! blob offset per string, `0x1b` six u32 per element `[name, text, n_attr, attr_first, n_child, child_first]` (record
//! 0 the root, children a contiguous run, `text` 0 meaning none), `0x1c` `(name, value)` string pairs. BXML
//! (`start_lights.xml`) and plain XML are not read.

/// One element: its attributes in file order, its text (trimmed, `None` when empty) and its children.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: Option<String>,
    pub children: Vec<Element>,
}

impl Element {
    /// The value of attribute `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// Attribute `name` as a number.
    pub fn attr_num(&self, name: &str) -> Option<f64> {
        self.attr(name)?.trim().parse().ok()
    }

    /// The first child called `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Every child called `name`, in file order.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The text as exactly `N` numbers (`"2858.25 -288.16 -2109.42"`).
    pub fn floats<const N: usize>(&self) -> Option<[f32; N]> {
        numbers(self.text.as_deref()?)
    }
}

/// `N` whitespace-separated numbers, nothing else.
pub fn numbers<const N: usize>(text: &str) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    let mut words = text.split_whitespace();
    for slot in &mut out {
        *slot = words.next()?.parse().ok()?;
    }
    words.next().is_none().then_some(out)
}

const MAGIC: [u8; 3] = [0x22, b'R', b'r'];
const MAX_DEPTH: usize = 64;

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    let bytes = b.get(at..at + 4).ok_or("binary xml: truncated")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// The words of a section's data.
fn words(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Parse a `"Rr` file into its root element.
pub fn parse(bytes: &[u8]) -> Result<Element, String> {
    if bytes.len() < 24 || bytes[1..4] != MAGIC || bytes[0] != 0x1a {
        return Err("binary xml: not a \"Rr file".into());
    }
    let (mut blob, mut offsets, mut records, mut pairs) = (None, None, None, None);
    // Past the file and string block headers, whose sizes only cover what follows.
    let mut at = 16;
    while at + 8 <= bytes.len() {
        if bytes[at + 1..at + 4] != MAGIC {
            return Err(format!("binary xml: bad section header at {at}"));
        }
        let size = u32_at(bytes, at + 4)? as usize;
        let data = bytes.get(at + 8..at + 8 + size).ok_or("binary xml: section runs past the file")?;
        match bytes[at] {
            0x1d => blob = Some(data),
            0x1e => offsets = Some(words(data)),
            0x1b => records = Some(words(data)),
            0x1c => pairs = Some(words(data)),
            tag => return Err(format!("binary xml: unknown section {tag:#x}")),
        }
        at += 8 + size;
    }
    let (Some(blob), Some(offsets), Some(records), Some(pairs)) = (blob, offsets, records, pairs) else {
        return Err("binary xml: a section is missing".into());
    };
    let strings = Strings { blob, offsets: &offsets };
    let tree = Tree { strings, records: &records, pairs: &pairs };
    if records.len() < 6 {
        return Err("binary xml: no root element".into());
    }
    tree.element(0, 0)
}

struct Strings<'a> {
    blob: &'a [u8],
    offsets: &'a [u32],
}

impl Strings<'_> {
    fn get(&self, index: u32) -> Result<String, String> {
        let start = *self.offsets.get(index as usize).ok_or("binary xml: string index out of range")? as usize;
        let tail = self.blob.get(start..).ok_or("binary xml: string offset out of range")?;
        let end = tail.iter().position(|&b| b == 0).ok_or("binary xml: unterminated string")?;
        Ok(tail[..end].iter().map(|&b| b as char).collect())
    }
}

struct Tree<'a> {
    strings: Strings<'a>,
    records: &'a [u32],
    pairs: &'a [u32],
}

impl Tree<'_> {
    fn element(&self, index: usize, depth: usize) -> Result<Element, String> {
        if depth > MAX_DEPTH {
            return Err("binary xml: nesting too deep".into());
        }
        let r = self.records.get(index * 6..index * 6 + 6).ok_or("binary xml: element index out of range")?;
        let [name, text, n_attr, attr_first, n_child, child_first] = [r[0], r[1], r[2], r[3], r[4], r[5]];
        let attrs = (attr_first as usize..attr_first as usize + n_attr as usize)
            .map(|k| {
                let p = self.pairs.get(k * 2..k * 2 + 2).ok_or("binary xml: attribute index out of range")?;
                Ok((self.strings.get(p[0])?, self.strings.get(p[1])?))
            })
            .collect::<Result<_, String>>()?;
        let text = if text == 0 { None } else { Some(self.strings.get(text)?.trim().to_owned()) };
        let first = child_first as usize;
        if n_child > 0 && first <= index {
            return Err("binary xml: child before its parent".into());
        }
        let children =
            (first..first + n_child as usize).map(|c| self.element(c, depth + 1)).collect::<Result<_, String>>()?;
        Ok(Element { name: self.strings.get(name)?, attrs, text: text.filter(|t| !t.is_empty()), children })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn section(tag: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, 0x22, b'R', b'r'];
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn words_bytes(w: &[u32]) -> Vec<u8> {
        w.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// `<root a="1"><gate id="7">1 2.5 -3</gate><gate id="8"/></root>`
    pub(crate) fn sample() -> Vec<u8> {
        let strings = ["root", "gate", "a", "1", "id", "7", "1 2.5 -3", "8"];
        let mut blob = Vec::new();
        let mut offsets = Vec::new();
        for s in strings {
            offsets.push(blob.len() as u32);
            blob.extend(s.bytes().chain([0]));
        }
        let records = [0, 0, 1, 0, 2, 1, 1, 6, 1, 1, 0, 0, 1, 0, 1, 2, 0, 0];
        let pairs = [2, 3, 4, 5, 4, 7];
        let body: Vec<u8> = [
            section(0x1d, &blob),
            section(0x1e, &words_bytes(&offsets)),
            section(0x1b, &words_bytes(&records)),
            section(0x1c, &words_bytes(&pairs)),
        ]
        .concat();
        let mut file = vec![0x1a, 0x22, b'R', b'r'];
        file.extend((body.len() as u32 + 16).to_le_bytes());
        file.extend([0x17, 0x22, b'R', b'r']);
        file.extend((body.len() as u32).to_le_bytes());
        file.extend(body);
        file
    }

    #[test]
    fn the_tree_is_read() {
        let root = parse(&sample()).unwrap();
        assert_eq!((root.name.as_str(), root.attr("a"), root.text.as_deref()), ("root", Some("1"), None));
        assert_eq!(root.children_named("gate").count(), 2);
        let gate = root.child("gate").unwrap();
        assert_eq!((gate.attr_num("id"), gate.floats::<3>()), (Some(7.0), Some([1.0, 2.5, -3.0])));
        assert_eq!(root.children[1].attr("id"), Some("8"));
        assert!(root.children[1].text.is_none() && root.child("nope").is_none());
    }

    #[test]
    fn numbers_need_the_exact_count() {
        assert_eq!(numbers::<2>(" 1\n2.5 "), Some([1.0, 2.5]));
        assert_eq!(numbers::<2>("1"), None);
        assert_eq!(numbers::<2>("1 2 3"), None);
        assert_eq!(numbers::<1>("x"), None);
    }

    #[test]
    fn damaged_files_are_errors() {
        let file = sample();
        assert!(parse(b"<?xml").is_err());
        for cut in [20, 40, file.len() - 1] {
            assert!(parse(&file[..cut]).is_err(), "{cut}");
        }
    }
}
