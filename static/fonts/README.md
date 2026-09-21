# Fonts

`CLAUDE.md` requires Archivo (UI) and JetBrains Mono (money, dates, line
numbers, commands, filenames) to be **self-hosted here** — the mockups link
Google Fonts only because they have no build step, and a household ledger
should not phone a CDN on every page load.

The `.woff2` files are not in the repo yet. Until they are, `--sans` and
`--mono` in `static/css/tokens.css` fall back to the system stack, which is why
those fallbacks are real rather than decorative.

To add them:

1. Download the Latin + Latin-Extended subsets (pt-BR needs `ã ç õ â ê ô`).
   Weights actually used: Archivo 400/500/600/700, JetBrains Mono 400/500/700 —
   exactly what the mockups' Google Fonts link requests, no more.
2. Drop the `.woff2` files here.
3. Add the `@font-face` blocks to `tokens.css` with `font-display: swap`.

Both are OFL-licensed; keep their licence files alongside.
