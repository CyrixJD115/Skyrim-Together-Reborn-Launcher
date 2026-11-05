//! SteamGridDB artwork for the "add a Steam game" launch mode.
//!
//! All five images are embedded in the binary and are only written to disk
//! when the user chooses the Steam-entry launch mode (Linux). They give the
//! new "Skyrim Together Reborn" entry in Steam its hero, grids, logo and
//! taskbar icon, matching how SteamGridDB's own manager installs artwork.
//!
//! Sources (SteamGridDB, used as game artwork):
//! - hero:   https://www.steamgriddb.com/hero/940      (1920x620)
//! - grid:   https://www.steamgriddb.com/grid/749110   (600x900 portrait)
//! - grid:   https://www.steamgriddb.com/grid/749111   (460x215 landscape)
//! - logo:   https://www.steamgriddb.com/logo/166498
//! - icon:   https://www.steamgriddb.com/icon/18951    (recolored red → white)

use std::path::Path;

/// 1920x620 — library detail page background.
pub const HERO_PNG: &[u8] = include_bytes!("../../assets/sgdb/hero.png");
/// 600x900 — Big Picture portrait box art.
pub const GRID_PORTRAIT_PNG: &[u8] = include_bytes!("../../assets/sgdb/grid_portrait.png");
/// 460x215 — classic library grid tile.
pub const GRID_LANDSCAPE_PNG: &[u8] = include_bytes!("../../assets/sgdb/grid_landscape.png");
/// Transparent overlay shown on top of the grid/hero.
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/sgdb/logo.png");
/// 256x256 — taskbar / in-library icon (recolored to white before writing).
pub const ICON_PNG_RED: &[u8] = include_bytes!("../../assets/sgdb/icon.png");

/// The icon ships red; the launcher theme is white, so red-dominant pixels
/// are remapped to their grayscale value at runtime (alpha is untouched and
/// antialiased edges blend naturally because they stay red-dominant too).
pub fn icon_white_png() -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(ICON_PNG_RED)
        .map_err(|e| format!("decoding the bundled icon failed: {e}"))?
        .to_rgba8();

    let mut img = img;
    for px in img.pixels_mut() {
        let [r, g, b, _a] = px.0;
        let red_dominance = r as i16 - g.max(b) as i16;
        if red_dominance > 8 {
            // Grayscale by luminance: bright reds turn near-white, dark reds
            // become their equivalent shade of gray, preserving the shading.
            let luma = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
            *px = image::Rgba([luma, luma, luma, px.0[3]]);
        }
    }

    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("encoding the recolored icon failed: {e}"))?;
    Ok(out.into_inner())
}

/// Write the whole artwork set into Steam's grid folder for `app_id`.
///
/// Naming follows the layout Steam (and SteamGridDB's manager) use for
/// non-Steam shortcuts: `<id>_hero.png`, `<id>_logo.png`, `<id>_icon.png`,
/// `<id>p.png` (portrait grid) and `<id>.png` (landscape grid).
pub fn write_artwork(grid_dir: &Path, app_id: u32) -> Result<Vec<std::path::PathBuf>, String> {
    std::fs::create_dir_all(grid_dir).map_err(|e| format!("create {}: {e}", grid_dir.display()))?;
    let white_icon = icon_white_png()?;

    let names = [
        format!("{app_id}_hero.png"),
        format!("{app_id}_logo.png"),
        format!("{app_id}_icon.png"),
        format!("{app_id}p.png"),
        format!("{app_id}.png"),
    ];
    let files: [(&str, &[u8]); 5] = [
        (names[0].as_str(), HERO_PNG),
        (names[1].as_str(), LOGO_PNG),
        (names[2].as_str(), &white_icon),
        (names[3].as_str(), GRID_PORTRAIT_PNG),
        (names[4].as_str(), GRID_LANDSCAPE_PNG),
    ];

    let mut written = Vec::with_capacity(files.len());
    for (name, bytes) in files {
        let path = grid_dir.join(name);
        std::fs::write(&path, bytes)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Remove every artwork file we own for `app_id` (all extensions, since older
/// installs may have used jpg).
pub fn remove_artwork(grid_dir: &Path, app_id: u32) {
    for stem in [
        format!("{app_id}_hero"),
        format!("{app_id}_logo"),
        format!("{app_id}_icon"),
        format!("{app_id}p"),
        format!("{app_id}"),
    ] {
        for ext in ["png", "jpg", "jpeg", "tga"] {
            let _ = std::fs::remove_file(grid_dir.join(format!("{stem}.{ext}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_grid() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("str-sgdb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_the_full_artwork_set() {
        let dir = tmp_grid();
        let written = write_artwork(&dir, 0x8cf8_0f8d).unwrap();
        assert_eq!(written.len(), 5);
        for path in &written {
            assert!(path.is_file(), "{} missing", path.display());
        }
        let names: Vec<String> = written
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"2365067149_hero.png".to_string()));
        assert!(names.contains(&"2365067149_logo.png".to_string()));
        assert!(names.contains(&"2365067149_icon.png".to_string()));
        assert!(names.contains(&"2365067149p.png".to_string()));
        assert!(names.contains(&"2365067149.png".to_string()));
        remove_artwork(&dir, 0x8cf8_0f8d);
        assert!(written.iter().all(|p| !p.exists()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recolor_maps_red_to_grayscale_white() {
        let white = icon_white_png().unwrap();
        let img = image::load_from_memory(&white).unwrap().to_rgba8();

        // The recolored icon must not contain any strongly red pixel.
        let mut red_pixels = 0u32;
        let mut non_transparent = 0u32;
        for px in img.pixels() {
            let [r, g, b, a] = px.0;
            if a > 16 {
                non_transparent += 1;
                if r as i16 - g.max(b) as i16 > 8 {
                    red_pixels += 1;
                }
            }
        }
        assert!(non_transparent > 100, "icon appears empty");
        assert_eq!(red_pixels, 0, "red survived the recolor");
    }
}
