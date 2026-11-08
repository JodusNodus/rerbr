//! BeamNG's JSON: standard JSON plus `//` and `/* */` comments, trailing commas and missing commas between members, as
//! its level files (`*.materials.json`, ground models, `map.json`) are written. `items.level.json` holds one object
//! per line ([`lines`]).

/// A JSON value. Objects keep their member order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(v) => Some(*v),
            Json::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// A boolean, also written as a number or `"0"`/`"1"`.
    pub fn flag(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            Json::Num(v) => Some(*v != 0.0),
            Json::Str(s) => match s.as_str() {
                "1" | "true" => Some(true),
                "0" | "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }

    pub fn members(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(members) => members,
            _ => &[],
        }
    }

    /// An array of numbers (`[x, y, z]`), or a string of them separated by spaces (`"1 0 0"`).
    pub fn nums(&self) -> Vec<f64> {
        match self {
            Json::Arr(items) => items.iter().filter_map(Json::num).collect(),
            Json::Str(s) => s.split_whitespace().filter_map(|t| t.parse().ok()).collect(),
            _ => Vec::new(),
        }
    }
}

/// Parses one document.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser { b: text.trim_start_matches('\u{feff}').as_bytes(), pos: 0 };
    let v = p.value()?;
    p.space();
    if p.pos < p.b.len() {
        return Err(format!("json: trailing text at byte {}", p.pos));
    }
    Ok(v)
}

/// The objects of a file with one per line (`items.level.json`); blank lines are skipped.
pub fn lines(text: &str) -> Result<Vec<Json>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| parse(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect()
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn space(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.pos += 1;
            }
            if self.b[self.pos.min(self.b.len())..].starts_with(b"//") {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.pos += 1;
                }
            } else if self.b[self.pos.min(self.b.len())..].starts_with(b"/*") {
                let rest = &self.b[self.pos + 2..];
                self.pos += 2 + rest.windows(2).position(|w| w == b"*/").map_or(rest.len(), |i| i + 2);
            } else {
                return;
            }
        }
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("json: {what} at byte {}", self.pos))
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(c) if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => self.number(),
            _ => self.fail("expected a value"),
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.b[self.pos..].starts_with(w.as_bytes()) {
            self.pos += w.len();
            Ok(v)
        } else {
            self.fail("bad literal")
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(&c)) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.b[start..self.pos]).unwrap_or("");
        text.trim_start_matches('+').parse().map(Json::Num).or_else(|_| self.fail("bad number"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("unterminated string") };
            self.pos += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let Some(e) = self.peek() else { return self.fail("unterminated escape") };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' | b'f' => {}
                        b'u' => {
                            let hex = self.b.get(self.pos..self.pos + 4).and_then(|h| std::str::from_utf8(h).ok());
                            let ch = hex.and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32);
                            self.pos += 4;
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.unwrap_or('\u{fffd}').encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            self.space();
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                Some(b',') => self.pos += 1,
                None => return self.fail("unterminated array"),
                _ => items.push(self.value()?),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.pos += 1;
        let mut members = Vec::new();
        loop {
            self.space();
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(members));
                }
                Some(b',') => self.pos += 1,
                Some(b'"') => {
                    let key = self.string()?;
                    self.space();
                    if self.peek() != Some(b':') && self.peek() != Some(b'=') {
                        return self.fail("expected ':'");
                    }
                    self.pos += 1;
                    members.push((key, self.value()?));
                }
                _ => return self.fail("expected a key"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_comments_and_trailing_commas() {
        let v = parse("{ \"a\": { // note\n \"b\": [1, 2,], /* x */ \"c\": \"d\" }, \"e\": -1.5e1, }").unwrap();
        let a = v.get("a").unwrap();
        assert_eq!(a.get("b").unwrap().nums(), vec![1.0, 2.0]);
        assert_eq!(a.get("c").unwrap().str(), Some("d"));
        assert_eq!(v.get("e").unwrap().num(), Some(-15.0));
        assert!(parse("[1, 2").is_err());
    }

    #[test]
    fn one_object_per_line() {
        let v = lines("{\"class\":\"TSStatic\"}\n\n{\"class\":\"DecalRoad\",\"nodes\":[[1,2,3,4]]}\n").unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].get("nodes").unwrap().arr()[0].nums(), vec![1.0, 2.0, 3.0, 4.0]);
    }
}
