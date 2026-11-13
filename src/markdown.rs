//! Inline Markdown subset (ADR-0016, spec §4): `**bold**`, `*italic*`, `` `code` ``,
//! `[label](url)` — parsed into a flat, non-nested token list before serialization.
//!
//! Flat by design: markers inside token contents are literal text, and unclosed or
//! malformed markers stay literal (no error, no recovery beyond moving on). User text is
//! never rewritten into Typst source; tokens reach templates as plain strings rendered
//! literally, so the escaping guarantee (spec §4, plan 2.6) holds without an
//! escaping table.

use serde::Serialize;

/// One inline token; serialized as `{kind, text, url?}` in `data.yaml` (ADR-0016).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineToken {
    Text { text: String },
    Bold { text: String },
    Italic { text: String },
    Code { text: String },
    Link { text: String, url: String },
}

/// A string that serializes as its parsed inline-token list instead of the raw text
/// (ADR-0016). Parsing is total: every input yields a token list.
#[derive(Debug, Clone)]
pub struct MdString<'a>(pub &'a str);

impl Serialize for MdString<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        parse_inline(self.0).serialize(serializer)
    }
}

/// Parse the ADR-0016 subset into flat tokens. Markers are recognized only when both the
/// opening and the closing marker are present; otherwise the marker character(s) are
/// literal text. Content between markers is not re-parsed (no nesting).
pub fn parse_inline(source: &str) -> Vec<InlineToken> {
    let mut tokens: Vec<InlineToken> = Vec::new();
    // Byte offset where the current literal (unmarked) run began. All marker characters
    // are ASCII, so byte scanning and slicing at found offsets stay on char boundaries.
    let mut run_start = 0usize;
    let mut pos = 0usize;

    while pos < source.len() {
        let rest = &source[pos..];
        match rest.as_bytes()[0] {
            b'*' if rest.starts_with("**") => {
                if let Some(close) = rest[2..].find("**") {
                    let content = &rest[2..2 + close];
                    push_text(&mut tokens, &source[run_start..pos]);
                    tokens.push(InlineToken::Bold {
                        text: content.to_owned(),
                    });
                    pos += 2 + close + 2;
                    run_start = pos;
                    continue;
                }
                // Unclosed: both stars are literal; skip them so the next `*` is not
                // re-tried as italic.
                pos += 2;
            }
            b'*' => {
                if let Some(close) = rest[1..].find('*') {
                    let content = &rest[1..1 + close];
                    push_text(&mut tokens, &source[run_start..pos]);
                    tokens.push(InlineToken::Italic {
                        text: content.to_owned(),
                    });
                    pos += 1 + close + 1;
                    run_start = pos;
                    continue;
                }
                pos += 1;
            }
            b'`' => {
                if let Some(close) = rest[1..].find('`') {
                    let content = &rest[1..1 + close];
                    push_text(&mut tokens, &source[run_start..pos]);
                    tokens.push(InlineToken::Code {
                        text: content.to_owned(),
                    });
                    pos += 1 + close + 1;
                    run_start = pos;
                    continue;
                }
                pos += 1;
            }
            b'[' => match match_link(rest) {
                Some((consumed, label, url)) => {
                    push_text(&mut tokens, &source[run_start..pos]);
                    tokens.push(InlineToken::Link {
                        text: label.to_owned(),
                        url: url.to_owned(),
                    });
                    pos += consumed;
                    run_start = pos;
                }
                // Not a link: the bracket is literal text.
                None => pos += 1,
            },
            _ => {
                // Advance one full character, not one byte, so `pos` stays on a char
                // boundary (markers are ASCII but the text between them is not).
                let width = source[pos..].chars().next().map_or(1, |c| c.len_utf8());
                pos += width;
            }
        }
    }

    push_text(&mut tokens, &source[run_start..]);
    tokens
}

/// Match `[label](url)` at the start of `rest`: label up to the first `]`, `(` right
/// after it, url up to the first `)`. Returns `(bytes consumed, label, url)`; `None` when
/// the shape is broken (then the bracket is literal text).
fn match_link(rest: &str) -> Option<(usize, &str, &str)> {
    let bracket = rest[1..].find(']')?;
    let after_label = 1 + bracket + 1;
    if !rest[after_label..].starts_with('(') {
        return None;
    }
    let paren = rest[after_label + 1..].find(')')?;
    let label = &rest[1..1 + bracket];
    let url = &rest[after_label + 1..after_label + 1 + paren];
    Some((after_label + 1 + paren + 1, label, url))
}

/// Literal runs only become tokens when non-empty, so `a**b` stays one text token.
fn push_text(tokens: &mut Vec<InlineToken>, text: &str) {
    if !text.is_empty() {
        tokens.push(InlineToken::Text {
            text: text.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(tokens: &[InlineToken]) -> Vec<&str> {
        tokens
            .iter()
            .map(|t| match t {
                InlineToken::Text { .. } => "text",
                InlineToken::Bold { .. } => "bold",
                InlineToken::Italic { .. } => "italic",
                InlineToken::Code { .. } => "code",
                InlineToken::Link { .. } => "link",
            })
            .collect()
    }

    #[test]
    fn plain_text_is_a_single_text_token() {
        let tokens = parse_inline("Senior Rust engineer");
        assert_eq!(
            tokens,
            vec![InlineToken::Text {
                text: "Senior Rust engineer".into()
            }]
        );
    }

    #[test]
    fn empty_input_yields_no_tokens() {
        assert_eq!(parse_inline(""), Vec::new());
    }

    #[test]
    fn bold_italic_code_are_parsed() {
        let tokens = parse_inline("**b** *i* `c`");
        assert_eq!(
            kinds(&tokens),
            vec!["bold", "text", "italic", "text", "code"]
        );
        assert_eq!(tokens[0], InlineToken::Bold { text: "b".into() });
        assert_eq!(tokens[2], InlineToken::Italic { text: "i".into() });
        assert_eq!(tokens[4], InlineToken::Code { text: "c".into() });
    }

    #[test]
    fn adjacent_markers_produce_no_text_tokens_between() {
        let tokens = parse_inline("**b**_plain");
        assert_eq!(kinds(&tokens), vec!["bold", "text"]);
        assert_eq!(
            tokens[1],
            InlineToken::Text {
                text: "_plain".into()
            }
        );
    }

    #[test]
    fn links_are_parsed_with_label_and_url() {
        let tokens = parse_inline("see [my site](https://example.dev)");
        assert_eq!(kinds(&tokens), vec!["text", "link"]);
        assert_eq!(
            tokens[1],
            InlineToken::Link {
                text: "my site".into(),
                url: "https://example.dev".into()
            }
        );
    }

    #[test]
    fn link_needs_parenthesis_right_after_label() {
        // `]` not followed by `(`: the bracket is literal text, nothing is marked up.
        let tokens = parse_inline("[a] (b)");
        assert_eq!(kinds(&tokens), vec!["text"]);
        assert_eq!(
            tokens[0],
            InlineToken::Text {
                text: "[a] (b)".into()
            }
        );
    }

    #[test]
    fn link_without_closing_paren_is_literal() {
        let tokens = parse_inline("[a](https://x");
        assert_eq!(kinds(&tokens), vec!["text"]);
    }

    #[test]
    fn unclosed_bold_is_literal_and_does_not_retrigger_italic() {
        let tokens = parse_inline("**un *closed");
        assert_eq!(kinds(&tokens), vec!["text"]);
        assert_eq!(
            tokens[0],
            InlineToken::Text {
                text: "**un *closed".into()
            }
        );
    }

    #[test]
    fn unclosed_italic_and_code_are_literal() {
        let tokens = parse_inline("*open `tick");
        assert_eq!(kinds(&tokens), vec!["text"]);
    }

    #[test]
    fn trailing_marker_is_literal() {
        let tokens = parse_inline("bold**");
        assert_eq!(kinds(&tokens), vec!["text"]);
        assert_eq!(
            tokens[0],
            InlineToken::Text {
                text: "bold**".into()
            }
        );
    }

    #[test]
    fn markers_inside_token_contents_are_literal() {
        let tokens = parse_inline("**a *b* c**");
        assert_eq!(kinds(&tokens), vec!["bold"]);
        assert_eq!(
            tokens[0],
            InlineToken::Bold {
                text: "a *b* c".into()
            }
        );
    }

    #[test]
    fn bold_closes_at_first_double_star() {
        let tokens = parse_inline("**a** **b**");
        assert_eq!(kinds(&tokens), vec!["bold", "text", "bold"]);
        assert_eq!(tokens[0], InlineToken::Bold { text: "a".into() });
        assert_eq!(tokens[2], InlineToken::Bold { text: "b".into() });
    }

    #[test]
    fn triple_star_is_literal_when_no_bold_closes() {
        // `**` cannot close inside a single trailing `*`, so all three stars stay text.
        let tokens = parse_inline("***");
        assert_eq!(kinds(&tokens), vec!["text"]);
        assert_eq!(tokens[0], InlineToken::Text { text: "***".into() });
    }

    #[test]
    fn empty_bold_marker_is_a_token() {
        let tokens = parse_inline("****");
        assert_eq!(tokens, vec![InlineToken::Bold { text: "".into() }]);
    }

    #[test]
    fn multibyte_content_stays_in_token() {
        let tokens = parse_inline("**Jürgen**");
        assert_eq!(
            tokens,
            vec![InlineToken::Bold {
                text: "Jürgen".into()
            }]
        );
    }

    #[test]
    fn multibyte_text_around_marker_is_preserved() {
        let tokens = parse_inline("ü **b** ä");
        assert_eq!(kinds(&tokens), vec!["text", "bold", "text"]);
        assert_eq!(tokens[0], InlineToken::Text { text: "ü ".into() });
        assert_eq!(tokens[2], InlineToken::Text { text: " ä".into() });
    }

    #[test]
    fn md_string_serializes_as_token_list() {
        let md = MdString("**a**");
        let yaml = serde_yaml_ng::to_string(&md).expect("token list serializes");
        assert_eq!(yaml, "- kind: bold\n  text: a\n");
    }

    #[test]
    fn md_string_link_serializes_with_url() {
        let md = MdString("[a](https://x.dev)");
        let yaml = serde_yaml_ng::to_string(&md).expect("token list serializes");
        assert_eq!(yaml, "- kind: link\n  text: a\n  url: https://x.dev\n");
    }

    #[test]
    fn match_link_returns_label_url_and_consumed_length() {
        let rest = "[my site](https://example.dev) tail";
        let (consumed, label, url) = match_link(rest).expect("well-formed link");
        assert_eq!(consumed, rest.find(" tail").expect("tail position"));
        assert_eq!(label, "my site");
        assert_eq!(url, "https://example.dev");
    }

    #[test]
    fn match_link_rejects_broken_shapes() {
        assert_eq!(match_link("[a] (b)"), None, "`(` must follow `]` directly");
        assert_eq!(match_link("[a](https://x"), None, "unclosed `(`");
        assert_eq!(match_link("[a"), None, "unclosed `]`");
    }
}
