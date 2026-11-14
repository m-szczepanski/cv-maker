//! `cv init` (plan 1.3): write a starter YAML into the current directory.

use std::path::Path;

use crate::cli::InitArgs;
use crate::error::{Error, Result};
use crate::templates;

/// The starter file `cv init` writes for the chosen built-in (plan 1.3.1, ADR-0011
/// amended in M5): the fixed content with its one `template:` line set to the
/// selection. Generated content, not user data, so a plain `const &str` is fine;
/// based on the spec §4 example so it demonstrates every section type and design knob.
pub fn starter_for(template: &str) -> String {
    STARTER_YAML.replace("template: compact", &format!("template: {template}"))
}

/// The starter file text `cv init` writes (plan 1.3.1). Based on the spec §4 example.
pub const STARTER_YAML: &str = r##"# yaml-language-server: $schema=./cv.schema.json
schema: 1

cv:
  name: Jane Doe
  headline: Staff Software Engineer
  location: Berlin, Germany
  contact:
    email: jane@example.dev
    phone: "+49 000 000000"
    links:
      - { label: GitHub, url: "https://github.com/janedoe" }
      - { label: Website, url: "https://janedoe.dev" }

  summary: >
    Backend engineer focused on high-throughput systems and developer tooling.

  sections:
    - title: Experience
      type: entries
      items:
        - heading: Staff Engineer
          org: Acme Corp
          location: Remote
          dates: 2021 – present
          bullets:
            - Cut p99 latency by 40% by moving the hot path to Rust
            - Led a 6-person team through a zero-downtime migration
    - title: Skills
      type: key_values
      items:
        - { key: Languages, value: "Rust, Go, Python, TypeScript" }
        - { key: Infrastructure, value: "Kubernetes, Postgres, Kafka" }
    - title: Education
      type: entries
      items:
        - heading: BSc Computer Science
          org: University of Example
          dates: 2012 – 2016

design:
  template: compact
  page: a4
  pages: 1
  margins: { x: 1.5cm, y: 1.2cm }
  fonts:
    body: Inter
    heading: Inter
    mono: JetBrains Mono
  sizes:
    name: 20pt
    heading: 11pt
    body: 9.5pt
    small: 8.5pt
  spacing:
    section_gap: 8pt
    entry_gap: 4pt
    line_height: 1.25
  colors:
    accent: "#2563eb"
    text: "#111111"
    muted: "#555555"
  fit_one_page: true
  fit_floor:
    min_body_size: 8.5pt
    min_scale: 0.85
"##;

pub fn run(args: &InitArgs) -> Result<()> {
    // `--template` selects a built-in by name (spec §3); custom folders are written
    // into the YAML by the user after init (plan 5.3).
    let known = templates::builtin_names();
    if !known.iter().any(|n| n == &args.template) {
        return Err(Error::Unsupported(format!(
            "unknown --template '{}'. Available templates: {}.",
            args.template,
            known.join(", ")
        )));
    }

    if Path::new(&args.out).exists() && !args.force {
        return Err(Error::Unsupported(format!(
            "{} already exists; pass --force to overwrite",
            args.out.display()
        )));
    }

    std::fs::write(&args.out, starter_for(&args.template)).map_err(|source| Error::Write {
        path: args.out.display().to_string(),
        message: source.to_string(),
    })?;

    // Write the JSON Schema next to the YAML so the `$schema=./cv.schema.json` header
    // works offline (plan 2.3, ADR-0019).
    let schema_path = schema_path_for(&args.out);
    std::fs::write(&schema_path, crate::commands::schema::schema_json()).map_err(|source| {
        Error::Write {
            path: schema_path.display().to_string(),
            message: source.to_string(),
        }
    })?;

    super::out_line(&format!(
        "wrote {} (+ {})",
        args.out.display(),
        schema_path.display()
    ));
    Ok(())
}

/// The schema file lives next to the YAML file, same directory, fixed name (ADR-0019).
fn schema_path_for(out: &Path) -> std::path::PathBuf {
    out.parent()
        .unwrap_or(Path::new("."))
        .join("cv.schema.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_for_default_keeps_the_text_verbatim() {
        assert_eq!(starter_for("compact"), STARTER_YAML);
    }

    #[test]
    fn starter_for_selects_the_requested_builtin() {
        let yaml = starter_for("classic");
        assert!(yaml.contains("template: classic"), "{yaml}");
        assert!(!yaml.contains("template: compact"), "{yaml}");
        let doc = crate::model::Document::from_yaml_str(&yaml).expect("classic starter parses");
        assert_eq!(doc.design.template, "classic");
    }
}
