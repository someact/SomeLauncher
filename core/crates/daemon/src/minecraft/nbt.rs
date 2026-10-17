//! Minecraft's NBT, read only: `level.dat` (gzip) and `servers.dat` (plain)
//! into JSON. Longs stay i64, byte/int/long arrays become lists of numbers.

use anyhow::{Result, bail};
use serde_json::{Map, Value};
use std::io::Read;

/// Deeper nesting than this is not a world file
const MAX_DEPTH: usize = 64;

/// A file's root compound, gunzipped first when it is gzip.
pub fn read(bytes: &[u8]) -> Result<Value> {
    let mut plain = Vec::new();
    let data = if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut plain)?;
        &plain[..]
    } else {
        bytes
    };
    let mut r = Reader { data, pos: 0 };
    if r.u8()? != 10 {
        bail!("not an NBT compound");
    }
    r.string()?;
    r.payload(10, 0)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let Some(end) = end else { bail!("NBT ends early") };
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 {
            bail!("negative NBT length");
        }
        Ok(n as usize)
    }
    /// Java's modified UTF-8; close enough to UTF-8 for names and addresses
    fn string(&mut self) -> Result<String> {
        let n = self.i16()? as u16 as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn payload(&mut self, tag: u8, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            bail!("NBT nested too deep");
        }
        Ok(match tag {
            1 => Value::from(self.u8()? as i8),
            2 => Value::from(self.i16()?),
            3 => Value::from(self.i32()?),
            4 => Value::from(self.i64()?),
            5 => Value::from(f32::from_be_bytes(self.take(4)?.try_into()?) as f64),
            6 => Value::from(f64::from_be_bytes(self.take(8)?.try_into()?)),
            7 => {
                let n = self.len()?;
                Value::from(self.take(n)?.iter().map(|&b| b as i8 as i64).collect::<Vec<_>>())
            }
            8 => Value::from(self.string()?),
            9 => {
                let inner = self.u8()?;
                let n = self.len()?;
                let mut list = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    list.push(self.payload(inner, depth + 1)?);
                }
                Value::Array(list)
            }
            10 => {
                let mut map = Map::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    map.insert(name, self.payload(t, depth + 1)?);
                }
                Value::Object(map)
            }
            11 => {
                let n = self.len()?;
                Value::from((0..n).map(|_| self.i32().map(i64::from)).collect::<Result<Vec<_>>>()?)
            }
            12 => {
                let n = self.len()?;
                Value::from((0..n).map(|_| self.i64()).collect::<Result<Vec<_>>>()?)
            }
            t => bail!("unknown NBT tag {t}"),
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    /// A tiny NBT writer for tests: (name, value) pairs into a root compound
    pub fn compound(fields: &[(&str, Field)]) -> Vec<u8> {
        let mut out = vec![10, 0, 0];
        write_fields(&mut out, fields);
        out
    }
    pub enum Field {
        Str(&'static str),
        Long(i64),
        Int(i32),
        Byte(i8),
        Compound(Vec<(&'static str, Field)>),
        List(Vec<Vec<(&'static str, Field)>>),
    }
    fn name(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u16).to_be_bytes());
        out.extend(s.as_bytes());
    }
    fn write_fields(out: &mut Vec<u8>, fields: &[(&str, Field)]) {
        for (n, f) in fields {
            let tag = match f {
                Field::Byte(_) => 1,
                Field::Int(_) => 3,
                Field::Long(_) => 4,
                Field::Str(_) => 8,
                Field::List(_) => 9,
                Field::Compound(_) => 10,
            };
            out.push(tag);
            name(out, n);
            match f {
                Field::Byte(b) => out.push(*b as u8),
                Field::Int(i) => out.extend(i.to_be_bytes()),
                Field::Long(l) => out.extend(l.to_be_bytes()),
                Field::Str(s) => name(out, s),
                Field::Compound(c) => write_fields(out, c),
                Field::List(items) => {
                    out.push(10);
                    out.extend((items.len() as i32).to_be_bytes());
                    for c in items {
                        write_fields(out, c);
                    }
                }
            }
        }
        out.push(0);
    }
    pub fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(bytes).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn reads_gzipped_and_plain_compounds() {
        let bytes = compound(&[(
            "Data",
            Field::Compound(vec![("LevelName", Field::Str("New World")), ("LastPlayed", Field::Long(1_788_192_932_419)), ("GameType", Field::Int(1)), ("hardcore", Field::Byte(0))]),
        )]);
        for b in [bytes.clone(), gzip(&bytes)] {
            let v = read(&b).unwrap();
            assert_eq!(v["Data"]["LevelName"], "New World");
            assert_eq!(v["Data"]["LastPlayed"], 1_788_192_932_419i64);
            assert_eq!(v["Data"]["GameType"], 1);
        }
        let servers = compound(&[("servers", Field::List(vec![vec![("name", Field::Str("Vibe")), ("ip", Field::Str("mc.example.org"))]]))]);
        assert_eq!(read(&servers).unwrap()["servers"][0]["ip"], "mc.example.org");
        assert!(read(&bytes[..bytes.len() - 3]).is_err(), "cut short");
        assert!(read(b"not nbt").is_err());
    }
}
