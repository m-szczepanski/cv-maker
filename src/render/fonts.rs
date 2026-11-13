//! Font loading (spec §8): bundled fonts embedded in the binary, plus user font
//! directories from `design.font_paths` and `--font-path`.
//!
//! Priority order used for the Typst font book (docs/decisions.md ADR-0007):
//! user directories → bundled fonts → system fonts. Explicit user intent wins;
//! bundled fonts beat same-named system fonts so output stays reproducible across
//! machines (spec §2 note).

use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

use crate::error::{Error, Result};

/// All files in `fonts/`, embedded at compile time.
pub static BUNDLED_FONT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/fonts");

/// The bundled fonts that can be embedded into the compiler: (family file name, bytes).
/// Sorted by name for determinism. Only .ttf/.otf files count; licenses/docs are skipped.
pub fn bundled_fonts() -> Vec<(&'static str, &'static [u8])> {
    let mut fonts: Vec<(&'static str, &'static [u8])> = BUNDLED_FONT_DIR
        .files()
        .filter(|f| {
            let ext = f.path().extension().and_then(|e| e.to_str());
            matches!(ext, Some("ttf") | Some("otf"))
        })
        .map(|f| {
            (
                f.path().to_str().expect("embedded paths are valid UTF-8"),
                f.contents(),
            )
        })
        .collect();
    fonts.sort_unstable_by_key(|(name, _)| *name);
    fonts
}

/// Family names provided by the bundled set (for `cv fonts` and validation in M2).
pub fn bundled_family_names() -> Vec<String> {
    bundled_fonts()
        .iter()
        .map(|(name, _)| display_name_from_file(name))
        .collect()
}

/// `Inter-Regular.ttf` → `Inter`; `JetBrainsMono-SemiBold.ttf` → `JetBrains Mono`.
/// The Typst-facing family name is read from the font itself; this is only the display
/// form used in listings until proper font metadata parsing lands (M3, `cv fonts`).
fn display_name_from_file(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    let family = stem.split('-').next().unwrap_or(stem);
    // Split camel-case words: JetBrainsMono → JetBrains Mono, Inter stays Inter.
    family
        .replace("JetBrainsMono", "JetBrains Mono")
        .replace("IBMPlexSans", "IBM Plex Sans")
        .replace("IBMPlexMono", "IBM Plex Mono")
        .replace("SourceSans3", "Source Sans 3")
}

/// Collect font files from user-provided directories (spec §8: `.ttf`/`.otf`).
/// Non-recursive; directories are scanned in the order given, files sorted per directory.
pub fn collect_font_dir_files(dirs: &[PathBuf]) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    for dir in dirs {
        let entries = std::fs::read_dir(dir).map_err(|source| Error::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let mut names: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("ttf") | Some("otf")
                )
            })
            .collect();
        names.sort();
        for path in names {
            let bytes = std::fs::read(&path).map_err(|source| Error::Io {
                path: path.display().to_string(),
                source,
            })?;
            files.push(bytes);
        }
    }
    Ok(files)
}
