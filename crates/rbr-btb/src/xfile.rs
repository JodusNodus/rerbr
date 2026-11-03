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
//! top-level unnamed `Mesh` with triangles only. See `notes/btb.md`
//! "Meshes".

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
    /// The 16-byte header is missing or names an unknown version or form.
    BadHeader(String),
    /// A file the reader does not decode: text (`txt `), 64-bit floats
    /// (`0064`) or compressed (`bzip`, `tzip`). None occur in the install.
    Unsupported(String),
    /// A compressed file (`bzip` or `tzip`): reported, not decoded.
    Compressed(String),
    /// An unknown binary token number at the offset.
    BadToken {
        offset: usize,
        token: u16,
    },
    /// A well-formed token that fits nowhere (a separator at the top level,
    /// a name where a value was decoded, a `{ name }` reference, ...). The
    /// install's files hold no references or named objects.
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

/// A bounds failure reading the binary token stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Truncated {
    offset: usize,
    needed: usize,
    len: usize,
}

/// A read position in the binary token stream. Offsets are absolute; a read
/// past the end fails as [`Truncated`].
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn at(bytes: &'a [u8], offset: usize) -> Result<Self, Error> {
        if offset > bytes.len() {
            return Err(Error::Truncated { offset, needed: 0, len: bytes.len() });
        }
        Ok(Reader { bytes, pos: offset })
    }

    fn truncated(&self, needed: usize) -> Error {
        Error::Truncated { offset: self.pos, needed, len: self.bytes.len() }
    }

    fn pos(&self) -> usize {
        self.pos
    }

    fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let stop = self.pos.checked_add(n).filter(|&stop| stop <= self.bytes.len()).ok_or_else(|| self.truncated(n))?;
        let slice = self.bytes.get(self.pos..stop).ok_or_else(|| self.truncated(n))?;
        self.pos = stop;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { offset, needed, len } => {
                write!(f, "truncated at offset {offset}: need {needed} bytes but file is {len} bytes")
            }
            Error::BadHeader(message) | Error::Unexpected(message) | Error::Decode(message) => {
                write!(f, "{message}")
            }
            Error::UnknownTemplate(name) => write!(f, "unknown template {name:?}"),
            Error::Unsupported(what) => write!(f, "{what}: not decoded"),
            Error::Compressed(form) => write!(f, "compressed .x file ({form}): not decoded"),
            Error::BadToken { offset, token } => write!(f, "unknown binary token {token} at offset {offset}"),
        }
    }
}

impl std::error::Error for Error {}

/// A parsed file: its template declarations and top-level data objects.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub templates: Vec<Template>,
    pub roots: Vec<Object>,
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
}

/// A `template` declaration: the member layout of its data objects.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub name: String,
    /// The `<uuid>`, absent when the declaration has none.
    pub uuid: Option<[u8; 16]>,
    pub members: Vec<Member>,
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

/// A data object: its template name, optional instance name, member values in
/// file order (adjacent same-kind values share one binary list), and children.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub ty: String,
    pub name: Option<String>,
    pub values: Vec<Value>,
    pub children: Vec<Object>,
}

impl Object {
    fn collect_type<'a>(&'a self, ty: &str, out: &mut Vec<&'a Object>) {
        if self.ty == ty {
            out.push(self);
        }
        for child in &self.children {
            child.collect_type(ty, out);
        }
    }
}

/// One member value scalar.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f64),
    Str(String),
}

/// Reads a binary 32-bit `.x` file into its templates and data objects.
/// Text, 64-bit and compressed files report [`Error::Unsupported`].
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
    let toks = match &bytes[8..12] {
        b"bin " => {
            if &bytes[12..16] != b"0032" {
                if &bytes[12..16] == b"0064" {
                    return Err(Error::Unsupported("64-bit .x files".to_string()));
                }
                return Err(Error::BadHeader(format!(
                    "unknown float size {}",
                    String::from_utf8_lossy(&bytes[12..16])
                )));
            }
            binary_tokens(bytes)?
        }
        b"txt " => return Err(Error::Unsupported("text .x files".to_string())),
        form => {
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
            Tok::Name(_) => roots.push(build.object()?),
            other => return Err(Error::Unexpected(format!("{other:?} at the top level"))),
        }
    }
    Ok(File { templates, roots })
}

/// The binary token stream: names, strings, integers, integer and float
/// lists, GUIDs, member type keywords and separators.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Template,
    Name(String),
    Str(String),
    Int(i32),
    Ints(Vec<i32>),
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

fn binary_tokens(bytes: &[u8]) -> Result<Vec<Tok>, Error> {
    let mut r = Reader::at(bytes, 16)?;
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
                let raw =
                    r.take(count.checked_mul(4).ok_or(Error::BadToken { offset: at, token: TOKEN_FLOAT_LIST })?)?;
                toks.push(Tok::Floats(
                    raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64).collect(),
                ));
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
    // Angle brackets never occur in binary files (only text `<uuid>` uses
    // them), so the builder rejects them on sight.
    Ok(toks)
}

/// The tree builder: template declarations and data objects
/// (`Name [name] { ... }`).
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
        loop {
            match self.next()? {
                Tok::CBrace => break,
                Tok::OBracket => {
                    // The restriction block (`[...]` or `[Material <uuid>]`):
                    // consumed, not used.
                    let mut depth = 1;
                    while depth > 0 {
                        match self.next()? {
                            Tok::OBracket => depth += 1,
                            Tok::CBracket => depth -= 1,
                            _ => {}
                        }
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
        Ok(Template { name, uuid, members })
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
                Tok::Floats(v) => values.extend(v.into_iter().map(Value::Float)),
                Tok::Str(v) => values.push(Value::Str(v)),
                // Binary files terminate strings with `;`.
                Tok::Semi | Tok::Comma => {}
                Tok::Name(ty) => {
                    if self.eat(&Tok::OBrace) {
                        children.push(self.object_body(ty, None)?);
                    } else {
                        let name = self.name()?;
                        if !self.eat(&Tok::OBrace) {
                            return self.expect("'{' after the instance name");
                        }
                        children.push(self.object_body(ty, Some(name))?);
                    }
                }
                // The install's files hold no `{ name }` references.
                Tok::OBrace => return self.expect("a child object, not a reference"),
                _ => return self.expect("a value, a child object or '}'"),
            }
        }
        Ok(Object { ty, name, values, children })
    }
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
            materials: None,
            duplication: None,
            decl: Vec::new(),
            fvf: Vec::new(),
        };
        for child in &object.children {
            match child.ty.as_str() {
                "MeshNormals" => mesh.normals = Some(MeshNormals::decode(child, file)?),
                "MeshTextureCoords" => mesh.uv_sets.push(MeshTextureCoords::decode(child, file)?),
                "MeshMaterialList" => mesh.materials = Some(MeshMaterialList::decode(child, file)?),
                "VertexDuplicationIndices" => mesh.duplication = Some(VertexDuplicationIndices::decode(child, file)?),
                "DeclData" => mesh.decl.push(DeclData::decode(child, file)?),
                "FVFData" => mesh.fvf.push(FvfData::decode(child, file)?),
                _ => {}
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

/// A typed `MeshMaterialList` child: one material index per mesh face and
/// the single material BTB files carry.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshMaterialList {
    pub face_materials: Vec<u32>,
    pub materials: Vec<Material>,
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
            materials.push(Material::decode(child, file)?);
        }
        Ok(MeshMaterialList { face_materials, materials })
    }
}

/// A typed `Material`: face colour, power, specular and emissive colours
/// (all zero in BTB files) and the texture file name.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub face_color: [f32; 4],
    pub power: f32,
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub texture: Option<String>,
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
            texture = Some(TextureFilename::decode(child, file)?);
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
