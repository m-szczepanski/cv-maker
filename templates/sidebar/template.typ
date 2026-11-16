// cv — sidebar template (spec §6): two-column layout — a tinted left rail carries the
// name, contact block and the short sections, the main column the narrative ones.
//
// Routing is by section type (deterministic, no per-section flags in the content
// model): `key_values` and `bullets` sections go to the rail, `entries` and `text`
// sections to the main column.
//
// Contract: reads the virtual `data.yaml` (cv + resolved design) and `sys.inputs.fit`
// (fit-loop scale factors). Pure Typst; no network, no file system access.
//
// Sizes/spacings/margins are multiplied by the fit scales as in `compact`, so the
// fitting loop (spec §7) drives this template through one contract. The rail width is
// structural and deliberately unscaled: shrinking it would only re-wrap the rail
// text without giving the fit loop usable height back.

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

// The rail: its content width, the tint panel behind it (bleeding to the page's left
// edge), the gutter between the columns, and the padding inside the panel.
#let rail-width = 5.2cm
#let gutter = 1.1em
#let rail-inset = 0.45cm
#let rail-tint = color.mix((accent, 6%), (white, 94%))

#set document(
  title: cv.name,
  author: (cv.name,),
  date: none,
)

#set page(
  width: page-width,
  height: page-height,
  margin: (x: margin-x, y: margin-y),
  // Relative lengths in page background content resolve against the full page, so
  // `height: 100%` spans every page: the columns split across pages, the rail tint
  // repeats on each as one continuous panel. Marginal content is laid out with
  // `center + horizon` alignment (verified against the pinned layout code), so the
  // rect must be pinned to the page edge explicitly — a bare rect lands mid-page.
  background: align(top + left, rect(
    width: margin-x + rail-width,
    height: 100%,
    fill: rail-tint,
  )),
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

// Section heading: uppercase accent label. Sticky (ADR-0044): when its section's
// body flows to the next page, the heading moves with it instead of stranding at
// the bottom of the previous one.
#let section-heading(title) = block(spacing: section-gap, breakable: false, sticky: true)[
  #text(size: heading-size, weight: "semibold", fill: accent, font: design.fonts.heading)[#upper(title)]
]

// One entries item: heading left, dates right; org and location on the second line.
// Built in code mode so the optional parts and bullets can be assembled with plain
// `if` (bare `if` lines inside markup brackets are literal text).
#let entry(item) = {
  let second-line = ()
  if item.org != none { second-line.push(text(weight: "semibold", item.org)) }
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
      text(size: body-size, weight: "semibold", font: design.fonts.heading)[#item.heading],
      dates,
    )
    #if second-line.len() > 0 [#v(-0.1em) #text(size: small-size)[#second-line.join()]]
    #if bullets != none [#v(0.25em) #bullets]
  ]
}

// Keep whole sections on a page (ADR-0046): a section whose measured height fits
// the content area is wrapped in an unbreakable block, so Typst moves it whole to
// the next page instead of cutting a list in half. `measure` is contextual: the
// page and margins above are its inputs; `width` is the column's text width. A
// section taller than the content area must flow (an unbreakable block that cannot
// fit a page is broken anyway — verified against the pinned Typst), and the sticky
// heading keeps its body off a page bottom.
#let content-height = page-height - 2 * margin-y
#let section-block(inner, width) = context {
  let height = measure(inner, width: width).height
  if height <= content-height {
    block(spacing: section-gap, breakable: false)[#inner]
  } else {
    inner
  }
}

// One section: heading, rhythm, then the body for its type — as content, so
// `section-block` can measure it and keep it whole when it fits a page.
// `rail: true` renders the narrow-column variants: key/value pairs stack, text is
// set at the small size.
#let section-content(section, rail: false) = {
  let body = if section.type == "entries" {
    for item in section.items { entry(item) }
  } else if section.type == "key_values" {
    if rail {
      for item in section.items {
        block(spacing: entry-gap * 0.6, breakable: false)[
          #text(size: small-size, weight: "semibold", font: design.fonts.heading)[#item.key]
          #text(size: small-size, fill: muted)[#item.value]
        ]
      }
    } else {
      for item in section.items {
        grid(
          columns: (auto, 1fr),
          align: (left, left),
          column-gutter: 0.8em,
          text(weight: "semibold", font: design.fonts.heading)[#item.key],
          text(size: body-size)[#item.value],
        )
      }
    }
  } else if section.type == "bullets" {
    let render = if rail { token => text(size: small-size, fill: text-color, inline(token)) } else { inline }
    list(..section.items.map(render), spacing: entry-gap * 0.6, tight: true)
  } else if section.type == "text" {
    for paragraph in section.items { par[#inline(paragraph)] }
  }
  section-heading(section.title) + v(0.35em) + body
}

// The columns' text widths, shared by the layout and the whole-section measurements.
#let main-width = page-width - 2 * margin-x - rail-width - gutter
#let rail-text-width = rail-width - rail-inset - rail-inset * 0.6

// ---- Rail header ------------------------------------------------------------------
// Name and headline lead the rail; contact follows. All header parts stay literal
// text (ADR-0016: no Markdown outside tokens).

#let contact-lines = {
  let lines = ()
  if cv.contact != none {
    if cv.contact.email != none { lines.push(text(size: small-size, fill: text-color)[#cv.contact.email]) }
    if cv.contact.phone != none { lines.push(text(size: small-size, fill: text-color)[#cv.contact.phone]) }
  }
  if cv.location != none { lines.push(text(size: small-size, fill: text-color)[#cv.location]) }
  if cv.contact != none and cv.contact.links != none {
    for link-item in cv.contact.links {
      lines.push(link(link-item.url)[#text(size: small-size, fill: accent, link-item.label)])
    }
  }
  lines
}

#{
  let rail-sections = cv.sections.filter(s => s.type == "key_values" or s.type == "bullets")
  let main-sections = cv.sections.filter(s => s.type == "entries" or s.type == "text")

  grid(
    columns: (rail-width, 1fr),
    column-gutter: gutter,
    align: (left, left),
    pad(
      left: rail-inset,
      right: rail-inset * 0.6,
      [
        #block(breakable: false)[
          #text(size: name-size * 0.8, weight: "bold", font: design.fonts.heading)[#cv.name]
          #if cv.headline != none [
            #v(0.1em)
            #text(size: small-size, fill: muted)[#cv.headline]
          ]
        ]
        #if contact-lines.len() > 0 [
          #section-heading("Contact")
          #v(0.2em)
          #contact-lines.join(linebreak())
        ]
        #for section in rail-sections {
          section-block(section-content(section, rail: true), rail-text-width)
        }
      ],
    ),
    [
      #if cv.summary != none [
        #par[#inline(cv.summary)]
        #v(section-gap * 0.5)
      ]
      #for section in main-sections {
        section-block(section-content(section), main-width)
      }
    ],
  )
}

// GDPR consent statement (ADR-0045): the user's own legal wording, always literal
// text, pinned to the bottom of the last page by the stretchable space above it.
// The grid keeps it aligned with the main column, clear of the rail tint.
#if cv.gdpr != none [
  #v(0.9em)
  #v(1fr)
  #grid(
    columns: (rail-width, 1fr),
    column-gutter: gutter,
    [],
    block(spacing: 0pt, breakable: false)[
      #text(size: small-size, fill: muted)[#cv.gdpr]
    ],
  )
]
