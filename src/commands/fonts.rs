//! `cv fonts` (plan 3.3): list every font family the tool can resolve, labeled with
//! its source (spec §3, §8).

use std::path::PathBuf;

use crate::error::Result;
use crate::render::fonts::{FontListing, available_fonts, collect_font_dir_files};

/// Render one listing line: family name padded to a stable column, then the source.
fn format_line(font: &FontListing, width: usize) -> String {
    format!(
        "{:<width$}  {}",
        font.family,
        font.source.label(),
        width = width
    )
}

pub fn run(user_font_dirs: &[PathBuf]) -> Result<()> {
    let fonts = available_fonts(&collect_font_dir_files(user_font_dirs)?);
    let width = fonts
        .iter()
        .map(|f| f.family.chars().count())
        .max()
        .unwrap_or_default();
    for font in &fonts {
        super::out_line(&format_line(font, width));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::fonts::FontSource;

    #[test]
    fn lines_align_family_and_source_columns() {
        let font = FontListing {
            family: "Inter".to_owned(),
            source: FontSource::Bundled,
        };
        assert_eq!(format_line(&font, 13), "Inter          bundled");
    }
}
