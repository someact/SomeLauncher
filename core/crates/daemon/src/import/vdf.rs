//! Valve's text KeyValues (`.vdf`, `.acf`): quoted or bare tokens, `{ }`
//! blocks, `//` comments. Keys are matched ignoring case, as Steam writes
//! both `apps` and `Apps`. Conditionals (`[$WIN32]`) are skipped.
//! Also binary KeyValues ([`parse_binary`]), Steam's `appcache/stats` files.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(entries) => entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }
    /// Follows a path of keys: `v.at(&["AppState", "name"])`.
    pub fn at(&self, path: &[&str]) -> Option<&Vdf> {
        path.iter().try_fold(self, |v, k| v.get(k))
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }
    pub fn str_at(&self, path: &[&str]) -> Option<&str> {
        self.at(path).and_then(Vdf::str)
    }
    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(e) => e,
            Vdf::Str(_) => &[],
        }
    }
}

pub fn parse(text: &str) -> Result<Vdf> {
    let mut tokens = Tokens { s: text.as_bytes(), i: 0 };
    let root = block(&mut tokens, false)?;
    Ok(root)
}

/// Binary KeyValues: a type byte, a NUL-ended key, then the value (`0` a
/// block up to `8`, `1` a string, `2` an i32, `3` an f32, `7` a u64).
/// Numbers come back as their text, like the text format.
pub fn parse_binary(b: &[u8]) -> Result<Vdf> {
    let mut i = 0;
    let root = binary_block(b, &mut i, 0)?;
    Ok(root)
}

fn binary_block(b: &[u8], i: &mut usize, depth: usize) -> Result<Vdf> {
    if depth > 64 {
        bail!("binary KeyValues nested too deep");
    }
    let cstr = |i: &mut usize| -> Result<String> {
        let end = b[*i..].iter().position(|&c| c == 0).ok_or_else(|| anyhow::anyhow!("unterminated string at {}", *i))?;
        let s = String::from_utf8_lossy(&b[*i..*i + end]).into_owned();
        *i += end + 1;
        Ok(s)
    };
    let take = |i: &mut usize, n: usize| -> Result<&[u8]> {
        let s = b.get(*i..*i + n).ok_or_else(|| anyhow::anyhow!("cut short at {}", *i))?;
        *i += n;
        Ok(s)
    };
    let mut entries = Vec::new();
    while *i < b.len() {
        let t = b[*i];
        *i += 1;
        if t == 8 {
            return Ok(Vdf::Map(entries));
        }
        let key = cstr(i)?;
        let value = match t {
            0 => binary_block(b, i, depth + 1)?,
            1 => Vdf::Str(cstr(i)?),
            2 => Vdf::Str(i32::from_le_bytes(take(i, 4)?.try_into()?).to_string()),
            3 => Vdf::Str(f32::from_le_bytes(take(i, 4)?.try_into()?).to_string()),
            7 => Vdf::Str(u64::from_le_bytes(take(i, 8)?.try_into()?).to_string()),
            _ => bail!("unknown binary KeyValues type {t} at {}", *i - 1),
        };
        entries.push((key, value));
    }
    Ok(Vdf::Map(entries))
}

enum Tok {
    Str(String),
    Open,
    Close,
}

struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl Tokens<'_> {
    fn next(&mut self) -> Result<Option<Tok>> {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"//") {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
        let Some(&c) = self.s.get(self.i) else { return Ok(None) };
        self.i += 1;
        Ok(Some(match c {
            b'{' => Tok::Open,
            b'}' => Tok::Close,
            b'"' => {
                let mut out = Vec::new();
                loop {
                    let Some(&c) = self.s.get(self.i) else { bail!("unterminated string") };
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let e = self.s.get(self.i).copied().unwrap_or(b'\\');
                            self.i += 1;
                            out.push(match e {
                                b'n' => b'\n',
                                b't' => b'\t',
                                other => other,
                            });
                        }
                        c => out.push(c),
                    }
                }
                Tok::Str(String::from_utf8_lossy(&out).into_owned())
            }
            _ => {
                let start = self.i - 1;
                while self.i < self.s.len() && !self.s[self.i].is_ascii_whitespace() && !b"{}\"".contains(&self.s[self.i]) {
                    self.i += 1;
                }
                Tok::Str(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
            }
        }))
    }
}

fn block(t: &mut Tokens, nested: bool) -> Result<Vdf> {
    let mut entries = Vec::new();
    loop {
        let key = match t.next()? {
            Some(Tok::Str(k)) => k,
            Some(Tok::Close) if nested => return Ok(Vdf::Map(entries)),
            None if !nested => return Ok(Vdf::Map(entries)),
            Some(Tok::Close) => bail!("unexpected }}"),
            Some(Tok::Open) => bail!("unexpected {{"),
            None => bail!("unexpected end of file"),
        };
        let value = match t.next()? {
            Some(Tok::Str(v)) => Vdf::Str(v),
            Some(Tok::Open) => block(t, true)?,
            _ => bail!("no value for {key:?}"),
        };
        // A `[$CONDITION]` after the value applies only to some platforms; we take the value as is
        if let Some(rest) = t.s.get(t.i..) {
            let trimmed = rest.iter().position(|c| !matches!(c, b' ' | b'\t')).unwrap_or(rest.len());
            if rest.get(trimmed) == Some(&b'[')
                && let Some(end) = rest[trimmed..].iter().position(|&c| c == b']')
            {
                t.i += trimmed + end + 1;
            }
        }
        entries.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_keyvalues() {
        let mut b = vec![0u8];
        b.extend(b"stats\0");
        b.extend([1u8]);
        b.extend(b"name\0Venti\0");
        b.extend([2u8]);
        b.extend(b"data\0");
        b.extend(4095i32.to_le_bytes());
        b.extend([8u8, 8u8]);
        let v = parse_binary(&b).unwrap();
        assert_eq!(v.str_at(&["stats", "name"]), Some("Venti"));
        assert_eq!(v.str_at(&["stats", "data"]), Some("4095"));
        assert!(parse_binary(&[1, b'x']).is_err());
        assert!(parse_binary(&[9, b'x', 0]).is_err());
    }

    #[test]
    fn parses_nested_blocks_comments_and_escapes() {
        let v = parse(
            r#"
            // header
            "libraryfolders"
            {
                "0" { "path" "/home/u/.local/share/Steam"  "apps" { "480" "123" } }
                "1"
                {
                    "Path"		"/games/Steam\\Library"
                    "label" "say \"hi\""
                }
                bare value [$LINUX]
            }"#,
        )
        .unwrap();
        assert_eq!(v.str_at(&["libraryfolders", "0", "path"]), Some("/home/u/.local/share/Steam"));
        assert_eq!(v.str_at(&["LibraryFolders", "1", "path"]), Some("/games/Steam\\Library"));
        assert_eq!(v.str_at(&["libraryfolders", "1", "label"]), Some("say \"hi\""));
        assert_eq!(v.at(&["libraryfolders", "0", "apps"]).unwrap().entries().len(), 1);
        assert_eq!(v.str_at(&["libraryfolders", "bare"]), Some("value"));
    }

    #[test]
    fn rejects_broken_files() {
        assert!(parse(r#""a" { "b" "c" "#).is_err());
        assert!(parse(r#""a" "b" }"#).is_err());
    }
}
