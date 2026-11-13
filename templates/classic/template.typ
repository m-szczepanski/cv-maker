// cv — classic template (spec §6): conventional layout with a header rule and
// section dividers.
//
// Contract: reads the virtual `data.yaml` (cv + resolved design) and `sys.inputs.fit`
// (fit-loop scale factors). Pure Typst; no network, no file system access.
//
// Sizes/spacings/margins are multiplied by the fit scales exactly as in `compact`, so
// the one-page fitting loop (spec §7) drives both templates through one contract.

#let data = yaml("data.yaml")
#let cv = data.cv
#let design = data.design
#let fit = sys.inputs.at("fit", default: (font_scale: 1, spacing_scale: 1, margin_scale: 1))
#import "lib/inline.typ": inline

// {value, unit} objects from data.yaml → Typst lengths. Unknown units fail the compile
// loudly — they cannot occur in tool-generated data.yaml, so reaching the assert would
// mean the data contract broke.
#let unit-scale(unit) = if unit == "pt" { 1pt } else if unit == "cm" { 1cm } else if unit == "mm" { 1mm } else if unit == "in" { 1in } else { assert(false, message: "unknown length unit in data.yaml: " + unit); 1pt }
#let to-length(x) = x.value * unit-scale(x.unit)

#let page-width = if design.page == "letter" { 8.5in } else { 21cm }
#let page-height = if design.page == "letter" { 11in } else { 29.7cm }
#let margin-x = to-length(design.margins.x) * fit.margin_scale
#let margin-y = to-length(design.margins.y) * fit.margin_scale

#let body-size = to-length(design.sizes.body) * fit.font_scale
#let heading-size = to-length(design.sizes.heading) * fit.font_scale
#let small-size = to-length(design.sizes.small) * fit.font_scale
#let name-size = to-length(design.sizes.name) * fit.font_scale

#let section-gap = to-length(design.spacing.section_gap) * fit.spacing_scale
#let entry-gap = to-length(design.spacing.entry_gap) * fit.spacing_scale
// Line-height tightening (ADR-0024): only the leading *above* 1.0 is scaled by
// the fit loop, so a tight fit can never pull line height below 1.0 and overlap
// lines.
#let line-height = 1 + (design.spacing.line_height - 1) * fit.spacing_scale

#let accent = rgb(design.colors.accent)
#let text-color = rgb(design.colors.text)
#let muted = rgb(design.colors.muted)

#set document(
  title: cv.name,
  author: (cv.name,),
  date: none,
)

#set page(
  width: page-width,
  height: page-height,
  margin: (x: margin-x, y: margin-y),
)

#set text(
  font: design.fonts.body,
  size: body-size,
  fill: text-color,
  lang: "en",
)

#set par(leading: body-size * line-height - body-size, spacing: body-size * line-height * 0.9, justify: false)

// Honor `design.fonts.mono`: Typst's `raw` element hardcodes DejaVu Sans Mono via a
// built-in show-set rule; a user show-set rule overrides it (same mechanism as
// `compact`, pinned by the mono byte-diff test in tests/design_fields.rs).
#show raw: set text(font: design.fonts.mono)

// Section heading: title-case, bold, with a thin divider rule across the text column.
#let section-heading(title) = block(spacing: 0pt, breakable: false)[
  #text(size: heading-size, weight: "bold", fill: accent, font: design.fonts.heading)[#title]
  #v(-0.3em)
  #line(length: 100%, stroke: 0.4pt + muted)
  #v(0.4em)
]

// One entries item: heading left, dates right; org and location on the second line.
// Built in code mode so the optional parts and bullets can be assembled with plain
// `if` (bare `if` lines inside markup brackets are literal text).
#let entry(item) = {
  let second-line = ()
  if item.org != none { second-line.push(text(weight: "medium", item.org)) }
  if item.location != none {
    if item.org != none { second-line.push(text(fill: muted, " — ")) }
    second-line.push(text(fill: muted, item.location))
  }
  let dates = if item.dates != none { text(size: small-size, fill: muted)[#item.dates] } else { none }
  let bullets = if item.bullets != none { list(..item.bullets.map(inline), spacing: entry-gap * 0.5, tight: true) } else { none }
  block(spacing: entry-gap, breakable: false)[
    #grid(
      columns: (1fr, auto),
      align: (left, right),
      text(size: body-size, weight: "bold", font: design.fonts.heading)[#item.heading],
      dates,
    )
    #if second-line.len() > 0 [#v(-0.1em) #text(size: small-size)[#second-line.join()]]
    #if bullets != none [#v(0.25em) #bullets]
  ]
}

// ---- Header ----------------------------------------------------------------------
// Name and headline left, contact stacked right under the same rule (the classic
// header). All contact parts stay literal text (ADR-0016: no Markdown outside tokens).

#{
  let lines = ()
  if cv.contact != none {
    if cv.contact.email != none { lines.push(text(size: small-size, fill: muted, cv.contact.email)) }
    if cv.contact.phone != none { lines.push(text(size: small-size, fill: muted, cv.contact.phone)) }
  }
  if cv.location != none { lines.push(text(size: small-size, fill: muted, cv.location)) }
  if cv.contact != none and cv.contact.links != none {
    for link-item in cv.contact.links {
      lines.push(link(link-item.url)[#text(size: small-size, fill: accent, link-item.label)])
    }
  }

  block(breakable: false)[
    #grid(
      columns: (1fr, auto),
      align: (left, right),
      [
        #text(size: name-size, weight: "bold", font: design.fonts.heading)[#cv.name]
        #if cv.headline != none [
          #v(0.15em)
          #text(size: heading-size, fill: muted)[#cv.headline]
        ]
      ],
      [
        #if lines.len() > 0 [#lines.join(linebreak())]
      ],
    )
    #v(0.5em)
    #line(length: 100%, stroke: 0.8pt + accent)
  ]
}

#if cv.summary != none [
  #v(section-gap * 0.75)
  #par[#inline(cv.summary)]
]

// ---- Sections --------------------------------------------------------------------

#for section in cv.sections [
  #block(spacing: section-gap, breakable: false)[
    #section-heading(section.title)
    #{
      if section.type == "entries" {
        for item in section.items { entry(item) }
      } else if section.type == "key_values" {
        for item in section.items {
          grid(
            columns: (auto, 1fr),
            align: (left, left),
            column-gutter: 0.8em,
            text(weight: "bold", font: design.fonts.heading)[#item.key],
            text(size: body-size)[#item.value],
          )
        }
      } else if section.type == "bullets" {
        list(..section.items.map(inline), spacing: entry-gap * 0.6, tight: true)
      } else if section.type == "text" {
        for paragraph in section.items { par[#inline(paragraph)] }
      }
    }
  ]
]
