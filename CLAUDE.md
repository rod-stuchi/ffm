# FFM — Family Finances Manager

A shared household ledger for two people, replacing a spreadsheet. Keyboard-first,
Vim-inspired, fully usable with a mouse.

**Read `design/overview.md` for the product rationale.** This file is the working
agreement: what has been decided, what is still open, and the rules that keep the
codebase coherent.

---

## Stack

| Layer | Choice | Why it was chosen |
|---|---|---|
| Web | **Axum** | Mature, Tokio team |
| DB access | **SQLx** | Compile-time checked queries against the real schema |
| Store | **SQLite (WAL)** | Single file, two users, no separate process |
| Templates | **Askama** | Compile-time checked against the structs passed in |
| Interactivity | **HTMX** | HTML fragments over the wire, no JSON API |
| Styling | **Tailwind** | No global stylesheet to collide |
| Visual state | **Alpine.js** | Toggles, modals, keystroke capture — nothing else |

### Division of responsibility — enforce this strictly

- **Rust / SQLite** owns real data, validation, and anything touching money.
- **HTMX** owns anything that talks to the server (fetch or persist).
- **Alpine** owns purely visual state only: open/closed, focused row, key capture.
  **No business logic in Alpine, ever.** If a decision affects what is stored or
  who sees it, it is not visual state.

### Architecture invariants

- **Hypermedia over the wire.** The server returns HTML fragments, not JSON.
  There is no client-side data model. The database is the single source of truth;
  the UI is its rendering.
- **Compile-time safety at every boundary.** A backend change that breaks the UI
  must fail `cargo build`, not surface at runtime.

---

## Design reference

`design/ffm-app-mockup.html` — the full-viewport app: real layout, real
interactions, live `ffm.toml`. This is the **normative** visual and interaction spec.

`design/ffm-prototype.html` — the annotated version: every row state with a written
explanation of *why* it looks that way. Read this when a state's meaning is unclear.

### Port, don't import

The mocks are throwaway vanilla JS. They hold all state in a JS array and re-render
`innerHTML` — **exactly the client-side data model this architecture forbids.**

Take from them: the CSS custom properties, the row markup and its states, the keymap
table, the copy. Rewrite the behavior against HTMX endpoints. Never `<script src>` a
mock, never copy its render loop.

---

## Visual system

### Tokens

The mock's `:root` custom properties are the design tokens. **Mirror them in
`tailwind.config.js` under `theme.extend`** — do not hand-copy hex values into
components.

A theme is **11 values**, not 20:

```
paper  surface  surface_2  ink  ink_2  ink_3  line  accent  pos  neg  info
```

Everything else is derived with `color-mix()`: `surface-3`, `line-2`, `accent-2`,
the soft fills (`accent-soft`, `pos-soft`, `neg-soft`), the skeleton gradient.
This is why overriding a single token stays coherent — change `accent` and the
focus rail, due ring, command caret and `NORMAL` badge all move together.

### Rules

- **Text on a semantic fill takes `var(--paper)`, never `#fff`.** A `--pos` or
  `--neg` colour is chosen to contrast with the page ground, so the ground is the
  correct foreground. Hardcoded white breaks on light themes (Gruvbox Light's
  `pos` is `#79740E`; Rosé Pine Dawn's is teal).
- **Semantic colour is separate from the accent.** Accent = "you are here"
  (focus rail, gutter zero, mode badge). Green/red = money facts (paid, up, down).
  They must never collide.
- **Selection vs. focus use different channels.** Focus = accent left rail +
  tinted ground. Selection (`x`) = `--info` inset bar. One is "where I am", the
  other is "what I picked".
- **Both themes, always.** Every theme ships a light *and* a dark variant. Define
  the full light palette on bare `:root`; redefine tokens under
  `@media (prefers-color-scheme: dark)` guarded as `:root:not([data-theme="light"])`,
  and again under `:root[data-theme="dark"]`. Never give a colour its only
  definition inside a media or `[data-theme]` block.

### Typography

Archivo (UI, headings) + JetBrains Mono (all money, dates, line numbers, commands,
regexes, filenames). **Self-host both** in `static/fonts/` — the mock uses Google
Fonts only because it has no build step. `font-variant-numeric: tabular-nums`
wherever digits line up.

### Themes

Eleven bundled palettes, each dark + light:

`ffm` (default) · `tokyonight` · `catppuccin` · `kanagawa` · `everforest` ·
`gruvbox` · `rosepine` · `onedark` · `dracula` · `nord` · `solarized`

Nord and Dracula have no canonical upstream light variant; those two light palettes
are derived (Dracula's from Alucard). Everything else uses the real published values.

---

See @design/spec.md for the keymap, row states and config schema.


## Layout

Three modes. `auto` is the default and resolves by available width.

**`auto` must be resolved in CSS, not on the server and not in Alpine.** The
server does not know the viewport width, so a round-trip would be wrong on first
paint and on every resize. Render the threshold into a media query from Askama:

```html
<style>
  @media (max-width: {{ cfg.layout.min_split_width }}px) { .stage { /* tabs */ } }
</style>
```

Zero JS, correct before the first frame, no business logic in Alpine. The mock uses
JS only because it has no template layer.

### Tabs mode

- Tab bar sits under the app bar, showing each member's name **and balance**.
  The balance is deliberate: tabs mode's real cost is losing sight of the other
  pane in a *shared* ledger, and the balance in the tab buys most of that back.
  Keep it at mobile width even when the `gt · gT` hint is dropped.
- `gt`/`gT` cycle; `H`/`L` keep working so nothing is relearned when the layout
  flips underneath you.

### Split mode

- The unfocused pane recedes with **opacity *and* desaturation** (`opacity: .46`,
  `saturate(.5)`). Dimming alone leaves the green checks and red deltas shouting
  from the inactive side. Hover lifts it to `.78` so it stays readable without
  stealing focus.
- Panes are cards with their own border, radius and surface, separated by
  `layout.gap`. The gap must read as separation, not as a crack.

---

## Data model

### Decided

- **Months are views, never copies.** A single continuous transactions table,
  time-sliced. There is no "copy last month's sheet" operation, ever.
- **No hard deletes.** Deleting sets a status and a deletion date. Nothing is
  removed from the database.
- **Restore inserts a new row** referencing the row it supersedes, so each
  transaction carries a traceable chain back through its prior versions. Never
  revert the status on the deleted row.
- **Attribution is a column.** Every transaction belongs to a member; the
  household total is the sum, and either person's pane is a filter.
- **Privacy is opt-in per transaction.** `household` by default. A `private` row
  is hidden from other members but **never from the admin (role 0)**.
- **Ordering uses a fractional sort key**, not an integer `position`. With two
  people entering transactions, renumbering N rows per move is write
  amplification and a merge conflict waiting to happen. Use LexoRank or
  Figma-style fractional indexing: a move between two neighbours writes **one
  row** — `sort_key = midpoint(prev.key, next.key)`. `ORDER BY sort_key` stays a
  plain index scan.
- **The sort key is global, not per-month.** Months are views; a per-month key
  would make a row's position vanish when seen from another slice. Ordering
  within a month falls out of `WHERE month = ? ORDER BY sort_key`.
- **Re-attribution needs a fresh sort key** in the destination member's ordering
  (append = after the current max).
- **Rolling two-month backup** of the SQLite file.

### Recurring rules

- Defined once, **materialized** into the ledger as real rows each period.
- Receipt tracking is **opt-in per rule**: a filesystem path, a match regex, and
  an ignore regex, active over the rule's start-date → end-date (end-date nullable
  = ongoing).
- **A tracked occurrence is `paid` only when a matching file exists on disk.**
  There is no manual tick. The established convention: `2026.07.energia.bill_.pdf`
  (trailing underscore) is *scheduled*, caught by the ignore regex;
  `2026.07.energia.bill.pdf` is *paid*.
- **An untracked recurring bill has no paid/unpaid state at all** — no filesystem
  proof either way, so it is just a plain transaction with a recurrence glyph.

### Sort and search semantics

- **`ledger.sort` gates reordering.** `J`/`K` work only in `manual`. Under `date`
  or `amount` they refuse with
  `ledger.sort = "date" — reorder needs sort = "manual"`. You cannot reorder a
  derived view; the app says so instead of pretending. Default is `manual`
  because this replaces a spreadsheet where rows *were* arranged by hand.
- **An unpaid row sorts by its due day**, since it has no paid-on date yet.
- **`/` is client-side and scoped to the visible month.** It filters rows already
  rendered — an Alpine highlight over the DOM, no round-trip, which is what makes
  incremental search feel instant. The term is a regex, falling back to a literal
  if it does not compile. It matches title, date, due date, formatted amount, and
  the words `private` / `recurring` / `paid`.
- **Searching every month is a different verb.** `:grep` returning a result list,
  hitting a SQLite FTS5 index over HTMX. Do not overload `/` with it.

### Open — not yet decided

1. **Materialization trigger.** App start, a scheduled task, or lazily on first
   view of a month? This shapes the recurring table. The mock generates months on
   the fly, which is exactly what the real app must *not* do.
2. **Receipt tracker parameter set.** Blocked on sharing the existing Go CLI.
   Leave those columns deliberately loose in migration 1.
3. **Concurrency.** Everything about the optimistic-UI flow is single-user in the
   mock. The interesting cases only exist with a server: Ana deletes a row while
   Rodrigo has its edit modal open; both add in the same second.
4. **Auth / current user.** Not modelled at all yet.

---

## Server-side rules

- **The focused member is data, not visual state.** It decides who a new
  transaction is attributed to. `gt` itself is visual (Alpine), but `a` / `i` /
  `:add` must **send the focused member id as a parameter**. Never keep a
  server-side session idea of "current member" — it drifts the moment the app is
  open in two windows.
- **Mutations follow one optimistic flow.** Submit → insert an animated skeleton
  row immediately → the server's fragment replaces it with either the real row or
  an error row carrying a **Try Again** that resubmits the original action.
- **Every soft delete, restore, reorder and re-attribution is undoable.** `u`
  must restore the exact prior state, including both ledgers for a
  re-attribution.

---

## Development workflow

- `tailwindcss --watch` — regenerates CSS as templates change.
- `cargo watch -x run` — recompiles and restarts on save.
- `tower-livereload`, dev-only behind `#[cfg(debug_assertions)]` — refreshes the
  browser once the server restarts.
- **Dev-only UI sandbox router**, also behind `#[cfg(debug_assertions)]`: renders
  every template — including success, error and skeleton states — against dummy
  data, so UI and CSS can be iterated without touching the real database. Keep it
  in sync with the row-state table above; it is how that table stays honest.

---

## Conventions

- **Locale is pt-BR.** Money as `R$ 1.234,56` (dot thousands, comma decimal),
  dates as `dd/mm/yyyy`. Format server-side; do not push locale logic into Alpine.
- **Copy is written from the user's side of the screen.** Name things by what
  people recognise, active voice, a control says exactly what happens. Errors
  explain what went wrong and how to fix it.
- **Never expose internal ids in UI copy** except where the supersedes chain
  makes them meaningful (`↩ #1042`).

---

## Gotchas already paid for

These cost real debugging time in the prototype. Do not re-introduce them.

- **`display: none` on a grid child reflows its siblings.** Hiding the line-number
  gutter for `nonu` pulled the row's content into the 10px first track and crushed
  it. Keep the gutter cell in the grid and vary only its width and content. The
  same bug appears in Tailwind via `hidden` on a grid child.
- **A quote-blind TOML comment stripper eats hex colours.** `line.replace(/#.*$/, '')`
  cuts `accent = "#c4a7e7"` in half at the colour's own `#`. The real `toml` crate
  handles this — which is a reason not to hand-roll a config reader.
- **Watch class-name collisions.** An `app.cfg` state class silently matched a
  leftover `.cfg { display: grid; align-items: start }` rule and broke the whole
  shell width. Namespace state classes (`is-`/`has-` prefix) and delete dead CSS.
- **A fixed-width key column overflows onto its neighbour.** Size it
  `minmax(64px, max-content)` so long commands fit their own box.
- **Hardcoded `#fff` on semantic fills** breaks on the fifth theme, not the first.
  Use `var(--paper)`.
