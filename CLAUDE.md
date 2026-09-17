# FFM — Family Finances Manager

A shared household ledger, replacing a spreadsheet. Multi-user by design — one
person, a couple, or the whole family. Keyboard-first, Vim-inspired, fully
usable with a mouse.

**Read `design/overview.md` for the product rationale.** This file is the working
agreement: what has been decided, what is still open, and the rules that keep the
codebase coherent.

---

## Stack

| Layer | Choice | Why it was chosen |
|---|---|---|
| Web | **Axum** | Mature, Tokio team |
| DB access | **SQLx** | Compile-time checked queries against the real schema |
| Store | **SQLite (WAL)** | Single file, one process per household, no separate process to run |
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
- **Members are real accounts, not just an attribution label.** `members` gets
  `password_hash` and `role` (0 = admin). No self-registration — an admin creates
  the row. See `design/spec.md` for the full session/auth model.
- **Personal display preferences live in the database, not in `ffm.toml`.** Theme
  and line-number style are per-`member_id` rows (`member_settings`), set at
  onboarding and editable via `:theme`/`:set` or a `:profile` form — never in the
  shared config file, which household members would fight over the moment
  there's more than one person's taste to account for.
  `ffm.toml` stays process-level: DB path, backup, receipt-vault root, layout
  thresholds.
- **Focused member ≠ logged-in member.** Focus (`H`/`L`/`gt`) decides who a *new
  transaction* is attributed to; login decides whose *settings and private rows*
  you can see and edit. `:profile` and privacy checks use the session's member
  id, never the focused pane's — otherwise focusing your spouse's pane would let
  you edit their preferences or expose what `private` hides from them.
- **An admin can preview their own session with admin rights switched off —
  this is not impersonation.** It never changes *who* is logged in: still the
  same member, own pane, own edits, own private rows all fully visible and
  editable throughout. The one thing it changes is whether admin-only
  visibility and actions are currently in effect — other members' `private`
  rows go back to actually being hidden (the same rule any non-admin already
  faces) and admin-only config (`ffm.toml`, household currency) becomes
  inaccessible, exactly as if this member held no admin role at all. No
  session swap, no acting-as-someone-else, no writes attributed to anyone
  else — see `design/spec.md`'s Auth & sessions section for the full
  reasoning and why that distinction ruled out "impersonate" as the name.

### Recurring rules

- **Created by turning an existing occurrence into the rule's first one —
  there is no separate "new rule" form.** `R` on a row that isn't recurring
  yet opens the same rule editor a recurring row's `R` does, just in
  creation mode: same fields, pre-filled from the transaction you're
  looking at (amount, day → start date, a bill-name guess from the title),
  a "Create rule" button instead of "Save rule". Saving flips that
  transaction's `recurring` flag and gives it the new rule's fields — it
  *becomes* the anchor occurrence, never a separate parallel row — which is
  what keeps materialization's `UNIQUE(rule_id, period)` idempotency intact
  for the period it's already in; inserting a second row for that same
  month would violate the very constraint that makes materialization safe
  to run hourly.
- **The due date lives on the rule, not the occurrence, once a row is
  recurring.** `R`'s Due date field (day-of-month) is the one place it's
  editable; `e`'s Day field disappears entirely for a recurring row, rather
  than offering a second control for the same fact with no rule for which
  one wins on the next materialized period. A non-recurring row has no rule
  to hold a due date, so `e`'s Day field is exactly where it stays.
- **Bucket is the same per-member autocomplete everywhere it appears** —
  the rule form's Bucket field and the occurrence form's Bucket field both
  read the identical `SELECT DISTINCT bucket WHERE member_id = ?` tag
  cloud (see Categorization & grouping below), not two separately-behaving
  widgets that happen to share a label. Creating a new rule starts Bucket
  empty regardless of whatever the anchor transaction happened to be
  tagged with — a fresh rule gets an explicit choice, not an inherited
  guess.
- Defined once, **materialized** into the ledger as real rows each period.
- Receipt tracking is **opt-in per rule** — no regex, no pattern, no traversal.
  A tracked rule uses its `bucket` / `bill_name` — general-purpose fields, see
  Categorization & grouping below — plus an `instance` field exclusive to
  tracked rules, to compute where its files live. `instance` disambiguates the
  same bill name across two properties/accounts (two houses, each with their
  own power bill); if there's only one, it's skipped entirely rather than
  named — "Power" alone is enough.
- **Path is fully computed, never authored**:
  `{member_uuid}-{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}.{ext}`
  — the `instance` segment is only present when the rule has one. Slugified
  (lowercase, spaces → `_`). Renaming a rule's taxonomy fields must move its
  existing files to the new path — it isn't just a label edit.
  `bucket`/`bill_name`/`instance` may not slugify to `delete` or start with
  `.` — reserved for the soft-delete folder below.
- **Three file kinds per period, each a separate fact:**
  - `{month}_barcode.{ext}` — the bill itself, as issued by the company (line
    items, amount, due date). Purely informational; its presence never changes
    the paid/scheduled state.
  - `{month}_scheduled.{ext}` — **optional.** Proof that a payment was set up
    ahead of the due date. Skipped entirely if the bill is just paid same-day
    out of balance — scheduling is a promise, not a required step.
  - `{month}.{ext}` — the definitive proof of payment. Its presence is `paid`,
    full stop.
- **A barcode can cover several periods from one upload instead of a fresh
  file every month** — a city tax office issuing one PDF with 10 barcodes
  for the whole Jan–Oct cycle, or a bank issuing one document with a loan's
  full run of installments. Opt-in per rule (`spans_period`, alongside
  `window_size` and `window_start`); off by default, since most bills
  genuinely do get a new file each period. When on, `{month}_barcode.{ext}`
  stops being computed per period — every occurrence inside the window
  points at the **same** uploaded file, and the app computes which page is
  theirs from `window_start` and their own period (`window_start`'s month
  is page 1, the next month page 2, and so on). This is a property of the
  barcode specifically: `{month}_scheduled.{ext}` and `{month}.{ext}` stay
  genuinely per-period regardless — each installment is still scheduled and
  paid on its own, only the *bill document* is shared.
  - **This applies whether the rule itself is finite or cyclic — the window
    doesn't have to span the rule's entire life.** A fixed-length loan gets
    exactly one window for its whole run (`window_size` = its
    `total_installments`, uploaded once, never replaced — when it's done,
    it's done). A cyclic bill like an annual property tax gets a **new**
    window every cycle: the same rule, re-opened each January with a fresh
    `window_start` and `window_size` when that year's document arrives,
    which is what resets the page count back to 1 for the new file without
    touching anything else about the rule. Re-entering those two fields
    *is* the "I got a new file" action — there's no separate upload flow
    modelled for this yet, same as barcode/scheduled/receipt uploads
    generally (see Upload, below).
  - **This is independent of, and doesn't resolve, the still-open cyclic/
    seasonal rule design** (`months`-of-year allow-list — see Open items,
    below). A cyclic rule already skips materializing outside its active
    months by whatever mechanism eventually lands there; the barcode window
    just needs *a* start period and size to compute pages from, and doesn't
    care how the rule decided which periods are active.
  - **An occurrence whose period falls outside the current window's page
    count is flagged, not silently wrong** — same warning channel as
    scheduled-overdue, both on the file badge and in the `P` preview
    carousel: "page 11 — outside this file's 10 pages, a new upload is
    needed for this month." That's the signal a fresh file needs
    uploading (a new tax year, a refinanced loan), not a bug to chase.
- **A scheduled payment still unconfirmed past its due date is a distinct,
  warning-coloured state** — not a generic `pending`. A
  `{month}_scheduled.{ext}` file with no `{month}.{ext}` and a due date already
  passed means a promise wasn't verified as kept; flag it, don't silently treat
  it the same as "nothing happened yet."
- **Uploading the final receipt supersedes the scheduled file automatically.**
  The system moves `{month}_scheduled.{ext}` into `.delete/` the moment
  `{month}.{ext}` is uploaded — the promise is resolved, only the definitive
  receipt is left standing. This is a system-triggered soft delete, logged the
  same as a manual one (so `u` on the receipt upload restores the scheduled
  file too), just tagged with a different reason so the audit trail shows it
  was superseded, not deleted on purpose.
- **`paid` is still "a matching file exists on disk," checked by exact path, not
  traversal.** No manual tick for tracked rules — full stop. The filesystem
  stays authoritative on purpose: the vault is meant to be human-browsable and
  manipulable directly, and a DB flag that could silently disagree with what's
  actually in the folder is exactly the failure mode this app avoids everywhere
  else. A DB row still accompanies every upload (uploader, timestamp, kind,
  original filename) but only as metadata/audit for undo — it never decides
  paid/not-paid.
- **Extensions are the closed, case-insensitive set**: `pdf`, `jpg`, `png`,
  `txt` — whatever the uploaded file actually was, not fixed per rule.
- **Upload, view, delete are scoped to recurring-rule occurrences only** — a
  one-off transaction has no taxonomy to file itself under, so it gets no
  attachment feature.
- **Upload is three distinct actions — add barcode / add scheduled / add
  receipt** — not one upload plus a question, since each file kind has its own
  fixed name and its own effect on state. The file is renamed to the
  deterministic convention on disk; the user's original filename is kept as
  metadata only.
- **A file badge or `g1`/`g2`/`g3` still opens the file in a new browser
  tab** — that stays the default, and no custom rendering is built for it:
  all four allowed extensions already render natively in a browser. `P`
  (see below) is a **second, additive** way to look at a row's files —
  reversing this repo's earlier "no in-app preview at all" stance, on the
  strength of flipping between a row's own bill/scheduled/receipt without
  losing the ledger underneath. It still doesn't reimplement a file
  renderer: the carousel shows the file's kind, computed path and an icon,
  not the file's actual bytes, and "Open in new tab" is right there for
  when the real content is needed. See `design/spec.md`'s row-component
  section for the on-row badges and their keybindings.
- **`P` opens that carousel for the focused row's files.** `1`/`2`/`3`
  jump straight to that fixed file kind (unchanged from `g1`/`g2`/`g3`'s
  numbering — never renumbered by what exists); `h`/`l` step to the
  previous/next file that actually exists on the row, clamping rather than
  wrapping at either end. Pressing a number for a kind that isn't there is
  the same no-op-with-a-statusline-message treatment `g1`/`g2`/`g3` already
  get, not a different error path to learn.
- **The carousel is full-viewport, not a dialog-sized modal.** The actual
  job isn't just "glance at which file this is" — it's reading a boleto's
  barcode digits, or holding a phone up to its QR code, both of which need
  the file genuinely large on screen. A 560px dialog can't do that; this is
  the one modal in the app sized to the full window on purpose.
- **Zoom (`+`/`-`, or `0` to reset) scales the preview**, floating controls
  in the stage's corner rather than in the button row, the same placement
  any image viewer's zoom control uses. Each file starts back at 100% when
  you flip to a different one with `h`/`l`/`1`/`2`/`3` — carrying a zoom
  level over to an unrelated file would be a surprise, not a convenience.
- **Carrying the amount forward is opt-in per rule, checked by default.**
  Materializing a new period seeds its amount from the *previous* period's
  actual value when this is on; off, the new occurrence starts with no amount
  at all — the same `R$ __,__` placeholder already used for a tracked row with
  no amount yet — forcing an explicit edit. For a bill that's genuinely
  different every period (a credit-card statement), auto-filling last month's
  number would be actively wrong, not just unhelpful. The very first
  occurrence of a rule has nothing to carry forward from, so it always seeds
  from the rule's own `amount` field.
- **Deleting any of the three files is a soft delete.** The file moves into a
  `.delete/` subfolder alongside where it lived — never removed from disk —
  and a DB row records the move (old path, `deleted_at`, `deleted_by`, reason)
  so `u` can put it back. Same "nothing is destroyed" rule as everything else
  in this app.
- **An untracked recurring bill gets an optional manual `paid` checkbox** — the
  one manual tick in the whole model, and only because there's no filesystem
  proof to contradict it. A *tracked* rule never gets this checkbox; the rule
  above is absolute for it.
- **The paid marks are visually distinct from each other** — file-verified
  paid, manually-asserted paid (untracked only), and scheduled-overdue-warning
  are three different colours/shades, never sharing a mark. Designs need
  updating for this.
- **`end_date` is a plain, independently-settable field on every rule** —
  nullable, null meaning ongoing — not exclusive to installment loans. This
  is how an ordinary open-ended bill gets cancelled: CPFL stops after July,
  set `end_date = 31/07`, nothing about `total_installments` involved. A
  **finite, non-cyclic rule** (e.g. a car loan) additionally gets
  `total_installments` (nullable int; null = ongoing), and *for that rule*
  `end_date` is derived from `start_date + cadence × total_installments` and
  stays editable two ways in sync — type a new installment count, or pick a
  new end date, either recomputes the other. Setting `end_date` alone, with
  no installment count, is the correct move for "this ends, but it was never
  a fixed-length loan to begin with." This is a different shape from a
  cyclic seasonal bill (an annual tax tracked only Jan–Oct, forever): cyclic
  bills repeat every year and remain open item 3 below; a rule with an
  `end_date` — installment-derived or set directly — runs once and ends
  permanently. Don't conflate the two.
- **Installment progress displays as a bare `{paid}/{total}` badge** (e.g.
  `7/12`), not baked into the transaction title — the same meta slot the
  due-date and recurring glyphs already occupy.
- **Paying off several installments early is a decoupled, manual action** — no
  guided "pay off N now" feature. Shorten `total_installments`/`end_date` on
  the rule, and record the lump payment as an ordinary one-off transaction.
  Simplest thing that works; revisit only if the manual version proves
  annoying in practice.

### Materialization

- **An in-process scheduled task, ticking hourly, and also run once at
  startup.** No external cron, no systemd unit — one process per household is
  the whole deployment. Startup matters as much as the tick: a process that
  was down across a month boundary must catch up the moment it's back, not up
  to an hour later.
- **Not lazily on view.** Materializing only when someone opens a month makes
  "does this row exist" depend on "did anyone happen to look," which silently
  breaks every query that reads across months — the unresolved-issues badge
  would report all-clear for a month nobody opened, and MoM deltas and `:grep`
  would have nothing to find.
- **Reconcile per rule; keep no watermark.** For each active rule, find the
  periods between `start_date` and `min(current_month, end_date)` that have no
  occurrence, and insert those. The absence of rows *is* the state, so there is
  nothing to keep in sync. A `last_checked` control value would look simpler
  and quietly get it wrong: create a rule today whose `start_date` is six
  months back, and a watermark says "already caught up" and never materializes
  its history. Reconciliation handles that, plus downtime of any length, edited
  start dates and reactivated rules, for free. At household scale this is a few
  hundred `(rule, period)` pairs to check — nothing.
- **`UNIQUE(rule_id, period)` makes the whole job idempotent**, so the startup
  run and an hourly tick can race harmlessly. This also removes the
  double-insert case from the concurrency question below.
- **The gap query must count soft-deleted occurrences as present.** This is
  where two decided rules collide: deletes are soft, so a `dd`'d occurrence is
  still a row. If the reconciler only looks for *live* rows, it will resurrect
  every deleted occurrence on the next tick and fight the user once an hour.
- **Never materialize past the current month.** Future months stay projections
  rendered on the fly and are never persisted — which is what the mock's
  "projected" label already implies.

### Categorization & grouping

- **`bucket` and `bill_name` are general-purpose on any transaction** —
  recurring or one-off, tracked or not. They started as receipt-path plumbing
  for tracked rules; that's no longer their only job. The old, never-formalized
  `Category` field on the Rule card is retired in favor of `bucket`.
- **`instance` stays exclusive to tracked rules** (see Recurring rules above) —
  it only ever existed to disambiguate a receipt file path, and that meaning
  doesn't generalize to a one-off grocery run.
- **`bucket` / `bill_name` are a per-member tag cloud, not a category table.**
  Autocompleted from `SELECT DISTINCT ... WHERE member_id = ?` over the
  logged-in member's own rows, no separate taxonomy entity to manage. Scoped
  per member on purpose — a kid's "Mesada" / school-cafeteria categories have
  nothing to do with a parent's "Consumption / Power," and a shared dropdown
  would just be noise for both. Nothing stops two members typing the same
  string (e.g. both calling something "Consumption"), it just isn't suggested
  across accounts.
- **Grouping renders as real Vim folds**, not an invented gesture: `:group
  bucket` switches a panel's ledger from flat to grouped; `za`/`zo`/`zc` toggle
  a single fold, `zR`/`zM` open/close every fold at once. Fold open/closed
  state is pure visual state — Alpine already owns exactly this per the
  existing division-of-responsibility rule — the server still renders the full
  grouped, subtotaled fragment; folding only hides/shows what's already there.
- **Nesting stops at `bucket`.** No nested `bill_name` sub-folds — one level is
  enough for a household-sized ledger.
- **Groups are framed, not colour-coded.** A bordered container (reusing the
  existing `.card` visual language) plus a text label — deliberately not a
  per-category colour system, since `bucket` is open-vocabulary and unbounded
  while a theme is deliberately only 11 tokens with `pos`/`neg`/`accent`/`info`
  already carrying fixed meanings. An unbounded set of user-typed strings
  either collides with those meanings or needs hash-based colour assignment
  for no real gain over a plain frame.
- **Scoped to each member's own panel** — no household-wide, cross-member
  category view. Grouping only reorganizes the rows already in front of you.
- **An `Uncategorized` group catches rows with no `bucket` set** — grouping
  never silently drops a row.
- **A group header always shows its subtotal, open or closed.** This is why
  there is no separate `:budget`/report screen: `zM` (fold everything) on the
  real ledger already reads as a per-category summary. Browsing and
  summarizing are one mechanism at two zoom levels, not two features.
- **Grouping is a derived view, exactly like `sort = date`/`amount`.** The
  moment rows are re-clustered by category instead of read in stored manual
  order, `J`/`K` refuses with the same message pattern already established for
  sort — you cannot reorder a derived view, the app says so instead of
  pretending.
- **Default fold state on load (expanded vs. collapsed) is a per-member
  setting** in `member_settings`, a radio choice, not a checkbox list, since
  the two states are mutually exclusive.

### Unresolved issues across months

- **The app opens to the current month by default** on first load — not the
  last-viewed month, not January.
- **A tracked occurrence that's overdue with no receipt is "unresolved"**:
  `pending` past its due date (ring fully red) or `scheduled overdue`. An
  untracked bill sitting without a manual tick is *not* an issue — there's no
  filesystem evidence to act on, so flagging it would be nagging based on
  nothing.
- **The lookback window is the current calendar year, not indefinite.** On a
  given date in September 2026 that means January–August 2026 get checked,
  not December 2025. Bounded on purpose.
- **Surfaced as a badge, not a message.** A small warning dot on the
  month-picker trigger button, and on every flagged month's cell inside the
  picker popup — ambient and persistent, like every other status in this app
  (marks, rings, deltas), not a toast that gets dismissed and forgotten. No
  separate "you have unpaid bills" message on load.
- **Getting there is just existing navigation.** Clicking a flagged month cell
  jumps to it — no new command needed on top of the month picker that already
  exists.
- **The check is per-viewer, respecting existing privacy rules.** A private
  row's unresolved status only counts toward the badge for its owner and the
  admin (role 0) — never for another member who can't see the row itself.

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

1. **Concurrency.** Everything about the optimistic-UI flow is single-user in the
   mock. The interesting cases only exist with a server: Ana deletes a row while
   Rodrigo has its edit modal open; both add in the same second. (The
   materialization half of this is already handled — see Materialization
   above: `UNIQUE(rule_id, period)` makes a double-insert impossible.)
2. **Seasonal / cyclic recurring rules.** Some bills only apply part of the
   year, every year (e.g. a tax tracked January–October, then again the next
   January) — a `months`-of-year allow-list on top of the rule's existing
   start/end dates is not yet designed. Distinct from `total_installments`
   (decided, above): that's a one-time, finite run that ends permanently; this
   is a recurring annual pattern. Nice-to-have, not blocking. The multi-period
   barcode window (Recurring rules, above) is decided and doesn't wait on
   this — it just needs a start period and a page count from whatever
   mechanism eventually decides which months are active.
3. **Split mode assumes exactly two panes.** "The unfocused pane," `H`/`L`
   moving focus left/right — the whole layout is binary by construction, which
   was fine when this was framed as a two-person app. It doesn't obviously
   generalize to a household of four or six: a scrollable row of panes? pick
   which one other member to compare against? fall back to tabs above some
   member count? Not designed yet — revisit when a household bigger than two
   is an actual case, not a hypothetical.

---

## Server-side rules

- **The focused member is data, not visual state.** It decides who a new
  transaction is attributed to. `gt` itself is visual (Alpine), but `a` / `i` /
  `:add` must **send the focused member id as a parameter**. Never keep a
  server-side session idea of "current member" — it drifts the moment the app is
  open in two windows.
- **Sessions are server-side, not a bare signed cookie.** A `sessions` table
  (`id`, `member_id`, `created_at`, `last_seen_at`) backs an opaque `HttpOnly`
  session cookie, so a single lost/stolen device can be logged out without
  rotating a secret that would log out the whole household. Sized for a
  LAN/Tailscale-only deployment: long-lived sliding sessions, no lockout or rate
  limiting, `SameSite=Lax` plus checking HTMX's own `HX-Request` header is enough
  CSRF defense at this scale. Revisit all of this if this is ever exposed to the
  open internet.
- **No self-registration.** An admin (role 0) creates a member row and sets its
  initial password — household membership is closed, the same way `ffm.toml`
  editing is admin-only.
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

- **Locale is pt-BR — except currency, which is household-wide config.**
  Dates stay `dd/mm/yyyy` unconditionally. Money's *symbol and number
  format* (decimal/thousands separator, digit grouping) come from
  `ffm.toml`'s `[locale] currency` — admin-only, one value for the whole
  household, **never a per-member preference like theme**. Every member's
  transactions land in the same shared totals (INs/OUTs/NET, balances); a
  per-member currency would mean either fake, unbacked exchange-rate
  conversion or two members seeing different-looking numbers for the exact
  same total. Picking a currency picks its whole conventional format as one
  preset (choosing USD switches to dot-decimal/comma-thousands, not just
  the `$` glyph) — there's no independent "keep comma-decimal but show $"
  combination. See `design/spec.md`'s Configuration section for the schema
  and the supported set. This still isn't a signup-flow setting: there is
  no self-registration (see Data model, below), so it's set the same way
  `layout`/`privacy` are — by the admin, in `ffm.toml`, whenever the
  household is first configured. Format server-side; do not push locale
  logic into Alpine.
- **UI copy is a separate axis from the pt-BR locale above, and defaults to
  English.** `dd/mm/yyyy` dates and the currency-driven number format are
  fixed regardless of language; the *words* — row status text, statusline
  messages, modal labels, button copy — are English by default, with i18n
  support planned but not built yet. Sample/seed data (bill names like
  "Água · Sabesp", bucket labels) stays realistic Brazilian-household
  Portuguese on purpose — that's content the household typed in, not app
  copy — but anything the app itself says needs to be in English until a
  real i18n layer lands, not written in Portuguese and translated later.
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
