//! Reader for DirectX `.x` mesh files.
//!
//! The binary layout follows Microsoft's documented `.x` binary format: after
//! the 16-byte header (`xof 0303bin 0032`) comes a token stream with the token
//! numbers 1 (`TOKEN_NAME`), 2 (`STRING`), 3 (`INTEGER`), 5 (`GUID`), 6
//! (`INTEGER_LIST`), 7 (`FLOAT_LIST`), 10/11 (braces), 12-20 (other separators),
//! 31 (`template`) and 40-52 (member type keywords). Text files (`txt `) carry
//! the same structure in ASCII. Compressed files (`bzip`, `tzip`) are reported,
//! not decoded.
//!
//! Parsing is template-driven: `template` declarations give every data
//! object's member layout, and the typed readers below decode through those
//! declarations rather than hard-coding member offsets. Adjacent values of one
//! kind merge into a single list in binary files (an `nVertices` count, the
//! `vertices` vectors and the `nFaces` count may share one `INTEGER_LIST`),
//! so members consume from one ordered value stream.
//!
//! BTB ("Bob's Track Builder") stages use the binary 32-bit form with the
//! templates `Vector`, `MeshFace`, `Mesh`, `MeshNormals`, `Coords2d`,
//! `MeshTextureCoords`, `ColorRGBA`, `ColorRGB`, `Material`,
//! `MeshMaterialList`, `TextureFilename`, `VertexDuplicationIndices` and, on
//! some meshes, `VertexElement`/`DeclData` or `FVFData`. Every mesh is a
//! top-level unnamed `Mesh` with triangles only. See `notes/formats.md`
//! "BTB meshes".

use crate::le::{self, Reader, Truncated};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};

// Binary token numbers (Microsoft's documented `.x` binary format).
const TOKEN_NAME: u16 = 1;
const TOKEN_STRING: u16 = 2;
const TOKEN_INTEGER: u16 = 3;
const TOKEN_GUID: u16 = 5;
const TOKEN_INTEGER_LIST: u16 = 6;
const TOKEN_FLOAT_LIST: u16 = 7;
const TOKEN_OBRACE: u16 = 10;
const TOKEN_CBRACE: u16 = 11;
const TOKEN_OPAREN: u16 = 12;
const TOKEN_CPAREN: u16 = 13;
const TOKEN_OBRACKET: u16 = 14;
const TOKEN_CBRACKET: u16 = 15;
const TOKEN_OANGLE: u16 = 16;
const TOKEN_CANGLE: u16 = 17;
const TOKEN_DOT: u16 = 18;
const TOKEN_COMMA: u16 = 19;
const TOKEN_SEMICOLON: u16 = 20;
const TOKEN_TEMPLATE: u16 = 31;

/// Failure to read a `.x` file.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Truncated {
        offset: usize,
        needed: usize,
        len: usize,
    },
    /// The 16-byte header is missing or names an unknown version, form or
    /// float size. Compressed forms (`bzip`, `tzip`) report [`Error::Compressed`].
    BadHeader(String),
    /// A compressed file (`bzip` or `tzip`): reported, not decoded.
    Compressed(String),
    /// An unknown binary token number at the offset.
    BadToken {
        offset: usize,
        token: u16,
    },
    /// A malformed text file, with its line number.
    Text {
        line: usize,
        message: String,
    },
    /// A well-formed token that fits nowhere (a separator at the top level,
    /// a name where a value was decoded, ...).
    Unexpected(String),
    /// A data object or member names a template the file never declares.
    UnknownTemplate(String),
    /// A member value has the wrong kind, a count disagrees, or a typed
    /// reader's expected member is missing.
    Decode(String),
}

impl From<Truncated> for Error {
    fn from(t: Truncated) -> Self {
        Error::Truncated { offset: t.offset, needed: t.needed, len: t.len }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { offset, needed, len } => le::fmt_truncated(f, *offset, *needed, *len),
            Error::BadHeader(message) | Error::Unexpected(message) | Error::Decode(message) => {
                write!(f, "{message}")
            }
            Error::UnknownTemplate(name) => write!(f, "unknown template {name:?}"),
            Error::Compressed(form) => write!(f, "compressed .x file ({form}): not decoded"),
            Error::BadToken { offset, token } => write!(f, "unknown binary token {token} at offset {offset}"),
            Error::Text { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// The float width the header declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatSize {
    F32,
    F64,
}

/// A parsed file: its template declarations and top-level data objects.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub float_size: FloatSize,
    pub templates: Vec<Template>,
    pub roots: Vec<Node>,
}

impl File {
    /// The template declaration `name`, if the file declares it.
    pub fn template(&self, name: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// Every data object of type `ty` in the tree, in file order.
    pub fn objects_of_type(&self, ty: &str) -> Vec<&Object> {
        let mut out = Vec::new();
        for root in &self.roots {
            root.collect_type(ty, &mut out);
        }
        out
    }

    /// The data object with instance name `name`, if any.
    pub fn find_named(&self, name: &str) -> Option<&Object> {
        for root in &self.roots {
            if let Some(found) = root.find_named(name) {
                return Some(found);
            }
        }
        None
    }
}

/// A `template` declaration: the member layout of its data objects.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub name: String,
    /// The `<uuid>`, absent when the declaration has none.
    pub uuid: Option<[u8; 16]>,
    pub members: Vec<Member>,
    pub restrictions: Restrictions,
}

/// The trailing `[...]` of a template declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum Restrictions {
    /// No restriction block (as in `MeshFace`): no children occur.
    None,
    /// `[...]`: any children may occur (as in `Mesh`).
    Any,
    /// Named member types (as in `MeshMaterialList`'s `[Material]`).
    Only(Vec<Restriction>),
}

/// One named member type of a restriction block.
#[derive(Debug, Clone, PartialEq)]
pub struct Restriction {
    pub name: String,
    pub uuid: Option<[u8; 16]>,
}

/// One declared member: a primitive or a nested template instance, scalar or
/// array. `array DWORD x[n]` and `FLOAT m[16]` decode the same way: `dims`
/// drives how many values the member consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub name: String,
    pub kind: MemberKind,
    pub dims: Vec<Dim>,
}

/// A member's element type: a primitive keyword or a template name.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberKind {
    Prim(PrimType),
    Struct(String),
}

/// An array dimension: a fixed length, a previous member's name, or `[]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Dim {
    Fixed(u32),
    Ref(String),
    Open,
}

/// A member type keyword (`WORD` .. `CSTRING`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimType {
    Word,
    Dword,
    Float,
    Double,
    Char,
    UChar,
    Sword,
    Sdword,
    Void,
    Lpstr,
    Unicode,
    Cstring,
}

impl PrimType {
    fn is_int(self) -> bool {
        matches!(
            self,
            PrimType::Word | PrimType::Dword | PrimType::Char | PrimType::UChar | PrimType::Sword | PrimType::Sdword
        )
    }

    fn is_float(self) -> bool {
        matches!(self, PrimType::Float | PrimType::Double)
    }

    fn is_string(self) -> bool {
        matches!(self, PrimType::Lpstr | PrimType::Unicode | PrimType::Cstring)
    }
}

/// A data object or a `{ name }` reference to a named one.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Object(Object),
    Ref(String),
}

impl Node {
    fn collect_type<'a>(&'a self, ty: &str, out: &mut Vec<&'a Object>) {
        if let Node::Object(object) = self {
            if object.ty == ty {
                out.push(object);
            }
            for child in &object.children {
                child.collect_type(ty, out);
            }
        }
    }

    fn find_named<'a>(&'a self, name: &str) -> Option<&'a Object> {
        if let Node::Object(object) = self {
            if object.name.as_deref() == Some(name) {
                return Some(object);
            }
            for child in &object.children {
                if let Some(found) = child.find_named(name) {
                    return Some(found);
                }
            }
        }
        None
    }
}

/// A data object: its template name, optional instance name, member values in
/// file order (adjacent same-kind values share one binary list), and children.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub ty: String,
    pub name: Option<String>,
    pub values: Vec<Value>,
    pub children: Vec<Node>,
}

/// One member value scalar.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f64),
    Str(String),
}

/// Reads a `.x` file (binary or text) into its templates and data objects.
pub fn parse(bytes: &[u8]) -> Result<File, Error> {
    if bytes.len() < 16 {
        return Err(Error::BadHeader(format!("file is {} bytes, shorter than the 16-byte header", bytes.len())));
    }
    if &bytes[0..4] != b"xof " {
        return Err(Error::BadHeader("missing the `xof ` magic".to_string()));
    }
    let version = &bytes[4..8];
    if version != b"0303" && version != b"0302" {
        return Err(Error::BadHeader(format!("unknown version {}", String::from_utf8_lossy(version))));
    }
    let form = &bytes[8..12];
    let float_size = match &bytes[12..16] {
        b"0032" => FloatSize::F32,
        b"0064" => FloatSize::F64,
        other => {
            return Err(Error::BadHeader(format!("unknown float size {}", String::from_utf8_lossy(other))));
        }
    };
    let toks = match form {
        b"bin " => binary_tokens(bytes, float_size)?,
        b"txt " => text_tokens(bytes)?,
        _ => {
            let name = String::from_utf8_lossy(form).into_owned();
            if name == "bzip" || name == "tzip" {
                return Err(Error::Compressed(name));
            }
            return Err(Error::BadHeader(format!("unknown form {name}")));
        }
    };
    let mut build = Build { toks: &toks, pos: 0 };
    let mut templates = Vec::new();
    let mut roots = Vec::new();
    while build.pos < build.toks.len() {
        match &build.toks[build.pos] {
            Tok::Template => templates.push(build.template_decl()?),
            Tok::Name(_) => roots.push(Node::Object(build.object()?)),
            other => return Err(Error::Unexpected(format!("{other:?} at the top level"))),
        }
    }
    Ok(File { float_size, templates, roots })
}

/// The shared token stream both header forms decode into.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Template,
    Name(String),
    Str(String),
    Int(i32),
    Ints(Vec<i32>),
    Float(f64),
    Floats(Vec<f64>),
    Guid([u8; 16]),
    Prim(PrimType),
    Array,
    OBrace,
    CBrace,
    OParen,
    CParen,
    OBracket,
    CBracket,
    OAngle,
    CAngle,
    Dot,
    Comma,
    Semi,
}

fn prim_of(token: u16) -> Option<PrimType> {
    match token {
        40 => Some(PrimType::Word),
        41 => Some(PrimType::Dword),
        42 => Some(PrimType::Float),
        43 => Some(PrimType::Double),
        44 => Some(PrimType::Char),
        45 => Some(PrimType::UChar),
        46 => Some(PrimType::Sword),
        47 => Some(PrimType::Sdword),
        48 => Some(PrimType::Void),
        49 => Some(PrimType::Lpstr),
        50 => Some(PrimType::Unicode),
        51 => Some(PrimType::Cstring),
        _ => None,
    }
}

/// A counted binary string: the length names the bytes as stored. A single
/// trailing NUL is dropped when present (some writers count it).
fn counted(bytes: &[u8]) -> String {
    let bytes = bytes.strip_suffix(b"\x00").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn binary_tokens(bytes: &[u8], float_size: FloatSize) -> Result<Vec<Tok>, Error> {
    let mut r = Reader::<Error>::at(bytes, 16)?;
    let mut toks = Vec::new();
    while !r.is_empty() {
        let at = r.pos();
        match r.u16()? {
            TOKEN_NAME => {
                let len = r.u32()? as usize;
                toks.push(Tok::Name(counted(r.take(len)?)));
            }
            TOKEN_STRING => {
                let len = r.u32()? as usize;
                toks.push(Tok::Str(counted(r.take(len)?)));
            }
            TOKEN_INTEGER => toks.push(Tok::Int(r.u32()? as i32)),
            TOKEN_GUID => {
                let mut uuid = [0u8; 16];
                uuid.copy_from_slice(r.take(16)?);
                toks.push(Tok::Guid(uuid));
            }
            TOKEN_INTEGER_LIST => {
                let count = r.u32()? as usize;
                let raw =
                    r.take(count.checked_mul(4).ok_or(Error::BadToken { offset: at, token: TOKEN_INTEGER_LIST })?)?;
                toks.push(Tok::Ints(
                    raw.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                ));
            }
            TOKEN_FLOAT_LIST => {
                let count = r.u32()? as usize;
                match float_size {
                    FloatSize::F32 => {
                        let raw = r.take(
                            count.checked_mul(4).ok_or(Error::BadToken { offset: at, token: TOKEN_FLOAT_LIST })?,
                        )?;
                        toks.push(Tok::Floats(
                            raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64).collect(),
                        ));
                    }
                    FloatSize::F64 => {
                        let raw = r.take(
                            count.checked_mul(8).ok_or(Error::BadToken { offset: at, token: TOKEN_FLOAT_LIST })?,
                        )?;
                        toks.push(Tok::Floats(
                            raw.chunks_exact(8)
                                .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                                .collect(),
                        ));
                    }
                }
            }
            TOKEN_OBRACE => toks.push(Tok::OBrace),
            TOKEN_CBRACE => toks.push(Tok::CBrace),
            TOKEN_OPAREN => toks.push(Tok::OParen),
            TOKEN_CPAREN => toks.push(Tok::CParen),
            TOKEN_OBRACKET => toks.push(Tok::OBracket),
            TOKEN_CBRACKET => toks.push(Tok::CBracket),
            TOKEN_OANGLE => toks.push(Tok::OAngle),
            TOKEN_CANGLE => toks.push(Tok::CAngle),
            TOKEN_DOT => toks.push(Tok::Dot),
            TOKEN_COMMA => toks.push(Tok::Comma),
            TOKEN_SEMICOLON => toks.push(Tok::Semi),
            TOKEN_TEMPLATE => toks.push(Tok::Template),
            token => {
                if let Some(prim) = prim_of(token) {
                    toks.push(Tok::Prim(prim));
                } else if token == 52 {
                    toks.push(Tok::Array);
                } else {
                    return Err(Error::BadToken { offset: at, token });
                }
            }
        }
    }
    // Binary files never store angle brackets (text wraps UUIDs in `<...>`;
    // binary uses GUID tokens), so the builder rejects them on sight.
    Ok(toks)
}

/// The tree builder both token streams share: template declarations and
/// data objects (`Name [name] { ... }`), with `{ name }` references.
struct Build<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl Build<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Result<Tok, Error> {
        let tok = self.peek().cloned().ok_or_else(|| Error::Unexpected("unexpected end of file".to_string()))?;
        self.pos += 1;
        Ok(tok)
    }

    fn expect<T>(&mut self, want: &str) -> Result<T, Error> {
        Err(Error::Unexpected(format!("expected {want}, found {:?}", self.peek())))
    }

    fn name(&mut self) -> Result<String, Error> {
        match self.next()? {
            Tok::Name(name) => Ok(name),
            _ => self.expect("a name"),
        }
    }

    fn eat(&mut self, want: &Tok) -> bool {
        if self.peek() == Some(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn template_decl(&mut self) -> Result<Template, Error> {
        debug_assert_eq!(self.toks[self.pos], Tok::Template);
        self.pos += 1;
        let name = self.name()?;
        if !self.eat(&Tok::OBrace) {
            return self.expect("'{'");
        }
        let uuid = match self.peek() {
            Some(Tok::Guid(uuid)) => {
                let uuid = *uuid;
                self.pos += 1;
                Some(uuid)
            }
            _ => None,
        };
        let mut members = Vec::new();
        let mut restrictions = Restrictions::None;
        loop {
            match self.next()? {
                Tok::CBrace => break,
                Tok::OBracket => {
                    if self.eat(&Tok::Dot) {
                        // `[...]`: any children may occur.
                        for _ in 0..2 {
                            if !self.eat(&Tok::Dot) {
                                return self.expect("'.'");
                            }
                        }
                        if !self.eat(&Tok::CBracket) {
                            return self.expect("']'");
                        }
                        restrictions = Restrictions::Any;
                    } else {
                        let mut only = Vec::new();
                        loop {
                            let name = self.name()?;
                            let uuid = match self.peek() {
                                Some(Tok::Guid(uuid)) => {
                                    let uuid = *uuid;
                                    self.pos += 1;
                                    Some(uuid)
                                }
                                _ => None,
                            };
                            only.push(Restriction { name, uuid });
                            if self.eat(&Tok::Comma) {
                                continue;
                            }
                            if self.eat(&Tok::CBracket) {
                                break;
                            }
                            return self.expect("',' or ']'");
                        }
                        restrictions = Restrictions::Only(only);
                    }
                    if !self.eat(&Tok::CBrace) {
                        return self.expect("'}' after the restrictions");
                    }
                    break;
                }
                Tok::Prim(prim) => members.push(self.member(MemberKind::Prim(prim))?),
                Tok::Array => {
                    let kind = match self.next()? {
                        Tok::Prim(prim) => MemberKind::Prim(prim),
                        Tok::Name(ty) => MemberKind::Struct(ty),
                        _ => return self.expect("an array element type"),
                    };
                    members.push(self.member(kind)?);
                }
                Tok::Name(ty) => members.push(self.member(MemberKind::Struct(ty))?),
                _ => return self.expect("a member or '}'"),
            }
        }
        Ok(Template { name, uuid, members, restrictions })
    }

    fn member(&mut self, kind: MemberKind) -> Result<Member, Error> {
        let name = self.name()?;
        let mut dims = Vec::new();
        while self.eat(&Tok::OBracket) {
            match self.next()? {
                Tok::CBracket => dims.push(Dim::Open),
                Tok::Int(n) if n >= 0 => dims.push(Dim::Fixed(n as u32)),
                Tok::Name(len) => dims.push(Dim::Ref(len)),
                _ => return self.expect("an array bound"),
            }
            if !self.eat(&Tok::CBracket) {
                return self.expect("']'");
            }
        }
        if !self.eat(&Tok::Semi) {
            return self.expect("';'");
        }
        Ok(Member { name, kind, dims })
    }

    fn object(&mut self) -> Result<Object, Error> {
        let ty = self.name()?;
        let name = match self.peek() {
            Some(Tok::Name(_)) => Some(self.name()?), // an instance name
            _ => None,
        };
        if !self.eat(&Tok::OBrace) {
            return self.expect("'{'");
        }
        self.object_body(ty, name)
    }

    fn object_body(&mut self, ty: String, name: Option<String>) -> Result<Object, Error> {
        let mut values = Vec::new();
        let mut children = Vec::new();
        loop {
            match self.next()? {
                Tok::CBrace => break,
                Tok::Int(v) => values.push(Value::Int(v)),
                Tok::Ints(v) => values.extend(v.into_iter().map(Value::Int)),
                Tok::Float(v) => values.push(Value::Float(v)),
                Tok::Floats(v) => values.extend(v.into_iter().map(Value::Float)),
                Tok::Str(v) => values.push(Value::Str(v)),
                Tok::Semi | Tok::Comma => {} // text separators
                Tok::Name(ty) => {
                    if self.eat(&Tok::OBrace) {
                        children.push(Node::Object(self.object_body(ty, None)?));
                    } else {
                        let name = self.name()?;
                        if !self.eat(&Tok::OBrace) {
                            return self.expect("'{' after the instance name");
                        }
                        children.push(Node::Object(self.object_body(ty, Some(name))?));
                    }
                }
                Tok::OBrace => {
                    // `{ name }`: a reference to a named object.
                    let name = self.name()?;
                    if !self.eat(&Tok::CBrace) {
                        return self.expect("'}' after the reference");
                    }
                    children.push(Node::Ref(name));
                }
                _ => return self.expect("a value, a child object or '}'"),
            }
        }
        Ok(Object { ty, name, values, children })
    }
}

fn text_error(bytes: &[u8], at: usize, message: String) -> Error {
    let line = bytes[..at.min(bytes.len())].iter().filter(|&&b| b == b'\n').count() + 1;
    Error::Text { line, message }
}

/// The ASCII token stream of a `txt ` file: names, `"strings"` (with
/// `\\` and `\"` escapes), integers, floats, `<uuid>` guids, punctuation,
/// and the `template`/`array`/type keywords. `#` and `//` run to end of line.
fn text_tokens(bytes: &[u8]) -> Result<Vec<Tok>, Error> {
    let err = |at: usize, message: &str| text_error(bytes, at, message.to_string());
    let mut toks = Vec::new();
    let mut pos = 16;
    while pos < bytes.len() {
        let at = pos;
        let b = bytes[pos];
        if b.is_ascii_whitespace() {
            pos += 1;
        } else if b == b'#' || (b == b'/' && bytes.get(pos + 1) == Some(&b'/')) {
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
        } else if b == b'"' {
            pos += 1;
            let mut out = String::new();
            let mut closed = false;
            while pos < bytes.len() {
                let c = bytes[pos];
                pos += 1;
                if c == b'"' {
                    closed = true;
                    break;
                }
                if c == b'\\' {
                    if pos >= bytes.len() {
                        break;
                    }
                    out.push(bytes[pos] as char);
                    pos += 1;
                } else {
                    out.push(c as char);
                }
            }
            if !closed {
                return Err(err(at, "unterminated string"));
            }
            toks.push(Tok::Str(out));
        } else if b == b'<' {
            pos += 1;
            let start = pos;
            while pos < bytes.len() && bytes[pos] != b'>' {
                pos += 1;
            }
            if pos >= bytes.len() {
                return Err(err(at, "unterminated <uuid>"));
            }
            let text: String = bytes[start..pos].iter().map(|&c| c as char).collect();
            pos += 1;
            let hex: String = text.chars().filter(|&c| c != '-').collect();
            if hex.len() != 32 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(err(at, &format!("bad <uuid> {text:?}")));
            }
            let mut uuid = [0u8; 16];
            for (i, slot) in uuid.iter_mut().enumerate() {
                *slot = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or(0);
            }
            toks.push(Tok::Guid(uuid));
        } else if b.is_ascii_alphabetic() || b == b'_' {
            while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
            }
            match String::from_utf8_lossy(&bytes[at..pos]).as_ref() {
                "template" => toks.push(Tok::Template),
                "array" => toks.push(Tok::Array),
                "WORD" => toks.push(Tok::Prim(PrimType::Word)),
                "DWORD" => toks.push(Tok::Prim(PrimType::Dword)),
                "FLOAT" => toks.push(Tok::Prim(PrimType::Float)),
                "DOUBLE" => toks.push(Tok::Prim(PrimType::Double)),
                "CHAR" => toks.push(Tok::Prim(PrimType::Char)),
                "UCHAR" => toks.push(Tok::Prim(PrimType::UChar)),
                "SWORD" => toks.push(Tok::Prim(PrimType::Sword)),
                "SDWORD" => toks.push(Tok::Prim(PrimType::Sdword)),
                "VOID" => toks.push(Tok::Prim(PrimType::Void)),
                "LPSTR" => toks.push(Tok::Prim(PrimType::Lpstr)),
                "UNICODE" => toks.push(Tok::Prim(PrimType::Unicode)),
                "CSTRING" => toks.push(Tok::Prim(PrimType::Cstring)),
                name => toks.push(Tok::Name(name.to_string())),
            }
        } else if b.is_ascii_digit() || b == b'+' || b == b'-' || b == b'.' {
            if b == b'.' && !bytes.get(pos + 1).is_some_and(|c| c.is_ascii_digit()) {
                pos += 1;
                toks.push(Tok::Dot);
                continue;
            }
            let mut end = pos;
            if bytes[end] == b'+' || bytes[end] == b'-' {
                end += 1;
            }
            let mut is_float = false;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'.' {
                is_float = true;
                end += 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
            }
            if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
                is_float = true;
                end += 1;
                if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
                    end += 1;
                }
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
            }
            if end == pos || (end == pos + 1 && (b == b'+' || b == b'-' || b == b'.')) {
                return Err(err(at, &format!("stray {:?}", b as char)));
            }
            let text = String::from_utf8_lossy(&bytes[pos..end]).into_owned();
            pos = end;
            if is_float {
                match text.parse::<f64>() {
                    Ok(v) => toks.push(Tok::Float(v)),
                    Err(_) => return Err(err(at, &format!("bad float {text:?}"))),
                }
            } else {
                match text.parse::<i64>() {
                    Ok(v) => match i32::try_from(v) {
                        Ok(v) => toks.push(Tok::Int(v)),
                        Err(_) => return Err(err(at, &format!("integer {text:?} exceeds 32 bits"))),
                    },
                    Err(_) => return Err(err(at, &format!("bad integer {text:?}"))),
                }
            }
        } else {
            pos += 1;
            match b {
                b'{' => toks.push(Tok::OBrace),
                b'}' => toks.push(Tok::CBrace),
                b'(' => toks.push(Tok::OParen),
                b')' => toks.push(Tok::CParen),
                b'[' => toks.push(Tok::OBracket),
                b']' => toks.push(Tok::CBracket),
                b',' => toks.push(Tok::Comma),
                b';' => toks.push(Tok::Semi),
                _ => return Err(err(at, &format!("unexpected {:?}", b as char))),
            }
        }
    }
    Ok(toks)
}

/// A data object decoded through its template declaration: one entry per
/// declared member, in declaration order.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedStruct {
    pub ty: String,
    pub members: Vec<NamedMember>,
}

/// One decoded member value.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedMember {
    pub name: String,
    pub value: MemberValue,
}

/// A member's values: scalars by kind, or decoded nested structs.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberValue {
    Ints(Vec<i32>),
    Floats(Vec<f64>),
    Strings(Vec<String>),
    Structs(Vec<DecodedStruct>),
}

impl DecodedStruct {
    /// The member `name`, or a [`Error::Decode`] naming the object and member.
    pub fn member(&self, name: &str) -> Result<&MemberValue, Error> {
        self.members
            .iter()
            .find(|m| m.name == name)
            .map(|m| &m.value)
            .ok_or_else(|| Error::Decode(format!("{}: no member {name:?}", self.ty)))
    }

    /// All `Floats` (and integer scalars, widened) of the struct's members.
    pub fn floats(&self) -> Result<Vec<f64>, Error> {
        let mut out = Vec::new();
        for member in &self.members {
            match &member.value {
                MemberValue::Floats(v) => out.extend(v),
                MemberValue::Ints(v) => out.extend(v.iter().map(|&v| v as f64)),
                other => {
                    return Err(Error::Decode(format!(
                        "{}: member {:?} holds {other:?}, not numbers",
                        self.ty, member.name
                    )))
                }
            }
        }
        Ok(out)
    }

    /// All `Ints` of the struct's members.
    pub fn ints(&self) -> Result<Vec<i32>, Error> {
        let mut out = Vec::new();
        for member in &self.members {
            match &member.value {
                MemberValue::Ints(v) => out.extend(v),
                other => {
                    return Err(Error::Decode(format!(
                        "{}: member {:?} holds {other:?}, not integers",
                        self.ty, member.name
                    )))
                }
            }
        }
        Ok(out)
    }
}

/// Decodes `object` through the file's declaration of its template: each
/// member consumes its declared values (array lengths from fixed bounds or
/// earlier count members), and every value must be consumed exactly.
///
/// An `Int` decodes a float member by widening; a `Float` never decodes an
/// integer member. An open `[]` dimension consumes the remaining values.
pub fn decode_object(object: &Object, file: &File) -> Result<DecodedStruct, Error> {
    let templates: HashMap<&str, &Template> = file.templates.iter().map(|t| (t.name.as_str(), t)).collect();
    let template = templates.get(object.ty.as_str()).ok_or_else(|| Error::UnknownTemplate(object.ty.clone()))?;
    let mut cursor = Cursor { values: &object.values, pos: 0, owner: object.ty.clone() };
    let decoded = decode_members(template, &mut cursor, &templates)?;
    if cursor.pos != object.values.len() {
        return Err(Error::Decode(format!(
            "{}: {} trailing values after its members",
            object.ty,
            object.values.len() - cursor.pos
        )));
    }
    Ok(DecodedStruct { ty: object.ty.clone(), members: decoded })
}

struct Cursor<'a> {
    values: &'a [Value],
    pos: usize,
    owner: String,
}

impl Cursor<'_> {
    fn error(&self, message: String) -> Error {
        Error::Decode(format!("{}: {message}", self.owner))
    }

    fn take_int(&mut self, member: &str) -> Result<i32, Error> {
        match self.values.get(self.pos) {
            Some(Value::Int(v)) => {
                self.pos += 1;
                Ok(*v)
            }
            found => Err(self.error(format!("member {member:?} wants an integer, found {found:?}"))),
        }
    }

    fn take_float(&mut self, member: &str) -> Result<f64, Error> {
        match self.values.get(self.pos) {
            Some(Value::Float(v)) => {
                self.pos += 1;
                Ok(*v)
            }
            Some(Value::Int(v)) => {
                self.pos += 1;
                Ok(*v as f64)
            }
            found => Err(self.error(format!("member {member:?} wants a number, found {found:?}"))),
        }
    }

    fn take_string(&mut self, member: &str) -> Result<String, Error> {
        match self.values.get(self.pos) {
            Some(Value::Str(v)) => {
                self.pos += 1;
                Ok(v.clone())
            }
            found => Err(self.error(format!("member {member:?} wants a string, found {found:?}"))),
        }
    }
}

fn decode_members(
    template: &Template,
    cursor: &mut Cursor<'_>,
    templates: &HashMap<&str, &Template>,
) -> Result<Vec<NamedMember>, Error> {
    let mut decoded: Vec<NamedMember> = Vec::with_capacity(template.members.len());
    for member in &template.members {
        let mut count = 1usize;
        let mut open = false;
        for dim in &member.dims {
            match dim {
                Dim::Fixed(n) => {
                    count = count
                        .checked_mul(*n as usize)
                        .ok_or_else(|| cursor.error(format!("member {:?} overflows", member.name)))?
                }
                Dim::Ref(len) => {
                    let len = decoded
                        .iter()
                        .find(|m| m.name == *len)
                        .and_then(|m| match &m.value {
                            MemberValue::Ints(v) if v.len() == 1 => Some(v[0]),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            cursor.error(format!("member {:?} needs an integer count {len:?}", member.name))
                        })?;
                    if len < 0 {
                        return Err(cursor.error(format!("member {:?} has negative length {len}", member.name)));
                    }
                    count = count
                        .checked_mul(len as usize)
                        .ok_or_else(|| cursor.error(format!("member {:?} overflows", member.name)))?;
                }
                Dim::Open => open = true,
            }
        }
        let value = match &member.kind {
            MemberKind::Prim(prim) => {
                let prim = *prim;
                if prim.is_int() {
                    let mut v = Vec::new();
                    if open {
                        while cursor.pos < cursor.values.len() {
                            v.push(cursor.take_int(&member.name)?);
                        }
                    } else {
                        for _ in 0..count {
                            v.push(cursor.take_int(&member.name)?);
                        }
                    }
                    MemberValue::Ints(v)
                } else if prim.is_float() {
                    let mut v = Vec::new();
                    if open {
                        while cursor.pos < cursor.values.len() {
                            v.push(cursor.take_float(&member.name)?);
                        }
                    } else {
                        for _ in 0..count {
                            v.push(cursor.take_float(&member.name)?);
                        }
                    }
                    MemberValue::Floats(v)
                } else if prim.is_string() {
                    let mut v = Vec::new();
                    if open {
                        while cursor.pos < cursor.values.len() {
                            v.push(cursor.take_string(&member.name)?);
                        }
                    } else {
                        for _ in 0..count {
                            v.push(cursor.take_string(&member.name)?);
                        }
                    }
                    MemberValue::Strings(v)
                } else {
                    debug_assert_eq!(prim, PrimType::Void);
                    MemberValue::Ints(Vec::new())
                }
            }
            MemberKind::Struct(ty) => {
                let template = templates.get(ty.as_str()).ok_or_else(|| Error::UnknownTemplate(ty.clone()))?;
                let mut v = Vec::new();
                if open {
                    while cursor.pos < cursor.values.len() {
                        let start = cursor.pos;
                        let members = decode_members(template, cursor, templates)?;
                        if cursor.pos == start {
                            return Err(cursor.error(format!("member {:?} makes no progress", member.name)));
                        }
                        v.push(DecodedStruct { ty: ty.clone(), members });
                    }
                } else {
                    for _ in 0..count {
                        let members = decode_members(template, cursor, templates)?;
                        v.push(DecodedStruct { ty: ty.clone(), members });
                    }
                }
                MemberValue::Structs(v)
            }
        };
        decoded.push(NamedMember { name: member.name.clone(), value });
    }
    Ok(decoded)
}

fn vec3(of: &DecodedStruct, what: &str) -> Result<[f32; 3], Error> {
    let v = of.floats()?;
    if v.len() != 3 {
        return Err(Error::Decode(format!("{}: {what} has {} numbers, not 3", of.ty, v.len())));
    }
    Ok([v[0] as f32, v[1] as f32, v[2] as f32])
}

fn vec2(of: &DecodedStruct, what: &str) -> Result<[f32; 2], Error> {
    let v = of.floats()?;
    if v.len() != 2 {
        return Err(Error::Decode(format!("{}: {what} has {} numbers, not 2", of.ty, v.len())));
    }
    Ok([v[0] as f32, v[1] as f32])
}

fn face_indices(of: &DecodedStruct, what: &str) -> Result<Vec<u32>, Error> {
    let flat = of.ints()?;
    if flat.is_empty() {
        return Err(Error::Decode(format!("{}: {what} is empty", of.ty)));
    }
    let (n, ids) = (flat[0], &flat[1..]);
    if n < 0 || n as usize != ids.len() {
        return Err(Error::Decode(format!("{}: {what} says {n} indices but holds {}", of.ty, ids.len())));
    }
    ids.iter()
        .map(|&i| u32::try_from(i).map_err(|_| Error::Decode(format!("{}: {what} has negative index {i}", of.ty))))
        .collect()
}

/// A polygon of a [`Mesh`]: `n` vertex indices into [`Mesh::positions`].
/// BTB meshes use triangles only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face(pub Vec<u32>);

impl Face {
    /// Triangulates the polygon as a fan (`(0, i, i + 1)`).
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        triangulate(&self.0)
    }
}

/// Triangulates a polygon as a fan (`(0, i, i + 1)`). Triangles pass through.
pub fn triangulate(polygon: &[u32]) -> Vec<[u32; 3]> {
    let mut out = Vec::new();
    for pair in polygon.get(1..).unwrap_or(&[]).windows(2) {
        out.push([polygon[0], pair[0], pair[1]]);
    }
    out
}

/// A typed `Mesh`: positions, polygon faces and its child objects, decoded
/// through the file's template declarations.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub faces: Vec<Face>,
    pub normals: Option<MeshNormals>,
    /// One entry per `MeshTextureCoords` child, in file order. BTB meshes
    /// carry at most one; further UV sets live in [`Mesh::decl`] and
    /// [`Mesh::fvf`].
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub colors: Option<MeshVertexColors>,
    pub materials: Option<MeshMaterialList>,
    pub duplication: Option<VertexDuplicationIndices>,
    pub decl: Vec<DeclData>,
    pub fvf: Vec<FvfData>,
}

impl Mesh {
    /// Decodes a `Mesh` data object. Member layouts come from the file's
    /// declarations; members are found by their standard names (`vertices`,
    /// `faces`) and children by template name.
    pub fn decode(object: &Object, file: &File) -> Result<Mesh, Error> {
        let decoded = decode_object(object, file)?;
        let structs_of = |name: &str| -> Result<&Vec<DecodedStruct>, Error> {
            match decoded.member(name)? {
                MemberValue::Structs(v) => Ok(v),
                other => Err(Error::Decode(format!("{}: member {name:?} holds {other:?}", decoded.ty))),
            }
        };
        let mut positions = Vec::new();
        for v in structs_of("vertices")? {
            positions.push(vec3(v, "a vertex")?);
        }
        let mut faces = Vec::new();
        for f in structs_of("faces")? {
            faces.push(Face(face_indices(f, "a face")?));
        }
        let mut mesh = Mesh {
            positions,
            faces,
            normals: None,
            uv_sets: Vec::new(),
            colors: None,
            materials: None,
            duplication: None,
            decl: Vec::new(),
            fvf: Vec::new(),
        };
        for child in &object.children {
            let Node::Object(child) = child else { continue };
            match child.ty.as_str() {
                "MeshNormals" => mesh.normals = Some(MeshNormals::decode(child, file)?),
                "MeshTextureCoords" => mesh.uv_sets.push(MeshTextureCoords::decode(child, file)?),
                "MeshVertexColors" => mesh.colors = Some(MeshVertexColors::decode(child, file)?),
                "MeshMaterialList" => mesh.materials = Some(MeshMaterialList::decode(child, file)?),
                "VertexDuplicationIndices" => mesh.duplication = Some(VertexDuplicationIndices::decode(child, file)?),
                "DeclData" => mesh.decl.push(DeclData::decode(child, file)?),
                "FVFData" => mesh.fvf.push(FvfData::decode(child, file)?),
                _ => {} // e.g. animation skins on non-BTB meshes
            }
        }
        Ok(mesh)
    }

    /// Every face triangulated as a fan, in face order.
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        self.faces.iter().flat_map(|f| f.triangles()).collect()
    }
}

/// A typed `MeshNormals` child: normals and one index face per mesh face.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshNormals {
    pub normals: Vec<[f32; 3]>,
    pub faces: Vec<Face>,
}

impl MeshNormals {
    pub fn decode(object: &Object, file: &File) -> Result<MeshNormals, Error> {
        let decoded = decode_object(object, file)?;
        let structs_of = |name: &str| -> Result<&Vec<DecodedStruct>, Error> {
            match decoded.member(name)? {
                MemberValue::Structs(v) => Ok(v),
                other => Err(Error::Decode(format!("{}: member {name:?} holds {other:?}", decoded.ty))),
            }
        };
        let mut normals = Vec::new();
        for v in structs_of("normals")? {
            normals.push(vec3(v, "a normal")?);
        }
        let mut faces = Vec::new();
        for f in structs_of("faceNormals")? {
            faces.push(Face(face_indices(f, "a normal face")?));
        }
        Ok(MeshNormals { normals, faces })
    }
}

/// A typed `MeshTextureCoords` child: one UV per mesh vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshTextureCoords(pub Vec<[f32; 2]>);

impl MeshTextureCoords {
    pub fn decode(object: &Object, file: &File) -> Result<Vec<[f32; 2]>, Error> {
        let decoded = decode_object(object, file)?;
        let coords = match decoded.member("textureCoords")? {
            MemberValue::Structs(v) => v,
            other => return Err(Error::Decode(format!("{}: member \"textureCoords\" holds {other:?}", decoded.ty))),
        };
        coords.iter().map(|c| vec2(c, "a UV")).collect()
    }
}

/// A typed `MeshVertexColors` child: `(vertex index, RGBA)` pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshVertexColors(pub Vec<(u32, [f32; 4])>);

impl MeshVertexColors {
    pub fn decode(object: &Object, file: &File) -> Result<MeshVertexColors, Error> {
        let decoded = decode_object(object, file)?;
        let entries = match decoded.member("vertexColors")? {
            MemberValue::Structs(v) => v,
            other => return Err(Error::Decode(format!("{}: member \"vertexColors\" holds {other:?}", decoded.ty))),
        };
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let index = entry.ints()?;
            let color = entry.floats()?;
            if index.len() != 1 || color.len() != 4 {
                return Err(Error::Decode(format!("{}: bad vertex color entry", entry.ty)));
            }
            out.push((index[0] as u32, [color[0] as f32, color[1] as f32, color[2] as f32, color[3] as f32]));
        }
        Ok(MeshVertexColors(out))
    }
}

/// A typed `MeshMaterialList` child: one material index per mesh face and the
/// materials, inline or `{ name }` references to named ones.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshMaterialList {
    pub face_materials: Vec<u32>,
    pub materials: Vec<MaterialEntry>,
}

/// One material of a [`MeshMaterialList`].
#[derive(Debug, Clone, PartialEq)]
pub enum MaterialEntry {
    Material(Material),
    Ref(String),
}

impl MeshMaterialList {
    pub fn decode(object: &Object, file: &File) -> Result<MeshMaterialList, Error> {
        let decoded = decode_object(object, file)?;
        let face_materials = match decoded.member("faceIndexes")? {
            MemberValue::Ints(v) => v
                .iter()
                .map(|&i| {
                    u32::try_from(i).map_err(|_| Error::Decode(format!("{}: negative face material {i}", decoded.ty)))
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => return Err(Error::Decode(format!("{}: member \"faceIndexes\" holds {other:?}", decoded.ty))),
        };
        let mut materials = Vec::new();
        for child in &object.children {
            match child {
                Node::Object(child) => materials.push(MaterialEntry::Material(Material::decode(child, file)?)),
                Node::Ref(name) => materials.push(MaterialEntry::Ref(name.clone())),
            }
        }
        Ok(MeshMaterialList { face_materials, materials })
    }
}

/// A typed `Material`: face colour, power, specular and emissive colours,
/// and the texture, inline or a reference to a named `TextureFilename`.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub face_color: [f32; 4],
    pub power: f32,
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub texture: Option<Texture>,
}

/// A material's texture.
#[derive(Debug, Clone, PartialEq)]
pub enum Texture {
    /// An inline `TextureFilename` with this file name.
    File(String),
    /// A `{ name }` reference to a named `TextureFilename`.
    Ref(String),
}

impl Material {
    pub fn decode(object: &Object, file: &File) -> Result<Material, Error> {
        let decoded = decode_object(object, file)?;
        let color4 = |name: &str| -> Result<[f32; 4], Error> {
            match decoded.member(name)? {
                MemberValue::Structs(v) if v.len() == 1 => {
                    let f = v[0].floats()?;
                    if f.len() != 4 {
                        return Err(Error::Decode(format!("{}: member {name:?} has {} numbers", decoded.ty, f.len())));
                    }
                    Ok([f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32])
                }
                other => Err(Error::Decode(format!("{}: member {name:?} holds {other:?}", decoded.ty))),
            }
        };
        let color3 = |name: &str| -> Result<[f32; 3], Error> {
            match decoded.member(name)? {
                MemberValue::Structs(v) if v.len() == 1 => Ok(vec3(&v[0], name)?),
                other => Err(Error::Decode(format!("{}: member {name:?} holds {other:?}", decoded.ty))),
            }
        };
        let face_color = color4("faceColor")?;
        let power = match decoded.member("power")? {
            MemberValue::Floats(v) if v.len() == 1 => v[0] as f32,
            other => return Err(Error::Decode(format!("{}: member \"power\" holds {other:?}", decoded.ty))),
        };
        let specular = color3("specularColor")?;
        let emissive = color3("emissiveColor")?;
        let mut texture = None;
        for child in &object.children {
            match child {
                Node::Object(child) => {
                    texture = Some(Texture::File(TextureFilename::decode(child, file)?));
                }
                Node::Ref(name) => texture = Some(Texture::Ref(name.clone())),
            }
        }
        Ok(Material { face_color, power, specular, emissive, texture })
    }
}

/// A typed `TextureFilename` child: the texture's file name.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureFilename(pub String);

impl TextureFilename {
    pub fn decode(object: &Object, file: &File) -> Result<String, Error> {
        let decoded = decode_object(object, file)?;
        match decoded.member("filename")? {
            MemberValue::Strings(v) if v.len() == 1 => Ok(v[0].clone()),
            other => Err(Error::Decode(format!("{}: member \"filename\" holds {other:?}", decoded.ty))),
        }
    }
}

/// A typed `VertexDuplicationIndices` child: BTB's shared-vertex map.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexDuplicationIndices {
    pub original_vertex_count: u32,
    pub indices: Vec<u32>,
}

impl VertexDuplicationIndices {
    pub fn decode(object: &Object, file: &File) -> Result<VertexDuplicationIndices, Error> {
        let decoded = decode_object(object, file)?;
        let int = |name: &str| -> Result<i32, Error> {
            match decoded.member(name)? {
                MemberValue::Ints(v) if v.len() == 1 => Ok(v[0]),
                other => Err(Error::Decode(format!("{}: member {name:?} holds {other:?}", decoded.ty))),
            }
        };
        let original_vertex_count = u32::try_from(int("nOriginalVertices")?)
            .map_err(|e| Error::Decode(format!("{}: bad nOriginalVertices: {e}", decoded.ty)))?;
        let indices = match decoded.member("indices")? {
            MemberValue::Ints(v) => v
                .iter()
                .map(|&i| {
                    u32::try_from(i)
                        .map_err(|_| Error::Decode(format!("{}: negative duplication index {i}", decoded.ty)))
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => return Err(Error::Decode(format!("{}: member \"indices\" holds {other:?}", decoded.ty))),
        };
        Ok(VertexDuplicationIndices { original_vertex_count, indices })
    }
}

/// One BTB `VertexElement`: a Direct3D vertex declaration element
/// (`Type`, `Method`, `Usage`, `UsageIndex`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VertexElement {
    pub ty: u32,
    pub method: u32,
    pub usage: u32,
    pub usage_index: u32,
}

impl VertexElement {
    /// The element's width in DWORDs under the `D3DDECLTYPE` numbering:
    /// `FLOAT1`..`FLOAT4` are 1..4, every other listed single-register type
    /// is 1, the two-register packed types are 2, or `None` when unlisted.
    pub fn width_dwords(self) -> Option<usize> {
        match self.ty {
            0 => Some(1),                                // FLOAT1
            1 => Some(2),                                // FLOAT2
            2 => Some(3),                                // FLOAT3
            3 => Some(4),                                // FLOAT4
            4 | 5 | 6 | 8 | 9 | 13 | 14 | 15 => Some(1), // D3DCOLOR, UBYTE4[N], SHORT2[N], UDEC3, DEC3N, FLOAT16_2
            7 | 10 | 11 | 12 | 16 => Some(2),            // SHORT4[N], USHORT2N/4N, FLOAT16_4
            _ => None,
        }
    }
}

/// A typed `DeclData`: BTB's extra per-vertex stream (tangents, blend
/// weights, further UV sets) described by [`VertexElement`]s, with its raw
/// DWORD payload.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclData {
    pub elements: Vec<VertexElement>,
    pub data: Vec<u32>,
}

impl DeclData {
    pub fn decode(object: &Object, file: &File) -> Result<DeclData, Error> {
        let decoded = decode_object(object, file)?;
        let elements = match decoded.member("Elements")? {
            MemberValue::Structs(v) => v
                .iter()
                .map(|e| {
                    let ints = e.ints()?;
                    if ints.len() != 4 {
                        return Err(Error::Decode(format!("{}: bad vertex element", e.ty)));
                    }
                    Ok(VertexElement {
                        ty: ints[0] as u32,
                        method: ints[1] as u32,
                        usage: ints[2] as u32,
                        usage_index: ints[3] as u32,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => return Err(Error::Decode(format!("{}: member \"Elements\" holds {other:?}", decoded.ty))),
        };
        let data = match decoded.member("data")? {
            MemberValue::Ints(v) => v.iter().map(|&v| v as u32).collect(),
            other => return Err(Error::Decode(format!("{}: member \"data\" holds {other:?}", decoded.ty))),
        };
        Ok(DeclData { elements, data })
    }

    /// The stride in DWORDs when every element's type is listed, else `None`.
    pub fn stride_dwords(&self) -> Option<usize> {
        self.elements.iter().map(|e| e.width_dwords()).sum::<Option<usize>>()
    }
}

/// A typed `FVFData`: BTB's extra UV sets with their raw DWORD payload. The
/// `code` is a Direct3D FVF code: 258 (`0x102`) files hold one further UV set
/// per vertex, 514 (`0x202`) files two.
#[derive(Debug, Clone, PartialEq)]
pub struct FvfData {
    pub code: u32,
    pub data: Vec<u32>,
}

impl FvfData {
    pub fn decode(object: &Object, file: &File) -> Result<FvfData, Error> {
        let decoded = decode_object(object, file)?;
        let int = |name: &str| -> Result<i32, Error> {
            match decoded.member(name)? {
                MemberValue::Ints(v) if v.len() == 1 => Ok(v[0]),
                other => Err(Error::Decode(format!("{}: member {name:?} holds {other:?}", decoded.ty))),
            }
        };
        let data = match decoded.member("data")? {
            MemberValue::Ints(v) => v.iter().map(|&v| v as u32).collect(),
            other => return Err(Error::Decode(format!("{}: member \"data\" holds {other:?}", decoded.ty))),
        };
        Ok(FvfData { code: int("dwFVF")? as u32, data })
    }

    /// The FVF texture-coordinate count (`(code & 0xF00) >> 8`).
    pub fn uv_set_count(&self) -> usize {
        ((self.code & 0xf00) >> 8) as usize
    }
}

/// A typed `Frame`: an optional instance name, the `FrameTransformMatrix`
/// child, nested frames and the meshes directly under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub name: Option<String>,
    pub matrix: Option<[f32; 16]>,
    pub meshes: Vec<Object>,
    pub frames: Vec<Frame>,
}

impl Frame {
    pub fn decode(object: &Object, file: &File) -> Result<Frame, Error> {
        let mut frame = Frame { name: object.name.clone(), matrix: None, meshes: Vec::new(), frames: Vec::new() };
        for child in &object.children {
            let Node::Object(child) = child else { continue };
            match child.ty.as_str() {
                "Frame" => frame.frames.push(Frame::decode(child, file)?),
                "FrameTransformMatrix" => frame.matrix = Some(frame_matrix(child, file)?),
                "Mesh" => frame.meshes.push(child.clone()),
                _ => {}
            }
        }
        Ok(frame)
    }

    /// Every mesh under this frame with its composed matrix: each frame's
    /// matrix applies after (outside) its children's, in row-major Direct3D
    /// order (`p' = p * child * parent`). Frames without a matrix contribute
    /// the identity.
    pub fn meshes_with_matrices(&self) -> Vec<([f32; 16], &Object)> {
        let mut out = Vec::new();
        self.append_meshes(&identity(), &mut out);
        out
    }

    fn append_meshes<'a>(&'a self, parent: &[f32; 16], out: &mut Vec<([f32; 16], &'a Object)>) {
        let here = match &self.matrix {
            Some(own) => concat_row_major(own, parent),
            None => *parent,
        };
        out.extend(self.meshes.iter().map(|mesh| (here, mesh)));
        for frame in &self.frames {
            frame.append_meshes(&here, out);
        }
    }
}

fn frame_matrix(object: &Object, file: &File) -> Result<[f32; 16], Error> {
    let decoded = decode_object(object, file)?;
    match decoded.member("frameMatrix")? {
        MemberValue::Floats(v) if v.len() == 16 => {
            let mut m = [0.0f32; 16];
            for (slot, value) in m.iter_mut().zip(v) {
                *slot = *value as f32;
            }
            Ok(m)
        }
        other => Err(Error::Decode(format!("{}: member \"frameMatrix\" holds {other:?}", decoded.ty))),
    }
}

fn identity() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

/// Row-major 4x4 product applying `first` before `second`
/// (`p' = p * first * second`), the Direct3D convention.
pub fn concat_row_major(first: &[f32; 16], second: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for row in 0..4 {
        for col in 0..4 {
            out[4 * row + col] = first[4 * row] * second[col]
                + first[4 * row + 1] * second[4 + col]
                + first[4 * row + 2] * second[8 + col]
                + first[4 * row + 3] * second[12 + col];
        }
    }
    out
}
