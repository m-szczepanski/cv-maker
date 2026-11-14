//! Semantic validation (spec §5, plan 2.1): checks the type system cannot express.
//!
//! The typed model parse (serde) already rejects unknown keys, wrong types and malformed
//! lengths/colors — those surface as parse-time diagnostics. This module runs afterwards
//! and checks *values*: schema version, template reference (built-in name or custom
//! folder, plan 5.3), font families, contact and Markdown link URLs, duplicate section
//! titles, and design fields the chosen template declares unsupported (plan 3.5) or
//! whose manifest contradicts the template sources (plan 5.4). All errors of all checks
//! are collected, not just the first (spec §5).

pub mod spans;

use std::path::{Path, PathBuf};

use miette::Severity;

use crate::error::{Diag, Error};
use crate::model::{Cv, Design, Document};
use crate::suggest::closest_match;
use crate::templates::Template;
use crate::templates::manifest::DESIGN_GROUPS;

/// Result of running the checks over one YAML file (ADR-0015).
#[derive(Debug, Default)]
struct Checks {
    errors: Vec<Diag>,
    warnings: Vec<Diag>,
    /// The template `design.template` resolved to, once `check_template` has run.
    template: Option<Template>,
}

/// The outcome of validating one YAML file.
#[derive(Debug)]
pub enum Outcome {
    /// The CV can render: the template its reference resolved to, the user font files
    /// and the warnings that never fail a build. Handing both back means the render uses
    /// exactly what was checked, and reads each input once, not twice.
    Valid {
        template: Template,
        font_items: Vec<Vec<u8>>,
        warnings: Vec<Diag>,
    },
    /// Every error the checks found, in check order (spec §5: all errors, not the
    /// first one).
    Invalid(Vec<Diag>),
}

/// Run every semantic check against the parsed document.
///
/// `yaml_source` is needed for span lookup (ADR-0014); `source_name` is the file name
/// shown in diagnostic snippets. `font_dirs` are the user font directories
/// (`--font-path` + `design.font_paths`, already resolved against the input file); they
/// are read once here and handed back on success. `base_dir` is the input file's
/// directory, the anchor for custom template path references (ADR-0027).
pub fn validate(
    yaml_source: &str,
    source_name: &str,
    document: &Document,
    font_dirs: &[PathBuf],
    base_dir: &Path,
) -> Outcome {
    let spans = spans::scalar_spans(yaml_source);
    let source = miette::NamedSource::new(source_name, yaml_source.to_owned());
    let mut checks = Checks::default();

    check_schema_version(document.schema, &spans, &source, &mut checks);
    check_template(&document.design, base_dir, &spans, &source, &mut checks);
    // Reading the user font directories can fail; that failure is one more thing wrong
    // with the input, reported alongside the rest (spec §5), not a reason to stop
    // looking at the file.
    let font_items = match crate::render::fonts::collect_font_dir_files(font_dirs) {
        Ok(items) => {
            check_fonts(&document.design, &items, &spans, &source, &mut checks);
            items
        }
        Err(error) => {
            checks
                .errors
                .push(Diag::plain(error.to_string(), Severity::Error));
            Vec::new()
        }
    };
    check_urls(&document.cv, &spans, &source, &mut checks);
    check_markdown_urls(&document.cv, &spans, &source, &mut checks);
    check_duplicate_titles(&document.cv, &spans, &source, &mut checks);

    let Some(template) = checks.template else {
        // `check_template` reports why a reference did not resolve, so this path
        // already carries its error along with everything else that was found.
        return Outcome::Invalid(checks.errors);
    };
    if checks.errors.is_empty() {
        Outcome::Valid {
            template,
            font_items,
            warnings: checks.warnings,
        }
    } else {
        Outcome::Invalid(checks.errors)
    }
}

/// Diagnostic for the value at `path`.
///
/// Labeled at the value's span when the span scan knows one; a value written as a
/// YAML alias has no span of its own (the text lives at the anchor), so the check
/// reports the resolved value without a snippet (ADR-0014, ADR-0036).
fn diag_at(
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    path: &str,
    message: String,
    label: &str,
    severity: Severity,
) -> Diag {
    let diag = match spans.get(path) {
        Some(span) => Diag::error(message, label, span, source.clone()),
        None => Diag::plain(message, severity),
    };
    Diag { severity, ..diag }
}

fn check_schema_version(
    version: u32,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut Checks,
) {
    if version != 1 {
        let message = format!("unsupported schema version {version}");
        let diag = diag_at(
            spans,
            source,
            "schema",
            message,
            "this schema version",
            Severity::Error,
        );
        result
            .errors
            .push(diag.with_help("this tool understands schema version 1"));
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
    result: &mut Checks,
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
            let diag = diag_at(
                spans,
                source,
                "design.template",
                message,
                "this template name",
                Severity::Error,
            );
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
    result.template = Some(template);
}

fn check_fonts(
    design: &Design,
    font_items: &[Vec<u8>],
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut Checks,
) {
    let available = crate::render::fonts::available_font_families(font_items);

    for (field, family) in [
        ("design.fonts.body", &design.fonts.body),
        ("design.fonts.heading", &design.fonts.heading),
        ("design.fonts.mono", &design.fonts.mono),
    ] {
        // Typst resolves a family name case-insensitively (pinned 0.15.1: the selector
        // is lowercased before the book lookup), so validation must not be stricter
        // than rendering (ADR-0031). `cv fonts` and the hints speak canonical names.
        let wanted = family.to_lowercase();
        if available.iter().any(|known| known.to_lowercase() == wanted) {
            continue;
        }
        let message = format!("unknown font \"{family}\"");
        let diag = diag_at(
            spans,
            source,
            field,
            message,
            "this font family",
            Severity::Error,
        );
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
    result: &mut Checks,
) {
    let links = cv.links();
    for (index, url) in links {
        if is_well_formed_url(url) {
            continue;
        }
        let path = format!("cv.contact.links[{index}].url");
        let message = format!("malformed URL \"{url}\"");
        let diag = diag_at(
            spans,
            source,
            &path,
            message,
            "this link URL",
            Severity::Error,
        )
        .with_help("URLs must start with http:// or https:// and include a host");
        result.errors.push(diag);
    }
}

fn check_duplicate_titles(
    cv: &Cv,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut Checks,
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
        result.warnings.push(diag_at(
            spans,
            source,
            &path,
            message,
            "first occurrence",
            Severity::Warning,
        ));
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

/// Check the URLs inside inline Markdown (ADR-0017, ADR-0033). Two severities, because
/// the two cases differ: an empty target makes the *export* fail (`link()` rejects it,
/// and Typst's diagnostic would surface as a template error — spec §5 keeps those
/// distinct from user errors), while a target no PDF reader can resolve — a bare host, a
/// path relative to the PDF file — still renders and is only worth a warning
/// (ADR-0015). Other schemes (`mailto:`, `tel:`) are working links and stay quiet: the
/// check must not be stricter than the renderer (ADR-0031).
fn check_markdown_urls(
    cv: &Cv,
    spans: &spans::SpanMap,
    source: &miette::NamedSource<String>,
    result: &mut Checks,
) {
    for field in cv.markdown_texts() {
        for token in crate::markdown::parse_inline(field.text) {
            let crate::markdown::InlineToken::Link { text: label, url } = token else {
                continue;
            };
            let (message, severity, help) = if url.is_empty() {
                (
                    format!("the Markdown link \"{label}\" has no URL"),
                    Severity::Error,
                    "the PDF export refuses an empty link target — give it a URL or write the text without brackets",
                )
            } else if !is_resolvable_url(&url) {
                (
                    format!(
                        "the Markdown link \"{label}\" points at \"{url}\", which no PDF reader can open"
                    ),
                    Severity::Warning,
                    "a link in a PDF needs an absolute URL, e.g. https://example.dev or mailto:jane@example.dev",
                )
            } else {
                continue;
            };
            let diag = diag_at(
                spans,
                source,
                &field.span_path,
                message,
                "this Markdown link",
                severity,
            )
            .with_help(help.to_owned());
            if severity == Severity::Warning {
                result.warnings.push(diag);
            } else {
                result.errors.push(diag);
            }
        }
    }
}

/// Whether a PDF reader can act on this destination: an absolute scheme with something
/// resolvable behind it — a host for the `//host/path` forms, an address for `mailto:`
/// and friends — and no whitespace or control characters.
fn is_resolvable_url(url: &str) -> bool {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((scheme, target)) = url.split_once(':') else {
        return false; // a bare host or a path relative to the PDF resolves to nothing
    };
    if scheme.is_empty() || target.is_empty() {
        return false;
    }
    match target.strip_prefix("//") {
        Some(rest) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
            !host.is_empty()
        }
        None => true,
    }
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
    result: &mut Checks,
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
    result: &mut Checks,
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
        let diag = diag_at(
            spans,
            source,
            "design.template",
            message,
            "template used here",
            Severity::Warning,
        );
        result.warnings.push(diag);
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

    /// A Markdown destination is followable when it carries a scheme and a target:
    /// `https://`, `mailto:`, `tel:` all work in a PDF reader; a bare host, a relative
    /// path, an empty or space-containing URL does not (ADR-0033).
    #[test]
    fn resolvable_markdown_targets_are_absolute_and_space_free() {
        for url in [
            "https://example.dev/x?y=1#z",
            "http://example.dev",
            "mailto:jane@example.dev",
            "MAILTO:jane@example.dev",
            "tel:+49000000000",
            "ftp://files.example.dev/a",
        ] {
            assert!(is_resolvable_url(url), "{url} must be accepted");
        }
        for url in [
            "",
            "github.com/me",
            "/absolute/path",
            "./relative",
            "notes",
            "#anchor",
            "https://exa mple.dev",
            "https://example.dev\n",
            "https://",
            ":no-scheme-before-colon",
        ] {
            assert!(!is_resolvable_url(url), "{url} must be reported");
        }
    }

    /// The URLs inside Markdown are checked where they are written, so every
    /// Markdown-enabled field is covered — and the two severities split the way the
    /// renderer does (ADR-0033): an empty target fails the export so it is an error, a
    /// target no reader can open still renders so it only warns.
    #[test]
    fn markdown_link_urls_are_checked_in_every_markdown_field() {
        let yaml = concat!(
            "schema: 1\ncv:\n  name: X\n",
            "  summary: bare host [a](github.com/x) and [b]()\n",
            "  sections:\n",
            "    - title: E\n      type: entries\n      items:\n",
            "        - heading: H\n          bullets:\n            - mail [c](mailto:me@example.dev)\n",
            "    - title: B\n      type: bullets\n      items:\n",
            "        - schemeless [d](notes)\n",
            "    - title: T\n      type: text\n      items:\n",
            "        - fine [e](https://ok.dev/x)\n",
        );
        let document = crate::model::Document::from_yaml_str(yaml).expect("parses");
        let (errors, warnings) = markdown_url_diags(&document.cv, yaml);

        assert_eq!(
            messages(&errors),
            ["the Markdown link \"b\" has no URL"],
            "only the empty target, which the export itself refuses, is an error"
        );
        assert_eq!(
            messages(&warnings),
            [
                "the Markdown link \"a\" points at \"github.com/x\", which no PDF reader can open",
                "the Markdown link \"d\" points at \"notes\", which no PDF reader can open",
            ],
            "a bare host or a relative path warns where it is written; mailto stays quiet"
        );
        assert!(
            errors
                .iter()
                .chain(warnings.iter())
                .all(|d| d.labels.len() == 1),
            "every problem is labeled at the field that holds it"
        );
        assert!(
            errors.iter().all(|d| d
                .help
                .as_deref()
                .is_some_and(|h| h.contains("empty link target"))),
            "the error explains why it breaks the build"
        );
        assert!(
            warnings
                .iter()
                .all(|d| d.help.as_deref().is_some_and(|h| h.contains("mailto:"))),
            "the warning says what a resolvable target looks like"
        );
    }

    fn messages(diags: &[Diag]) -> Vec<&str> {
        diags.iter().map(|d| d.message.as_str()).collect()
    }

    /// Runs the check over one CV, returning errors and warnings separately.
    fn markdown_url_diags(cv: &crate::model::Cv, yaml: &str) -> (Vec<Diag>, Vec<Diag>) {
        let spans = spans::scalar_spans(yaml);
        let source = miette::NamedSource::new("cv.yaml", yaml.to_owned());
        let mut result = Checks::default();
        check_markdown_urls(cv, &spans, &source, &mut result);
        (result.errors, result.warnings)
    }

    #[test]
    fn a_value_the_span_scan_does_not_know_is_reported_without_a_snippet() {
        // The shape a YAML alias produces: the value is known, its text is not.
        let mut result = Checks::default();
        let source = miette::NamedSource::new("cv.yaml", "schema: 2\n".to_owned());
        check_schema_version(2, &spans::SpanMap::default(), &source, &mut result);
        assert_eq!(result.errors.len(), 1);
        let diag = &result.errors[0];
        assert_eq!(diag.message, "unsupported schema version 2");
        assert!(diag.labels.is_empty(), "no span, no label");
        assert_eq!(
            diag.help.as_deref(),
            Some("this tool understands schema version 1")
        );
        assert_eq!(diag.severity, Severity::Error);
    }

    fn support_warnings(yaml: &str, supported: &[&str]) -> Vec<Diag> {
        let spans = spans::scalar_spans(yaml);
        let source = miette::NamedSource::new("cv.yaml", yaml.to_owned());
        let supported: Vec<String> = supported.iter().map(|s| (*s).to_owned()).collect();
        let mut result = Checks::default();
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
        let mut result = Checks::default();
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
