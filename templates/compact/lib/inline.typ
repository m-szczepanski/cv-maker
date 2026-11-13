// Inline Markdown rendering for cv templates (docs/decisions.md ADR-0016).
//
// The tool pre-parses the Markdown subset (`**bold**`, `*italic*`, `` `code` ``,
// `[label](url)`) into a flat token list carried in data.yaml. Each token is
// `{kind, text, url?}` with kind one of text|bold|italic|code|link. Tokens are flat and
// pre-parsed, and every string here renders literally — user text can never inject
// Typst syntax through data.yaml.
//
// Unknown kinds fail the compile loudly: they cannot occur in tool-generated data.yaml,
// so reaching the assert would mean the data contract broke.

#let render-token(token) = {
  if token.kind == "text" {
    token.text
  } else if token.kind == "bold" {
    strong(token.text)
  } else if token.kind == "italic" {
    emph(token.text)
  } else if token.kind == "code" {
    raw(token.text)
  } else if token.kind == "link" {
    link(token.url, token.text)
  } else {
    assert(false, message: "unknown inline token kind in data.yaml: " + token.kind)
    token.text
  }
}

// Render a token list (as stored in data.yaml) as Typst content.
#let inline(tokens) = tokens.map(render-token).join()
