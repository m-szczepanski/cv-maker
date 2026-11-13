//! Font loading (spec §8): bundled fonts embedded in the binary, plus user font
//! directories from `design.font_paths` and `--font-path`.
//!
//! Priority order used for the Typst font book (docs/decisions.md ADR-0007):
//! user directories → bundled fonts → system fonts. Explicit user intent wins;
//! bundled fonts beat same-named system fonts so output stays reproducible across
//! machines (spec §2 note).
//!
//! Family names come from the parsed font metadata (the same Typst font book a
//! render uses), never from file names — see ADR-0021.

use std::path::PathBuf;

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

/// Where the compiler loads a font family from (spec §8 resolution order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FontSource {
    /// A `--font-path` / `design.font_paths` directory (wins over the rest).
    FontPath,
    /// Embedded in the binary (wins over system fonts).
    Bundled,
    /// Discovered via typst-kit system font search.
    System,
}

impl FontSource {
    /// Column label used by `cv fonts`.
    pub fn label(self) -> &'static str {
        match self {
            FontSource::FontPath => "font-path",
            FontSource::Bundled => "bundled",
            FontSource::System => "system",
        }
    }
}

/// One font family and the source that wins for it (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontListing {
    pub family: String,
    pub source: FontSource,
}

/// Every font family available to the compiler, each labeled with its winning source
/// (ADR-0007 priority: font-path dirs → bundled → system), grouped by source and
/// sorted by name within each group.
///
/// This is exactly the set Typst would match against at render time, so validation,
/// `cv fonts` and rendering cannot disagree.
pub fn available_fonts(
    user_font_dirs: &[PathBuf],
) -> std::result::Result<Vec<FontListing>, RenderError> {
    let user_items =
        collect_font_dir_files(user_font_dirs).map_err(|e| RenderError::Font(e.to_string()))?;
    let bundled_items: Vec<Vec<u8>> = bundled_fonts()
        .iter()
        .map(|(_, bytes)| bytes.to_vec())
        .collect();

    let user = book_families(user_items.clone(), None)?;
    let bundled = book_families(bundled_items.clone(), None)?;

    let mut all_items = user_items;
    all_items.extend(bundled_items);
    let available = book_families(all_items, Some(TypstKitFontOptions::new()))?;

    let mut listings: Vec<FontListing> = available
        .into_iter()
        .map(|family| {
            let source = if user.contains(&family) {
                FontSource::FontPath
            } else if bundled.contains(&family) {
                FontSource::Bundled
            } else {
                FontSource::System
            };
            FontListing { family, source }
        })
        .collect();
    listings.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.family.to_lowercase().cmp(&b.family.to_lowercase()))
    });
    Ok(listings)
}

/// Every font family the compiler can resolve (user dirs → bundled → system),
/// sorted and de-duplicated (plan 2.1, spec §5: "font family exists").
pub fn available_font_families(
    user_font_dirs: &[PathBuf],
) -> std::result::Result<Vec<String>, RenderError> {
    Ok(available_fonts(user_font_dirs)?
        .into_iter()
        .map(|font| font.family)
        .collect())
}

/// The sorted, de-duplicated family list of a font book made of exactly these font
/// file bytes, optionally extended by typst-kit system discovery.
///
/// The engine is only used for its font book — the `"main.typ"` source stays empty,
/// nothing is ever compiled (same trick as [`available_fonts`]).
fn book_families(
    font_items: Vec<Vec<u8>>,
    kit_options: Option<TypstKitFontOptions>,
) -> std::result::Result<Vec<String>, RenderError> {
    let mut builder = TypstEngine::builder()
        .with_static_source_file_resolver([("main.typ", "")])
        .fonts(font_items);
    if let Some(options) = kit_options {
        builder = builder.search_fonts_with(options);
    }
    let engine = builder.build();

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

    /// The five families spec §2 requires us to bundle.
    const SPEC_FAMILIES: [&str; 5] = [
        "Inter",
        "JetBrains Mono",
        "IBM Plex Sans",
        "IBM Plex Mono",
        "Source Sans 3",
    ];

    #[test]
    fn every_spec_family_is_available_and_marked_bundled() {
        let fonts = available_fonts(&[]).expect("listing built");
        for expected in SPEC_FAMILIES {
            let found = fonts
                .iter()
                .find(|f| f.family == expected)
                .unwrap_or_else(|| panic!("bundled set must cover {expected}, got: {fonts:?}"));
            assert_eq!(
                found.source,
                FontSource::Bundled,
                "{expected} must be served from the bundle without user dirs"
            );
        }
    }

    #[test]
    fn family_names_come_from_font_metadata_not_file_names() {
        // `IBMPlexSans-Var-Roman.ttf` is served under its typographic family, the
        // suffix-trimmed `IBM Plex Sans` — a file-name heuristic would say otherwise.
        let items: Vec<Vec<u8>> = bundled_fonts().iter().map(|(_, b)| b.to_vec()).collect();
        let families = book_families(items, None).expect("bundled book builds");
        assert!(families.iter().any(|f| f == "IBM Plex Sans"));
        assert!(
            !families
                .iter()
                .any(|f| f.contains("Var") || f.contains("SmBld")),
            "no file-name or intermediate-weight artifacts in the bundled book: {families:?}"
        );
    }

    #[test]
    fn user_dir_font_shadows_the_bundled_family() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inter = bundled_fonts()
            .iter()
            .find(|(name, _)| *name == "Inter-Regular.ttf")
            .map(|(_, bytes)| *bytes)
            .expect("Inter is bundled");
        std::fs::write(dir.path().join("my-inter.ttf"), inter).expect("write font copy");

        let fonts = available_fonts(&[dir.path().to_path_buf()]).expect("listing built");
        let inter = fonts
            .iter()
            .find(|f| f.family == "Inter")
            .expect("Inter listed");
        assert_eq!(
            inter.source,
            FontSource::FontPath,
            "the user dir copy wins (ADR-0007)"
        );
    }

    #[test]
    fn unparsable_font_files_are_skipped_without_failing_the_listing() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("fake.ttf"), b"not a real font").expect("write fake");

        let fonts = available_fonts(&[dir.path().to_path_buf()]).expect("listing built");
        assert!(
            fonts.iter().any(|f| f.family == "Inter"),
            "bundled fonts still list despite a garbage font file"
        );
    }

    /// The validation contract (plan 2.1): `available_font_families` must accept every
    /// family the listing shows and only those — validation and `cv fonts` agree.
    /// If either side ever filters or sorts differently, this catches the drift.
    #[test]
    fn validation_accepts_exactly_the_listed_families() {
        let fonts = available_fonts(&[]).expect("listing built");
        let accepted = available_font_families(&[]).expect("families built");
        assert_eq!(
            accepted,
            fonts.iter().map(|f| f.family.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn listings_are_grouped_by_source_then_sorted_by_name() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inter = bundled_fonts()
            .iter()
            .find(|(name, _)| *name == "Inter-Regular.ttf")
            .map(|(_, bytes)| *bytes)
            .expect("Inter is bundled");
        std::fs::write(dir.path().join("inter.ttf"), inter).expect("write copy");

        let fonts = available_fonts(&[dir.path().to_path_buf()]).expect("listing built");
        let sources: Vec<FontSource> = fonts.iter().map(|f| f.source).collect();
        let mut grouped = sources.clone();
        grouped.sort_unstable(); // FontSource Ord = priority order
        assert_eq!(
            sources, grouped,
            "grouped by source (font-path → bundled → system)"
        );
        let bundled = fonts
            .iter()
            .filter(|f| f.source == FontSource::Bundled)
            .map(|f| f.family.to_lowercase())
            .collect::<Vec<_>>();
        let mut sorted = bundled.clone();
        sorted.sort();
        assert_eq!(bundled, sorted, "sorted by family name within a source");
    }

    #[test]
    fn bundled_fonts_are_sorted_by_file_name() {
        let fonts = bundled_fonts();
        assert!(!fonts.is_empty(), "fonts/ must ship at least one font");
        let names: Vec<&str> = fonts.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "deterministic order (ADR-0007)");
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

    #[test]
    fn font_source_labels_are_the_cv_fonts_columns() {
        assert_eq!(FontSource::FontPath.label(), "font-path");
        assert_eq!(FontSource::Bundled.label(), "bundled");
        assert_eq!(FontSource::System.label(), "system");
    }
}
