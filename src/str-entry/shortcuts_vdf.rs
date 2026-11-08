//! Lossless reader/writer for Steam's binary `shortcuts.vdf` (the
//! non-Steam-game shortcut list).
//!
//! Grammar, verified against real files and the `steam_shortcuts_util` test
//! fixture (bundled at `tests/fixtures/shortcuts.vdf`):
//!
//! ```text
//! file    := 00 "shortcuts" 00 entry* 08 08
//! entry   := 00 <order> 00 line* 00 "tags" 00 tagline* 08 08
//! line    := 01 <key> 00 <text> 00          ; string field
//!          | 02 <key> 00 <4-byte LE int>    ; integer field
//! tagline := 01 <index> 00 <tag> 00
//! ```
//!
//! Strategy: entries are never re-serialised in place. The file's raw bytes
//! are kept and entries are edited by *byte-range replacement*, so unknown
//! keys, exotic encodings and Steam's own formatting survive untouched (the
//! `steam_shortcuts_util` crate re-serialises every entry and silently drops
//! fields like `compatToolName`).

/// `00 "shortcuts" 00`
const HEADER: &[u8] = b"\x00shortcuts\x00";
const END: u8 = 0x08;
const TEXT: u8 = 0x01;
const INT: u8 = 0x02;

/// The canonical field order Steam itself writes (from the fixture).
const FIELD_ORDER: [&str; 14] = [
    "appid",
    "AppName",
    "Exe",
    "StartDir",
    "icon",
    "ShortcutPath",
    "LaunchOptions",
    "IsHidden",
    "AllowDesktopConfig",
    "AllowOverlay",
    "openvr",
    "Devkit",
    "DevkitGameID",
    "LastPlayTime",
];

/// The values needed to (re)build one shortcut entry.
#[derive(Debug, Clone)]
pub struct ShortcutSpec {
    pub app_name: String,
    /// Stored exactly as it appears in the file — Steam wraps paths in quotes.
    pub exe: String,
    pub start_dir: String,
    pub icon: String,
}

impl ShortcutSpec {
    /// The app id Steam derives from the stored `exe` + `appname` strings.
    /// Both sides hash the verbatim stored strings, so they agree.
    pub fn app_id(&self) -> u32 {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(format!("{}{}", self.exe, self.app_name).as_bytes());
        hasher.finalize() | 0x8000_0000
    }
}

// ---------------------------------------------------------------------------
// Structural walk
// ---------------------------------------------------------------------------

/// Walk the file and record the byte range of every entry, validating the
/// grammar on the way. Errors name the offset so corruption is debuggable.
fn entry_spans(data: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    if data.len() < HEADER.len() || &data[..HEADER.len()] != HEADER {
        return Err("not a shortcuts.vdf file (bad header)".into());
    }
    let mut pos = HEADER.len();
    let mut spans = Vec::new();

    loop {
        match data.get(pos) {
            // Terminators: close the shortcuts map, then the legacy final one.
            Some(&END) => {
                let mut rest = &data[pos..];
                while rest.first() == Some(&END) {
                    rest = &rest[1..];
                }
                if !rest.is_empty() {
                    return Err(format!("trailing garbage after terminator at byte {pos}"));
                }
                return Ok(spans);
            }
            Some(0x00) => {
                let start = pos;
                pos = skip_entry(data, pos)?;
                spans.push((start, pos));
            }
            _ => return Err(format!("expected an entry or terminator at byte {pos}")),
        }
    }
}

/// `00 <order> 00 line* 00 "tags" 00 tagline* 08 08` — return offset after.
fn skip_entry(data: &[u8], mut pos: usize) -> Result<usize, String> {
    let at = |p: usize, what: &str| format!("corrupt shortcuts.vdf: expected {what} at byte {p}");

    pos += 1; // 0x00
    pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "entry order"))?; // order + NUL

    // Fields until the tags section opens.
    loop {
        match data.get(pos) {
            Some(&TEXT) => {
                pos += 1;
                pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "field key"))?;
                pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "field value"))?;
            }
            Some(&INT) => {
                pos += 1;
                pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "field key"))?;
                if pos + 4 > data.len() {
                    return Err(at(pos, "4-byte integer"));
                }
                pos += 4;
            }
            Some(0x00) => break, // `00 "tags" 00`
            _ => return Err(at(pos, "field or tags section")),
        }
    }

    pos += 1; // 0x00 before "tags"
    pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "tags header"))?;

    // Tag lines until the tags map closes.
    loop {
        match data.get(pos) {
            Some(&TEXT) => {
                pos += 1;
                pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "tag index"))?;
                pos = skip_cstr(data, pos).ok_or_else(|| at(pos, "tag name"))?;
            }
            Some(&END) => {
                pos += 1;
                break;
            }
            _ => return Err(at(pos, "tag or end of tags")),
        }
    }

    // Entry map terminator.
    if data.get(pos) != Some(&END) {
        return Err(at(pos, "end of entry"));
    }
    Ok(pos + 1)
}

/// Read a NUL-terminated string; returns the offset after the NUL.
fn skip_cstr(data: &[u8], pos: usize) -> Option<usize> {
    let end = pos + data[pos..].iter().position(|&b| b == 0)?;
    Some(end + 1)
}

fn read_cstr(data: &[u8], pos: usize) -> Option<(&str, usize)> {
    let end = pos + data[pos..].iter().position(|&b| b == 0)?;
    let text = std::str::from_utf8(&data[pos..end]).ok()?;
    Some((text, end + 1))
}

// ---------------------------------------------------------------------------
// Reading fields from one entry's bytes
// ---------------------------------------------------------------------------

fn field_bytes<'a>(entry: &'a [u8], wanted: &str) -> Option<&'a [u8]> {
    let mut pos = 1; // leading 0x00
    let (_, next) = read_cstr(entry, pos)?; // order
    pos = next;

    loop {
        match entry.get(pos) {
            Some(&TEXT) => {
                pos += 1;
                let (key, next) = read_cstr(entry, pos)?;
                pos = next;
                let start = pos;
                let (value, next) = read_cstr(entry, pos)?;
                pos = next;
                if key.eq_ignore_ascii_case(wanted) {
                    return Some(&entry[start..start + value.len()]);
                }
            }
            Some(&INT) => {
                pos += 1;
                let (key, next) = read_cstr(entry, pos)?;
                pos = next;
                let value = entry.get(pos..pos + 4)?;
                pos += 4;
                if key.eq_ignore_ascii_case(wanted) {
                    return Some(value);
                }
            }
            _ => return None, // tags section or end
        }
    }
}

pub fn entry_text<'a>(entry: &'a [u8], key: &str) -> Option<&'a str> {
    field_bytes(entry, key).and_then(|v| std::str::from_utf8(v).ok())
}

/// The entry's position label ("0", "1", …) — preserved on replacement.
pub fn entry_order(entry: &[u8]) -> &str {
    read_cstr(entry, 1).map(|(s, _)| s).unwrap_or("0")
}

pub fn entry_int(entry: &[u8], key: &str) -> Option<u32> {
    let bytes = field_bytes(entry, key)?;
    let word: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_le_bytes(word))
}

/// Does this entry look like the one we manage? Identity is the
/// (appname, exe) pair — or the derived app id — never the exe alone.
pub fn entry_is_ours(entry: &[u8], spec: &ShortcutSpec) -> bool {
    if entry_int(entry, "appid") == Some(spec.app_id()) {
        return true;
    }
    let name = entry_text(entry, "AppName").unwrap_or_default();
    let exe = entry_text(entry, "Exe").unwrap_or_default();
    name == spec.app_name && exe == spec.exe
}

/// A loose matcher for removal/preflight (used when no exact spec is at hand).
pub fn entry_matches(entry: &[u8], app_name: &str, exe_contains: &str) -> bool {
    let name = entry_text(entry, "AppName").unwrap_or_default();
    let exe = entry_text(entry, "Exe").unwrap_or_default();
    name == app_name || exe.contains(exe_contains)
}

// ---------------------------------------------------------------------------
// Serialising a fresh entry
// ---------------------------------------------------------------------------

fn push_cstr(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

fn push_text_field(out: &mut Vec<u8>, key: &str, value: &str) {
    out.push(TEXT);
    push_cstr(out, key);
    push_cstr(out, value);
}

fn push_int_field(out: &mut Vec<u8>, key: &str, value: u32) {
    out.push(INT);
    push_cstr(out, key);
    out.extend_from_slice(&value.to_le_bytes());
}

/// Serialise one complete entry (including its terminator bytes). `order` is
/// the position label Steam keeps per entry ("0", "1", …).
fn serialize_entry(order: &str, spec: &ShortcutSpec) -> Vec<u8> {
    let app_id = spec.app_id();
    let texts: Vec<(String, String)> = vec![
        ("AppName".into(), spec.app_name.clone()),
        ("Exe".into(), spec.exe.clone()),
        ("StartDir".into(), spec.start_dir.clone()),
        ("icon".into(), spec.icon.clone()),
        ("ShortcutPath".into(), String::new()),
        ("LaunchOptions".into(), String::new()),
        ("DevkitGameID".into(), String::new()),
    ];
    let ints: Vec<(String, u32)> = vec![
        ("appid".into(), app_id),
        ("IsHidden".into(), 0),
        ("AllowDesktopConfig".into(), 1),
        ("AllowOverlay".into(), 1),
        ("openvr".into(), 0),
        ("Devkit".into(), 0),
        ("LastPlayTime".into(), 0),
    ];

    let mut out = Vec::new();
    out.push(0x00);
    push_cstr(&mut out, order);
    // appid goes first, the rest in Steam's canonical order.
    push_int_field(&mut out, "appid", app_id);
    for key in FIELD_ORDER.iter().skip(1) {
        if let Some((_, value)) = texts.iter().find(|(k, _)| k == key) {
            push_text_field(&mut out, key, value);
        }
        if let Some((_, value)) = ints.iter().find(|(k, _)| k == key) {
            push_int_field(&mut out, key, *value);
        }
    }
    out.push(0x00);
    push_cstr(&mut out, "tags");
    out.push(END); // close tags map
    out.push(END); // close entry map
    out
}

/// Serialise a complete file that contains only `specs` (used when no
/// shortcuts.vdf exists yet).
pub fn serialize_file(specs: &[ShortcutSpec]) -> Vec<u8> {
    let mut out = HEADER.to_vec();
    for (i, spec) in specs.iter().enumerate() {
        out.extend_from_slice(&serialize_entry(&i.to_string(), spec));
    }
    out.push(END);
    out.push(END);
    out
}

// ---------------------------------------------------------------------------
// Lossless edit operations
// ---------------------------------------------------------------------------

/// Insert `spec` or replace the existing matching entry. Everything outside
/// the replaced byte range is copied verbatim (including the original entry's
/// order label, so Steam's ordering stays stable).
pub fn upsert(original: &[u8], spec: &ShortcutSpec) -> Result<Vec<u8>, String> {
    let spans = entry_spans(original)?;
    let mut out = original[..HEADER.len()].to_vec();
    let mut replaced = false;

    let mut cursor = HEADER.len();
    for &(start, end) in &spans {
        out.extend_from_slice(&original[cursor..start]);
        let entry = &original[start..end];
        if entry_is_ours(entry, spec) {
            let order = entry_order(entry).to_string();
            out.extend_from_slice(&serialize_entry(&order, spec));
            replaced = true;
        } else {
            out.extend_from_slice(entry);
        }
        cursor = end;
    }
    if !replaced {
        let order = spans.len().to_string();
        out.extend_from_slice(&serialize_entry(&order, spec));
    }
    out.extend_from_slice(&original[cursor..]);
    Ok(out)
}

/// Remove every entry matching `app_name` or whose exe contains
/// `exe_contains`. Returns the new bytes and the app ids of removed entries
/// (empty when nothing matched).
pub fn remove_matching(
    original: &[u8],
    app_name: &str,
    exe_contains: &str,
) -> Result<(Vec<u8>, Vec<u32>), String> {
    let spans = entry_spans(original)?;
    let mut removed_ids = Vec::new();
    let mut out = original[..HEADER.len()].to_vec();
    let mut cursor = HEADER.len();

    for (start, end) in spans {
        out.extend_from_slice(&original[cursor..start]);
        let entry = &original[start..end];
        if entry_matches(entry, app_name, exe_contains) {
            if let Some(id) = entry_int(entry, "appid") {
                removed_ids.push(id);
            }
        } else {
            out.extend_from_slice(entry);
        }
        cursor = end;
    }
    out.extend_from_slice(&original[cursor..]);
    Ok((out, removed_ids))
}

/// Look for an entry matching `app_name`/`exe_contains`; returns
/// `(app_name, exe, app_id)` when found.
pub fn find_matching(
    original: &[u8],
    app_name: &str,
    exe_contains: &str,
) -> Option<(String, String, u32)> {
    let spans = entry_spans(original).ok()?;
    spans
        .iter()
        .map(|&(start, end)| &original[start..end])
        .find(|entry| entry_matches(entry, app_name, exe_contains))
        .and_then(|entry| {
            Some((
                entry_text(entry, "AppName")?.to_string(),
                entry_text(entry, "Exe")?.to_string(),
                entry_int(entry, "appid").unwrap_or(0),
            ))
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/shortcuts.vdf").to_vec()
    }

    fn celeste() -> ShortcutSpec {
        ShortcutSpec {
            app_name: "Celeste".into(),
            exe: r#""C:\MySmallPrograms\epic_launcher.exe""#.into(),
            start_dir: r#""C:\MySmallPrograms\""#.into(),
            icon: r#""F:\EpicExtra\Celeste\Celeste.exe""#.into(),
        }
    }

    #[test]
    fn parses_fixture_and_leaves_other_entries_alone() {
        let original = fixture();
        let spans = entry_spans(&original).unwrap();
        assert_eq!(spans.len(), 42);

        // Upserting Celeste re-serialises only Celeste's byte range; the
        // other 41 entries must survive byte for byte.
        let edited = upsert(&original, &celeste()).unwrap();
        let after = entry_spans(&edited).unwrap();
        assert_eq!(after.len(), 42);
        for i in 1..spans.len() {
            let before = &original[spans[i].0..spans[i].1];
            let now = &edited[after[i].0..after[i].1];
            assert_eq!(before, now, "entry {i} changed unexpectedly");
        }
        let first = &edited[after[0].0..after[0].1];
        assert!(entry_is_ours(first, &celeste()));
        assert_eq!(entry_text(first, "LaunchOptions"), Some(""));
        assert_eq!(entry_int(first, "appid"), Some(celeste().app_id()));
    }

    #[test]
    fn reads_fixture_fields() {
        let original = fixture();
        let spans = entry_spans(&original).unwrap();
        let first = &original[spans[0].0..spans[0].1];
        assert_eq!(entry_text(first, "AppName"), Some("Celeste"));
        assert_eq!(entry_text(first, "Exe"), Some(r#""C:\MySmallPrograms\epic_launcher.exe""#));
        assert_eq!(entry_int(first, "appid"), Some(0x8cf8_0f8d));
        assert_eq!(entry_int(first, "LastPlayTime"), Some(1628913700));
        assert!(entry_is_ours(first, &celeste()));
    }

    #[test]
    fn appends_entry_when_file_has_none_of_ours() {
        let original = fixture();
        let spec = ShortcutSpec {
            app_name: "Skyrim Together Reborn".into(),
            exe: r#""/games/str/launch.sh""#.into(),
            start_dir: r#""/games/Skyrim""#.into(),
            icon: r#""/games/icon.png""#.into(),
        };
        let edited = upsert(&original, &spec).unwrap();
        let spans = entry_spans(&edited).unwrap();
        assert_eq!(spans.len(), 43);
        let found = find_matching(&edited, "Skyrim Together Reborn", "launch.sh").unwrap();
        assert_eq!(found.0, "Skyrim Together Reborn");
        assert_eq!(found.2, spec.app_id());
        // The untouched prefix (everything up to the appended entry) is
        // byte-identical.
        let last = spans[spans.len() - 2].1;
        assert_eq!(&edited[..last], &original[..original.len() - 2]);
    }

    #[test]
    fn removes_only_matching_entries() {
        let original = fixture();
        let (edited, ids) = remove_matching(&original, "Celeste", "Celeste.exe").unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], 0x8cf8_0f8d);
        let (edited, ids) = remove_matching(&edited, "Nonexistent", "zzz").unwrap();
        assert!(ids.is_empty());
        let spans = entry_spans(&edited).unwrap();
        assert_eq!(spans.len(), 41);
        assert!(find_matching(&edited, "Celeste", "Celeste.exe").is_none());
    }

    #[test]
    fn roundtrips_a_fresh_file() {
        let spec = ShortcutSpec {
            app_name: "Skyrim Together Reborn".into(),
            exe: r#""/home/u/.local/share/str-launcher/scripts/launch.sh""#.into(),
            start_dir: r#""/home/u/.local/share/Steam/steamapps/common/Skyrim Special Edition""#
                .into(),
            icon: r#""/home/u/.local/share/Steam/userdata/1/config/grid/1_icon.png""#.into(),
        };
        let bytes = serialize_file(std::slice::from_ref(&spec));
        let spans = entry_spans(&bytes).unwrap();
        assert_eq!(spans.len(), 1);
        let entry = &bytes[spans[0].0..spans[0].1];
        assert_eq!(entry_text(entry, "AppName").unwrap(), spec.app_name);
        assert_eq!(entry_int(entry, "appid").unwrap(), spec.app_id());
        // Upserting into our own file must be a no-op.
        assert_eq!(upsert(&bytes, &spec).unwrap(), bytes);
    }

    #[test]
    fn rejects_corrupt_files() {
        assert!(entry_spans(b"garbage").is_err());
        assert!(entry_spans(&[]).is_err());
        let mut truncated = fixture();
        truncated.truncate(truncated.len() - 3);
        assert!(entry_spans(&truncated).is_err());
    }

    #[test]
    fn app_id_matches_the_reference_formula() {
        // crc32("hello world") | 0x80000000, cross-checked against zlib.
        let spec = ShortcutSpec {
            app_name: "world".into(),
            exe: "hello ".into(),
            start_dir: String::new(),
            icon: String::new(),
        };
        let expected: u32 = 0x0d4a_1185 | 0x8000_0000;
        assert_eq!(spec.app_id(), expected);
        assert!(spec.app_id() & 0x8000_0000 != 0);
    }
}
