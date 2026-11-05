//! Read the `VS_VERSIONINFO` file version out of a Windows PE executable.
//!
//! Used to report the Skyrim Special Edition build (e.g. 1.6.1170) and the
//! installed SkyrimTogether.exe version without any external tooling. Returns
//! `None` gracefully for anything unexpected — never fatal.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub revision: u16,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.major, self.minor, self.build, self.revision)
    }
}

struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn u16(&self, off: usize) -> Option<u16> {
        self.data.get(off..off + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&self, off: usize) -> Option<u32> {
        self.data.get(off..off + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn slice(&self, off: usize, len: usize) -> Option<&'a [u8]> {
        self.data.get(off..off + len)
    }
}

pub fn file_version(path: &std::path::Path) -> Option<Version> {
    let data = std::fs::read(path).ok()?;
    pe_version_from_bytes(&data)
}

pub fn pe_version_from_bytes(data: &[u8]) -> Option<Version> {
    let r = Reader { data };
    // DOS header
    if r.slice(0, 2)? != b"MZ" {
        return None;
    }
    let pe_off = r.u32(0x3C)? as usize;
    // PE signature
    if r.slice(pe_off, 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe_off + 4;
    let num_sections = r.u16(coff + 2)? as usize;
    let opt_size = r.u16(coff + 16)? as usize;
    let opt = coff + 20;
    let magic = r.u16(opt)?;
    let is_pe32plus = magic == 0x20b;
    if !is_pe32plus && magic != 0x10b {
        return None;
    }
    // Data directories: PE32 at +96, PE32+ at +112 after the optional header start.
    let dd_off = opt + if is_pe32plus { 112 } else { 96 };
    let res_rva = r.u32(dd_off + 8 * 2)? as usize;
    if res_rva == 0 {
        return None;
    }
    let num_dirs = r.u32(dd_off - 4)? as usize;
    if num_dirs < 3 {
        return None;
    }
    // Section table
    let sect = opt + opt_size;
    let rva_to_off = |rva: u32| -> Option<usize> {
        for i in 0..num_sections {
            let s = sect + i * 40;
            let virt_size = r.u32(s + 8)?;
            let virt_addr = r.u32(s + 12)?;
            let raw_size = r.u32(s + 16)?;
            let raw_ptr = r.u32(s + 20)?;
            if rva >= virt_addr && rva < virt_addr + virt_size.max(raw_size).max(1) {
                return Some((rva - virt_addr + raw_ptr) as usize);
            }
        }
        None
    };

    let res_off = rva_to_off(res_rva as u32)?;
    // Walk: root -> RT_VERSION (16) -> id 1 -> language leaf.
    // NB: every directory entry's offset is relative to the resource section
    // root, not to the containing directory.
    let type_dir = dir_entry(&r, res_off, res_off, 16)?;
    let id_dir = dir_entry(&r, type_dir, res_off, 1)?;
    // First language entry points at a DATA ENTRY (also root-relative).
    let num_named = r.u16(id_dir + 12)? as usize;
    let num_id = r.u16(id_dir + 14)? as usize;
    if num_id + num_named == 0 {
        return None;
    }
    let entry = id_dir + 16;
    let entry_off = r.u32(entry + 4)?;
    let data_entry = res_off + (entry_off & 0x7FFF_FFFF) as usize;
    let data_rva = r.u32(data_entry)?;
    let data_off = rva_to_off(data_rva)?;

    // VS_VERSIONINFO
    let mut p = data_off;
    let _w_length = r.u16(p)?;
    let w_value_length = r.u16(p + 2)? as usize;
    let _w_type = r.u16(p + 4)?;
    // Skip key ("VS_VERSION_INFO", UTF-16LE, NUL-terminated), then align to 4.
    p += 6;
    while r.u16(p).unwrap_or(1) != 0 {
        p += 2;
    }
    p += 2;
    p = (p + 3) & !3;
    // VS_FIXEDFILEINFO
    if w_value_length < 52 {
        return None;
    }
    if r.u32(p)? != 0xFEEF_04BD {
        return None;
    }
    let ms = r.u32(p + 8)?;
    let ls = r.u32(p + 12)?;
    Some(Version {
        major: (ms >> 16) as u16,
        minor: (ms & 0xFFFF) as u16,
        build: (ls >> 16) as u16,
        revision: (ls & 0xFFFF) as u16,
    })
}

/// Return the offset (relative to the resource root) of the child entry of a
/// resource directory matching `id`.
fn dir_entry(r: &Reader, dir_off: usize, res_root: usize, id: u16) -> Option<usize> {
    let num_named = r.u16(dir_off + 12)? as usize;
    let num_id = r.u16(dir_off + 14)? as usize;
    for i in 0..(num_named + num_id) {
        let e = dir_off + 16 + i * 16;
        let name_or_id = r.u32(e)?;
        let offset = r.u32(e + 4)?;
        if name_or_id & 0x8000_0000 == 0 && (name_or_id as u16) == id {
            return Some(res_root + (offset & 0x7FFF_FFFF) as usize);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert!(pe_version_from_bytes(b"not a pe file at all").is_none());
        assert!(pe_version_from_bytes(&[]).is_none());
    }

    /// Build a minimal PE32+ image with a real VS_VERSIONINFO resource tree
    /// (root -> RT_VERSION -> id 1 -> language leaf), exercising the
    /// root-relative offset rules against the SkyrimSE.exe layout.
    #[test]
    fn parses_synthetic_pe_version() {
        let mut pe = vec![0u8; 0x800];
        // DOS header
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        // PE signature + COFF (PE32+, 1 section)
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        pe[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes()); // x64
        pe[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());      // sections
        pe[0x54..0x56].copy_from_slice(&240u16.to_le_bytes());    // opt size
        let opt = 0x58usize;
        pe[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        pe[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes()); // dirs
        // data dir 2 = resource: RVA 0x1000, size 0x400
        pe[opt + 112 + 16..opt + 112 + 24].copy_from_slice(&[0x00u8, 0x10, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00]);
        // section .rsrc: vaddr 0x1000 -> raw 0x400
        let sect = opt + 240;
        pe[sect..sect + 8].copy_from_slice(b".rsrc\0\0\0");
        pe[sect + 8..sect + 12].copy_from_slice(&0x400u32.to_le_bytes());  // vsize
        pe[sect + 12..sect + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // vaddr
        pe[sect + 16..sect + 20].copy_from_slice(&0x400u32.to_le_bytes());  // rsize
        pe[sect + 20..sect + 24].copy_from_slice(&0x400u32.to_le_bytes());  // rptr

        let res = 0x400usize; // file offset of resource root (RVA 0x1000)
        // root dir: 1 entry, id 16 -> subdir at res+0x40
        pe[res + 12..res + 14].copy_from_slice(&1u16.to_le_bytes()); // num_id
        pe[res + 16..res + 20].copy_from_slice(&16u32.to_le_bytes());
        pe[res + 20..res + 24].copy_from_slice(&(0x8000_0000u32 | 0x40).to_le_bytes());
        // subdir 1 at res+0x40: id 1 -> subdir at res+0x80
        pe[res + 0x40 + 12..res + 0x40 + 14].copy_from_slice(&1u16.to_le_bytes());
        pe[res + 0x40 + 16..res + 0x40 + 20].copy_from_slice(&1u32.to_le_bytes());
        pe[res + 0x40 + 20..res + 0x40 + 24].copy_from_slice(&(0x8000_0000u32 | 0x80).to_le_bytes());
        // subdir 2 at res+0x80: lang 0x409 -> DATA ENTRY at res+0x90 (no high bit)
        pe[res + 0x80 + 12..res + 0x80 + 14].copy_from_slice(&1u16.to_le_bytes());
        pe[res + 0x80 + 16..res + 0x80 + 20].copy_from_slice(&0x409u32.to_le_bytes());
        pe[res + 0x80 + 20..res + 0x80 + 24].copy_from_slice(&0xA0u32.to_le_bytes());
        // data entry at res+0xA0: DataRVA 0x1100 (file 0x500), size 0x80
        pe[res + 0xA0..res + 0xA4].copy_from_slice(&0x1100u32.to_le_bytes());
        pe[res + 0xA4..res + 0xA8].copy_from_slice(&0x80u32.to_le_bytes());

        // VS_VERSIONINFO at file 0x500 (RVA 0x1100): version 1.7.104.0
        let v = 0x500usize;
        pe[v..v + 2].copy_from_slice(&0x0C4u16.to_le_bytes()); // wLength
        pe[v + 2..v + 4].copy_from_slice(&52u16.to_le_bytes()); // wValueLength
        pe[v + 4..v + 6].copy_from_slice(&0u16.to_le_bytes()); // wType
        let key: Vec<u8> = "VS_VERSION_INFO\0".encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        pe[v + 6..v + 6 + key.len()].copy_from_slice(&key);
        let fixed = v + 6 + key.len();
        let fixed = (fixed + 3) & !3;
        pe[fixed..fixed + 4].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
        pe[fixed + 4..fixed + 8].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        pe[fixed + 8..fixed + 12].copy_from_slice(&((1u32 << 16) | 7).to_le_bytes());
        pe[fixed + 12..fixed + 16].copy_from_slice(&(104u32 << 16).to_le_bytes());

        let ver = pe_version_from_bytes(&pe).expect("should parse synthetic PE");
        assert_eq!(ver.major, 1);
        assert_eq!(ver.minor, 7);
        assert_eq!(ver.build, 104);
        assert_eq!(ver.revision, 0);
        assert_eq!(ver.to_string(), "1.7.104.0");
    }
}
