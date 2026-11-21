// cv — DIY template (spec §6): a fully parametrized layout engine. All structure
// choices live in `config.typ` next to this file — edit that, not this one, unless
// you want to go deeper. Everything typographic (fonts, sizes, spacing, colors,
// margins, page size) still comes from the `design` block of your YAML, and the
// fit loop (spec §7) keeps working through the same `fit` scales as `compact`.
//
// Extract a personal copy with `cv templates --extract diy --out ./my-template`
// and point `design.template: ./my-template` at it.
//
// Contract: reads the virtual `data.yaml` (cv + resolved design) and
// `sys.inputs.fit` (fit-loop scale factors). Pure Typst; no network, no file
// system access.

#let data = yaml("data.yaml")
#let cv = data.cv
#let design = data.design
#let fit = sys.inputs.at("fit", default: (font_scale: 1, spacing_scale: 1, margin_scale: 1))
#import "lib/inline.typ": inline
#import "config.typ": cfg

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
#let tint-fill = color.mix((accent, 7%), (white, 93%))

// ---- Knob normalization -----------------------------------------------------------
// Unknown knob values fail the compile loudly with the accepted set: a typo in
// config.typ is a user error to fix there, not a silent fallback to a different look.

#let layout = if "single sidebar-left sidebar-right".split(" ").contains(cfg.layout) { cfg.layout } else { assert(false, message: "diy: cfg.layout must be \"single\", \"sidebar-left\" or \"sidebar-right\""); "single" }
#let sidebar = layout != "single"
#let rail-right = layout == "sidebar-right"

#let rail-width = if type(cfg.sidebar-width) == length { cfg.sidebar-width } else { assert(false, message: "diy: cfg.sidebar-width must be a length like 6cm"); 6cm }
#let rail-gutter = 1.1em
#let rail-inset = 0.5cm

#let heading-case = if "upper title none".split(" ").contains(cfg.heading-case) { cfg.heading-case } else { assert(false, message: "diy: cfg.heading-case must be \"upper\", \"title\" or \"none\""); "upper" }
#let heading-style = if "plain bar rule".split(" ").contains(cfg.heading-style) { cfg.heading-style } else { assert(false, message: "diy: cfg.heading-style must be \"plain\", \"bar\" or \"rule\""); "plain" }
#let dates-style = if "right below none".split(" ").contains(cfg.dates) { cfg.dates } else { assert(false, message: "diy: cfg.dates must be \"right\", \"below\" or \"none\""); "right" }
#let header-style = if "none tint accent".split(" ").contains(cfg.header-style) { cfg.header-style } else { assert(false, message: "diy: cfg.header-style must be \"none\", \"tint\" or \"accent\""); "none" }
#let header-align = if "left center".split(" ").contains(cfg.header-align) { cfg.header-align } else { assert(false, message: "diy: cfg.header-align must be \"left\" or \"center\""); "left" }
#let contact-style = if "dots stack split".split(" ").contains(cfg.contact) { cfg.contact } else { assert(false, message: "diy: cfg.contact must be \"dots\", \"stack\" or \"split\""); "dots" }
#let justify = if type(cfg.justify) == bool { cfg.justify } else { assert(false, message: "diy: cfg.justify must be true or false"); false }

// Typst's `upper`/`lower` cover two of the three cases; title case is spelled out
// (0.15 has no `titlecase`).
#let title-case(s) = {
  s.split(" ").map(w => if w.len() == 0 { w } else { upper(w.slice(0, 1)) + w.slice(1, w.len()) }).join(" ")
}

#let set-case(s) = {
  if heading-case == "upper" { upper(s) }
  else if heading-case == "title" { title-case(s) }
  else { s }
}

#set document(
  title: cv.name,
  author: (cv.name,),
  date: none,
)

#set page(
  width: page-width,
  height: page-height,
  margin: (x: margin-x, y: margin-y),
  // In sidebar layouts the rail keeps its tint on every page: relative lengths in
  // page background resolve against the full page, so one rect spans each page and
  // the columns split underneath it. Marginal content is laid out with `center +
  // horizon` alignment (verified against the pinned layout code), so the rect must
  // be pinned to its page edge explicitly — a bare rect lands mid-page.
  background: if sidebar {
    let rail-band = rect(width: margin-x + rail-width, height: 100%, fill: tint-fill)
    if rail-right { align(top + right, rail-band) } else { align(top + left, rail-band) }
  } else { none },
)

#set text(
  font: design.fonts.body,
  size: body-size,
  fill: text-color,
  lang: "en",
)

#set par(leading: body-size * line-height - body-size, spacing: body-size * line-height * 0.9, justify: justify)

// Honor `design.fonts.mono`: Typst's `raw` element hardcodes DejaVu Sans Mono via a
// built-in show-set rule; a user show-set rule overrides it (same mechanism as
// `compact`, pinned by the mono byte-diff test in tests/design_fields.rs).
#show raw: set text(font: design.fonts.mono)

// ---- Building blocks --------------------------------------------------------------

// Section heading per the config knobs. Sticky (ADR-0044): when its section's body
// flows to the next page, the heading moves with it instead of stranding at the
// bottom of the previous one. `on-rail: true` renders it in the rail's smaller size.
#let section-heading(title, on-rail: false) = {
  let label = set-case(title)
  let text-part = text(
    size: if on-rail { heading-size * 0.9 } else { heading-size },
    weight: "semibold",
    fill: accent,
    font: design.fonts.heading,
  )[#label]
  let decorated = if heading-style == "rule" {
    grid(
      columns: (1fr,),
      row-gutter: 0.3em,
      text-part,
      line(length: 100%, stroke: (thickness: 0.4pt, paint: color.mix((accent, 45%), (white, 55%)))),
    )
  } else if heading-style == "bar" {
    grid(
      columns: (0.55em, auto),
      column-gutter: 0.5em,
      align: (left + horizon, left + horizon),
      rect(width: 0.55em, height: 0.55em, fill: accent),
      text-part,
    )
  } else {
    text-part
  }
  block(spacing: section-gap, breakable: false, sticky: true)[#decorated]
}

// One entries item: heading and dates per the dates knob; org and location on the
// second line. Built in code mode so the optional parts and bullets can be assembled
// with plain `if` (bare `if` lines inside markup brackets are literal text).
#let entry(item) = {
  let second-line = ()
  if item.org != none { second-line.push(text(weight: "semibold", item.org)) }
  if item.location != none {
    if item.org != none { second-line.push(text(fill: muted, " — ")) }
    second-line.push(text(fill: muted, item.location))
  }
  let dates = if item.dates != none { text(size: small-size, fill: muted)[#item.dates] } else { none }
  let heading-cell = text(size: body-size, weight: "semibold", font: design.fonts.heading)[#item.heading]
  let top-row = if dates-style == "right" {
    grid(
      columns: (1fr, auto),
      align: (left, right),
      heading-cell,
      dates,
    )
  } else {
    heading-cell
  }
  let bullets = if item.bullets != none { list(..item.bullets.map(inline), spacing: entry-gap * 0.5, tight: true) } else { none }
  block(spacing: entry-gap, breakable: false)[
    #top-row
    #if dates-style == "below" and dates != none [#v(-0.05em) #dates]
    #if second-line.len() > 0 [#v(-0.1em) #text(size: small-size)[#second-line.join()]]
    #if bullets != none [#v(0.25em) #bullets]
  ]
}

// One certificates item: the name in the entry heading's style, with the issuer
// and verification link on the second line (the entry row's shape without the dates).
#let certificate(item) = {
  let second-line = ()
  if item.issuer != none { second-line.push(text(weight: "semibold", item.issuer)) }
  if item.url != none {
    if item.issuer != none { second-line.push(text(fill: muted, " — ")) }
    second-line.push(link(item.url)[#text(fill: accent, item.url)])
  }
  block(spacing: entry-gap, breakable: false)[
    #text(size: body-size, weight: "semibold", font: design.fonts.heading)[#item.name]
    #if second-line.len() > 0 [#v(-0.1em) #text(size: small-size)[#second-line.join()]]
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

// One section: heading, rhythm, then the body for its type. `on-rail: true` renders
// the narrow-column variants: key/value pairs and certificates stack, text is set at
// the small size.
#let section-content(section, on-rail: false) = {
  let body = if section.type == "entries" {
    for item in section.items { entry(item) }
  } else if section.type == "key_values" {
    if on-rail {
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
    let render = if on-rail { token => text(size: small-size, fill: text-color, inline(token)) } else { inline }
    list(..section.items.map(render), spacing: entry-gap * 0.6, tight: true)
  } else if section.type == "text" {
    for paragraph in section.items { par[#inline(paragraph)] }
  } else if section.type == "certificates" {
    if on-rail {
      for item in section.items {
        let midline = ()
        if item.issuer != none { midline.push(text(size: small-size, fill: muted, item.issuer)) }
        if item.url != none {
          if midline.len() > 0 { midline.push(text(fill: muted, "  ·  ")) }
          midline.push(link(item.url)[#text(size: small-size, fill: accent, item.url)])
        }
        block(spacing: entry-gap * 0.6, breakable: false)[
          #text(size: small-size, weight: "semibold", font: design.fonts.heading)[#item.name]#if midline.len() > 0 [, #midline.join()]
        ]
      }
    } else {
      for item in section.items { certificate(item) }
    }
  }
  section-heading(section.title, on-rail: on-rail) + v(0.35em) + body
}

// ---- Header -----------------------------------------------------------------------
// Name, headline and contact, styled by the header-style / header-align / contact
// knobs. In sidebar layouts the contact always lives in the rail (config.typ).
// All header parts stay literal text (ADR-0016: no Markdown outside tokens).

// Contact lines in a given palette: `plain` colors email/phone/location, `link-fill`
// colors the link labels. The palette must match what sits behind the lines — the
// page background or rail tint gets `text-color`/`accent`, the accent band must
// not: dark body text on saturated accent is unreadable and accent-colored links
// vanish accent-on-accent (ADR-0016 keeps the parts literal text either way).
// (`link-fill` is not named `link`: it would shadow Typst's `link` element.)
#let contact-lines(plain, link-fill) = {
  let lines = ()
  if cv.contact != none {
    if cv.contact.email != none { lines.push(text(size: small-size, fill: plain)[#cv.contact.email]) }
    if cv.contact.phone != none { lines.push(text(size: small-size, fill: plain)[#cv.contact.phone]) }
  }
  if cv.location != none { lines.push(text(size: small-size, fill: plain)[#cv.location]) }
  if cv.contact != none and cv.contact.links != none {
    for link-item in cv.contact.links {
      lines.push(link(link-item.url)[#text(size: small-size, fill: link-fill, link-item.label)])
    }
  }
  lines
}

#let name-block = {
  let centered = header-align == "center" and contact-style != "split"
  let head-align = if centered { center } else { left }
  let on-band = header-style == "accent"
  let band-soft = color.mix((white, 80%), (accent, 20%))
  let name-fill = if on-band { white } else { text-color }
  let headline-fill = if on-band { band-soft } else { muted }
  // The contact lines sit on the band when there is one, on the page otherwise;
  // the palette follows, and the middot separator with it.
  let lines = contact-lines(if on-band { white } else { text-color }, if on-band { band-soft } else { accent })
  let sep-fill = if on-band { band-soft } else { muted }
  let title = [
    #align(head-align)[
      #text(size: name-size, weight: "bold", fill: name-fill, font: design.fonts.heading)[#cv.name]
      #if cv.headline != none [
        #v(0.15em)
        #text(size: heading-size, fill: headline-fill)[#cv.headline]
      ]
    ]
  ]
  let head-row = title
  if contact-style == "split" and not sidebar {
    head-row = grid(
      columns: (1fr, auto),
      align: (left, right),
      title,
      {
        if lines.len() > 0 { align(right)[#lines.join(linebreak())] }
      },
    )
  }
  let below = if sidebar or contact-style == "split" {
    ()
  } else if contact-style == "dots" {
    if lines.len() > 0 {
      (v(0.3em), align(head-align)[#text(size: small-size)[#lines.join(text(fill: sep-fill, "  ·  "))]])
    } else { () }
  } else {
    if lines.len() > 0 {
      (v(0.3em),) + lines.map(entry-line => align(head-align)[#entry-line])
    } else { () }
  }
  let inner = (head-row,) + below
  if header-style == "none" {
    block(breakable: false)[#inner.join()]
  } else {
    // `rect` has no `breakable`; wrapping it in an unbreakable block keeps the
    // header band whole on one page (ADR-0046 applies to the header too).
    block(breakable: false)[
      #rect(
        width: 100%,
        fill: if header-style == "accent" { accent } else { tint-fill },
        inset: (x: 0.8em, y: 0.55em),
      )[#inner.join()]
    ]
  }
}

// ---- Body --------------------------------------------------------------------------

#{
  let rail-sections = if sidebar { cv.sections.filter(s => s.type == "key_values" or s.type == "bullets" or s.type == "certificates") } else { () }
  let main-sections = if sidebar { cv.sections.filter(s => s.type == "entries" or s.type == "text") } else { cv.sections }

  let main-width = page-width - 2 * margin-x - (if sidebar { rail-width + rail-gutter } else { 0cm })
  let rail-text-width = rail-width - rail-inset - rail-inset * 0.6

  let main-cell = [
    #if cv.summary != none [
      #par[#inline(cv.summary)]
      #v(section-gap * 0.5)
    ]
    #for section in main-sections {
      section-block(section-content(section), main-width)
    }
  ]

  // The rail is always the light tint, so its contact block keeps the page palette.
  let rail-lines = contact-lines(text-color, accent)
  let rail-cell = pad(
    left: if rail-right { rail-inset * 0.6 } else { rail-inset },
    right: if rail-right { rail-inset } else { rail-inset * 0.6 },
    [
      #if rail-lines.len() > 0 [
        #section-heading("Contact", on-rail: true)
        #v(0.2em)
        #rail-lines.join(linebreak())
      ]
      #for section in rail-sections {
        section-block(section-content(section, on-rail: true), rail-text-width)
      }
    ],
  )

  name-block
  if not sidebar {
    v(section-gap * 0.4)
    main-cell
  } else if rail-right {
    grid(
      columns: (1fr, rail-width),
      column-gutter: rail-gutter,
      align: (left, left),
      main-cell,
      rail-cell,
    )
  } else {
    grid(
      columns: (rail-width, 1fr),
      column-gutter: rail-gutter,
      align: (left, left),
      rail-cell,
      main-cell,
    )
  }
}

// GDPR consent statement (ADR-0045): the user's own legal wording, always literal
// text, pinned to the bottom of the last page by the stretchable space above it.
// In sidebar layouts it aligns with the main column, clear of the rail tint.
#if cv.gdpr != none [
  #v(0.9em)
  #v(1fr)
  #let gdpr-line = block(spacing: 0pt, breakable: false)[
    #text(size: small-size, fill: muted)[#cv.gdpr]
  ]
  #if sidebar {
    grid(
      columns: if rail-right { (1fr, rail-width) } else { (rail-width, 1fr) },
      column-gutter: rail-gutter,
      if rail-right { gdpr-line } else { [] },
      if rail-right { [] } else { gdpr-line },
    )
  } else {
    gdpr-line
  }
]
