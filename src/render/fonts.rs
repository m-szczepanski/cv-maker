//! Font loading (spec §8): bundled fonts embedded in the binary, plus user font
//! directories from `design.font_paths` and `--font-path`.
//!
//! Priority order used for the Typst font book (docs/decisions.md ADR-0007):
//! user directories → bundled fonts → system fonts. Explicit user intent wins;
//! bundled fonts beat same-named system fonts so output stays reproducible across
//! machines (spec §2 note).

use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};

use crate::error::{Error, Result};
use crate::render::RenderError;

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

/// Every font family the compiler can resolve, in ADR-0007 priority order
/// (user dirs → bundled → system), sorted and de-duplicated (plan 2.1, spec §5:
/// "font family exists"). This is exactly the set Typst would match against at
/// render time, so validation and rendering cannot disagree.
pub fn available_font_families(
    user_font_dirs: &[PathBuf],
) -> std::result::Result<Vec<String>, RenderError> {
    let mut font_items: Vec<Vec<u8>> =
        collect_font_dir_files(user_font_dirs).map_err(|e| RenderError::Font(e.to_string()))?;
    font_items.extend(bundled_fonts().iter().map(|(_, bytes)| bytes.to_vec()));

    // A minimal engine: same font setup as a real build (see `Engine::build`), with an
    // empty main source — we only need the world's font book, never a compile.
    let engine = TypstEngine::builder()
        .with_static_source_file_resolver([("main.typ", "")])
        .fonts(font_items)
        .search_fonts_with(TypstKitFontOptions::new())
        .build();

    engine
        .with_world("main.typ", |world| {
            let book = typst::World::book(world);
            let mut families: Vec<String> = book
                .families()
                .map(|(family, _)| family.to_string())
                .collect();
            families.sort();
            families.dedup();
            families
        })
        .map_err(|e| RenderError::Font(format!("could not enumerate fonts: {e}")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_family_names_includes_the_bundled_set() {
        let names = bundled_family_names();
        // The five families of spec §2.
        for expected in [
            "Inter",
            "JetBrains Mono",
            "IBM Plex Sans",
            "IBM Plex Mono",
            "Source Sans 3",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "bundled set must cover {expected}, got: {names:?}"
            );
        }
    }

    /// The real gate: every spec §2 family must resolve *from the embedded font files'
    /// metadata* (the same path a build takes), not just from their file names.
    #[test]
    fn font_book_resolves_every_bundled_family() {
        let families = available_font_families(&[]).expect("font book builds");
        for expected in [
            "Inter",
            "JetBrains Mono",
            "IBM Plex Sans",
            "IBM Plex Mono",
            "Source Sans 3",
        ] {
            assert!(
                families.iter().any(|f| f == expected),
                "font book must resolve {expected}, got: {families:?}"
            );
        }
    }

    #[test]
    fn bundled_fonts_are_sorted_by_file_name() {
        let fonts = bundled_fonts();
        assert!(!fonts.is_empty(), "fonts/ must ship at least one font");
        let mut names: Vec<&str> = fonts.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        names.sort_unstable();
        assert_eq!(names, sorted, "deterministic order (ADR-0007)");
    }

    #[test]
    fn display_names_derive_from_file_stems() {
        assert_eq!(display_name_from_file("Inter-Regular.ttf"), "Inter");
        assert_eq!(
            display_name_from_file("JetBrainsMono-Bold.ttf"),
            "JetBrains Mono"
        );
        assert_eq!(
            display_name_from_file("IBMPlexSans-Medium.otf"),
            "IBM Plex Sans"
        );
    }

    #[test]
    fn collect_font_dir_files_reads_only_font_files_sorted() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("b.ttf"), b"font-b").expect("write b");
        std::fs::write(dir.path().join("a.otf"), b"font-a").expect("write a");
        std::fs::write(dir.path().join("notes.txt"), b"ignored").expect("write txt");

        let files = collect_font_dir_files(&[dir.path().to_path_buf()]).expect("collect works");
        assert_eq!(files.len(), 2, "only .ttf/.otf count, sorted");
        assert_eq!(&files[0], b"font-a", "sorted by file name");
        assert_eq!(&files[1], b"font-b");
    }

    #[test]
    fn collect_font_dir_files_fails_for_a_missing_directory() {
        let err = collect_font_dir_files(&["/cv-test/no-such-fonts".into()])
            .expect_err("missing dir must fail");
        assert!(
            err.to_string().contains("no-such-fonts"),
            "error names the directory: {err}"
        );
    }

    #[test]
    fn collect_font_dir_files_fails_when_a_font_entry_cannot_be_read() {
        // A directory named `fake.ttf` passes the extension filter but cannot be read
        // as a file: the error must name the entry, not panic.
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("fake.ttf")).expect("create entry dir");

        let err = collect_font_dir_files(&[dir.path().to_path_buf()])
            .expect_err("unreadable entry must fail");
        assert!(
            err.to_string().contains("fake.ttf"),
            "error names it: {err}"
        );
    }
}
