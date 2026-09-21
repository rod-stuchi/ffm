# Fonts

Archivo (UI, headings) and JetBrains Mono (money, dates, line numbers,
commands, filenames), self-hosted per `CLAUDE.md` — the mockups link Google
Fonts only because they have no build step, and a household ledger has no
business announcing every page view to a third party.

Same families and weights the mockups request:

| family | weights | why |
|---|---|---|
| Archivo | 400 · 500 · 600 · 700 | UI and headings |
| JetBrains Mono | 400 · 500 · 700 | anything where digits line up |

Subsets are **latin + latin-ext only**. pt-BR needs the accents (`ã ç õ â ê ô`),
which live in latin-ext; nothing in this app is Cyrillic, Greek or Vietnamese,
and dropping those three is most of the weight. 14 files, ~408K total, and a
browser only fetches the subsets a page actually uses because the
`unicode-range` values are kept verbatim from Google's own CSS.

`static/css/fonts.css` is generated from these. To refresh both:

    just fonts

Both families are SIL Open Font License 1.1 — `OFL-Archivo.txt` and
`OFL-JetBrainsMono.txt`, kept alongside as the licence requires.
