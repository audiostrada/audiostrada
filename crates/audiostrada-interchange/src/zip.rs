//! The ZIP container of a DAWproject: reading stored and deflated entries (ZIP64 sizes too),
//! writing stored ones (audio) and deflated ones (XML).

use std::io::{Read, Write};

use flate2::Crc;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

struct Entry {
    name: String,
    method: u16,
    compressed: u64,
    size: u64,
    header: u64,
}

pub struct Archive {
    data: Vec<u8>,
    entries: Vec<Entry>,
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

impl Archive {
    pub fn read(data: Vec<u8>) -> Result<Self, String> {
        let bad = || "not a ZIP file".to_owned();
        let end = (0..data.len().saturating_sub(21))
            .rev()
            .take(65_557)
            .find(|&i| u32_at(&data, i) == Some(0x0605_4b50))
            .ok_or_else(bad)?;
        let count = u16_at(&data, end + 10).ok_or_else(bad)? as usize;
        let mut at = u32_at(&data, end + 16).ok_or_else(bad)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(&data, at) != Some(0x0201_4b50) {
                return Err(bad());
            }
            let method = u16_at(&data, at + 10).ok_or_else(bad)?;
            let mut compressed = u64::from(u32_at(&data, at + 20).ok_or_else(bad)?);
            let mut size = u64::from(u32_at(&data, at + 24).ok_or_else(bad)?);
            let name_len = u16_at(&data, at + 28).ok_or_else(bad)? as usize;
            let extra_len = u16_at(&data, at + 30).ok_or_else(bad)? as usize;
            let comment_len = u16_at(&data, at + 32).ok_or_else(bad)? as usize;
            let mut header = u64::from(u32_at(&data, at + 42).ok_or_else(bad)?);
            let name = data.get(at + 46..at + 46 + name_len).ok_or_else(bad)?;
            let name = String::from_utf8_lossy(name).into_owned();
            // ZIP64: the sizes and offset that did not fit, in this order.
            let mut x = at + 46 + name_len;
            let extra_end = x + extra_len;
            while x + 4 <= extra_end {
                let id = u16_at(&data, x).ok_or_else(bad)?;
                let len = u16_at(&data, x + 2).ok_or_else(bad)? as usize;
                if id == 1 {
                    let mut f = x + 4;
                    for field in [&mut size, &mut compressed, &mut header] {
                        if *field == 0xFFFF_FFFF {
                            *field = u64_at(&data, f).ok_or_else(bad)?;
                            f += 8;
                        }
                    }
                }
                x += 4 + len;
            }
            entries.push(Entry {
                name,
                method,
                compressed,
                size,
                header,
            });
            at = extra_end + comment_len;
        }
        Ok(Self { data, entries })
    }

    #[cfg(test)]
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// The content of the entry `name` (a path inside the archive, `/`-separated).
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        let name = name.trim_start_matches("./");
        let e = self.entries.iter().find(|e| e.name == name)?;
        let h = usize::try_from(e.header).ok()?;
        if u32_at(&self.data, h)? != 0x0403_4b50 {
            return None;
        }
        let start =
            h + 30 + u16_at(&self.data, h + 26)? as usize + u16_at(&self.data, h + 28)? as usize;
        let raw = self
            .data
            .get(start..start + usize::try_from(e.compressed).ok()?)?;
        match e.method {
            0 => Some(raw.to_vec()),
            8 => {
                let mut out = Vec::with_capacity(usize::try_from(e.size).unwrap_or(0));
                DeflateDecoder::new(raw).read_to_end(&mut out).ok()?;
                Some(out)
            }
            _ => None,
        }
    }
}

/// Writes entries one after the other, then the directory.
pub struct Writer<W: Write> {
    out: W,
    written: u64,
    directory: Vec<u8>,
    count: u16,
}

impl<W: Write> Writer<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            written: 0,
            directory: Vec::new(),
            count: 0,
        }
    }

    /// Adds an entry, deflated when `compress`.
    pub fn add(&mut self, name: &str, data: &[u8], compress: bool) -> std::io::Result<()> {
        let mut crc = Crc::new();
        crc.update(data);
        let body = if compress {
            let mut e = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data)?;
            e.finish()?
        } else {
            data.to_vec()
        };
        let too_big = || std::io::Error::other("a file over 4 GB cannot go in the project");
        let size = u32::try_from(data.len()).map_err(|_| too_big())?;
        let packed = u32::try_from(body.len()).map_err(|_| too_big())?;
        let offset = u32::try_from(self.written).map_err(|_| too_big())?;
        let method: u16 = if compress { 8 } else { 0 };
        let name = name.as_bytes();
        let fields = |out: &mut Vec<u8>| {
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0x0800u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0x21u16.to_le_bytes());
            out.extend_from_slice(&crc.sum().to_le_bytes());
            out.extend_from_slice(&packed.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
        };
        let mut local = 0x0403_4b50u32.to_le_bytes().to_vec();
        fields(&mut local);
        local.extend_from_slice(name);
        self.out.write_all(&local)?;
        self.out.write_all(&body)?;
        self.written += (local.len() + body.len()) as u64;

        let d = &mut self.directory;
        d.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        d.extend_from_slice(&20u16.to_le_bytes());
        fields(d);
        d.extend_from_slice(&[0; 6]);
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&offset.to_le_bytes());
        d.extend_from_slice(name);
        self.count += 1;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<W> {
        let start = u32::try_from(self.written)
            .map_err(|_| std::io::Error::other("the project is over 4 GB"))?;
        self.out.write_all(&self.directory)?;
        let mut end = 0x0605_4b50u32.to_le_bytes().to_vec();
        end.extend_from_slice(&[0; 4]);
        end.extend_from_slice(&self.count.to_le_bytes());
        end.extend_from_slice(&self.count.to_le_bytes());
        end.extend_from_slice(&(self.directory.len() as u32).to_le_bytes());
        end.extend_from_slice(&start.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.out.write_all(&end)?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_written_read_back_stored_or_deflated() {
        let mut w = Writer::new(Vec::new());
        w.add("project.xml", b"<Project version=\"1.0\"/>", true)
            .unwrap();
        w.add("audio/a.wav", &[1, 2, 3, 4, 5], false).unwrap();
        let zip = Archive::read(w.finish().unwrap()).unwrap();
        assert_eq!(
            zip.names().collect::<Vec<_>>(),
            ["project.xml", "audio/a.wav"]
        );
        assert_eq!(
            zip.get("project.xml").unwrap(),
            b"<Project version=\"1.0\"/>"
        );
        assert_eq!(zip.get("audio/a.wav").unwrap(), [1, 2, 3, 4, 5]);
        assert!(zip.get("missing").is_none());
        assert!(Archive::read(b"hello".to_vec()).is_err());
    }
}
