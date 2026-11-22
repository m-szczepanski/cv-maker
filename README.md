# cv — your CV as code, one page as PDF

`cv` keeps a CV in one plain-text YAML file and renders a clean, one-page PDF from it —
or a two-page one, if that is what you ask for (see [Fitting](#fitting-to-your-page-count)).
No web app, no account, no upload: one binary that you can put in a repo next to the
source, diff, and rebuild in a second.

```
cv init          # writes a starter cv.yaml
$EDITOR cv.yaml  # edit in any editor
cv build         # writes cv.pdf next to it
```

```
$ cv build
cv.pdf  1 page  fit: scale 1.25 spacing 3.00 margins 1.40  (240 ms)
```

**What you get**

- **Guaranteed fit.** If your content does not fit one page, `cv` quietly tightens
  spacing and type until it does — or until it fits the page count you set, filling
  those pages instead of leaving a fragment on the last one — and tells you what it
  applied.
- **Everything in the file.** Fonts, margins, sizes, spacing, colours and the template
  live in a `design:` block in the same YAML — no separate theme files.
- **The GDPR clause, included.** A standard consent statement at the bottom of the last
  page, the way Polish and EU applications expect it — your own wording or none at all
  with one line of YAML (see [GDPR](#gdpr-consent-clause)).
- **Text you can select and copy.** Real text, not an image: selectable, searchable,
  with a tagged structure tree and embedded fonts, so it reads the same on any machine.
- **Deterministic.** The same file and the same version of `cv` produce byte-identical
  PDFs, so the output can live in version control next to its source.
- **Errors that point at the line**, with a suggestion when you meant something else.

---

## Install

Grab the archive for your platform from this repository's **Releases** page, unpack it,
and put the `cv` binary on your `PATH`. Templates and fonts are inside the binary;
nothing else needs to be installed.

| Platform | Archive |
|---|---|
| Linux x86-64 | `cv-<version>-x86_64-unknown-linux-gnu.tar.xz` |
| Linux arm64 | `cv-<version>-aarch64-unknown-linux-gnu.tar.xz` |
| macOS (Apple silicon) | `cv-<version>-aarch64-apple-darwin.tar.xz` |
| macOS (Intel) | `cv-<version>-x86_64-apple-darwin.tar.xz` |
| Windows x86-64 | `cv-<version>-x86_64-pc-windows-gnu.zip` |

Each release carries a `SHA256SUMS.txt`; from inside the download directory,
`shasum -a 256 -c SHA256SUMS.txt` confirms the archive you have is the one that was
built (`sha256sum -c SHA256SUMS.txt` on Linux). On macOS, a binary you downloaded but
did not build is quarantined: allow it with right-click → Open the first time.

Or build it yourself from a checkout (needs a Rust toolchain, see `rust-toolchain.toml`):

```sh
cargo build --release      # target/release/cv
```

Check it works:

```sh
cv --version
cv templates
```

---

## Your first CV in a minute

```sh
mkdir cv && cd cv
cv init
```

That writes two files: `cv.yaml` (your CV) and `cv.schema.json` (editor help — the
first line of `cv.yaml` points at it, so VS Code's YAML extension gives you
autocomplete and flags mistakes while you type).

Open `cv.yaml` and replace the sample content. It is split in two:

```yaml
schema: 1

cv:
  name: Jane Doe
  headline: Staff Software Engineer
  contact:
    email: jane@example.dev
    links:
      - { label: GitHub, url: "https://github.com/janedoe" }
  summary: Backend engineer focused on high-throughput systems.
  sections:
    - title: Experience
      type: entries
      items:
        - heading: Staff Engineer
          org: Acme Corp
          dates: 2021 – present
          bullets:
            - Cut p99 latency by **40%** by moving the hot path to Rust

design:
  template: compact
  page: a4
  fonts: { body: Inter, heading: Inter, mono: JetBrains Mono }
  colors: { accent: "#2563eb" }
```

Then build:

```sh
cv build                 # cv.yaml → cv.pdf, in the same folder
cv watch                 # or keep rebuilding while you edit
cv check                 # or just validate, no PDF
```

### Section types

| `type:` | Use it for | Item fields |
|---|---|---|
| `entries` | jobs, projects, education | `heading`, `org`, `location`, `dates`, `bullets` |
| `key_values` | skills, one-line pairs | `key`, `value` |
| `bullets` | awards, highlights | plain strings |
| `text` | paragraphs, a note | plain strings |
| `certificates` | certifications, licenses | `name`, `issuer`, `url` |

Every field is optional except `cv.name`, a section's `title`, and the fields an item
needs to render: `heading` for `entries`, `key` and `value` for `key_values`, `name` for
`certificates`, `label` and `url` for a link. `schema: 1` is required and stays `1`.

### A little formatting

In `summary`, in the bullets of an `entries` item, and in the items of `bullets` and
`text` sections you can write:

| You write | You get |
|---|---|
| `**bold**` | **bold** |
| `*italic*` | *italic* |
| `` `code` `` | `code` |
| `[label](https://example.dev)` | a hyperlink |

Anything else is literal text — hashes, dollar signs, underscores and backslashes in a
job title stay exactly as you typed them. Headings, organisations, dates, certificate
names and issuers, and section titles are always literal. A link target needs to be an
absolute URL for a PDF reader to open it (`https://example.dev`,
`mailto:jane@example.dev`); a bare `github.com/me` warns, and an empty `[label]()` is
an error, because the export refuses it. A certificate's `url` is held to the stricter
contact-link rule: absolute `http(s)` with a host, or the check fails.

---

## Commands

| Command | What it does |
|---|---|
| `cv init` | Write a starter `cv.yaml` (plus `cv.schema.json`) |
| `cv build [file]` | Render `file` (default `cv.yaml`) to `<name>.pdf` next to it |
| `cv watch [file]` | Build once, then rebuild whenever the YAML — or a custom template — changes |
| `cv check [file]` | Validate only; nothing is written |
| `cv templates` | List the built-in templates; `--extract <name> [--out <dir>] [--force]` writes one to disk to edit |
| `cv fonts` | List every font family you can use, and where each comes from |
| `cv schema` | Print the JSON Schema of the file format |

### Flags

Global:

| Flag | Effect |
|---|---|
| `--font-path <dir>` | Add a directory of `.ttf`/`.otf` fonts. Repeatable |
| `--quiet` | Print only what matters: errors and warnings |
| `--verbose` | Extra diagnostics, e.g. how many compiles the fit loop used |

`cv build`:

| Flag | Effect |
|---|---|
| `--output <path>` | Write the PDF somewhere else |
| `--no-fit` | Render at the sizes you asked for, even across pages |

`cv init`:

| Flag | Effect |
|---|---|
| `--template <name>` | Which built-in to start from (`cv templates` lists them) |
| `--out <file>` | File name to write (default `cv.yaml`) |
| `--force` | Overwrite an existing file |

---

## Fitting to your page count

One page is the default, and the count you set is the count you get: when the first
render is too long, `cv` tightens spacing and margins first, then reduces type size —
and always keeps the *roomiest* setting that reaches your count, so a setting is only
sacrificed when it actually changes the page count. If you want a longer CV, set the
target and the same contract works both ways:

```yaml
design:
  pages: 2                # the exact page count to fit; default 1
```

The target is a promise the pages are *used*, not just allowed for: when the content
already fits with room to spare — on any target, including the default one page —
`cv` grows the layout instead of leaving the bottom of the page empty: spacing first,
then type size, then margins, up to the largest setting that still fits. If the caps
of that normal growth still do not span a multi-page count, one last search stretches
the rhythm further, only as far as needed to land on the page count exactly (a
one-page CV never gets that far — its count is exact already, so growth stops at the
caps). A section that fits on one page is never split across two: it moves whole to
the next page. You can see what the loop did in the last line of `cv build`:

```
cv.pdf  1 page  fit: scale 1.00  (218 ms)                              # the page was already full
cv.pdf  1 page  fit: scale 1.00 spacing 1.44 margins 1.00  (240 ms)    # ...grew the rhythm to fill a half-full page
cv.pdf  1 page  fit: scale 1.00 spacing 0.70 margins 0.88  (262 ms)    # tightened spacing to fit
cv.pdf  2 pages  fit: scale 1.00  (231 ms)                             # fills its 2-page target as written
cv.pdf  2 pages  fit: scale 1.22 spacing 3.00 margins 1.00  (240 ms)   # grew to fill its 2-page target
cv.pdf  2 pages  fit: scale 1.25 spacing 4.41 margins 1.40  (240 ms)   # ...stretched past the caps to land on 2
```

You control how far the loop may go, in both directions:

```yaml
design:
  fit_one_page: true      # false, or `--no-fit`, turns the loop off
  fit_stretch_max: 12.0   # the exact-count stretch never passes this spacing scale
  fit_floor:
    min_body_size: 8.5pt  # never smaller body text than this
    min_scale: 0.85       # never smaller than 85% of the sizes you set
```

Sometimes the count is simply out of reach — too much content to shrink, or too little
to stretch. You still get the PDF and a clear warning naming the count that *is*
reachable, and `cv` still exits 0, because you asked for a document and got one:

```
⚠ content does not fit 2 page(s): auto-fitted to 3 page(s) with the gentlest settings
  that reach this count — about 43 lines spill onto 1 extra page(s); set design.pages: 3
  to keep this placement, or cut content (e.g. bullets or paragraphs) to fit

⚠ content is too short for 2 page(s): the pages hold at 1 page(s) even fully stretched
  — set design.pages: 1 to keep this layout, or add content to span 2
```

---

## GDPR consent clause

Applications for jobs in Poland — and more generally in the EU — expect a consent
statement at the bottom of the CV, so `cv` writes the standard one for you:

> I hereby give consent for my personal data included in my application to be processed
> for the purposes of the recruitment process in accordance with Regulation (EU)
> 2016/679 of the European Parliament and of the Council of 27 April 2016 (GDPR).

It renders as small muted text pinned to the bottom of the **last page** (the only page,
for a one-page CV). To use your own wording, set `cv.gdpr` — it is literal text, never
formatted:

```yaml
cv:
  gdpr: I agree that my personal data may be stored and processed for the purpose of
    this application.
```

and to leave it off entirely:

```yaml
cv:
  gdpr: null
```

The clause counts as page content: the fitting loop sees it, so a full page tightens a
little rather than pushing the statement onto a page of its own.

---

## Templates and design

`cv templates` lists what is built in:

```
classic  Conventional single-column layout with a right-aligned contact header.
compact  Dense single-column layout, ideal for experienced engineers.
diy      Configurable layout engine — edit config.typ (or extract with `cv templates --extract diy`).
modern   Bold accent header band, uppercase section labels, roomy two-column contact.
sidebar  Two-column layout with a tinted left rail for contact and skills.
```

Pick one with `design.template`. The starter file shows every design field; delete any
of them to take the default. Page is `a4` or `letter`; lengths accept `pt`, `cm`, `mm`,
`in`; colours are `#rgb` or `#rrggbb`.

### DIY: configure the layout without writing Typst

`diy` is one template with the layout choices pulled out into knobs — extract it,
edit `config.typ`, build:

```sh
cv templates --extract diy          # writes ./diy/ with template.typ + config.typ
$EDITOR ./diy/config.typ            # layout, header band, heading case, dates, justify…
```

```yaml
design:
  template: ./diy
```

Every knob is documented and commented in `config.typ`; a wrong value fails the build
with the knob named. Fonts, sizes, spacing, colours, margins and the fit loop keep
working exactly as with the other templates — `config.typ` only changes structure.

### Using your own template

A template is a folder with a `template.typ` entry point, a `manifest.yaml`, and any
number of `.typ` helpers. Start from a built-in by extracting it — the built-ins are
embedded in the binary, so no clone is needed:

```sh
cv templates --extract classic --out ./my-template
$EDITOR ./my-template/template.typ
```

then point at it from the YAML — a reference containing `/` (or starting with `.`) is
read from disk, relative to the CV file:

```yaml
design:
  template: ./my-template
```

`cv watch` also rebuilds when a file in that folder changes, and `cv check` warns if the
template ignores a design group your file writes. A custom template sees only the files
in its own folder: there is no package registry and no network access.

---

## Fonts

Five open-licence families ship inside the binary, so output looks the same on every
machine: Inter, IBM Plex Sans, IBM Plex Mono, Source Sans 3, JetBrains Mono. `cv fonts`
lists those, plus anything found in your own directories or on the system, and says
where each one came from:

```
IBM Plex Mono                   bundled
Inter                           bundled
Source Sans 3                   bundled
Helvetica Neue                  system
```

Add your own either permanently with `design.font_paths: [./fonts]` (relative to the
CV file) or per run with `cv --font-path ./fonts build`. Family names are matched the
way Typst matches them, so `Inter` and `inter` are the same font — a name that does not
resolve is an error with a suggestion, never a quiet substitution.

---

## Errors and exit codes

```
× unknown font "Inte"
   ╭─[cv.yaml:34:11]
34 │     body: Inte
   ·           ──┬─
   ·             ╰── this font family
   ╰────
  help: did you mean "Inter"?
```

Every error names the file, line and field, and `cv check` reports all of them at once
rather than one at a time.

| Exit code | Meaning |
|---|---|
| `0` | Success. Warnings (an overflowing CV, a template ignoring a field) do not change this |
| `1` | Something went wrong: invalid CV, unreadable file, template error, unwritable output |
| `2` | The command line itself was not valid (unknown command or flag) |

---

## Notes

- **Your CV never leaves your machine.** `cv` has no network access at all.
- **Reproducible output.** Pin the version of `cv` and the PDF is byte-identical
  anywhere, which is why the output can live in git next to its source. That holds while
  the fonts come from the binary or from your own `font_paths`; a family resolved from the
  operating system can differ between machines (`cv fonts` shows where each came from).
  It also assumes the template is time-free — the built-ins are (`document(date: none)`,
  no `datetime` calls); a custom template that reads the clock breaks the guarantee.
- **Dates are strings.** Write `2021 – present`, `Q3 2019`, whatever you want to see.
- **Licence:** MIT. Bundled fonts are under their own open licences (see
  `fonts/LICENSES.md`).
