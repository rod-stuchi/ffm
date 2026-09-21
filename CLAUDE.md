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
interactions. This is the **normative** visual and interaction spec. Its live
`ffm.toml` editor pane is **superseded** — see `ffm-settings-mockup.html` — and
will be replaced when this file is next revised.

`design/ffm-prototype.html` — the annotated version: every row state with a written
explanation of *why* it looks that way. Read this when a state's meaning is unclear.

`design/ffm-auth-mockup.html` — the three pre-auth pages (`/login`, `/setup`,
`/invite/:token`) and their error states. The only surface that lives **outside**
the `.app` shell.

`design/ffm-settings-mockup.html` — `:household`, `:profile` and `:members` as
vsplit panes, shown inside the app shell because watching the ledger while a
setting takes effect is the entire argument for a vsplit.

### Port, don't import

The mocks are throwaway vanilla JS. They hold all state in a JS array and re-render
`innerHTML` — **exactly the client-side data model this architecture forbids.**

Take from them: the CSS custom properties, the row markup and its states, the keymap
table, the copy. Rewrite the behavior against HTMX endpoints. Never `<script src>` a
mock, never copy its render loop.

---

## Visual system

### Tokens

The mock's `:root` custom properties are the design tokens. **Never hand-copy a
hex value into a component** — every component rule reads `var(--token)`, which
is what lets a palette swap actually reach it. `just css-check` enforces this.

**Where the tokens live, amended after the port.** The original instruction was
"mirror them in `tailwind.config.js` under `theme.extend`", which turned out to
be the wrong shape twice over:

- **The values do not live in CSS at all.** They are served as a `<style>` block
  rendered by `ui::theme::render()` — the full light/dark triple — because
  `ffm.theme = system` means the server cannot know which mode the device will
  use. A JS config would put the eleven values in a second place that the
  rendered block cannot override, which is precisely the drift this rule exists
  to prevent. `static/css/tokens.css` therefore carries scalars and fonts only:
  no colour, deliberately, so there is exactly one source for those.
- **Tailwind v4, with a CSS-first `@theme` block, and no `tailwind.config.js`.**
  `@theme` maps utility names onto `var(--token)` rather than onto hex, so the
  utilities follow the runtime swap like everything else.

**And "no custom stylesheets to write" (the Styling row above) does not
survive contact with the row.** `.row.is-focus .fbadge .flabel` is a descendant
rule firing off state set on an ancestor; expressing that combinatorial state
machine in utilities means re-deriving every combination as a template
conditional. The split actually used:

| layer | where | what |
|---|---|---|
| tokens | server `<style>` + `tokens.css` | the 11 values, the derivations, fonts, scalars |
| components | `static/css/*.css` in `@layer components` | every named class, ported near-verbatim from the mocks |
| utilities | inline in templates | one-off spacing and grid scaffolding in *new* markup |

`@layer components` is what keeps the Styling row's real claim true: layered
rules lose to utilities regardless of specificity, so `class="row is-focus mt-2"`
works without `!important` and nothing collides.

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

**Which palette and light-versus-dark are two independent facts, stored in two
different places.** The palette is a personal preference and belongs to the
member (`member_settings.theme_name` + `theme_overrides`). Light/dark is a
property of the **device** — the same person uses a bright laptop and a dark
phone — so it lives in the `ffm.theme` cookie alongside `system`.

Keeping them separate is what makes a pre-auth page themeable at all: `/login`
can honour your light/dark without knowing who you are, and falls back to the
bundled `ffm` palette because an anonymous visitor's palette choice would have
nowhere to live. `/setup` and `/invite` are the exception that proves it — they
*do* show the full 11-palette picker, because there it's a form field writing
`member_settings`, not chrome.

**The picker previews by swapping a server-rendered `<style>` block**, not by
recomputing colours in JS: the control does an `hx-get` and the server returns
the 11 tokens plus their `color-mix()` derivations from the same palette table
the app itself uses. One round trip per click, zero JS, and the preview provably
cannot drift from the real thing.

---

See @design/spec.md for the keymap, row states and config schema.


## Layout

Three modes. `auto` is the default and resolves by available width.

**`auto` must be resolved in CSS, not on the server and not in Alpine.** The
server does not know the viewport width, so a round-trip would be wrong on first
paint and on every resize. Render the threshold into a media query from Askama:

```html
<style>
  @media (max-width: {{ me.layout_min_split_width }}px) { .stage { /* tabs */ } }
</style>
```

Zero JS, correct before the first frame, no business logic in Alpine. The mock uses
JS only because it has no template layer.

**The rule is unchanged; only the source of the number moved** — the threshold is
a per-member setting now rather than an `ffm.toml` key (see Configuration in
`design/spec.md`), and it is still rendered into one media query server-side. The
pre-auth pages are the one place this does not apply: there is no member row
before you log in, so `.auth` must not read any `layout_*` value and sizes its
card in plain CSS.

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

### Editing — forms live in the ledger, not in modals

- **`e` edits the row in place**: the focused row opens into its own form
  (Cancel / Save at its foot). **`A` / `I` / `E` open the rule pane** — a column
  in the ledger grid right after the member pane being edited, pushing
  later panes right (the ledger scrolls sideways rather than hiding a pane
  — households can have more than two members), with an arrow on its edge
  pointing at the row. `A` and `I` add a dashed placeholder row at the cursor
  for the arrow to point at; Create turns it into the real row.
- **Why:** the rule form outgrew a dialog, and a dialog hides the ledger
  you're editing — the same argument that made `:household` a vsplit instead
  of a modal (see `design/spec.md`'s Configuration section).
- **Drafts outlive focus.** Leaving a row mid-edit collapses it back to
  read-only; unchanged drafts vanish, changed ones stay marked on the row
  (dashed border, `unsaved` tag, a statusline count) until saved or
  discarded, and several can be parked at once. In the real app a draft is
  visual state — unsubmitted form values — so it lives in Alpine, never on
  the server; Save is the only thing that sends it.
- **Still modal:** the `dd` delete confirmation, the private-row
  re-attribution confirm, the `?` keymap and the `P` file carousel —
  confirmations and viewers, not forms.

---

## Data model

### Decided

- **Months are views, never copies.** A single continuous transactions table,
  time-sliced. There is no "copy last month's sheet" operation, ever.
- **No hard deletes.** Deleting sets a status and a deletion date. Nothing is
  removed from the database.
- **Restore puts the deleted row back, in place — reversed from the earlier
  "restore inserts a new superseding row" decision.** Cloning the row on undo
  meant one `dd` + `u` left two rows in the ledger for one transaction (three
  after a second round trip), and the ledger is what people read every day;
  paying that permanent cost for a chain almost nobody follows was the wrong
  trade. The traceability it was there for now lives in the **change log**
  (below), which records far more than restores ever did. "No hard deletes"
  is unchanged: the row is kept and marked deleted, `u` clears that mark, and
  both facts are logged.
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
  `password_hash` and `role` (0 = admin). **No self-registration** — household
  membership stays closed, and there is no `/signup` route. There are three ways
  in: `/setup` (first run only, creates the household and the first admin),
  `/invite/:token` (a single-use link an admin issues), and `/login`.
- **The invitee sets their own password — an admin never types somebody
  else's.** This reverses the earlier "an admin creates the row and sets its
  initial password". The admin still decides *who* gets a link, so nothing about
  the closed household changed; what changed is that there is no initial password
  to hand over, and the member picks their own name, language and colour scheme
  when they accept. The `members` row is created on accept, not when the link is
  issued — so `password_hash` stays `NOT NULL` rather than having a
  half-real, password-less member in the table. See `design/spec.md`'s Auth &
  sessions section for the full model, including how a forgotten password is
  recovered.
- **Configuration is three tiers, and `ffm.toml` is now three keys.** The file
  holds `db_path`, `receipts_root` and `backup_dir` — **nothing else**. Everything
  a household would actually want to change moved into the database:
  - **`household_settings`** (one row, admin-only, via `:household`) — name,
    currency, default language, default visibility, `show_recurring`. What's here
    is what cannot coherently differ between members.
  - **`member_settings`** (one row per member, via `:profile`) — language, theme
    and overrides, line numbers, all four `layout_*` values, `ledger_sort`,
    `search_scope`, `show_delta`, group and fold defaults, and a nullable
    privacy default that falls back to the household's.

  The reason is the one already argued for theme: a shared file that two people
  both want to edit is the file *disagreeing with itself*. Once that's admitted
  for theme it applies unchanged to line numbers, sort and every layout
  threshold — all of which describe a person at a screen, not a process. Only
  `db_path` genuinely cannot move, since you can't read a setting out of a
  database you haven't opened. **The live `ffm.toml` editor pane is retired**;
  the three remaining keys are read once at startup, not watched, and shown
  read-only in `:household`.
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
  faces) and admin-only settings become inaccessible — `:household`,
  `:currency` and `:members` all refuse, and the **Household** button
  disappears from the toolbar rather than sitting there disabled — exactly as if
  this member held no admin role at all. No
  session swap, no acting-as-someone-else, no writes attributed to anyone
  else — see `design/spec.md`'s Auth & sessions section for the full
  reasoning and why that distinction ruled out "impersonate" as the name.

### Change log

Every change to a transaction, a rule or a file is recorded, append-only,
and read per row with `gl` ("go to log") in a side pane — the same column,
arrow and parking behaviour the `A`/`I`/`E` rule editor uses.

- **Two tables, not one.** `change` holds one row per user action (a save):
  `at`, `member_id`, `source` (`user` | `import` | `materializer` |
  `system`), `action`, `entity` (`transaction` | `rule` | `file`),
  `entity_id`, a nullable `rule_id`, `period`, a nullable `undo_of` and a
  one-line `note`. `entity` also covers `member`, for the two account actions
  an admin can take against somebody else — `password_reset` and
  `sessions_revoked` — so the one place an admin can affect another member's
  ability to sign in is recorded in the same append-only log as everything
  else. `change_field` holds one row per field actually touched:
  `field`, `old_value`, `new_value`, `kind`. One wide table would either
  repeat the actor and time per field or bury the diff in unqueryable JSON.
- **`kind` travels with the value** (`money` | `date` | `day` | `bool` |
  `text` | `path`), so the pane formats old/new the way the app does without
  inferring from the field name, and a stored amount never depends on the
  household's *current* currency setting.
- **`rule_id` is stamped on transaction changes**, which makes "this row plus
  the rule behind it" a single query — exactly what `gl` shows, with the
  rule's own entries tagged so the two are never confused.
- **An undo is an entry, not a rewrite.** `undo_of` points at the change
  being reversed, so the log reads "deleted", then "restored · undo of
  18:39". Nothing in the log is ever updated or deleted — the same "nothing
  is destroyed" rule the ledger follows.
- **It absorbs the audit commitments made elsewhere in this file** — the file
  soft-delete record (`old_path`, `deleted_at`, `deleted_by`, `reason`) and
  the upload metadata row (uploader, timestamp, kind, original filename)
  are `file_*` changes with fields, not a second parallel audit mechanism.
- **One entry per save, expandable to its fields.** The pane groups by day,
  newest first, and filters to `this row` / `rule` / `all`. It is read-only:
  `Esc` closes, `Ctrl-h` hands focus back to the ledger, `j`/`k` walk the
  entries and `Enter` expands one. It closes when the cursor leaves the row.
- **A row with nothing recorded still opens the pane**, saying so. An empty
  history is an answer, not an error.

### Recurring rules

- **Created from scratch via a dedicated key — never by turning an existing
  occurrence into one.** This reverses this repo's earlier "no separate
  new-rule form" stance: `R` on a row that wasn't recurring yet used to open
  the rule editor in a creation mode, pre-filled from that transaction.
  That's gone — the create keys open a blank rule editor with no anchor row
  at all, and editing a rule is its own key (a no-op with a statusline
  message on a plain row, the same treatment `gd` already gets there). The
  reason is the barcode-binding feature below: a recurring rule can
  optionally be bound to a barcode file, and that binding has to happen at
  creation time or not at all — there's no still-plain transaction lying
  around afterward to retroactively attach it to. Saving the form inserts a
  genuinely new row (the same `add`-family undo entry as `a`/`i`/`:add`),
  it never mutates one that was already there — materialization's
  `UNIQUE(rule_id, period)` idempotency was never actually at risk from
  this change, since there's still exactly one row per period either way.
- **The create/edit keys are a matrix, which is a second reversal.** The
  first pass bound `r` to "create a recurring bill" and `R` to "edit the
  rule" — a split justified by barcode binding, which is a property of
  *bills*. That justification doesn't generalise the moment income can
  recur too, so the bindings are now one consistent grid rather than four
  ad-hoc keys:

  |  | one-off (occurrence) | recurring (rule) |
  |---|---|---|
  | **spend** | `a` · `:add 50 mercado` | `A` |
  | **income** | `i` · `:in 2400 freelance` | `I` |
  | **edit** | `e` | `E` |

  Lowercase acts on the occurrence, uppercase on the rule — the rule this
  repo already claimed to follow, with no exceptions left. `r` and `R` are
  now unbound. **`i` stops meaning "add before the current row"**: `a` adds
  below, and above is `a` then `K`. That gesture was documented but never
  actually implemented (`a` and `i` ran identical code), so nothing real is
  lost — but it is a deletion, not a rename, and shouldn't be quietly
  reintroduced.
- **The due date lives on the rule, not the occurrence, once a row is
  recurring.** `E`'s Due date field (day-of-month) is the one place it's
  editable; `e`'s Day field disappears entirely for a recurring row, rather
  than offering a second control for the same fact with no rule for which
  one wins on the next materialized period. A non-recurring row has no rule
  to hold a due date, so `e`'s Day field is exactly where it stays.
- **Bucket is the same per-member autocomplete everywhere it appears** —
  the rule form's Bucket field and the occurrence form's Bucket field both
  read the identical `SELECT DISTINCT bucket WHERE member_id = ?` tag
  cloud (see Categorization & grouping below), not two separately-behaving
  widgets that happen to share a label. A new rule's form starts Bucket empty —
  a fresh rule gets an explicit choice, not an inherited guess.
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
  - **KNOWN HOLE: renewing a window silently rewrites history.** The page is
    derived from the rule's *current* `window_start`, and a cyclic bill renews
    by re-entering it — so the moment the 2027 window is set, January 2026's
    row stops reading "page 1" and starts reading "page −11, outside this
    file's 10 pages". The file rail and the `P` carousel would both begin
    flagging history that was never wrong. The fix is a
    `rule_barcode_window(rule_id, window_start, window_size, file_path,
    uploaded_at, retired_at)` table, with an occurrence resolving against the
    window that covered *its own* period. "Derived, never stored" still holds:
    the page number is not stored, the **window** is. Deferred to the receipts
    phase, but written down here so it stays a decision rather than becoming a
    discovery.
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
- **Dropping a barcode/boleto PDF onto a member's panel binds nothing — it's
  a notifier, not an upload path.** An earlier design had the drop parse the
  file and infer which rule it belonged to (by issuer + period + value
  proximity, or by matching the dropped filename against a row's title);
  both were rejected as too fragile — a real bill's filename, or its
  content, shouldn't have to line up with what's already in the ledger. The
  only place a barcode file is ever attached now is the rule's own editor:
  `A`'s optional upload field at creation, or `E`'s upload field afterward —
  both scoped to one specific, already-identified rule, so there's no
  matching left to get wrong. A panel drop just checks that panel for
  recurring, tracked rules still missing a barcode
  (`tracked && recurring && !no_barcode && !client_code`) and says so in the
  statusline if any exist; otherwise the dropped file is discarded with no
  message at all. Full parsing design (FEBRABAN barcode decoding,
  value/due-date extraction, issuer profiles) still lives in
  `design/spec-bill-import.md` — it runs on whatever file lands in `A`/`E`'s
  own upload control, the same as it would have on a panel-dropped file
  before. Income rules never have one at all: money arriving has no boleto
  to scan, so the whole barcode apparatus is absent from `I`'s form.
  - **`no_barcode` is a rule-level flag, decided once at creation, independent
    of `tracked`.** It exists for bills paid by wire transfer / PIX / TED,
    which never have a barcode at all — but the household may still want
    the *receipt* tracked on disk (`tracked` stays about the
    `{month}.{ext}` proof-of-payment file, an entirely separate fact per
    the three-file-kinds model above). A rule with `no_barcode = true`
    never shows the barcode upload control again, in either form.
  - **A rule gets an optional `client_code`** — the subscriber's own
    account number (Sabesp's "matrícula", Enel's "número do cliente"),
    read from the bill's text layer, never the barcode itself. It lives on
    the rule, not the occurrence — the same account every period, unlike
    `barcode_raw`/value/due date which are genuinely per-period — and it's
    populated by attaching a file in the rule's own editor, not typed by
    hand: no form field, shown read-only once set, absent until a file has
    been attached. A parse that resolves to an existing rule but reads a
    *different* client code than the one on file is flagged, not silently
    overwritten — same "flag it, don't guess" posture as the multi-page
    barcode window above. It can also be unlinked independently of the
    rule itself — a narrow correction for "the wrong file got attached the
    first time," distinct from `u`'s full revert of the last change.
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
- **`receipts` is also the extension index, and that is its real second job.**
  The path ends in `.{ext}`, and the extension is whatever was uploaded, so the
  path cannot be *constructed* without reading `receipts.ext`. That makes the
  check two steps, not one:
  1. read `receipts.ext`, build the one exact path, `exists()` it — so a file
     deleted by hand correctly reads as unpaid;
  2. when there is no `receipts` row, probe the four allowed extensions — so a
     file *added* by hand is still found.
  Four `stat` calls at worst, still not a traversal. Skip step 2 and
  hand-filing into the vault silently stops working, which would quietly
  remove the entire reason the filesystem is authoritative in the first place.
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

### Income

Income was always in the ledger — `kind = 'in'`, a positive amount, summed
into the pane header — but it was second-class: no rule, no recurrence, no
files. It now uses the same recurring-rule machinery as a bill, pointed the
other way. `I` creates one, `E` edits it, `i` still adds a one-off.

- **Income runs on its own axis: arrival, not payment.** A bill is
  `paid`/`scheduled`/`pending`; income is **`expected`** until the money
  lands and **`received`** after. It never shows a paid or scheduled mark.
  The ring is the same elapsed-time arc a bill's due date drives, counting
  toward the rule's **expected day** instead — same fact, same colours, not
  a second thing to learn.
- **Confirmation needs no new verb.** A tracked income is confirmed when
  its payslip exists on disk — the identical `exists()` rule, no manual
  tick, no exception. An untracked income uses the manual tick that
  untracked rules already get, relabelled "Mark as received manually".
- **An amount that varies is projected, not carried forward.** A rule with
  `varies` set (commission, freelance) can't seed a new period from its own
  number the way a fixed bill does. Until that period is confirmed the row
  shows an **estimate**, computed from history by one of `lowest` |
  `average` | `last` over the last N confirmed periods.
  - **`lowest` of the last 3 is the default**, deliberately conservative:
    for income the dangerous error is over-estimating, which produces a
    shortfall. Bills are the mirror image, which is why their
    carry-forward default is the opposite.
  - **Only confirmed periods feed an estimate.** An unconfirmed month is
    skipped, never folded in — otherwise two unconfirmed periods in a row
    would project a guess from another guess.
  - **A rule's first occurrence has nothing behind it** and falls back to
    the rule's own stated amount, the same carve-out bills already have.
- **An estimate is computed, never stored.** The persisted amount is always
  the rule's baseline or the real confirmed figure; the projection is
  derived on read. That way it follows new history instead of freezing an
  old guess into the data, and it never writes a spurious "amount changed"
  entry into the change log.
- **An estimate is marked everywhere it reaches.** The row shows `~R$ …` in
  a quieter colour; the pane's INs and Balance and the household's IN and
  NET all carry the `~` when any component is a projection. Excluding
  projections from the totals would make a future month's NET useless;
  showing them unmarked would present a guess as a fact. The mark carries
  whether the money moved, the tilde carries whether the number is real —
  one channel per fact, as everywhere else.
- **The MoM delta inverts for income.** The arrow is direction, the colour
  is whether it's good news: more money out is bad, more money in is good.
  A recurring income gets a delta (worth comparing period to period); a
  one-off and an unconfirmed projection don't — the latter would be
  comparing a guess against a fact.
- **Income lands at the top of its pane at creation**, which is where a
  household reads what came in. That's a starting sort key, not a pin:
  `J`/`K` move it anywhere afterwards and `:sort date` treats it like any
  other row. A hard "income always floats" tier was rejected — it would
  contradict being able to move it, and it would override an explicit sort.
- **File slots keep their fixed numbering; only the labels change.** Slot 1
  is the document stating the amount (a spend's `Bill`, an income's
  `Invoice`), slot 3 is proof the money moved (`Receipt` / `Payslip`).
  Slot 2 is scheduling, which is spend-only — you don't schedule money
  arriving — so `g2` on an income row is a no-op with a message, the same
  treatment a missing file already gets. `g1` and `g3` therefore still mean
  one consistent thing on every row in the ledger.
- **Taxonomy is the same columns with income wording.** `bucket` is
  unchanged; `bill_name` is labelled **Source** and `total_installments`
  **Payments** in the income form. Same columns, same computed path — do
  not add parallel fields for them.

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
- **The materializer stamps `transactions.due_date`; the rule owns `due_day`.**
  Both columns exist, which looks like one fact in two places — it is not, and
  the split has a rule. The rule carries the day-of-month; materializing an
  occurrence stamps a real date from it, **clamped to that month's length**, so
  a rule due on the 31st means the 28th in February and the 30th in April
  rather than failing. Downstream, the occurrence's stamped date is what the
  ring, the overdue check and the unresolved badge all read.
  - `e` never edits it for a recurring row — that is why the occurrence form's
    Day field disappears there, so there are not two controls for one fact.
  - Editing `due_day` on the rule changes **future materializations only**.
    Past occurrences are left as they were recorded, exactly as the rule form's
    own notebox already promises.
- **"Today" is the process's local date, and the deployment must set `TZ`.**
  The ring, `scheduled overdue`, the unresolved badge and `min(current_month,
  end_date)` all depend on it, and `ffm.toml` is three keys, none of them a
  timezone. Adding one would be wrong: one process serves one household in one
  place. So read `chrono::Local` at startup and **log it at boot** —
  `ffm · timezone America/Sao_Paulo · today 21/09/2026` — because a household
  watching a bill go overdue at 21:00 otherwise has no way to find out why.

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
   Rodrigo has it open in an edit form (or parked as a draft); both add in the same second. (The
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
  (`id`, `member_id`, `created_at`, `last_seen_at`, `label`) backs an opaque
  `HttpOnly` session cookie, so a single lost/stolen device can be logged out
  without rotating a secret that would log out the whole household. Sized for a
  LAN/Tailscale-only deployment: long-lived sliding sessions, no lockout or rate
  limiting, `SameSite=Lax` plus checking HTMX's own `HX-Request` header is enough
  CSRF defense at this scale. Revisit all of this if this is ever exposed to the
  open internet.
- **The `HX-Request` CSRF check is for *authenticated* routes only.** `/login`,
  `/setup`, `/invite/:token` and `/lang` are plain form POSTs and never carry
  that header, so a blanket middleware would reject all four and make the app
  impossible to enter. They rely on `SameSite=Lax` alone — sound here, because a
  forged login POST gains an attacker nothing, `/lang` has no security value, and
  setup and invite-accept are single-use by construction. **This is written down
  because the obvious "fix" is to apply the check everywhere.**
- **No self-registration, and no `/signup` route.** Household membership is
  closed. An admin (role 0) issues a single-use invite link; the person accepting
  it sets their **own** name and password. An admin never types somebody else's
  password, and there is no initial password to hand over. First run is `/setup`,
  which exists only while `members` is empty.
- **A forgotten password is recovered by an admin-issued reset link, not by a
  fresh invite.** `member_invites` carries a nullable `member_id`: NULL means
  the link creates a member, set means it sets `password_hash` on one that
  already exists. The distinction is load-bearing — an invite's accept path
  *inserts*, and `members.name` is `UNIQUE`, so recovering through one would
  force a different name, which rewrites `slug`, which is that member's
  receipt-vault folder. **Issuing a reset writes a `password_reset` entry to
  the change log and revokes that member's sessions**, because it hands the
  admin the ability to write to the ledger as somebody else — the one thing
  preview-as-non-admin was carefully built *not* to allow. There is no
  self-service reset: no mail sender, nowhere to send a token.
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

- **There is no single "locale". There are three independent axes, and fusing
  them is the mistake to avoid.**

  | axis | source | what it changes |
  |---|---|---|
  | date format | **fixed** | nothing — always `dd/mm/yyyy` |
  | money format | `household_settings.locale_currency` | symbol, decimal and thousands separators |
  | language | `member_settings.language` | **the words, and only the words** |

  A `pt-BR` UI in a `USD` household shows Portuguese copy and `$1,234.56`. A
  member switching to English does not move a single decimal point. Format all
  three server-side; never push any of this into Alpine.
- **Currency is household-wide and admin-only, never a per-member preference
  like theme.** Every member's transactions land in the same shared totals
  (INs/OUTs/NET, balances); a per-member currency would mean either fake,
  unbacked exchange-rate conversion or two members seeing different-looking
  numbers for the exact same total. Picking a currency picks its whole
  conventional format as one preset (choosing USD switches to
  dot-decimal/comma-thousands, not just the `$` glyph) — there's no independent
  "keep comma-decimal but show $" combination. It's chosen on `/setup` and
  edited in `:household`; see `design/spec.md`'s Configuration section for the
  schema and the supported set.
- **UI copy defaults to English, and i18n is now designed** — `en` and `pt-BR`,
  per member, in `member_settings.language`. The catalogs are Fluent files with
  **compile-time-generated accessors**, so a renamed message id fails
  `cargo build` rather than surfacing as a blank string at runtime; the build
  also rejects any date or currency formatting inside a catalog, which is what
  structurally enforces the three-axis separation above. The pre-login choice
  lives in an `ffm.lang` cookie, not `localStorage`, so the first byte of HTML is
  already in the right language with zero JS. See `design/spec.md`'s
  **Language & copy (i18n)** section.
- **Sample/seed data stays Brazilian-household Portuguese on purpose** — bill
  names like "Água · Sabesp", bucket labels like "Mesada". That's content a
  household typed into its own ledger, not app copy: it never passes through the
  catalog, it stays Portuguese when the UI language is English, and it must not
  appear in any `.ftl` file. Anything the *app itself* says is a catalog message.
- **Copy is written from the user's side of the screen.** Name things by what
  people recognise, active voice, a control says exactly what happens. Errors
  explain what went wrong and how to fix it.
- **Never expose internal ids in UI copy.** The old `↩ #1042` exception went
  away with the superseding restore row — the change log speaks in times,
  actors and verbs, not row ids.

---

## Gotchas already paid for

These cost real debugging time in the prototype. Do not re-introduce them.

- **An HTMX swap destroys the Alpine state inside it — and drafts live in
  Alpine.** Three decided rules collide here: a draft is unsubmitted form values
  so it never goes to the server; several can be parked at once; and the ledger
  is replaced by HTMX fragments. Swap the container a draft lives in and the
  draft is gone, with no error and nothing to recover.
  **This dictates where the swap boundaries go, so get it right in the first
  template rather than re-cutting every fragment later:** the `x-data` holding
  parked drafts must sit on a container *outside* every `hx-target`, or the
  swap must preserve it (`hx-preserve`, or a morph swap). A fragment that both
  holds draft state and is its own swap target cannot work.
- **A scan for forbidden content must read the content, not the prose about
  it.** Bitten three times already, in three different tools: `build.rs`'s
  currency-glyph gate (scan Fluent text elements, never raw bytes — a comment
  legitimately says "formatted by fmt::money, e.g. R$ 1.234,56"); the sandbox's
  seed-purity test (strip `#` comment lines — a comment names the sample data
  in order to say it does not belong); and `just css-check` (strip `/* */`
  first — a comment says "never `#fff`"). A rule that forbids a thing will
  quote the thing. Strip commentary before matching.
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
- **Fluent wraps every placeable in invisible BiDi isolation marks
  (U+2068/U+2069) by default.** In a mono statusline those render as phantom
  width, breaking the `.st.grow` ellipsis measurement and any string comparison
  in a test. Call `set_use_isolating(false)` on the bundle.
- **A pre-auth template has no member row.** Every layout context must carry
  `me: Option<&Member>`, and the `.auth` layout must not read `layout_*` or
  `theme_name` — otherwise the login page either won't compile, or worse,
  compiles against defaults that silently diverge from a real member's.
