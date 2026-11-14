// cv — compact template (spec §6): dense, single column, ideal for experienced engineers.
//
// Contract: reads the virtual `data.yaml` (cv + resolved design) and `sys.inputs.fit`
// (fit-loop scale factors). Pure Typst; no network, no file system access.
//
// Sizes/spacings are multiplied by the fit scales so the fitting loop (spec §7)
// can drive this template without source changes.

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
// built-in show-set rule; a user show-set rule overrides it (documented for 0.15,
// verified by the mono byte-diff test in tests/design_fields.rs).
#show raw: set text(font: design.fonts.mono)

// Section heading: small, semibold, accent-colored. Sticky (ADR-0044): when its
// section's body flows to the next page, the heading moves with it instead of
// stranding at the bottom of the previous one.
#let section-heading(title) = block(spacing: section-gap, breakable: false, sticky: true)[
  #text(size: heading-size, weight: "semibold", fill: accent, font: design.fonts.heading)[#upper(title)]
]

// One entries item. Built in code mode so the optional midline parts and bullets can be
// assembled with plain `if` (bare `if` lines inside markup brackets are literal text).
#let entry(item) = {
  let midline = ()
  if item.org != none { midline.push(text(item.org)) }
  if item.location != none { midline.push(text(fill: muted, item.location)) }
  let dates = if item.dates != none { text(size: small-size, fill: muted)[#item.dates] } else { none }
  let mid = if midline.len() > 0 { text(size: small-size)[#midline.join(text(fill: muted, "  ·  "))] } else { none }
  let bullets = if item.bullets != none { list(..item.bullets.map(inline), spacing: entry-gap * 0.5, tight: true) } else { none }
  block(spacing: entry-gap, breakable: false)[
    #grid(
      columns: (1fr, auto),
      align: (left, right),
      text(size: body-size, weight: "semibold", font: design.fonts.heading)[#item.heading],
      dates,
    )
    #if mid != none [#mid]
    #if mid != none [#v(0.3em)]
    #if bullets != none [#bullets]
  ]
}

// Sections stay whole on a page (ADR-0046): a section whose measured height fits
// the content area is wrapped in an unbreakable block, so Typst moves it whole to
// the next page instead of cutting it mid-list (the `block` case below). A section
// taller than the content area must flow — an unbreakable block that cannot fit a
// page is broken anyway (verified against the pinned Typst) — and the sticky
// heading keeps its body off a page bottom.
#let content-width = page-width - 2 * margin-x
#let content-height = page-height - 2 * margin-y
#let section-block(inner) = context {
  let height = measure(inner, width: content-width).height
  if height <= content-height {
    block(spacing: section-gap, breakable: false)[#inner]
  } else {
    inner
  }
}

// One section: heading, rhythm, then the body for its type — as content, so
// `section-block` can measure it and keep it whole when it fits a page.
#let section-content(section) = {
  let body = if section.type == "entries" {
    for item in section.items { entry(item) }
  } else if section.type == "key_values" {
    for item in section.items {
      grid(
        columns: (auto, 1fr),
        align: (left, left),
        column-gutter: 0.8em,
        text(weight: "semibold", font: design.fonts.heading)[#item.key],
        text(size: body-size)[#item.value],
      )
    }
  } else if section.type == "bullets" {
    list(..section.items.map(inline), spacing: entry-gap * 0.6, tight: true)
  } else if section.type == "text" {
    for paragraph in section.items { par[#inline(paragraph)] }
  }
  section-heading(section.title) + v(0.35em) + body
}

// ---- Header ----------------------------------------------------------------------

#align(center)[
  #text(size: name-size, weight: "semibold", font: design.fonts.heading)[#cv.name]

  #if cv.headline != none [
    #v(0.1em)
    #text(size: heading-size, fill: muted)[#cv.headline]
  ]

  #if cv.contact != none {
    let parts = ()
    if cv.contact.email != none { parts.push(text(fill: muted, cv.contact.email)) }
    if cv.contact.phone != none { parts.push(text(fill: muted, cv.contact.phone)) }
    if cv.location != none { parts.push(text(fill: muted, cv.location)) }
    if cv.contact.links != none {
      for entry-link in cv.contact.links {
        parts.push(link(entry-link.url)[#text(fill: accent, entry-link.label)])
      }
    }
    if parts.len() > 0 [
      #v(0.2em)
      #text(size: small-size)[#parts.join(text(fill: muted, "  ·  "))]
    ]
  }
]

#if cv.location != none and cv.contact == none [
  #v(0.2em)
  #text(size: small-size, fill: muted)[#cv.location]
]

#if cv.summary != none [
  #v(section-gap * 0.5)
  #par[#inline(cv.summary)]
]

// ---- Sections --------------------------------------------------------------------

// Sections that fit on one page are kept whole (ADR-0046): `section-block` wraps
// them in an unbreakable block, so Typst moves the entire section to the next
// page instead of cutting it mid-list. A section taller than a page flows, with
// its sticky heading glued to the first entry.
#{
  for section in cv.sections {
    section-block(section-content(section))
  }
}

// GDPR consent statement (ADR-0045): the user's own legal wording, always literal
// text, pinned to the bottom of the last page by the stretchable space above it.
#if cv.gdpr != none [
  #v(0.9em)
  #v(1fr)
  #block(spacing: 0pt, breakable: false)[
    #text(size: small-size, fill: muted)[#cv.gdpr]
  ]
]
