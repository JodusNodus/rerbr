//! The MessagePack subset BeamNG's `.cdae` files use (<https://github.com/msgpack/msgpack/blob/master/spec.md>): a
//! stream of values read one at a time, as the shape reader asks for them.

/// One MessagePack value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match *self {
            Value::Int(v) => Some(v),
            Value::Float(v) if v.fract() == 0.0 => Some(v as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match *self {
            Value::Int(v) => Some(v as f64),
            Value::Float(v) => Some(v),
            _ => None,
        }
    }

    /// The value of `key` in a map with string keys.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| matches!(k, Value::Str(s) if s == key)).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// A cursor over a MessagePack byte stream.
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| format!("msgpack: {n} bytes past the end at {}", self.pos))?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn be(&mut self, n: usize) -> Result<u64, String> {
        Ok(self.take(n)?.iter().fold(0u64, |acc, &b| acc << 8 | u64::from(b)))
    }

    /// The next value, whatever its type.
    pub fn value(&mut self) -> Result<Value, String> {
        let at = self.pos;
        let t = self.take(1)?[0];
        Ok(match t {
            0x00..=0x7f => Value::Int(i64::from(t)),
            0xe0..=0xff => Value::Int(i64::from(t as i8)),
            0x80..=0x8f => self.map(usize::from(t & 0x0f))?,
            0x90..=0x9f => self.array(usize::from(t & 0x0f))?,
            0xa0..=0xbf => self.string(usize::from(t & 0x1f))?,
            0xc0 => Value::Nil,
            0xc2 => Value::Bool(false),
            0xc3 => Value::Bool(true),
            0xc4..=0xc6 => {
                let n = self.be(1 << (t - 0xc4))? as usize;
                Value::Bin(self.take(n)?.to_vec())
            }
            0xca => Value::Float(f64::from(f32::from_bits(self.be(4)? as u32))),
            0xcb => Value::Float(f64::from_bits(self.be(8)?)),
            0xcc..=0xcf => {
                let v = self.be(1 << (t - 0xcc))?;
                Value::Int(i64::try_from(v).map_err(|_| format!("msgpack: u64 {v} at {at}"))?)
            }
            0xd0..=0xd3 => {
                let n = 1usize << (t - 0xd0);
                let v = self.be(n)?;
                let shift = 64 - 8 * n as u32;
                Value::Int(((v << shift) as i64) >> shift)
            }
            0xd9..=0xdb => {
                let n = self.be(1 << (t - 0xd9))? as usize;
                self.string(n)?
            }
            0xdc | 0xdd => {
                let n = self.be(if t == 0xdc { 2 } else { 4 })? as usize;
                self.array(n)?
            }
            0xde | 0xdf => {
                let n = self.be(if t == 0xde { 2 } else { 4 })? as usize;
                self.map(n)?
            }
            _ => return Err(format!("msgpack: type byte {t:#04x} at {at}")),
        })
    }

    fn string(&mut self, n: usize) -> Result<Value, String> {
        Ok(Value::Str(String::from_utf8_lossy(self.take(n)?).into_owned()))
    }

    fn array(&mut self, n: usize) -> Result<Value, String> {
        (0..n).map(|_| self.value()).collect::<Result<_, _>>().map(Value::Array)
    }

    fn map(&mut self, n: usize) -> Result<Value, String> {
        (0..n).map(|_| Ok((self.value()?, self.value()?))).collect::<Result<_, String>>().map(Value::Map)
    }

    pub fn int(&mut self, what: &str) -> Result<i64, String> {
        let v = self.value()?;
        v.as_int().ok_or_else(|| format!("{what}: expected an integer, found {v:?}"))
    }

    pub fn float(&mut self, what: &str) -> Result<f32, String> {
        let v = self.value()?;
        v.as_float().map(|f| f as f32).ok_or_else(|| format!("{what}: expected a number, found {v:?}"))
    }

    pub fn string_value(&mut self, what: &str) -> Result<String, String> {
        match self.value()? {
            Value::Str(s) => Ok(s),
            v => Err(format!("{what}: expected a string, found {v:?}")),
        }
    }

    pub fn bin(&mut self, what: &str) -> Result<Vec<u8>, String> {
        match self.value()? {
            Value::Bin(b) => Ok(b),
            Value::Nil => Ok(Vec::new()),
            v => Err(format!("{what}: expected bytes, found {:?}", short(&v))),
        }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

fn short(v: &Value) -> String {
    let s = format!("{v:?}");
    s.chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_scalar_and_container_types() {
        let bytes = [
            0x05, 0xff, 0xcd, 0x01, 0x00, 0xd1, 0xff, 0x00, 0xca, 0x3f, 0x80, 0, 0, 0xa2, b'h', b'i', 0xc4, 2, 9, 8,
            0x92, 0xc3, 0xc0, 0x81, 0xa1, b'k', 0x01,
        ];
        let mut r = Reader::new(&bytes);
        assert_eq!(r.int("a").unwrap(), 5);
        assert_eq!(r.int("b").unwrap(), -1);
        assert_eq!(r.int("c").unwrap(), 256);
        assert_eq!(r.int("d").unwrap(), -256);
        assert_eq!(r.float("e").unwrap(), 1.0);
        assert_eq!(r.string_value("f").unwrap(), "hi");
        assert_eq!(r.bin("g").unwrap(), vec![9, 8]);
        assert_eq!(r.value().unwrap(), Value::Array(vec![Value::Bool(true), Value::Nil]));
        assert_eq!(r.value().unwrap().get("k"), Some(&Value::Int(1)));
        assert!(r.at_end());
        assert!(r.value().is_err());
    }
}
