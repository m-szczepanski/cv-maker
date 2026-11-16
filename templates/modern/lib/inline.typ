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
