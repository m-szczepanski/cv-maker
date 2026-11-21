// cv — DIY configuration. This is the single place to tune the template's look without
// editing any Typst logic. Every key is a plain value; `cv templates --extract diy`
// writes this folder to disk so you can edit it, then point `design.template` at it.
//
// Fonts, sizes, spacing, colors, margins and page size are NOT here on purpose: they
// come from the `design` block in your YAML (spec §4) so the same layout knobs work with
// any theme. Only layout/structure choices live in this file.

#let cfg = (
  // Page layout:
  //   "single"        — one column, narrative CV (the default).
  //   "sidebar-left"  — a tinted rail on the left for contact + short sections.
  //   "sidebar-right" — the same rail on the right.
  layout: "single",

  // Rail width, used only by the sidebar layouts. A bare Typst length.
  sidebar-width: 6cm,

  // Which sections go in the rail (sidebar layouts only), matched by section type:
  // every "key_values", "bullets" and "certificates" section is routed to the rail;
  // "entries" and "text" sections stay in the main column. This is deterministic —
  // it needs no per-section flags in your content.

  // Header band behind the name. One of:
  //   "none"   — no band (default), cleanest.
  //   "tint"   — a soft accent tint panel.
  //   "accent" — a solid accent band with light text (a bold masthead).
  header-style: "none",

  // Header content alignment: "left" or "center".
  header-align: "left",

  // Contact line placement in the header:
  //   "dots"   — one line under the headline, items separated by middots (default).
  //   "stack"  — each contact item on its own line.
  //   "split"  — name/headline on the left, contact stacked on the right.
  contact: "dots",

  // Section title case transform: "upper", "title", or "none".
  heading-case: "upper",

  // Section title decoration:
  //   "plain" — color + weight only (house style, ADR-0030).
  //   "bar"   — a short accent tick before the title.
  //   "rule"  — a thin accent rule under the title.
  heading-style: "plain",

  // Where an entry's dates go: "right" (flush with the heading), "below" (its own
  // muted line), or "none" (omit dates).
  dates: "right",

  // Justify body paragraphs to the margin: true or false (ragged-right).
  justify: false,
)
