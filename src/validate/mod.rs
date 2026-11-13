//! Semantic validation (spec §5, plan 2.1): checks the type system cannot express.
//!
//! The typed model parse (serde) already rejects unknown keys, wrong types and malformed
//! lengths/colors — those surface as parse-time diagnostics. This module runs afterwards
//! and checks *values*: schema version, template reference (built-in name or custom
//! folder, plan 5.3), font families, link URLs, duplicate section titles, and design
//! fields the chosen template declares unsupported (plan 3.5) or whose manifest
//! contradicts the template sources (plan 5.4). All errors of all checks are collected,
//! not just the first (spec §5).

pub mod spans;

use std::path::{Path, PathBuf};

use miette::Severity;

use crate::error::{Diag, Error};
use crate::model::{Cv, Design, Document};
use crate::suggest::closest_match;
use crate::templates::manifest::DESIGN_GROUPS;

/// Result of validating one YAML file: errors fail the build, warnings do not (ADR-0015).
#[derive(Debug, Default)]
pub struct ValidationResult {
    pub errors: Vec<Diag>,
    pub warnings: Vec<Diag>,
}

impl ValidationResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Run every semantic check against the parsed document.
///
/// `yaml_source` is needed for span lookup (ADR-0014); `source_name` is the file name
/// shown in diagnostic snippets. `font_dirs` are the user font directories
/// (`--font-path` + `design.font_paths`, relative to the input file). `base_dir` is the
/// input file's directory, the anchor for custom template path references (ADR-0027).
pub fn validate(
    yaml_source: &str,
    source_name: &str,
    document: &Document,
    font_dirs: &[PathBuf],
    base_dir: &Path,
) -> ValidationResult {
    let spans = spans::scalar_spans(yaml_source);
    let source = miette::NamedSource::new(source_name, yaml_source.to_owned());
    let mut result = ValidationResult::default();

    check_schema_version(document.schema, &spans, &source, &mut result);
    check_template(&document.design, base_dir, &spans, &source, &mut result);
    check_fonts(&document.design, font_dirs, &spans, &source, &mut result);
    check_urls(&document.cv, &spans, &source, &mut result);
    check_duplicate_titles(&document.cv, &spans, &source, &mut result);

    result
}

/// Diagnostic labeled at the value at `path`, when the span scan knows it.
fn labeled(
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    path: &str,
    message: impl Into<String>,
    label: impl Into<String>,
) -> Option<Diag> {
    let span = spans.get(path)?;
    Some(Diag::error(message, label, span, source.clone()))
}

fn check_schema_version(
    version: u32,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    if version != 1 {
        let message = format!("unsupported schema version {version}");
        // The `labeled` fallback is unreachable when the typed parse succeeded: `schema`
        // was then a scalar, and the span scan reads the same source with the same event
        // grammar, so `spans.get("schema")` is always Some here.
        let diag = labeled(spans, source, "schema", message, "this schema version")
            .unwrap_or_else(|| {
                Diag::plain(
                    format!("unsupported schema version {version}"),
                    Severity::Error,
                )
            })
            .with_help("this tool understands schema version 1");
        result.errors.push(diag);
    }
}

/// Validate the `design.template` reference and the loaded template's manifest.
///
/// Bare names must hit a built-in: that mistake keeps its snapshot-pinned "did you
/// mean" diagnostic (ADR-0027) and skips the manifest checks (the unknown-template
/// error already covers it). Path references are loaded from disk; any load failure
/// is a user-facing error labeled at `design.template`. On success the manifest
/// support enforcement (plan 3.5, ADR-0023) runs, and for custom templates also the
/// manifest-vs-reads consistency check (plan 5.4, ADR-0028).
fn check_template(
    design: &Design,
    base_dir: &Path,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    let reference = &design.template;
    let template = match crate::templates::load(reference, base_dir) {
        Ok(template) => template,
        Err(err) => {
            // A bare unknown name keeps the dedicated did-you-mean message; a path
            // reference reports the loader's reason (folder/manifest/entry point).
            let message = match &err {
                Error::Unsupported(_) if !crate::templates::is_path_like(reference) => {
                    format!("unknown template '{reference}'")
                }
                other => other.to_string(),
            };
            // The plain fallback cannot run when the typed parse succeeded: a scalar
            // at `design.template` is always in the span map (same invariant as
            // `check_schema_version`).
            let diag = labeled(
                spans,
                source,
                "design.template",
                message.clone(),
                "this template name",
            )
            .unwrap_or_else(|| Diag::plain(message, Severity::Error));
            let diag = if crate::templates::is_path_like(reference) {
                diag.with_help(
                    "a custom template is a folder with template.typ and manifest.yaml, referenced relative to the YAML file",
                )
            } else {
                let known = crate::templates::builtin_names();
                match closest_match(reference, &known) {
                    Some(best) => diag.with_help(format!("did you mean '{best}'?")),
                    None => diag.with_help(format!(
                        "available templates: {} (or a folder path like './my-template')",
                        known.join(", ")
                    )),
                }
            };
            result.errors.push(diag);
            return;
        }
    };

    check_manifest_support(
        &template.name,
        &template.supported_options,
        spans,
        source,
        result,
    );
    if crate::templates::is_path_like(reference) {
        check_manifest_consistency(&template, spans, source, result);
    }
}

fn check_fonts(
    design: &Design,
    font_dirs: &[PathBuf],
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    let available = match crate::render::fonts::available_font_families(font_dirs) {
        Ok(families) => families,
        Err(e) => {
            // Font directories could not be read: surface it, skip family checks.
            result
                .errors
                .push(Diag::plain(e.to_string(), Severity::Error));
            return;
        }
    };

    for (field, family) in [
        ("design.fonts.body", &design.fonts.body),
        ("design.fonts.heading", &design.fonts.heading),
        ("design.fonts.mono", &design.fonts.mono),
    ] {
        if available.iter().any(|known| known == family) {
            continue;
        }
        let message = format!("unknown font \"{family}\"");
        let diag = labeled(spans, source, field, message.clone(), "this font family")
            .unwrap_or_else(|| Diag::plain(message, Severity::Error));
        let diag = match closest_match(family, &available) {
            Some(best) => diag.with_help(format!("did you mean \"{best}\"?")),
            None => diag.with_help(
                "fonts are resolved from --font-path directories, then bundled fonts, then system fonts".to_owned(),
            ),
        };
        result.errors.push(diag);
    }
}

fn check_urls(
    cv: &Cv,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    let links = cv.links();
    for (index, url) in links {
        if is_well_formed_url(url) {
            continue;
        }
        let path = format!("cv.contact.links[{index}].url");
        let message = format!("malformed URL \"{url}\"");
        let diag = labeled(spans, source, &path, message.clone(), "this link URL")
            .unwrap_or_else(|| Diag::plain(message, Severity::Error))
            .with_help("URLs must start with http:// or https:// and include a host");
        result.errors.push(diag);
    }
}

fn check_duplicate_titles(
    cv: &Cv,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    let mut seen: Vec<&str> = Vec::new();
    for (index, section) in cv.sections.iter().enumerate() {
        if seen.contains(&section.title.as_str()) {
            continue;
        }
        seen.push(&section.title);
        let duplicates: Vec<usize> = cv
            .sections
            .iter()
            .enumerate()
            .filter(|(i, s)| *i != index && s.title == section.title)
            .map(|(i, _)| i)
            .collect();
        if duplicates.is_empty() {
            continue;
        }
        let path = format!("cv.sections[{index}].title");
        let message = format!(
            "section title \"{}\" is also used for section(s) {}",
            section.title,
            duplicates
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let diag =
            labeled(spans, source, &path, message, "first occurrence").unwrap_or_else(|| {
                // Same invariant as `check_schema_version`: a parsed section title is
                // always in the span map, so the plain fallback cannot run.
                Diag::plain(
                    format!("section title \"{}\" is used more than once", section.title),
                    Severity::Warning,
                )
            });
        result.warnings.push(Diag {
            severity: Severity::Warning,
            ..diag
        });
    }
}

/// Conservative URL check (ADR-0017): `http(s)` scheme plus a non-empty host, no spaces
/// or control characters. Rejects clearly broken input; does not fully parse RFC 3986.
fn is_well_formed_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return false;
    }
    !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Warn when the YAML writes values under a design group the template does not declare
/// supported (plan 3.5): the render will silently ignore them (spec §6).
///
/// "Written" means present in the span scan, so an explicit value equal to the default
/// still counts — the user authored it and expects an effect.
fn check_manifest_support(
    template_name: &str,
    supported: &[String],
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    for group in DESIGN_GROUPS {
        if supported.iter().any(|declared| declared == group) {
            continue;
        }
        let Some(span) = spans.first_used(&format!("design.{group}")) else {
            continue;
        };
        let message = format!(
            "template '{template_name}' does not support the design field '{group}'; its values are ignored"
        );
        let diag = Diag::error(message, "ignored by this template", span, source.clone());
        result.warnings.push(Diag {
            severity: Severity::Warning,
            ..diag
        });
    }
}

/// Warn when a custom template's manifest and its sources disagree (plan 5.4,
/// ADR-0028): a declared group it never reads (so written values are silently
/// ignored without the ADR-0023 warning firing), or a group it reads but does not
/// declare (so the manifest under-promises and the ADR-0023 warning misfires).
///
/// The scan (`Template::references_group`) is a heuristic, and this is exactly why
/// built-ins are exempt — their agreement is pinned by unit tests instead, so no
/// shipped template can produce these warnings.
fn check_manifest_consistency(
    template: &crate::templates::Template,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut ValidationResult,
) {
    for group in DESIGN_GROUPS {
        let declared = template
            .supported_options
            .iter()
            .any(|declared| declared == group);
        let read = template.references_group(group);
        if declared == read {
            continue;
        }
        let message = if declared {
            format!(
                "template '{}' declares support for '{group}' but its sources never read design.{group}; values written under it are ignored without warning",
                template.name
            )
        } else {
            format!(
                "template '{}' reads design.{group} but its manifest.yaml does not declare '{group}' in supported",
                template.name
            )
        };
        let diag = labeled(
            spans,
            source,
            "design.template",
            message,
            "template used here",
        )
        .unwrap_or_else(|| {
            Diag::plain(
                format!(
                    "template '{}' and its manifest.yaml disagree on '{group}'",
                    template.name
                ),
                Severity::Warning,
            )
        });
        result.warnings.push(Diag {
            severity: Severity::Warning,
            ..diag
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_urls() {
        assert!(is_well_formed_url("https://github.com/janedoe"));
        assert!(is_well_formed_url("http://example.dev"));
        assert!(is_well_formed_url("https://example.dev/path?q=1#frag"));
    }

    #[test]
    fn rejects_urls_without_http_scheme() {
        assert!(!is_well_formed_url("github.com/janedoe"));
        assert!(!is_well_formed_url("ftp://example.dev"));
        assert!(!is_well_formed_url(""));
    }

    #[test]
    fn rejects_urls_without_host() {
        assert!(!is_well_formed_url("https://"));
        assert!(!is_well_formed_url("https:///path"));
    }

    #[test]
    fn rejects_urls_with_spaces_or_control_characters() {
        assert!(!is_well_formed_url("https://exa mple.dev"));
        assert!(!is_well_formed_url("https://example.dev/\n"));
    }

    fn support_warnings(yaml: &str, supported: &[&str]) -> Vec<Diag> {
        let spans = spans::scalar_spans(yaml);
        let source = miette::NamedSource::new("cv.yaml", yaml.to_owned());
        let supported: Vec<String> = supported.iter().map(|s| (*s).to_owned()).collect();
        let mut result = ValidationResult::default();
        check_manifest_support("plain", &supported, &spans, &source, &mut result);
        result.warnings
    }

    fn template_declaring(supported: &[&str], reads: &[&str]) -> crate::templates::Template {
        let main_source = reads
            .iter()
            .map(|group| format!("design.{group}.x"))
            .collect::<Vec<_>>()
            .join("\n");
        crate::templates::Template {
            name: "custom".into(),
            description: "test".into(),
            supported_options: supported.iter().map(|s| (*s).to_owned()).collect(),
            main_source,
            extra_sources: vec![],
        }
    }

    fn consistency_warnings(template: &crate::templates::Template) -> Vec<Diag> {
        let yaml = "design:\n  template: ./custom\n";
        let spans = spans::scalar_spans(yaml);
        let source = miette::NamedSource::new("cv.yaml", yaml.to_owned());
        let mut result = ValidationResult::default();
        check_manifest_consistency(template, &spans, &source, &mut result);
        result.warnings
    }

    #[test]
    fn consistency_warns_when_the_manifest_overpromises() {
        let warnings =
            consistency_warnings(&template_declaring(&["margins", "colors"], &["margins"]));
        let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "template 'custom' declares support for 'colors' but its sources never read design.colors; values written under it are ignored without warning"
            ],
            "only the declared-but-unread group warns"
        );
        assert_eq!(warnings[0].severity, Severity::Warning, "never an error");
    }

    #[test]
    fn consistency_warns_when_the_manifest_underdeclares() {
        let warnings = consistency_warnings(&template_declaring(&[], &["page"]));
        assert_eq!(
            warnings[0].message,
            "template 'custom' reads design.page but its manifest.yaml does not declare 'page' in supported"
        );
    }

    #[test]
    fn consistency_is_quiet_when_manifest_and_sources_agree() {
        assert!(
            consistency_warnings(&template_declaring(
                &["margins", "colors"],
                &["margins", "colors"]
            ))
            .is_empty()
        );
    }

    #[test]
    fn warns_for_written_but_unsupported_design_groups() {
        let yaml = "design:\n  page: letter\n  colors:\n    accent: \"#ff0000\"\n";
        let warnings = support_warnings(yaml, &["page"]);
        let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
        assert_eq!(
            messages,
            ["template 'plain' does not support the design field 'colors'; its values are ignored"],
            "only the used, undeclared group warns"
        );
        assert_eq!(warnings[0].severity, Severity::Warning, "never an error");
    }

    #[test]
    fn warning_labels_the_first_value_written_under_the_ignored_group() {
        let yaml = "design:\n  colors:\n    accent: \"#ff0000\"\n";
        let warning = &support_warnings(yaml, &[])[0];
        let label = warning.labels.first().expect("span-labeled");
        let start = label.offset();
        assert_eq!(
            &yaml[start..start + label.len()],
            "\"#ff0000\"",
            "points at the ignored value, not the group key"
        );
    }

    #[test]
    fn no_warnings_when_every_used_group_is_supported() {
        let yaml = "design:\n  page: letter\n  margins: { x: 2cm, y: 2cm }\n";
        assert!(
            support_warnings(
                yaml,
                &["page", "margins", "fonts", "sizes", "spacing", "colors"]
            )
            .is_empty()
        );
    }

    #[test]
    fn unsupported_but_unwritten_groups_stay_silent() {
        let yaml = "schema: 1\ncv:\n  name: X\n";
        assert!(support_warnings(yaml, &["page"]).is_empty());
    }

    #[test]
    fn builtins_ship_full_design_support() {
        // Every built-in declares every group, so no built-in CV can ever trigger the
        // ADR-0023 warning (the warning itself is exercised against a declared-minimal
        // template above).
        for name in crate::templates::builtin_names() {
            let template = crate::templates::load_builtin(&name).expect("built-in loads");
            for group in DESIGN_GROUPS {
                assert!(
                    template.supported_options.iter().any(|s| s == group),
                    "'{name}' manifest must declare {group}: {:?}",
                    template.supported_options
                );
            }
        }
    }
}
