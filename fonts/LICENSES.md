# Bundled font licenses

Fonts embedded into the `cv` binary (spec §2: open licenses only). License texts ship
alongside each font file in this directory.

| Font | Files | License | Source |
|---|---|---|---|
| Inter (4.001) | `Inter-{Regular,SemiBold,Bold,Italic,BoldItalic}.ttf` | SIL Open Font License 1.1 (`Inter-LICENSE.txt`) | https://github.com/rsms/inter/releases/tag/v4.1 |
| JetBrains Mono (2.304) | `JetBrainsMono-{Regular,Bold,Italic}.ttf` | SIL Open Font License 1.1 (`JetBrainsMono-LICENSE.txt`) | https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304 |
| IBM Plex Sans (variable, 3.000) | `IBMPlexSans-Var-{Roman,Italic}.ttf` | SIL Open Font License 1.1 (`IBMPlexSans-LICENSE.txt`) | https://github.com/IBM/plex (`@ibm/plex-sans-variable@0.2.0` release, `fonts/complete/ttf`) |
| IBM Plex Mono (2.005) | `IBMPlexMono-{Regular,Bold,Italic}.ttf` | SIL Open Font License 1.1 (`IBMPlexMono-LICENSE.txt`) | https://github.com/IBM/plex (`packages/plex-mono/fonts/complete/ttf`) |
| Source Sans 3 (3.052) | `SourceSans3-{Regular,Italic,SemiBold,SemiBoldItalic,Bold,BoldItalic}.ttf` | SIL Open Font License 1.1 (`SourceSans3-LICENSE.txt`) | https://github.com/adobe-fonts/source-sans/releases/tag/3.052R (`TTF-source-sans-3.052R.zip`) |

Notes:

- IBM Plex Sans ships as a **variable font** (axes `wdth`/`wght`). The static IBM Plex
  weights other than Regular/Bold register under separate family names (e.g.
  `IBM Plex Sans SmBld`), which Typst (0.15.1) cannot reach through
  `text(font: "IBM Plex Sans", weight: "semibold")`; the variable build keeps one family
  and instantiates the requested weight from the `wght` axis.
- Source Sans 3 statics group all weights under one family (Typst trims the "Semibold"
  suffix from the family name), so they ship as static files.
- `IBMPlexMono` statics ship only the faces the templates use (Regular, Bold, Italic);
  same for `JetBrainsMono`.
