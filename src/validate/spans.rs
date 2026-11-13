//! Span-annotated YAML scan (ADR-0014): a map from value paths to source spans.
//!
//! `serde_yaml_ng` gives typed deserialization but no per-value spans, so semantic checks
//! use this second, span-annotated event scan (`saphyr-parser`) to find where a rejected
//! value lives in the source. Paths address values the way a template would read them:
//! `design.fonts.body`, `cv.sections[0].items[2].bullets[1]`.
//!
//! Spans are byte offsets into the source string, suitable for `miette` labels.

use std::collections::BTreeMap;

use saphyr_parser::{Event, Marker, Parser, Span, SpannedEventReceiver, StrInput};

/// A scalar's byte span `(start, end)` in the YAML source.
pub type ByteSpan = (usize, usize);

/// Path → span map for every scalar *value* in the document.
#[derive(Debug, Default)]
pub struct SpanMap {
    spans: BTreeMap<String, ByteSpan>,
}

impl SpanMap {
    pub fn get(&self, path: &str) -> Option<ByteSpan> {
        self.spans.get(path).copied()
    }

    /// The span of the first scalar at `path` or anywhere under it (`design.margins`
    /// covers `design.margins.x`). Deterministic in path order — not necessarily the
    /// source order of the user's keys.
    pub fn first_used(&self, path: &str) -> Option<ByteSpan> {
        let under = format!("{path}.");
        self.spans
            .range(path.to_owned()..)
            .find_map(|(key, span)| (key == path || key.starts_with(&under)).then_some(*span))
    }
}

/// Scan `yaml` and collect the byte span of every scalar value, addressed by path.
///
/// Malformed YAML yields a partial map (whatever was scanned before the error); callers
/// only reach semantic checks after the typed parse succeeded, so the map is complete on
/// every real path of use.
pub fn scalar_spans(yaml: &str) -> SpanMap {
    let mut walker = Walker {
        source: yaml,
        map: SpanMap::default(),
        stack: Vec::new(),
    };
    let mut parser = Parser::new(StrInput::new(yaml));
    // Ignore a scan error: a partial map is fine, the typed parse fails first anyway.
    let _ = parser.load(&mut walker, false);
    walker.map
}

/// Marker positions count characters; `miette` spans are byte-offset based, so convert to
/// stay exact when the YAML contains multi-byte characters.
fn byte_index(source: &str, marker: Marker) -> usize {
    source
        .char_indices()
        .nth(marker.index())
        .map_or(source.len(), |(byte, _)| byte)
}

fn span_of(source: &str, span: Span) -> ByteSpan {
    (byte_index(source, span.start), byte_index(source, span.end))
}

/// Join a parent path with a mapping key (`""` + `cv` → `cv`; `cv` + `name` → `cv.name`).
fn join_key(base: &str, key: &str) -> String {
    if base.is_empty() {
        key.to_owned()
    } else {
        format!("{base}.{key}")
    }
}

#[derive(Debug)]
enum Frame {
    /// Inside a mapping: `pending_key` is the key awaiting its value.
    Mapping {
        base: String,
        pending_key: Option<String>,
    },
    /// Inside a sequence: `next_index` is the index of the next element.
    Sequence { base: String, next_index: usize },
}

struct Walker<'a> {
    source: &'a str,
    map: SpanMap,
    stack: Vec<Frame>,
}

impl Walker<'_> {
    fn record(&mut self, path: String, span: ByteSpan) {
        self.map.spans.insert(path, span);
    }
}

/// What to do with a scalar event, decided from an immutable stack view so the mutable
/// phase never overlaps the lookup borrow.
enum Step {
    /// A mapping key seen before its value; remember it for the value event.
    RememberKey(String),
    /// A scalar value: record it under `path`, then update the owning frame
    /// (clear the pending key in a mapping, advance the index in a sequence).
    RecordValue(String),
}

impl SpannedEventReceiver<'_> for Walker<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::Scalar(value, ..) => {
                let byte_span = span_of(self.source, span);
                let step = match self.stack.last() {
                    Some(Frame::Mapping { base, pending_key }) => match pending_key {
                        None => Step::RememberKey(value.into_owned()),
                        Some(key) => Step::RecordValue(join_key(base, key)),
                    },
                    Some(Frame::Sequence { base, next_index }) => {
                        Step::RecordValue(format!("{base}[{next_index}]"))
                    }
                    None => Step::RecordValue(String::new()),
                };
                match step {
                    Step::RememberKey(key) => {
                        if let Some(Frame::Mapping { pending_key, .. }) = self.stack.last_mut() {
                            *pending_key = Some(key);
                        }
                    }
                    Step::RecordValue(path) => {
                        self.record(path, byte_span);
                        match self.stack.last_mut() {
                            Some(Frame::Mapping { pending_key, .. }) => *pending_key = None,
                            Some(Frame::Sequence { next_index, .. }) => *next_index += 1,
                            None => {}
                        }
                    }
                }
            }
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                let base = match self.stack.last_mut() {
                    Some(Frame::Mapping { base, pending_key }) => {
                        let key = pending_key.take().unwrap_or_default();
                        join_key(base, &key)
                    }
                    Some(Frame::Sequence { base, next_index }) => {
                        let path = format!("{base}[{next_index}]");
                        *next_index += 1;
                        path
                    }
                    None => String::new(),
                };
                self.stack.push(match event {
                    Event::MappingStart(..) => Frame::Mapping {
                        base,
                        pending_key: None,
                    },
                    _ => Frame::Sequence {
                        base,
                        next_index: 0,
                    },
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                self.stack.pop();
            }
            Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_)
            | Event::DocumentEnd
            | Event::Alias(_)
            | Event::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "schema: 1\ncv:\n  name: Jane Doe\n  sections:\n    - title: Experience\n      items:\n        - heading: A\ndesign:\n  fonts:\n    body: Inter\n  margins: { x: 1cm, y: 1cm }\n";

    #[test]
    fn maps_top_level_scalars() {
        let map = scalar_spans(DOC);
        let (start, end) = map.get("schema").expect("schema span");
        let text = &DOC[start..end];
        assert_eq!(text, "1");
    }

    #[test]
    fn maps_nested_scalars() {
        let map = scalar_spans(DOC);
        let (start, end) = map.get("cv.name").expect("name span");
        assert_eq!(&DOC[start..end], "Jane Doe");
        let (start, end) = map.get("design.fonts.body").expect("font span");
        assert_eq!(&DOC[start..end], "Inter");
    }

    #[test]
    fn maps_sequence_elements_with_indices() {
        let map = scalar_spans(DOC);
        let (start, end) = map.get("cv.sections[0].title").expect("title span");
        assert_eq!(&DOC[start..end], "Experience");
        let (start, end) = map
            .get("cv.sections[0].items[0].heading")
            .expect("heading span");
        assert_eq!(&DOC[start..end], "A");
    }

    #[test]
    fn maps_flow_mapping_values() {
        let map = scalar_spans(DOC);
        let (start, end) = map.get("design.margins.x").expect("margin span");
        assert_eq!(&DOC[start..end], "1cm");
    }

    #[test]
    fn spans_stay_exact_with_multibyte_characters() {
        let yaml = "cv:\n  name: Jürgen Müller\n";
        let map = scalar_spans(yaml);
        let (start, end) = map.get("cv.name").expect("name span");
        assert_eq!(&yaml[start..end], "Jürgen Müller");
    }

    #[test]
    fn block_scalars_cover_their_block() {
        let yaml = "cv:\n  summary: >\n    long text\n    more text\n";
        let map = scalar_spans(yaml);
        let (start, end) = map.get("cv.summary").expect("summary span");
        assert!(end > start, "block scalar must have a non-empty span");
    }

    #[test]
    fn quoted_scalars_span_the_quotes() {
        let yaml = "cv:\n  name: \"Jane\"\n";
        let map = scalar_spans(yaml);
        let (start, end) = map.get("cv.name").expect("name span");
        assert_eq!(&yaml[start..end], "\"Jane\"");
    }

    #[test]
    fn missing_paths_return_none() {
        let map = scalar_spans(DOC);
        assert_eq!(map.get("cv.nonexistent"), None);
    }

    #[test]
    fn first_used_covers_a_group_and_an_exact_scalar() {
        let yaml = "design:\n  page: a4\n  margins: { x: 1cm, y: 1cm }\n";
        let map = scalar_spans(yaml);
        let (start, _) = map.first_used("design.page").expect("page is a scalar");
        assert_eq!(&yaml[start..start + 2], "a4");
        let (start, _) = map
            .first_used("design.margins")
            .expect("margins has scalars under it");
        assert_eq!(&yaml[start..start + 3], "1cm", "first key in path order");
        assert_eq!(map.first_used("design.spacing"), None, "nothing written");
        assert!(
            map.first_used("design").is_some(),
            "a bare group with values under it counts as used"
        );
    }

    #[test]
    fn malformed_yaml_yields_a_partial_map() {
        let map = scalar_spans("schema: 1\nbad: [unclosed\n");
        assert!(
            map.get("schema").is_some(),
            "scanned entries before the error"
        );
    }

    #[test]
    fn a_bare_scalar_document_records_the_empty_path() {
        let yaml = "just text\n";
        let map = scalar_spans(yaml);
        let (start, end) = map.get("").expect("root scalar span");
        assert_eq!(&yaml[start..end], "just text");
    }
}
