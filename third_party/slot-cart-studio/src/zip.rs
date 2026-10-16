//! A store-only zip. Labels are PNGs, already compressed, so deflate would only cost time. It
//! writes just enough of the format for `unzip` and each OS's archive tool: local headers, a
//! central directory and its end record. Names are flagged UTF-8, since a stem can hold `é`.
//! There is no Zip64: a card's labels are far short of 4 GB and 65,535 files.

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const VERSION: u16 = 20;
const UTF8: u16 = 1 << 11;
/// 1980-01-01, the earliest date the format has. A real one would need a clock, and nothing
/// that unzips labels reads it.
const DOS_DATE: u16 = (1 << 5) | 1;

struct Entry {
    name: Vec<u8>,
    crc: u32,
    size: u32,
    offset: u32,
}

#[derive(Default)]
pub struct Zip {
    body: Vec<u8>,
    /// How much of the archive `take` has handed out. Offsets still count from its first byte.
    taken: u32,
    entries: Vec<Entry>,
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

impl Zip {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: &str, data: &[u8]) {
        let entry = Entry {
            name: name.as_bytes().to_vec(),
            crc: crc32fast::hash(data),
            size: data.len() as u32,
            offset: self.taken + self.body.len() as u32,
        };
        let b = &mut self.body;
        put32(b, LOCAL);
        put16(b, VERSION);
        put16(b, UTF8);
        put16(b, 0); // stored
        put16(b, 0); // time
        put16(b, DOS_DATE);
        put32(b, entry.crc);
        put32(b, entry.size);
        put32(b, entry.size);
        put16(b, entry.name.len() as u16);
        put16(b, 0); // extra
        b.extend_from_slice(&entry.name);
        b.extend_from_slice(data);
        self.entries.push(entry);
    }

    /// The archive written since the last take, leaving only the directory behind. In WASM a
    /// card's worth of labels would otherwise sit in memory that never shrinks, so the page takes
    /// each entry as it goes, and what `finish` returns follows the last of them.
    pub fn take(&mut self) -> Vec<u8> {
        let out = std::mem::take(&mut self.body);
        self.taken += out.len() as u32;
        out
    }

    pub fn finish(self) -> Vec<u8> {
        let mut out = self.body;
        let body = out.len();
        let start = self.taken + body as u32;
        for e in &self.entries {
            put32(&mut out, CENTRAL);
            put16(&mut out, VERSION); // made by
            put16(&mut out, VERSION); // needed
            put16(&mut out, UTF8);
            put16(&mut out, 0); // stored
            put16(&mut out, 0); // time
            put16(&mut out, DOS_DATE);
            put32(&mut out, e.crc);
            put32(&mut out, e.size);
            put32(&mut out, e.size);
            put16(&mut out, e.name.len() as u16);
            put16(&mut out, 0); // extra
            put16(&mut out, 0); // comment
            put16(&mut out, 0); // disk
            put16(&mut out, 0); // internal attributes
            put32(&mut out, 0); // external attributes
            put32(&mut out, e.offset);
            out.extend_from_slice(&e.name);
        }
        let size = (out.len() - body) as u32;
        let count = self.entries.len() as u16;
        put32(&mut out, END);
        put16(&mut out, 0); // this disk
        put16(&mut out, 0); // directory's disk
        put16(&mut out, count);
        put16(&mut out, count);
        put32(&mut out, size);
        put32(&mut out, start);
        put16(&mut out, 0); // comment
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: u32 = 0x0403_4b50;
    const CENTRAL: u32 = 0x0201_4b50;
    const END: u32 = 0x0605_4b50;
    const UTF8: u16 = 1 << 11;

    const FILES: [(&str, &[u8]); 3] = [
        ("Labels/Metroid Fusion (USA).png", b"one"),
        ("Labels/Pokémon Émeraude.png", &[7u8; 1000]),
        ("Labels/Empty.png", b""),
    ];

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
    }

    #[test]
    fn the_directory_agrees_with_every_entry() {
        let mut zip = Zip::new();
        for (name, data) in FILES {
            zip.add(name, data);
        }
        let z = zip.finish();

        let end = z.len() - 22;
        assert_eq!(u32_at(&z, end), END);
        assert_eq!(u16_at(&z, end + 10) as usize, FILES.len());
        let size = u32_at(&z, end + 12) as usize;
        let mut at = u32_at(&z, end + 16) as usize;
        assert_eq!(at + size, end, "the directory runs up to the end record");

        for (name, data) in FILES {
            assert_eq!(u32_at(&z, at), CENTRAL);
            assert_eq!(u16_at(&z, at + 8) & UTF8, UTF8, "names are flagged UTF-8");
            assert_eq!(u16_at(&z, at + 10), 0, "stored, not deflated");
            let crc = u32_at(&z, at + 16);
            let len = u32_at(&z, at + 24) as usize;
            let name_len = u16_at(&z, at + 28) as usize;
            let local = u32_at(&z, at + 42) as usize;
            assert_eq!(&z[at + 46..at + 46 + name_len], name.as_bytes());
            assert_eq!((crc, len), (crc32fast::hash(data), data.len()));

            assert_eq!(u32_at(&z, local), LOCAL);
            assert_eq!(u32_at(&z, local + 14), crc);
            let body =
                local + 30 + u16_at(&z, local + 26) as usize + u16_at(&z, local + 28) as usize;
            assert_eq!(&z[body..body + len], data);

            at += 46 + name_len;
        }
    }

    #[test]
    fn an_archive_taken_as_it_grows_is_the_same_archive() {
        let mut whole = Zip::new();
        let mut pieces = Zip::new();
        let mut taken = pieces.take();
        for (name, data) in FILES {
            whole.add(name, data);
            pieces.add(name, data);
            taken.extend(pieces.take());
        }
        taken.extend(pieces.finish());
        assert!(taken == whole.finish());
    }
}
