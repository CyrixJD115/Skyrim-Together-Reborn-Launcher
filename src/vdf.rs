//! Minimal Valve Data Format (VDF / KeyValues) support.
//!
//! Steam stores its library list (`libraryfolders.vdf`) and compatibility tool
//! selection (`config.vdf`) in VDF. We need two capabilities:
//!
//! 1. Parse trees (to find Skyrim in the library list).
//! 2. Careful textual surgery on `config.vdf` to set `CompatToolMapping` for one
//!    app id — preserving the rest of the file byte-for-byte, exactly like the
//!    proven approach used by SteamDeckSTR's `set-compat-tool.py`.

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }

    pub fn map(&self) -> Option<&Vec<(String, Vdf)>> {
        match self {
            Vdf::Map(m) => Some(m),
            Vdf::Str(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
enum Tok {
    /// Quoted string (spans kept for textual surgery on `config.vdf`).
    Quoted {
        value: String,
        #[allow(dead_code)]
        start: usize,
        #[allow(dead_code)]
        end: usize,
    },
    Bare {
        value: String,
        #[allow(dead_code)]
        start: usize,
        #[allow(dead_code)]
        end: usize,
    },
    Open { pos: usize },
    Close { pos: usize },
}

fn tokenize(text: &str) -> Vec<Tok> {
    let bytes = text.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut value = String::new();
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' if i + 1 < bytes.len() => {
                            value.push(bytes[i + 1] as char);
                            i += 2;
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => {
                            // Collect a full UTF-8 scalar starting here.
                            let ch_len = utf8_len(bytes[i]);
                            let s = &text[i..(i + ch_len).min(text.len())];
                            value.push_str(s);
                            i += ch_len;
                        }
                    }
                }
                toks.push(Tok::Quoted { value, start, end: i });
            }
            b'{' => {
                toks.push(Tok::Open { pos: i });
                i += 1;
            }
            b'}' => {
                toks.push(Tok::Close { pos: i });
                i += 1;
            }
            _ => {
                let start = i;
                let mut value = String::new();
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'{'
                    && bytes[i] != b'}'
                    && bytes[i] != b'"'
                {
                    let ch_len = utf8_len(bytes[i]);
                    let s = &text[i..(i + ch_len).min(text.len())];
                    value.push_str(s);
                    i += ch_len;
                }
                toks.push(Tok::Bare { value, start, end: i });
            }
        }
    }
    toks
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// Parse a whole VDF document into a tree.
pub fn parse(text: &str) -> Result<Vdf, String> {
    let toks = tokenize(text);
    let mut i = 0usize;
    let v = parse_map(&toks, &mut i, true)?;
    Ok(v)
}

fn parse_map(toks: &[Tok], i: &mut usize, top: bool) -> Result<Vdf, String> {
    let mut entries: Vec<(String, Vdf)> = Vec::new();
    while *i < toks.len() {
        let key = match &toks[*i] {
            Tok::Close { .. } => {
                if top {
                    break;
                }
                *i += 1;
                return Ok(Vdf::Map(entries));
            }
            Tok::Quoted { value, .. } | Tok::Bare { value, .. } => {
                let k = value.clone();
                *i += 1;
                k
            }
            Tok::Open { .. } => {
                return Err("unexpected '{' while expecting a key".into());
            }
        };
        let value = match toks.get(*i) {
            Some(Tok::Open { .. }) => {
                *i += 1;
                parse_map(toks, i, false)?
            }
            Some(Tok::Quoted { value, .. }) | Some(Tok::Bare { value, .. }) => {
                let v = Vdf::Str(value.clone());
                *i += 1;
                v
            }
            _ => return Err(format!("missing value for key '{key}'")),
        };
        entries.push((key, value));
    }
    Ok(Vdf::Map(entries))
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
/// Find the `(open_brace, close_brace)` byte positions of the child block named
/// `key` directly inside `text[start..end]` — mirroring the semantics of the
/// reference implementation used for `config.vdf` surgery.
pub fn find_block(text: &str, start: usize, end: usize, key: &str) -> Option<(usize, usize)> {
    let rel = tokenize(&text[start..end]);
    let mut depth: i32 = 0;
    let mut last_key: Option<String> = None;
    let mut i = 0usize;
    while i < rel.len() {
        match &rel[i] {
            Tok::Open { pos } => {
                if depth == 0 {
                    if let Some(k) = &last_key {
                        if k.eq_ignore_ascii_case(key) {
                            // Find the matching close brace for this block.
                            let mut level = 0i32;
                            let open = *pos;
                            let mut j = i;
                            let mut close = None;
                            while j < rel.len() {
                                match rel[j] {
                                    Tok::Open { .. } => level += 1,
                                    Tok::Close { pos: cp } => {
                                        level -= 1;
                                        if level == 0 {
                                            close = Some(cp);
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                                j += 1;
                            }
                            return close.map(|cp| (start + open, start + cp));
                        }
                    }
                    last_key = None;
                }
                depth += 1;
            }
            Tok::Close { .. } => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                last_key = None;
            }
            Tok::Quoted { value, .. } | Tok::Bare { value, .. } => {
                if depth == 0 {
                    last_key = if last_key.is_none() {
                        Some(value.clone())
                    } else {
                        None
                    };
                }
            }
        }
        i += 1;
    }
    None
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
fn compat_entry(appid: &str, tool: &str) -> String {
    format!(
        "\n\t\t\t\t\t\"{appid}\"\n\t\t\t\t\t{{\n\
         \t\t\t\t\t\t\"name\"\t\t\"{tool}\"\n\
         \t\t\t\t\t\t\"config\"\t\t\"\"\n\
         \t\t\t\t\t\t\"priority\"\t\t\"250\"\n\
         \t\t\t\t\t}}"
    )
}

/// Read the compatibility tool currently selected for `appid` in a `config.vdf`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn get_compat_tool(text: &str, appid: &str) -> Option<String> {
    let block = find_block(text, 0, text.len(), "InstallConfigStore")?;
    let block = find_block(text, block.0 + 1, block.1, "Software")?;
    let block = find_block(text, block.0 + 1, block.1, "Valve")?;
    let block = find_block(text, block.0 + 1, block.1, "Steam")?;
    let mapping = find_block(text, block.0 + 1, block.1, "CompatToolMapping")?;
    let game = find_block(text, mapping.0 + 1, mapping.1, appid)?;
    // Search for the "name" value inside the game's block.
    let rel = tokenize(&text[game.0..game.1]);
    let mut expect_name = false;
    for t in rel {
        if let Tok::Quoted { value, .. } | Tok::Bare { value, .. } = t {
            if expect_name {
                return Some(value);
            }
            expect_name = value.eq_ignore_ascii_case("name");
        }
    }
    None
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
/// Set the compatibility tool for `appid` inside a `config.vdf`, preserving all
/// other content. Returns the new text. Mirrors `set-compat-tool.py`.
pub fn set_compat_tool(text: &str, appid: &str, tool: &str) -> Result<String, String> {
    let block = find_block(text, 0, text.len(), "InstallConfigStore")
        .ok_or("'InstallConfigStore' not found in config.vdf")?;
    let block = find_block(text, block.0 + 1, block.1, "Software")
        .ok_or("'Software' not found in config.vdf")?;
    let block = find_block(text, block.0 + 1, block.1, "Valve")
        .ok_or("'Valve' not found in config.vdf")?;
    let block = find_block(text, block.0 + 1, block.1, "Steam")
        .ok_or("'Steam' not found in config.vdf")?;

    let entry = compat_entry(appid, tool);
    let out;
    match find_block(text, block.0 + 1, block.1, "CompatToolMapping") {
        None => {
            let insert = format!("\n\t\t\t\t\"CompatToolMapping\"\n\t\t\t\t{{{entry}\n\t\t\t\t}}");
            let head = text[..block.1].trim_end();
            out = format!("{head}{insert}\n\t\t\t{}", &text[block.1..]);
        }
        Some(mapping) => match find_block(text, mapping.0 + 1, mapping.1, appid) {
            Some(game) => {
                let needle = format!("\"{appid}\"");
                let key_start = text[mapping.0..game.0]
                    .rfind(&needle)
                    .map(|i| mapping.0 + i)
                    .ok_or("appid key position not found")?;
                let head = text[..key_start].trim_end();
                out = format!("{head}{entry}{}", &text[game.1 + 1..]);
            }
            None => {
                let head = text[..mapping.1].trim_end();
                out = format!("{head}{entry}\n\t\t\t\t{}", &text[mapping.1..]);
            }
        },
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"apps"
		{
			"489830"		"123456"
		}
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
		"apps"
		{
			"72850"		"999"
		}
	}
}
"#;
        let v = parse(text).unwrap();
        let folders = v.get("libraryfolders").unwrap();
        let m = folders.map().unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(
            folders
                .get("0")
                .unwrap()
                .get("path")
                .unwrap()
                .as_str()
                .unwrap(),
            "/home/user/.local/share/Steam"
        );
        assert!(folders
            .get("1")
            .unwrap()
            .get("apps")
            .unwrap()
            .get("72850")
            .is_some());
    }

    #[test]
    fn finds_and_sets_compat_tool() {
        let text = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"Other\"\t\t\"x\"\n\t\t\t}\n\t\t}\n\t}\n}\n";
        let new = set_compat_tool(text, "489830", "GE-Proton10-34").unwrap();
        assert_eq!(
            get_compat_tool(&new, "489830").as_deref(),
            Some("GE-Proton10-34")
        );
        // Idempotent replace keeps other keys intact.
        let new2 = set_compat_tool(&new, "489830", "GE-Proton10-35").unwrap();
        assert_eq!(
            get_compat_tool(&new2, "489830").as_deref(),
            Some("GE-Proton10-35")
        );
        assert!(new2.contains("\"Other\"\t\t\"x\""));
    }
}
