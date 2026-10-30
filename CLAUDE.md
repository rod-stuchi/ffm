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

`design/ffm-charts-mockup.html` — the `:charts` viewer: its three tabs, every
chart, the IN-up / OUT-down convention and the hover tips, in all eleven
palettes. Port the markup and the CSS; its JS render loop is the mock's own.

`design/ffm-settings-mockup.html` — `:household`, `:profile` and `:members`
shown inside the app shell, because watching the ledger while a setting takes
effect is the entire argument for a split rather than a full-screen buffer.

**The mockup puts them in a left vsplit; the app puts them in a full-width
split at the foot of the ledger, and that is deliberate.** A 420px column is
the wrong shape for a settings form and worse for a result list, and these
surfaces are about the *household* — not about one member and not about one
row, so they do not belong in the member panes' row. The two constraints the
vsplit was chosen for still hold: the split takes half, so the ledger stays
visible and live behind it, and it is still not a dialog with a pending copy
of the truth. See `design/spec.md`'s Configuration §Surface. A pane that IS
about one row — the rule editor and the change log — still opens as a column
beside that row.

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
  no color, deliberately, so there is exactly one source for those.
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
  `--neg` color is chosen to contrast with the page ground, so the ground is the
  correct foreground. Hardcoded white breaks on light themes (Gruvbox Light's
  `pos` is `#79740E`; Rosé Pine Dawn's is teal).
- **Semantic color is separate from the accent.** Accent = "you are here"
  (focus rail, gutter zero, mode badge). Green/red = money facts (paid, up, down).
  They must never collide.
- **Selection vs. focus use different channels.** Focus = accent left rail +
  tinted ground. Selection (`x`) = `--info` inset bar. One is "where I am", the
  other is "what I picked".
- **Both themes, always.** Every theme ships a light *and* a dark variant. Define
  the full light palette on bare `:root`; redefine tokens under
  `@media (prefers-color-scheme: dark)` guarded as `:root:not([data-theme="light"])`,
  and again under `:root[data-theme="dark"]`. Never give a color its only
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
can honor your light/dark without knowing who you are. **Its palette is the
last signed-in member's, from an `ffm.palette` cookie** — amended from "always
the bundled `ffm`", which made signing out look like leaving for another app.
The cookie is written from `member_settings` at sign-in, on a palette change
and at sign-out, is never read once signed in (the row decides there), and is
`ffm` when absent or unknown. A cookie and not `localStorage` for `ffm.lang`'s
reason: storage reaches the page only after a script runs, which is a flash of
`ffm` and a repaint. There is still no picker on `/login` — an anonymous
visitor's own choice would have nowhere to live. `/setup` and `/invite` are the
exception that proves it — they *do* show the full 11-palette picker, because
there it's a form field writing `member_settings`, not chrome.

**The picker previews by swapping a server-rendered `<style>` block**, not by
recomputing colors in JS: the control does an `hx-get` and the server returns
the 11 tokens plus their `color-mix()` derivations from the same palette table
the app itself uses. One round trip per click, zero JS, and the preview provably
cannot drift from the real thing.

---

**`design/spec.md` holds the keymap, row states and config schema. It is not
loaded each session: read the section you need** (`rg -n '^##' design/spec.md`).

- **The row component**: row states, density, add form, statusline, loader, gutter, file rail, keymap (Act · Mouse · Find · Fold · Files · Undo · Type · Edit fields), hiding amounts, phone, toasts, `:order`, confirmation rule
- **Auth & sessions**: pre-auth shell, `/setup`, invites, turning off, resets, console, `/login`, preview
- **Income** · **Change log (`gl`)**: tables, pane
- **Receipt tracking & files**: taxonomy, layout, file kinds, `spans_period`, watcher, live feed, `:prune`, upload, trim/shrink, bill import, manual paid
- **Categorization & grouping**: folds, ring, `S`, fold state
- **Charts (`:charts` / `gc`)**: tabs, scope, keys, how it is drawn, where the numbers come from
- **Unresolved issues across months** · **Configuration — three tiers**: schemas, currency, Surface · **Language & copy (i18n)**


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

- **The pane is centered.** Tabs mode shows one pane, so there is nothing for
  it to sit beside and no reason for it to hug the left edge of a wide screen.
  `justify-content: safe center`, never a plain `center`: the stage scrolls
  sideways, and a plain `center` puts overflow past the *start* edge where
  nothing can scroll back to it. **Split mode is deliberately left alone** —
  panes there are a row, and centering a row is a different question with a
  different answer.
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
- **A pane has a floor it will not be squeezed below**, and it is a member
  setting (`member_settings.layout_min_width`, default 420px), not a number
  in a stylesheet. Under it a row's file rail and amount start colliding
  with the title, which is what the ledger is read for — so the stage
  scrolls sideways instead, exactly as it already does when the panes
  outnumber the width. Three unreadable panes are worse than two readable
  ones and a scroll. `0` means no floor, the same way `layout_max_width`'s
  `0` means unlimited. **Tabs mode lifts it**: one pane has nothing to share
  the stage with, so there a floor would only overflow a narrow viewport.
  It shipped as a CSS constant first, which was the wrong shape — how narrow
  a pane may get describes a person at a screen, which is the whole reason
  the `layout_*` values live on the member row at all.
- **A row narrower than 520px puts its name on a line of its own** — the
  rail and the amount move to a second line. On a phone the name had no room
  at all. The 520px is a container query on `.rows`, measured on the row's
  room rather than the device, and a fixed CSS number rather than a
  `layout_*` value: it decides whether a row fits its pane, not how panes
  share a screen. See `design/spec.md`'s On a narrow row.

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
- **The rule pane folds** — an always-open top block, then **Schedule** and
  **Files**, each one line of summary while closed — and the bill is **one
  card that is itself the drop target**, neutral rather than accent-tinted.
  It had grown to a ~1500px column with a dropzone wedged between two tinted
  boxes. See `design/spec.md`'s Edit fields.
- **Drafts outlive focus.** Leaving a row mid-edit collapses it back to
  read-only; unchanged drafts vanish, changed ones stay marked on the row
  (dashed border, `unsaved` tag, a statusline count) until saved or
  discarded, and several can be parked at once. In the real app a draft is
  visual state — unsubmitted form values — so it lives in Alpine, never on
  the server; Save is the only thing that sends it.
- **A form's actions end it: their own row at the foot, aligned right,
  primary last.** Cancel before Save, "no" before "yes" — the add form and
  every confirmation already did this; the `e` row (centered, and only by
  accident: `.entry`'s `align-items: center` inside a flex column) and the
  rule pane (Create at the left) did not. A destructive action that is not
  the primary — the rule pane's Delete — sits apart at the far left, never
  beside the button Enter would press. The pre-auth pages are the one
  exception: a single full-width submit on a card, with nothing to put
  beside it.
- **Still modal:** the `dd` delete confirmation (and its twin for removing a
  file from the `P` carousel), the question every `u`/`U` asks first, the
  `ga` member picker, the `?` keymap, the `P` file carousel and the
  `:charts` viewer — confirmations and viewers, not forms.

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
  Figma-style fractional indexing: a move between two neighbors writes **one
  row** — `sort_key = midpoint(prev.key, next.key)`. `ORDER BY sort_key` stays a
  plain index scan.
- **The sort key is global, not per-month.** Months are views; a per-month key
  would make a row's position vanish when seen from another slice. Ordering
  within a month falls out of `WHERE month = ? ORDER BY sort_key`.
- **Re-attribution needs a fresh sort key** in the destination member's ordering
  (append = after the current max).
- **Rolling two-month backup** of the SQLite file.
- **Member names are stored lowercase, matched ignoring case, and shown
  capitalized** — `normal_name` on the way in, `fmt::name` on every way out
  (each word: `maria clara` → *Maria Clara*). A name is the login identifier,
  so `Rodrigo` and `rodrigo` must be one account. Matching a name — sign-in,
  the taken check, `reset-link`, re-attribution's stored values — never goes
  through `fmt::name`; showing one always does. Names stored before this are
  left as typed and still match.
- **Members are real accounts, not just an attribution label.** `members` gets
  `password_hash` and `role` (0 = admin). **No self-registration** — household
  membership stays closed, and there is no `/signup` route. There are three ways
  in: `/setup` (first run only, creates the household and the first admin),
  `/invite/:token` (a single-use link an admin issues), and `/login`.
- **The invitee sets their own password — an admin never types somebody
  else's.** This reverses the earlier "an admin creates the row and sets its
  initial password". The admin still decides *who* gets a link, so nothing about
  the closed household changed; what changed is that there is no initial password
  to hand over, and the member picks their own name, language and color scheme
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
    It also holds what a new member's palette, layout and density **start
    as** (`default_theme`, `default_layout_*`, `default_density`): seeded into their own row when they
    join, never inherited, so changing one moves nobody's screen; `:profile`'s
    *Use the household's defaults* adopts them on request.
  - **`member_settings`** (one row per member, via `:profile`) — language, theme
    and overrides, line numbers, all five `layout_*` values, `density`
    (`default` | `comfortable` | `compact` — see `design/spec.md`'s Density),
    `ledger_sort`, `search_scope`, `show_delta`, group and fold defaults, and
    a nullable privacy default that falls back to the household's.

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
- **A non-admin reads the other ledgers and writes only their own.** Another
  member's shared rows, rules and files are read-only to them: no add, edit,
  delete, reorder, privacy toggle or file attach/remove, and no undo that
  would reach into it. **Re-attribution (`ga`) is admin-only**,
  even on your own row — handing a row to somebody writes into *their*
  ledger. The one server-side answer is `Viewer::may_write` (with
  `tx::may_write`, `tx::may_touch` and `rules::rule_access` built on it), and
  **every write endpoint added later must ask it** — the manual-paid tick
  (`POST /tx/{id}/paid`) does, and batch operations must when they exist. The pane shows `read-only` and
  the keymap refuses before a request leaves the browser, but that is the page
  saying so first; the server is the rule.
- **An admin can preview their own session with admin rights switched off —
  this is not impersonation.** It never changes *who* is logged in: still the
  same member, own pane, own edits, own private rows all fully visible and
  editable throughout. The one thing it changes is whether admin-only
  visibility and actions are currently in effect — other members' `private`
  rows go back to actually being hidden (the same rule any non-admin already
  faces) and admin-only settings become inaccessible — `:household`,
  `:currency` and `:members` all refuse, and the **Household** button
  disappears from the toolbar rather than sitting there disabled — exactly as if
  this member held no admin role at all. **That button exists now**: it is the
  Household entry in the ledger bar's profile menu, gated on `admin`, which
  `viewer.is_admin()` already makes false under preview. No
  session swap, no acting-as-someone-else, no writes attributed to anyone
  else — see `design/spec.md`'s Auth & sessions section for the full
  reasoning and why that distinction ruled out "impersonate" as the name.

### Change log

Every change to a transaction, a rule or a file is recorded, append-only,
and read per row with `gl` ("go to log") in a side pane — the same column,
arrow and parking behavior the `A`/`I`/`E` rule editor uses.

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
- **The trash (`Ctrl-u` / `:trash`) restores deletions out of `u`'s order.** A
  bottom-split list of every deleted row and removed file in the ledgers the
  viewer may write, each with a checkbox. Only deletions, because only they
  are independent — undoing an older edit out of order would overwrite a
  newer one. A restore from there is an ordinary action (`restore` /
  `file_upload`, no `undo_of`), so `u` reverses it next. Restoring an older
  file into a taken slot **swaps** — the live one goes to the trash as
  `superseded` — and `u` does the same, instead of refusing "occupied" and
  blocking every `u` behind it. See `design/spec.md`'s Undo section.
- **`u` undoes, `U` redoes — your own changes, in the focused pane, in the
  month on screen.** Redo is an undo of an undo: depth along the `undo_of`
  chain says what an entry is (0 original, odd undo, even redo), so there is
  no schema change and no second implementation of any verb, and a fresh
  change clears the redo stack. A change somebody else has changed since is
  **skipped and named**, never reversed over their work; every step says
  what it did and blinks the row. Not Ctrl-R: it reloads on Linux, and a
  reload drops parked drafts. `r`/`R` stay unbound.
- **Every `u` and `U` asks first** — reversing "a change from before today
  asks first", which left a stray `u` on today's edit acting at once. The
  first request answers `ffm:undo-ask`: a popup saying what the step would put
  back — the verb, the row, when, each field old → new the other way round,
  and anything skipped — every string the server's. The same key again, or
  `y`, sends the request again naming that change; Enter, Esc and any other
  key leave it, nothing written. A stale confirmation (the stack moved in
  between) is asked about again, never acted on.
- **`part_of`: an entry that is part of another, never a step of its own**
  (migration 0019). `:order` moves rows in several months at once, and `u`
  must take it back in one press from the month it was made in — where those
  rows are not. So the copy writes one **anchor** — `entity = 'member'` (the
  pane's owner), `action = 'edit'`, `period` the month copied from, note
  `order::COPIED`, a `months` field — and each row moved is a `reorder` in its
  own month, `part_of` the anchor, with `position` for `gl` and the exact
  `sort_key` for the undo. `tx::next` picks the anchor in that pane and month
  and never a part; `db::order::reverse` writes a new anchor and puts back
  every row still on the key the copy gave it, leaving one moved since. Redo
  is `reverse` on the undo's anchor. Two amendments to 0001 follow: a member
  entry with a `period` is that member's pane, not an account action; and
  this *is* the "ad-hoc reuse of `edit`" 0001 warned against — `change`
  cannot be rebuilt for a new verb, because `change_field` and `undo_of`
  reference it and the drop fails under foreign keys inside a migration's
  transaction.
  - **A selection's `J`/`K` uses it too, anchored on a row.** Every row it
    writes is a `reorder` with `position` and `sort_key`; the first is the
    anchor, an ordinary transaction entry `tx::next` picks like any move,
    and the rest are `part_of` it. `u` reverses it by keys
    (`tx::reorder_back`, through the same `order::put_back` the copy's
    reversal uses), not by the position delta a single move's undo replays.
    Which is why "a copied order moved X since" asks that a part's anchor be
    a member entry, and why `gl` never shows a `sort_key` — `position` beside
    it says it in words.
  - **A copy's rows still count as fresh changes in their own months**, so a
    copy onto September clears September's redo stack, as any change there
    would. And a move by hand that a later copy moved again is skipped by
    `u` there and named (*a copied order moved "X" since*): reversing it
    would be a relative move from the copied position.
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
  *bills*. That justification doesn't generalize the moment income can
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
  `{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}.{ext}`
  — the `instance` segment is only present when the rule has one.
  **The member folder is `{slug}`, no longer `{member_uuid}-{slug}`.** The
  uuid was there so a rename would not move the folder, but names are
  `UNIQUE` and cannot be changed, and `slug` is `UNIQUE` too, so it bought
  nothing except a path nobody could read in a vault meant to be browsed by
  hand. `services::vault` moves an old `{uuid}-{slug}` folder, and the stored
  paths pointing into it, once at startup; `domain::receipt_path::vault` is
  the one place the folder name is decided. Slugified
  (lowercase, spaces → `_`). Renaming a rule's taxonomy fields must move its
  existing files to the new path — it isn't just a label edit. **It does
  now, and the path has one builder:** `receipts::slot_of` — the **rule's**
  bucket, bill name and instance, the row's member and amount — read by the
  upload, the ledger's mark, the carousel and the watcher alike. Any write
  that moves the computed path (a rule saved, a row given to another member,
  an amount in a filename changed, and the undo of each) calls
  `receipts::refile_row`, which moves the live files there, never over
  another file, logged as the system (*re-filed where its row now points*)
  so `u` treats it as part of the change that caused it; a startup pass
  (`vault::refile_all`) re-files whatever a crash or an older version left.
  A rule's bucket or bill-name edit also renames every one of its rows, so
  `:group bucket` folds where the files are; recorded once, on the
  `rule_edit` entry. A restore puts a file back where its row looks *now*.
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
- **A bill can cover several months from one upload instead of a fresh
  file every month** — a city's IPTU (one PDF holding the single payment and
  ten installments), or a bank's carnê with a loan's whole run. Opt-in per
  rule (`spans_period`, *Barcode covers multiple months*); off by default,
  since most bills genuinely do get a new file each period. Only the **bill**
  is shared: `{month}_scheduled.{ext}` and `{month}.{ext}` stay per month —
  each installment is still scheduled and paid on its own.
  - **Each month takes the code due in it** (`services::installments`). Every
    utility code carries its due date in its free field and every bank slip
    in its date factor, so attaching the file reads every code in it, keeps
    one per month in `rule_bill_code` (migration 0017) with the page it is
    printed on, and **stamps every existing month** with its own amount,
    due date and code — logged as one `import` entry per row, so `u` puts
    each back. A month the materializer creates later is stamped from its
    code when it appears. **Whichever way the file arrives** — `E`'s
    dropzone, a row's slot 1, or `A`'s before Create — it goes through one
    function (`import::attach_covering`); `A`'s Create used to skip it and
    stamp only the current month, with the ÚNICA. A month whose code another
    live row already holds still takes its amount and date, sourced `text`
    (a `barcode` source needs the code on the row — a CHECK says so).
  - **Toggle on, the installments; toggle off, the single payment.** Two codes
    due in one month (the IPTU's ÚNICA and installment 1) resolve to the one
    whose value repeats in the file. With the toggle off the bill is read
    like any other — its first code, which on an IPTU is the ÚNICA.
  - **Every covered month shows the one file**, stored on the row it was
    dropped on: slot 1 on January opens it on January's page
    (`receipts::shared_bill`, *Bill · p.1*). A month the file has no code
    for shows nothing from it.
  - **No page count, no first month — reversed.** The first design asked for
    *Pages in this file* and *First page covers* and derived "page N is month
    N". A real IPTU holds eleven codes on three pages, so neither field could
    describe it, and the window's `file_path` was never written — no month
    but the one the file was dropped on ever showed it. The two fields are
    gone from the pane; the rule's `window_size`/`window_start` columns and
    `rule_barcode_window` stay unread (the rule's CHECK still wants the two
    columns set while `spans_period` is on, so the form fills them).
  - **Codes with no date** go, in the order the file prints them, to the
    rule's active months (`months`) of the year — a value that appears only
    once is left out first, so a single payment does not take January.
  - **A new file retires the last file's codes** rather than overwriting
    them, so history keeps the code it was billed with.
- **A bill dropped anywhere on the page goes to the row of the rule that
  already knows it — recognized, never bound.** This reverses "a panel drop
  binds nothing — it's a notifier" (which was never built: a drop outside a
  row's targets did nothing at all). An earlier design had a drop parse the
  file and *infer* its rule — by issuer + period + value proximity, or by
  matching the filename against a row's title — and that stays rejected: a
  real bill's name, or a near match, shouldn't have to line up with the
  ledger. What changed is that rules now carry a binding — client code,
  issuer CNPJ, card, company code — that a first attach in `A`/`E` or on a
  row's slot 1 set **explicitly**. `POST /bills/find` (`import::find`) only
  *recognizes* a bill by that binding: candidates are the viewer's writable,
  tracked spend rules a bill has bound, each reads the text with its own
  recipe, and a rule matches only when `import::names_rule` says an account
  or an issuer agreed — the barcode kind alone never does, an unconfirmed or
  unbound rule never does. The row is the rule's in the bill's **due-date
  month** (the month on screen when it shows none); several matches are a
  picker in `ga`'s shape, never a guess; none is a statusline line. Nothing
  is written there: the file then takes the upload a drop on target `1`
  takes — trim view (titled with the row), binding check, questions, the
  amount read off the bill, the log — and binding still happens only where
  it did. Full parsing design (FEBRABAN barcode decoding,
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
    been attached.
  - **The first bill attached BINDS the rule, and a later bill that
    disagrees is ASKED about — before anything is written.** This hardened
    the earlier "flagged, not silently overwritten", which stored the file
    and noted the mismatch afterwards; it then softened from a refusal to a
    question (see *Never a dead end*, below). The binding, checked in this
    order (`import::binding_check`): `client_code` when a recipe could read
    one (it decides alone — two houses with the same water company share a
    company code); the **issuer's CNPJ** (`issuer_id`, 14 digits since 0016
    — a bill agrees when its text prints it anywhere, read and checksummed
    by `issuers::cnpjs_in` with no recipe at all); the **card's last four**
    (`card_last4`, 0014 —
    a statement lists every card, so a later one agrees when it lists the
    bound card anywhere); the barcode `kind`; the arrecadação
    `barcode_company`. A value a bill did not carry proves nothing; a bound
    rule that could compare **nothing at all** is *unconfirmed*, and asked
    about too. Only what is empty is filled. A cobrança bank code is
    deliberately not a binding: it names the bank, and one bank issues slips
    for a hundred billers.
  - **Never a dead end.** A bill that contradicts the binding ("wrong
    row?"), or that nothing can confirm, is a **question**, not a refusal:
    the server answers `ffm:ask` (`tx::ask`), the page raises the
    `dd`-shaped modal — only `y` attaches; Enter, Esc and anything else
    leave it out, nothing written — and `y` re-sends the same upload with
    `attach_anyway`. It skips **only** the identity check: a duplicate, a
    locked or too-large file, permissions, all still refuse. The log notes
    it (`ATTACHED_UNCONFIRMED` / `ATTACHED_MISMATCH: found ≠ known`, not
    localized, like `receipts::MOVED`), and after a **contradiction** the
    rule learns nothing from that bill.
  - **A rule learns to read its own bills — `config/issuers.toml` is
    retired.** It was a hand-written file of per-biller patterns; now each
    rule has a **recipe** (`rule_recipe`, 0016: the issuer's name and CNPJ,
    and patterns for the amount, due date, account and card), applied
    locally to every bill of that rule (`RuleRecipe::profiles` — the same
    pipeline, a one-issuer profile). A bill with no barcode is read from its
    text by its rule's recipe (`issuers::resolve_text`, dates in
    `09 JAN 2026` form too), `barcode_raw` NULL and sources `text`.
  - **The household's assistant writes the recipe** — a CLI on the server
    (`services::assistant`: `codex exec`, `claude -p`, `gemini -p`), on the
    subscription its login belongs to, chosen by the admin in `:household`
    (`household_settings.assistant`, default `codex`, `none` works too). It
    is asked only when a spend rule's bill leaves something open (no amount,
    no due date, not readable), or a rule with no recipe cannot name its
    issuer from one printed CNPJ: on the **first** bill, and again when a
    redesigned layout stops the recipe reading (`web::import::wants_teaching`).
    Every other month is read here, with no assistant.
    - **What leaves the machine is page 1, redacted** (`assistant::redact`):
      CPF-shaped numbers and full card numbers masked, a company's CNPJ kept.
      Tools off, nothing persisted, a 120 s limit, run from a scratch dir.
    - **Nothing it says is trusted until checked** (`assistant::verify`):
      each pattern must compile, capture once, and find — first, in the
      bill's own text — exactly the value claimed; the CNPJ must be valid and
      printed. An assistant's `R\$\s*([\d.,]+)` reads a card's credit limit
      as its total, and is dropped, never offered.
    - **A person approves it.** The proposal is parked under a token and put
      as a question (`tx::Ask` — the lines it read, *Keep and attach* `y`);
      `y` stores it (`recipes::store`, retiring any old one — history kept,
      logged "bill recipe taught by codex"), anything else attaches without
      it. In `A` the box lists what it read and Create keeps it.
    - **Retiring the file**: at startup, `recipes::convert_profiles` gives
      every rule bound to one of the old profiles that profile as a
      `migrated` recipe and re-binds it by CNPJ — from the operator's copy, or
      the frozen last one shipped (`src/db/retired_issuers.toml`).
    - A subscription is for the subscriber's own use: this is a household's
      tool on its own server. Serving other people's bills would need API
      keys.
  - **Every way a bill reaches a row is checked the same way** — `A`/`E`'s
    dropzone, and the row's own slot 1 (the `+` menu, a drop on target `1`,
    `gu1`). The row path used to store whatever it was given, so a
    credit-card statement replaced a bound utility bill unchecked. A
    different barcode *kind* is a mismatch on its own: a bank slip is never
    a utility bill, even though it carries no company code to compare. A file
    with no readable barcode is asked about on a bound rule — nothing could
    confirm it; `y` files it as a plain document, noted — and filed as a
    plain document on an unbound one, from `E` as from the row.
  - **A row holds a bill only while it holds the file.** Removing the bill
    leaves its barcode on the row, and that used to refuse the same PDF on
    every other row — "already on Moto CG", about a row showing no bill.
    Now attaching it elsewhere **takes it over** (`tx::release_bill`, inside
    the attach's transaction): the old row's barcode is cleared, and its
    trashed copy is **retired** — a `file_unlink` noted `receipts::MOVED`,
    which the trash (`U`), `u` and every restore leave alone, because a bill
    is on one row only and that copy can never come back. The file stays in
    `.delete/`; nothing is destroyed. A holder that still has the file, or
    one the viewer may not write to, refuses as before. A row deleted with
    `dd` is untouched: it is outside the unique index and the check alike.
  - **A card's payment slip names an account, not a bill.** A cobrança
    whose due factor and amount are both zero (`… 3 00000000000000`) lets
    the payer choose what to pay, and an issuer like Inter prints the very
    same digits on every month's fatura — so January's read as "already on
    10/2026" and was refused, which was never an error. Such a barcode is
    exempt from one-barcode-one-row: no duplicate refusal, no takeover, and
    `idx_tx_barcode` leaves it out (migration 0013), as does the trash's
    `bill_held_by`. The test is `boleto::names_an_account` — positions 6–19
    of the 44 digits all zero on a cobrança — and the SQL repeats it; keep
    them in step. Every other barcode keeps the rule.
  - **Where no barcode can tell bills apart, the text does.** A text-only
    bill, or a card's slip that is the same every month, gets a
    **fingerprint**: its text reduced to letters and digits, lowercased,
    blake3 (`import::fingerprint`), on its `receipts` row (`text_hash`,
    0014). Spacing, page breaks and punctuation are left out, so a trimmed or
    shrunk copy fingerprints the same. The same fingerprint on another live
    row is *"already on 01/2026"* — a refusal `y` never lifts.
  - **A bill's amount replaces the row's amount, whoever set it.** The
    checksummed barcode is the evidence of what is owed, so it wins over a
    seed from the materializer *and* over a figure somebody typed with `e` —
    reversing "replaces a seeded one, and only a seeded one", which kept a
    typed amount and only flagged the difference. What makes the overwrite
    acceptable is that it is never silent: it is **its own entry in the
    change log** (`entity = transaction`, `action = import`, source
    `import`, read *"read from a bill · amount R$ 883,34 → R$ 131,45"*),
    not one field folded inside the attach entry's "3 fields"; it bumps the
    row's `version`, so an `e` form open on the row is refused rather than
    writing the old figure back; and it is written after the upload's
    entry, so `u` puts the old amount back first and takes the file away
    second. **The due date keeps the older rule**: a date set with `e`
    (`due_date_source = 'user'`) is kept and the difference flagged. A date
    the bill *does* replace is a field on the same `import` entry, beside
    the amount, so `u` puts both back — it used to log only the amount, and a
    drop that replaced a wrong date (Vivo's 30/05/2022, read off a QR code)
    could not have its date undone.
  - **`A` takes the bill too, not only `E`.** The rule does not exist yet, so
    the drop is read and parked (`services::pending`) and the pane shows what
    it will bind to; Create carries a token and attaches it to the occurrence
    it just materialized. Either pane shows a **Bill on file** box — issuer,
    barcode kind, company code, client code, never the amount or due date,
    which change every period. It can also be unlinked independently of the
    rule itself — a narrow correction for "the wrong file got attached the
    first time," distinct from `u`'s full revert of the last change.
- **A scheduled payment still unconfirmed past its due date is a distinct,
  warning-colored state** — not a generic `pending`. A
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
- **The vault is watched, and a file changed by hand is recorded and shown
  live** (`services::watch`, `receipts::reconcile_row`, `GET /live`). `paid`
  never needed it — it is read from the disk on every render — but the record
  and the open pages did. Each change, debounced 1.5s, reconciles that member's
  tracked rows for that month against the disk: a file with no live
  `receipts` row was **filed by hand** (a row is added, `file_upload` logged as
  the system, *found on disk — filed by hand*); a live row whose file is at
  neither its computed nor its stored path was **removed by hand** (marked
  deleted, `file_delete` logged, *removed from disk by hand*, and kept out of
  the trash — there is nothing to restore). No list of the app's own writes
  is kept: an app write updates its row in the same handler, so reconcile
  finds nothing. It never moves a file — a hand-filed receipt does **not**
  supersede the scheduled one. Startup catches up on what changed while the
  process was down, in its own task after the bind — behind the vault's upkeep
  and the materializer's first pass (see the gotcha on two writers at
  startup). A folder that arrives is read for its files' months; only one that
  is gone reconciles the whole member. What it finds is pushed over
  Server-Sent Events as **ids only**; each page re-fetches the row as itself,
  never swapping a row open in `e` (its kept copy goes stale instead).
- **Every change reaches every open page — announced from the change log,
  not by each handler** (`services::live_feed`). A receipt uploaded on one
  computer used to show paid there and nowhere else: only the watcher
  announced anything, and a burst of row fetches from one page was cut to its
  last by htmx. Every write already appends to `change`, so once a second the
  feed reads the new entries and publishes what they reach — an edit, the paid
  tick, privacy, an import or a file is **its row**; a create, delete, restore
  or reorder **its member's pane** (`GET /panes/{member}/{period}`); a
  re-attribution **every pane** of the month; a rule **its member's pane in
  every month**. No handler can forget to announce. The page coalesces a
  burst for 250ms, fetches each row with its own element as the htmx source,
  keeps the cursor on its row by id, and holds a pane with a row open in `e`
  until that form closes. The watcher now announces only what it found by
  hand, for *updated from disk*.
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
- **A tracked rule may carry a PDF password, and the vault keeps the file
  unlocked.** Banks send statements (employers, payslips) that open only with
  a password. `recurring_rules.pdf_password` (migration 0012) holds it, and
  every way a file reaches a rule's row — its slots, `A`'s and `E`'s dropzone
  — passes through `import::unlock_for` before anything reads, parks or
  stores it: a locked PDF is opened with `qpdf` (`services::pdflock`) and the
  **unlocked copy** is what lands, so the tab, the `P` carousel, `pdftotext`
  and a person browsing the folder never meet the password again. One with
  only an owner password is unlocked with none. A locked file the password
  does not open, or a rule with none, is **refused before anything is
  written** — and `receipts::store` refuses one too, before moving the live
  file aside, as the backstop for a path that forgot.
  - **Plain text, on purpose**: it is a key that has to open files, so a hash
    is useless, and whoever reads `ffm.db` already reads the ledger. It is
    **never in the change log** — a save records only whether one is set
    (`always` when one replaces another, so the move still shows) — and only
    `E`, which opens for writers alone, renders it.
  - **A dropzone sends what the pane's field says now**, saved or not, so
    typing the password and dropping the statement works before Save or
    Create; `A` parks the unlocked copy.
  - **A file filed by hand is never rewritten** — the watcher records, it
    does not rearrange — so a locked PDF dropped into the vault stays locked.
- **Upload, view, delete are scoped to recurring-rule occurrences only** — a
  one-off transaction has no taxonomy to file itself under, so it gets no
  attachment feature.
- **Upload is three distinct actions — add barcode / add scheduled / add
  receipt** — not one upload plus a question, since each file kind has its own
  fixed name and its own effect on state. The file is renamed to the
  deterministic convention on disk; the user's original filename is kept as
  metadata only.
- **Every PDF stops at a trim-and-shrink view before it is filed** — a row's
  three slots and `A`/`E`'s bill dropzone alike — and a stored one can go
  back through it from `P` with `t`. It replaces what the household did by
  hand: `qpdf` drops the pages nobody needs (the advert at the end of a
  statement), Ghostscript's `/ebook` shrinks what is left, and the view says
  what that comes to (*987 KB → 78 KB −93%*) before anything is written. A
  text file goes straight in.
  - **A screenshot or a photo stops there too** (`.png`, `.jpg`), as one
    page with nothing to trim — only Shrink, done in pure Rust
    (`services::imgshrink`), so it needs no `gs` and is always offered. It
    replaces the household's `img2ocr` (OCR into a PDF, then Ghostscript):
    almost all of a phone screenshot's size is its alpha channel, its 16 bits
    a channel and colors nobody can tell apart, so the OCR bought no size on
    top: a 297 KB Nubank receipt comes to 58 KB as a palette PNG.
    Laid over white, 8 bits, a side capped at 2400 px; a PNG becomes a
    palette PNG — exact at ≤ 256 colors, else 32 by NeuQuant, **never
    dithered** (dither took it from 69 KB to 205 KB) and unfiltered; a JPEG
    is turned upright by its EXIF orientation, which re-encoding would drop,
    and saved at q80. **The extension never changes** — a PNG stays a PNG —
    and a file whose name and bytes disagree is filed as it came. The large
    view shows the copy Shrink would file, since for an image it is lossy.
    OCR stays a later step (`services/import.rs`'s tier 5).
  - **Save re-sends the finished bytes through the same upload the file was
    headed for** (`services::stage` holds the original meanwhile). Nothing
    about filing was reimplemented, so the bill's binding check, the
    duplicate check, the amount read off the bill, `superseded` and the log
    all run on what is actually filed — a bill whose barcode page was
    dropped is refused with the usual message, and the view stays open to
    put it back. From `P` it is a replacement, so the old copy goes to the
    trash and `u` brings it back.
  - **What Save sends is trimmed, never shrunk; the shrink is the last step
    before the vault** (`import::filed_bytes`, from the copy the size line
    already built, via `stage`). Every read — barcode, recipe, binding,
    amount, fingerprint — is of the unshrunk file. It used to be the shrunk
    one, and Ghostscript can re-encode a PDF's text (*é* came back *Ž*): the
    recipe stopped reading the bill, and its fingerprint stopped matching the
    same bill dropped again. `A` parks the unshrunk bytes with the shrink
    still to do (`pending`), and Create does it. No unshrunk copy is kept —
    the household's originals live in its email.
  - **`P` can make a PDF longer, through the same view**: `b` adds a page
    with a barcode drawn from a typed code (`services::itf`,
    `services::barcode_page` — a hand-written PDF page, nothing embedded),
    `a` appends another PDF. Both stage the longer file and open the trim
    view on it, so Save files it through the slot's upload like `t` does.
  - **Ghostscript is optional.** Without `gs` on the server the Shrink
    switch is absent and trimming still works; `qpdf` was already required.
  - **A shrink that does not come out smaller is thrown away** — an already
    lean PDF can grow under `/ebook` — and the view says so; only the page
    selection, if any, is applied.
  - **A bill spanning several months (`spans_period`) keeps every page.**
    Page N is month N, so dropping one would point every later month at the
    wrong page; the view offers only the shrink there, and says why.
- **A file badge or `g1`/`g2`/`g3` still opens the file in a new browser
  tab** — that stays the default, and no custom rendering is built for it:
  all four allowed extensions already render natively in a browser. `P`
  (see below) is a **second, additive** way to look at a row's files —
  reversing this repo's earlier "no in-app preview at all" stance, on the
  strength of flipping between a row's own bill/scheduled/receipt without
  losing the ledger underneath. **The carousel shows the file itself** —
  reversing "the kind, the path and an icon, never the bytes", which could
  not do the job the carousel is full-viewport *for*: reading a boleto's
  digits, or holding a phone up to its QR code. **A PDF's pages are drawn
  server-side by `pdftoppm`** (poppler, already required for `pdftotext`),
  at 300 dpi, and shown as images; a photo is itself, a text file is text.
  **As JPEG, kept by the browser, and loaded as they come into view**
  (`services::pdfpage`): PNG's encoder was the cost — 0.3–2.5 s a page, against
  45–190 ms — the page URL carries the file's version (`?v=`) so a reopen asks
  for nothing, and only the page `P` opens on loads at once.
  The browser's own PDF viewer was tried first, in a frame, and could not be
  zoomed from the carousel reliably in either Chrome or Safari — a frame is
  somebody else's program and answers to its own controls. **It is offered
  now as the second way:** `v` (an *Image ⇄ PDF* switch) swaps the images for
  an `<embed>` of the file, sharp at any size and zooming itself. Which side
  `P` opens on is the member's (`member_settings.pdf_view`, in `:profile`) —
  amended from "every `P` opens on the images" — and the images stay the default,
  because the images' zoom and keys work alike in both browsers. "Open in new tab" still
  hands over the real file. A bill shared across months scrolls to this
  month's page. See `design/spec.md`'s row-component
  section for the on-row badges and their keybindings.
- **`P` opens that carousel for the focused row's files** — and so does
  `gp`, the `g`-prefix spelling beside `gl` and `gd`. `1`/`2`/`3`
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
  the one modal in the app sized to the full window on purpose. **All but
  the statusline**, which it and the trim view stop above (`--foot-h`):
  covering it hid every answer given from inside them — a removal, a bill
  refused on Save. Those answers are toasts now, drawn above both overlays.
- **Zoom (`+`/`-`, or `0` to reset) scales the preview** — the page images'
  width, so 100% is fitted to the view and above it the view scrolls to the
  rest. It is the same on every file type and in every browser, which is the
  reason the pages are images: three attempts at zooming the browser's PDF
  viewer from outside (CSS zoom on the frame, a fresh frame with `#zoom=`,
  widening the frame) each worked in one browser at most, its controls
  at the caption's right end rather than in the button row, the same
  placement any image viewer's zoom control uses. **Each file keeps its own place**
  — page, scroll and zoom — while the carousel is open, so `h` back to the
  bill finds it where it was left; a file not yet shown opens at its top, at
  100%. Amended from "each file starts back at 100% when you flip to a
  different one", which threw away where you were in the bill every time you
  glanced at the receipt. Its reason still holds: nothing is *carried over*
  to an unrelated file, each one only remembers its own.
- **The row's other files load behind the one on show.** The carousel keeps
  one stage per file, stacked, only the current one visible
  (`visibility`, not `display: none`, which would drop its scroll): the file
  on show loads first, and once its pages are in, the others load out of
  sight, so `h`/`l`/`1`/`2`/`3` fetch nothing. Kept in the DOM rather than
  warmed in the HTTP cache because the page images are `no-cache` with no
  validator — a replaced bill keeps its path — so a cache would fetch, and
  `pdftoppm` would draw, every page again.
- **Carrying the amount forward is opt-in per rule, checked by default.**
  Materializing a new period seeds its amount from the *previous* period's
  actual value when this is on; off, the new occurrence starts with no amount
  at all — the same `R$ __,__` placeholder already used for a tracked row with
  no amount yet — forcing an explicit edit. For a bill that's genuinely
  different every period (a credit-card statement), auto-filling last month's
  number would be actively wrong, not just unhelpful. The very first
  occurrence of a rule has nothing to carry forward from, so it always seeds
  from the rule's own `amount` field.
- **Deleting any of the three files is a soft delete.** The file moves into
  the **member's trash**, `{slug}/.delete/`, under the same path it had —
  never removed from disk — and a DB row records the move (old path,
  `deleted_at`, `deleted_by`, reason) so `u` can put it back. Same "nothing
  is destroyed" rule as everything else in this app. It was a `.delete/`
  beside each file, which left a bill's folder standing in the vault with
  only a hidden folder inside long after the bill was gone; one trash per
  member keeps the live tree to what is live, and folders a move empties are
  removed. A startup step moved the old layout over (`services::vault`).
  **A row's files follow the row.** `dd` moves every live file of the row into
  the trash — a deleted bill left in the vault reads as live to anybody
  browsing it — and one `u` brings the row and its files back together: the
  moves are tagged `moved with its row`, which `u` never picks on its own.
  **Replacing a file is the same soft delete**, tagged `superseded` — it used
  to write over the old path, destroying the old bytes while the row marked
  "deleted" pointed at the new file. And each deleted copy is named after its
  own `receipts` row (`09_barcode.r42.pdf`): a fixed `09_barcode.pdf` let a
  second delete of the same slot overwrite the first. Restore finds a file in
  any place a delete has ever put one, and refuses to overwrite a live file.
  **`:prune` is the one exception to "never removed from disk"**: an admin can
  remove a copy in the trash that is byte-identical (blake3) to a file that
  stays — no bytes are lost, which is the whole argument. Checked again on the
  server against a fresh scan, never the last copy of a group, confirmed with
  `y`, and logged as a retirement (`prune::PRUNED`) that the trash, `u` and
  every restore leave alone. See `design/spec.md`'s Pruning duplicates.
  **And a file in the trash longer than the household keeps them** —
  `household_settings.trash_keep_days` (0020), set in `:household`, 90 by
  default, 0 forever — is offered too, duplicate or not. Here bytes *are*
  lost, on purpose: the trash used to only grow. So the age counts from when
  the file was removed (`receipts.deleted_at`, never the bill's month), a
  file with no record is never offered by age, nothing goes but at an admin's
  `y`, and it is retired as `prune::EXPIRED` — `receipts::RETIRED` now has
  three notes, and every query that reads it takes all three.
- **Deleting a rule leaves its occurrences as orphaned bills** — never
  re-parented, never cleared. They keep `rule_id` (the change log and the
  files still need it), their files and their tracked status, because the
  files live at the rule's taxonomy; they stop claiming to recur: `↻` dim, no
  cadence or `7/12`, the popup says when the rule was deleted, and `E` says
  there is nothing to open. The ledger query still joins a deleted rule for
  exactly that reason — filtering it out of the join would orphan the files
  instead.
- **An untracked recurring bill gets an optional manual `paid` checkbox** — the
  one manual tick in the whole model, and only because there's no filesystem
  proof to contradict it. A *tracked* rule never gets this checkbox; the rule
  above is absolute for it. **It exists now, in three places writing one
  column** (`transactions.manual_paid`): a toggle in the `e` form, `m` on the
  focused row, and the row menu's *Mark paid* — "received" on an income. It
  was drawn by the row and explained by the `?` legend long before anything
  could set it. `tx::manual_paid_applies` is the one server-side answer to
  "does it apply" — live, generated by a rule, that rule untracked (a deleted
  rule still counts) — and every path asks it; the row's `data-paid` only lets
  the page refuse first. A one-off gets no tick either: a spend is recorded
  when it is made. It is logged as an `edit` of one `paid` field — the
  schema's action list has no `paid` verb, and one field reads inline — so
  `u` and `U` take it back and forth like any edit.
- **The paid marks are visually distinct from each other** — file-verified
  paid, manually-asserted paid (untracked only), and scheduled-overdue-warning
  are three different colors/shades, never sharing a mark. Designs need
  updating for this.
- **`end_date` is a plain, independently-settable field on every rule** —
  nullable, null meaning ongoing — not exclusive to installment loans. This
  is how an ordinary open-ended bill gets canceled: CPFL stops after July,
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
  due-date and recurring glyphs already occupy. **Paid** is installments
  confirmed (receipt on disk, or the manual tick) up to and including the
  row's month, so a month missed earlier is a gap on every row after it —
  the badge used to count occurrences, which read as position and named
  every month "paid". The **total** is the rule's run
  (`domain::schedule::Plan::run_for`): an installment count, or an end date
  (a canceled bill reads `5/7`), or — for a seasonal rule with neither —
  its months **this calendar year**, so IPTU's `1-10` reads `x/10` and starts
  again each January. An open-ended monthly rule has nothing to count to
  and no badge. `gd` adds the money: *3 of 10 · R$ 364,86 paid · R$ 851,34
  to go*, each month left at its own row's amount, else its bill code's,
  else the rule's, else the newest the run has seen — marked `~` when any
  month is projected that way. With no figure at all for some month, the
  *to go* half is left out rather than reading `R$ 0,00`.
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
  toward the rule's **expected day** instead — same fact, same colors, not
  a second thing to learn.
- **Confirmation needs no new verb.** A tracked income is confirmed when
  its payslip exists on disk — the identical `exists()` rule, no manual
  tick, no exception. An untracked income uses the manual tick that
  untracked rules already get, relabeled "Mark as received manually".
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
  a quieter color; the pane's INs and Balance and the household's IN and
  NET all carry the `~` when any component is a projection. Excluding
  projections from the totals would make a future month's NET useless;
  showing them unmarked would present a guess as a fact. The mark carries
  whether the money moved, the tilde carries whether the number is real —
  one channel per fact, as everywhere else.
- **The MoM delta inverts for income.** The arrow is direction, the color
  is whether it's good news: more money out is bad, more money in is good.
  A recurring income gets a delta (worth comparing period to period); a
  one-off and an unconfirmed projection don't — the latter would be
  comparing a guess against a fact.
- **Income lands at the top of its pane at creation**, which is where a
  household reads what came in — a hand-entered one, and a rule's first
  month; later months arrive where the rule sat the month before (see
  Materialization). That's a starting sort key, not a pin:
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
  unchanged; `bill_name` is labeled **Source** and `total_installments`
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
- **A new month arrives in the order the last one was left in.** An
  occurrence lands where its rule sat in the latest earlier month it has a
  live row in (the same member's pane): right after the counterpart of the
  nearest row above it, else right before the nearest row below
  (`db::order::inherited_key`, `domain::order::placement`, matched like
  `:order`). Only with nothing to go by — a rule's first month — does the old
  rule hold: income on top, a spend last. This is what makes a month arranged
  by hand, or by `:order`, carry forward without anybody arranging it again,
  and why `:order` never needs to write a month that does not exist yet. The
  rule-by-rule loop needs no change: whatever order the rules arrive in, the
  placement rebuilds the previous month's order.
- **The materializer stamps `transactions.due_date`; the rule owns `due_day`.**
  Both columns exist, which looks like one fact in two places — it is not, and
  the split has a rule. The rule carries the day-of-month; materializing an
  occurrence stamps a real date from it, **clamped to that month's length**, so
  a rule due on the 31st means the 28th in February and the 30th in April
  rather than failing. Downstream, the occurrence's stamped date is what the
  ring, the overdue check and the unresolved badge all read.
  - `e` never edits it for a recurring row — that is why the occurrence form's
    Day field disappears there, so there are not two controls for one fact.
  - Editing `due_day` on the rule changes **future materializations, and
    this month's row while it is unconfirmed** (`rules::edit_reporting`) —
    amended from "future only", which left the row a person was looking at on
    the old day after they changed the rule, and a rule ending this month never
    showed the new day anywhere. The restamp is an `edit` of the row's
    `due_date`, so `u` puts it back, and the save's statusline says where it
    moved. Earlier months, and this month once paid or received, keep the date
    they were stamped with: a confirmed row's due date is what it was paid
    against.
  - **The rule's title follows the same rule**, in the same `edit` entry —
    it used to reach only the next materialized month, so a rename looked
    like it had not worked. Only while the row still bears the old title: a
    row renamed with `e` keeps its own name.
- **"Today" is the process's local date, and the deployment must set `TZ`.**
  The ring, `scheduled overdue`, the unresolved badge and `min(current_month,
  end_date)` all depend on it, and `ffm.toml` is three keys, none of them a
  timezone. Adding one would be wrong: one process serves one household in one
  place. So read `chrono::Local` at startup and **log it at boot** —
  `ffm · timezone America/Sao_Paulo · today 21/09/2026` — because a household
  watching a bill go overdue at 21:00 otherwise has no way to find out why.
  - **`clock::today()` / `clock::now()` are the only readers** (`src/clock.rs`)
    — never `chrono::Local::now()` or `Utc::now()` at a call site. That is what
    lets a dev server run **as of another date**: `FFM_TODAY=2026-10-15`, set
    by `just dev 2026-10-15`, moves the clock by whole days in a **debug build
    only** (a release binary logs that it is ignoring it). It is an offset, not
    a frozen date, and it moves the UTC stamps too: faked alone, the date would
    disagree with the change log's `at`, and every `u` would ask about a
    change made a minute ago as one "from 12/09/2026".

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
- **Groups are framed, not color-coded.** A bordered container (reusing the
  existing `.card` visual language) plus a text label — deliberately not a
  per-category color system, since `bucket` is open-vocabulary and unbounded
  while a theme is deliberately only 11 tokens with `pos`/`neg`/`accent`/`info`
  already carrying fixed meanings. An unbounded set of user-typed strings
  either collides with those meanings or needs hash-based color assignment
  for no real gain over a plain frame.
- **Scoped to each member's own panel** — no household-wide, cross-member
  category view. Grouping only reorganizes the rows already in front of you.
- **An `Uncategorized` group catches rows with no `bucket` set** — grouping
  never silently drops a row. **It is always the last group**, whatever the
  order.
- **Groups have three orders, cycled with `S`** — first seen (the default),
  A–Z ignoring accents, and by cost, biggest spend first. A per-member
  setting (`member_settings.group_order`), like sort, and written by `S`, a
  statusline cell and a `:profile` radio alike. `s` beside it orders the
  rows, inside the groups as much as in a flat pane — **and never the groups
  themselves**: first seen is where a bucket's earliest row sits in the
  *manual* order (`LedgerRow::manual`), not in whatever `s` sorts by. It was
  the current sort's order, so `name` moved Moradia above Cartão. See
  `design/spec.md`'s Categorization & grouping.
- **A group header always shows its subtotal, open or closed.** This is why
  there is no separate `:budget`/report screen: `zM` (fold everything) on the
  real ledger already reads as a per-category summary. Browsing and
  summarizing are one mechanism at two zoom levels, not two features.
- **Beside the subtotal, a ring: the fold's marks, counted.** Each arc is the
  color a row's own mark wears — done, scheduled, late, and what is still
  open in the track's color — **sized by row count, not amount**, so a bill
  waiting for its amount still counts. Late is a red mark, which on the due
  day is one day before the month-picker's dot; plain rows and one-offs,
  which nothing could settle, are left out, and a fold of only those has no
  ring. `LedgerRow::tally` decides, beside `mark()`. Slices with paper
  between them, not a filled pie, which at 16px reads as a status mark. See
  `design/spec.md`'s The ring beside the subtotal.
- **A fold header is a total, so a row's answer carries it.** Every row
  fragment in a grouped pane swaps each fold's count and ring + subtotal out
  of band, into spans keyed by a hash of the bucket (never the fold's place,
  which a bill can reorder). A `dd`'d row used to go on counting in its
  header until a reload.
- **`x` on a fold's header selects the fold; `X` from inside it, `gx` the
  pane** — all selected → none, otherwise → all, the trash's rule. The
  statusline's Σ then reads that fold's subtotal; a fully selected header
  wears the `--info` bar. `x` is the one row key a header takes.
- **Grouping is a derived view, exactly like `sort = date`/`amount`.** The
  moment rows are re-clustered by category instead of read in stored manual
  order, `J`/`K` refuses with the same message pattern already established for
  sort — you cannot reorder a derived view, the app says so instead of
  pretending.
- **Default fold state on load (expanded vs. collapsed) is a per-member
  setting** in `member_settings`, a radio choice, not a checkbox list, since
  the two states are mutually exclusive. It was stored and never read; it is
  now where a window starts.
- **Fold state survives a month change.** A fold is keyed by its bucket
  (`data-fold`, the header's hashed id), never its place, and kept on the
  device per member in `localStorage` (`ffm.folds.{member}`), so closing the
  tab or the installed app keeps it; changing *Folds start* drops it.
  `zM`/`zR` reach buckets only other months have. See `design/spec.md`'s Fold state survives a month change.

### Charts (`:charts` / `gc`)

- **The one report screen, and it is about trend.** `zM` stays the month's
  per-category summary; `:charts` draws what it cannot — a range of months,
  and year against year. Full-viewport in the `P` carousel's frame,
  read-only, three tabs: Month, Year (a range, at most 36 months) and Years,
  which is **admin-only** (`viewer.is_admin()`, so absent under preview and
  refused if asked for). See `design/spec.md`'s Charts.
- **Drawn on the server, no chart library and no crate** (`ui::chart`,
  `web::charts`): boxes placed by percentage plus one stretched SVG per line,
  every mark a `var(--token)`, every amount `.money`. A JS library means the
  data as JSON in the page — the client-side model this app forbids.
- **IN up from the zero line, OUT down from it.** Position tells them apart,
  because no shade of `--ink` separates from `--pos` in all twenty-two
  palettes. OUT is a neutral; red is a deficit, a late bill or a bucket that
  got dearer. No color per bucket — small multiples on one scale.
- **Month reads `member_panes`; Year and Years read `db::charts::rows`**, one
  query for the range with the pane's own privacy and turned-off predicates,
  and the varying-income projection through the same steps the pane takes
  (`ledger::unconfirmed_projections`: history, confirmation, `project`). `tests/charts.rs` holds every month's sums to the panes',
  viewer by viewer — keep it passing when either side changes.
- **Years opens on the last ten years**, its *from* field reaching the rest,
  and its spent/earned-so-far cards draw this year against last or, with
  `v` (`lines=5`), against up to four years, each fainter — shading and
  end labels stay on last year alone.
- **A selection (`x`) narrows every chart**, followed across months as the
  same line (rule, else exact title, in one member's ledger); `s` toggles.
- **The viewer's keys press the stage's own links** (`data-ck`), so the
  keyboard and the mouse cannot disagree and Alpine decides nothing.

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
- **The dot stays current without a page load.** `GET /months/dot`
  re-renders only the trigger button — after a mutation that can change it
  (one route group marked by `live::mark_dot`, which merges `ffm:dot` into
  `HX-Trigger`), after any live event in any month, and when the tab becomes
  visible (unguarded: only the button is swapped, never a form).
- **Getting there is just existing navigation.** Clicking a flagged month cell
  jumps to it — no new command needed on top of the month picker that already
  exists.
- **The check is per-viewer, respecting existing privacy rules.** A private
  row's unresolved status only counts toward the badge for its owner and the
  admin (role 0) — never for another member who can't see the row itself.

### Sort and search semantics

- **The cursor follows the row it moves.** `j` moves me and `J` moves the
  thing — and the cursor is an *index*, so the instant the row under it
  moves, that index names whichever row took the vacated slot. Left alone,
  a second `J` moved *that* row back down and the two traded places
  forever: a row could not be walked down the ledger a press at a time at
  all. A count hid it completely, because `5J` is one request and one swap.
  The swap re-renders the pane, so the id is the only thing that survives
  it and the only thing the cursor can be restored by.
- **A count past the end lands on the end**, the way `99j` goes to the last
  line rather than declining to move. Refusing it was silent — `99J` on the
  first row did nothing at all, with an empty statusline, which reads as a
  broken key rather than as a row already where it was asked to go. Being
  genuinely at the end stays the quiet no-op it always was.
- **A selection moves together.** With rows selected (`x`) in the focused
  pane, `J`/`K` move every one of them, wherever the cursor is; with none,
  the cursor's row, as ever. One place is each selected row passing one
  that is not, so a gapped selection keeps its gaps and piles up at the end
  (`domain::order::shift`). Only the selected rows that changed place are
  written — the others keep their keys — so the log and `u` name what was
  moved, and one `u` puts all of it back. It had been promised in the keymap
  table for a long time while `J` moved the cursor's row alone.
- **`ledger.sort` gates reordering.** `J`/`K` work only in `manual`. Under `name`,
  `amount` or `date` they refuse with
  `ledger.sort = "date" — reorder needs sort = "manual"`. You cannot reorder a
  derived view; the app says so instead of pretending. Default is `manual`
  because this replaces a spreadsheet where rows *were* arranged by hand.
- **Four orders, and `s` cycles them `manual` → `name` → `amount` → `date`** —
  the fourth gesture writing `ledger_sort`, beside `:sort`, the statusline
  cell and `:profile`, and all four read one order: `Sort::ALL`, spelled by
  `settings::LEDGER_SORTS`, stepped by `Sort::next` (the page is handed the
  next value rather than keeping a second copy of the cycle). It pairs with
  `S`: `s` orders the rows, `S` the groups.
  - **`name` is A–Z by title, ignoring case and accents** — *Água* among the
    A's — with ties in manual order. It is a stable sort in Rust after the
    pane query (`domain::collate::name_key`, which `S`'s A–Z uses too):
    SQLite's `lower()` folds only ASCII, and a custom collation would be
    missing from the connection SQLx checks queries against. Adding the value
    rebuilt `member_settings` (migration 0018), since a CHECK cannot be
    altered in place.
- **An unpaid row sorts by its due day**, since it has no paid-on date yet.
- **`:order` / `gs` copies the pane's manual order to other months of the
  year** — a bottom drawer taking a months expression in *Only some months*'s
  grammar (`1-9`, `^10`), with a read-back the server renders per keystroke
  saying what each month would end in. See `design/spec.md`'s Copying the
  order to other months.
  - **Rows match by rule, then by exact title** (the k-th "Mercado" ↔ the
    k-th) — the "same line" the cursor follows across months. Matched rows
    take the source's order; **rows with no match go after them**, in their
    own order (`domain::order`).
  - **Only the rows out of place are written**: the longest run already in
    order (by strictly increasing key — two live rows can share one) keeps
    its keys, and the rest get keys spread between them.
  - **Refused under a sort that is not manual, on a read-only pane** — in the
    words `J`/`K` use — and on a month with no rows. **Not when grouped**, where
    `J`/`K` do refuse: a move by one place is ambiguous among folds, a copy of
    the whole manual order is not, and the folds are derived from it. An empty
    expression is refused, unlike a rule's, where empty means every month:
    here it would be a bulk write nobody typed.
  - **One copy is one `u`**, from the pane and month it was made in — see the
    change log's `part_of`, below.
- **`/` is client-side and scoped to the visible month.** It filters rows already
  rendered — an Alpine highlight over the DOM, no round-trip, which is what makes
  incremental search feel instant. The term is a regex, falling back to a literal
  if it does not compile. It matches title, date, due date, formatted amount, and
  the words `private` / `recurring` / `paid`.
- **Searching every month is a different verb.** `:grep` returning a result list,
  hitting a SQLite FTS5 index over HTMX. Do not overload `/` with it.

### The three that were open, and what they were decided to be

These carried "not yet decided" through the whole phase map. All three are
settled now; the reasoning is kept because each one closes off an alternative
that will otherwise be re-proposed.

1. **Concurrency — a row version, checked on the edit form's save, and
   nothing else.** `transactions.version` is bumped by every write and
   rendered into the edit form as a hidden field, so the save is
   `UPDATE … WHERE id = ?1 AND version = ?2` and zero rows affected means
   somebody got there first.
   - **The failure being prevented is a lost update**, not a conflict to
     resolve. Two people editing one row in one minute is rare in a
     household; one of them silently overwriting the other is what makes the
     ledger untrustworthy when it happens.
   - **Only `e`'s save is guarded**, because a lost update needs a form held
     open across somebody else's change and `e` is the only gesture that
     does that. `p`, `dd`, `J`/`K` and `ga` are instantaneous and fully
     undoable, and a stale-version refusal there would fire because a
     background materializer touched the row.
   - **A refused save keeps the draft.** The row collapses to read-only
     showing the other person's version and keeps its `unsaved · e to
     resume` mark, so nothing typed is lost and `e` reopens it. Discarding
     into the generic error row would throw away an edit somebody just
     typed.
   - **Rejected:** row locking (somebody walks away mid-edit and a bill is
     locked until a timeout nobody can see), SSE/websocket invalidation of
     open forms *as the guarantee* (see below), and last-write-wins with a
     "changed by Ana" note after the fact — which is the lost update, with a
     receipt.
   - **A live notification is a separate, additive requirement**, written up
     in `TODO_2.md`: a websocket telling you *while you type* that the row
     moved, with the diff against your draft and Save becoming Update. The
     version check is the floor and does not depend on it — if the socket is
     down the save is still refused and the draft is still parked.

2. **Seasonal / cyclic rules — one nullable `months` column, read only by
   the materializer.** `recurring_rules.months` holds a sorted comma list of
   month numbers, `NULL` meaning every month. The mock already demonstrated
   the behavior in seed data (`design/ffm-prototype.html:974`, IPTU running
   Jan–Oct; `:983`, school fees running Feb–Dec) and filtered on it in one
   line; what was missing was a control, because no mockup ever drew one.
   - **One function decides which months a rule runs in** —
     `domain::schedule::Plan`, with the cadence, the end date and the
     installment count — and two readers ask it: the reconciler, which skips
     a period whose month is not in the list, and the row's `x/N` badge,
     which counts them. It was the reconciler's loop alone until the badge
     needed the same answer; a second copy would have been two answers.
     Everything else reads occurrences, and an occurrence that was never
     materialized is simply not there — which is what keeps this a small
     change rather than a cross-cutting one.
   - **Editing it affects future materializations only**, exactly like
     `due_day`. Narrowing Jan–Oct to Jan–Aug does **not** delete the
     September and October rows already recorded: a form field must not
     trigger a silent bulk delete, and past occurrences are what happened.
   - **It is an expression field, not twelve toggles** — see
     `design/spec.md`'s Edit fields for the grammar. Toggles are a mouse
     gesture in a keyboard-first app; `1-10` is one thing to type. A live
     read-back underneath shows which months it resolved to, the same
     pattern the computed receipt path and the income estimate box already
     use.
   - **It applies to income rules too.** Income runs on the same rule
     machinery pointed the other way, and seasonal income is real — a 13th
     salary in December, teaching income only during term. One column, one
     control, no parallel concept. (Unlike the barcode apparatus, which is
     spend-only because money arriving has no boleto.)
   - **Still independent of the multi-period barcode window**: that asks
     *which page of the shared file is this month's*, this asks *does this
     month exist for this rule at all*. IPTU has both, which is why it is
     the demo row for each.

3. **Split mode does not assume two panes, and has not for a while.** The
   stage is a horizontally scrolling flex row of panes; `H`/`L` step through
   however many there are, and the rule pane joins as another column.
   `just seed` builds a three-member household, which is what proves it.
   - **The degenerate case is answered**: below `layout_min_split_width`
     every unfocused pane steps aside, which is tabs mode — now reachable by
     name with `gt` / `gT` / `2gt` and a real tab bar carrying each member's
     balance.
   - **The middle case — three of six panes fit — keeps scrolling
     sideways.** Every pane stays reachable and nothing new is invented. A
     "pin the focused pane, tabs for the rest" mode is the more usable
     answer at six members and is a whole new layout to design, style and
     translate; it is not worth inventing before a household that size
     exists. Falling back to tabs above a member *count* was also rejected:
     it puts a second rule in charge of a layout that a width setting
     already decides.
   - **`ga` re-attributes to a member picked by name — reversing
     `Ctrl-H`/`Ctrl-L`.** Those moved the row one pane left or right per
     press: one key away from `H`/`L`, so a slip moved money between two
     people's totals, and reaching the fourth member meant stepping through
     the two between. `ga` ("give to") opens a list of the household, each
     member under their pane's number — the `2gt` number — so `ga2` is one
     deliberate choice at any household size; `j`/`k` + `Enter` and a click
     work too, and the row menu has *Move to…*. A private row's warning sits
     in the picker, before the choice, so picking a name is the
     confirmation — there is no second `y` modal. `Ctrl-H`/`Ctrl-L` now only
     say where the gesture went.

### The ledger bar

- **Light/dark is a plain form POST to `/theme`, never JS.** The `ffm.theme`
  cookie is `HttpOnly`, so a script could not set it if it wanted to. The
  button shows the state you are **in** and submits the next one — the
  pre-auth control's shape unchanged — and the cycle itself lives on
  `ModePref::next` so the two controls cannot disagree about the order.
  - **Its `next` is restamped from `location` on submit.** `?pane=` is
    rewritten client-side, so the server-rendered value is stale the moment
    you press `L`; the rendered one stays as the JS-less fallback, and
    `safe_next` refuses anything that is not a local path either way.
- **The profile menu is built like `.mnav`, not `.filemenu`** — trigger and
  popup in one relatively-positioned wrapper with its own small `x-data`.
  `.filemenu` computes `position: fixed` coordinates only because it is
  anchored to a row in a stage that scrolls sideways; a bar control has no
  such problem. It anchors right, because it sits at the end of the bar.
- **Narrower than one row, the bar is two, in two tiers.** One row needs
  about 1070px, and squeezed below it the household name wrapped to three
  lines, IN/OUT/NET stacked into six, and on a phone the light/dark and
  profile buttons ran off the edge and scrolled the page sideways.
  **≤ 1100px** only breaks the line: row 1 is logo · household ·
  light/dark · profile, row 2 the month and the totals; nothing leaves.
  **≤ 720px** is the phone: the month arrows go (the picker and
  `[`/`]` remain), light/dark moves into the profile
  menu, and each total puts its label over its figure. The menu's copy is
  shown by the **same** media query that hides the original, so no width
  has both or neither; there light/dark is three direct choices rather than
  the bar's cycle, because through a menu a cycle is up to three rounds of
  open-and-tap. Both breakpoints are fixed CSS, not `layout_*` values: those
  decide how panes share a screen, and this is only whether the bar fits.
  A wrapping flex row with `order` and a zero-height `::after` as the line
  break, not a grid — the two rows share no columns.
- **The ADMIN / PREVIEW badge is the profile menu's `:preview`, at every
  width** — it used to sit in the bar beside the totals, and moved into the
  menu only on a phone. The bar is for the month and its money. While
  previewing, the profile button wears the badge's warning color (and says
  so in its title), so the state still reads at a glance.
- **Light/dark in the bar is its icon alone** — sun, moon or screen says the
  state, the word is its title and accessible name. A zero-width character
  gives it the profile button's line height, so the two match in any font.
  The pre-auth chrome keeps its words: there the buttons sit beside their
  `Alt-L` / `Alt-T` key-caps.
- **Hiding the amounts is a device cookie and a CSS filter** (`gh`, the
  statusline's eye). `ffm.hide` → `data-hide` on `<html>`, rendered by the
  server so the first frame is already hidden; `gh` flips the attribute at
  once and `POST /hide` only remembers it. Every amount on screen carries
  **`.money`**, which `ui.css` draws through the page's `#ffm-pixelate` SVG
  filter — **a new money element must carry it**, or it shows while the rest
  are hidden. The filter names no `x`/`y`: Safari places a subregion in page
  coordinates. See `design/spec.md`'s Hiding the amounts.
- **The bell sits beside light/dark, at every width** — every toast this
  window was shown, in `sessionStorage` (`ffm.notes`), cleared by every
  pre-auth page so a sign-out takes it along. Built like the profile menu;
  on a phone its popup is pinned to the gutters, since anchored to its button
  it ran off the left edge. "The statusline says …" elsewhere in these docs
  now means a toast: `say()` is the one place a message is shown.
- **The member's name is on the profile button and nowhere else in the bar.**
  It used to sit in `.htotals` as well, which printed it twice within 200px.
- **`:theme` and `:profile` both open `:profile`** — the difference is that
  `:theme` leaves the keyboard on the first swatch, which is the pane's first
  control. There is deliberately no second palette picker.
- **A settings pane stays open across the refresh a setting causes.** Several
  settings are rendered by the server — the gutter, the row order, every
  layout value — so writing one answers `HX-Refresh`, and a reload took the
  pane with it: the split is a swap into an empty mount, and a fresh page has
  an empty mount. You changed one radio and the pane you changed it in
  vanished. `?split=profile|household|members` remembers it, for the same
  reasons `?pane=` does: per-window, survives a reload, and needs no
  server-side memory that two windows would disagree about. **`:grep` is
  deliberately not remembered** — it is a result list for a term you typed,
  and reopening it on every reload is a nuisance, not a courtesy.
  **And it comes back scrolled to where you were.** Changing a control
  keeps the pane body's scroll offset in `localStorage` (`ffm.splitScroll`),
  restored once when `?split=` reopens the pane — and only within a few
  seconds, so a change that did not reload never scrolls a pane opened later.
  Otherwise a setting near the foot of `:profile` sent you back to its top on
  every change.
- **A palette change does not refresh at all.** It is one `<style>` block and
  nothing else on the page depends on it, so the write answers with that block
  `hx-swap-oob` and every color changes in place. This is what the Themes
  section above already described — the picker previews by swapping a
  server-rendered style block from the same palette table the app uses — and
  refreshing instead was the lazy reading of "the server renders this". It
  also made trying eleven themes eleven reloads.
- **The menu's items are named after the commands they run**, in mono, and
  those names never pass through the catalog: a command is an identifier a
  member types back, exactly like the values in the sort-refusal message and
  the command bar's own hint. The words live in each item's `title`, so a
  screen reader still gets them and neither locale loses anything. The menu
  teaches the command bar rather than duplicating it in prose.
- **Which obliges a bare command to mean something.** Naming an item `:theme`
  is a promise that typing `:theme` does the same thing, so it now opens the
  picker instead of answering `✗ theme is not a bundled palette` — the same
  reading `:grep` already had: a command missing its argument, not a broken
  one. **Do not put a command in that menu without checking its bare form.**

### Installed on a phone (PWA)

- **The ledger installs as an app** — Android's *Install app*, iOS's *Add to
  Home Screen*, opening full screen. `GET /manifest.webmanifest`
  (`web::pwa`) names it after the household; `partials/icons.html` carries
  the iOS tags, each page its own `apple-mobile-web-app-title` (or the icon
  is named after the tab title, month and all).
- **The manifest is public**, in the `open` router: a browser fetches it
  without cookies, and `/login` is where a new phone installs from. It says
  nothing `/login` does not.
- **No service worker, on purpose.** Installing no longer needs one, and the
  one thing it could add — an offline copy of a page — is a stale copy of the
  truth. Do not add one to "complete" the PWA.
- **Android installs only over HTTPS.** The household's own root
  certificate, trusted on each phone, is enough; the server's certificate
  must name the address typed.
- **The phone's bar takes the palette** — `<meta name="theme-color">` from
  `RenderedTheme::meta_tags`, the same pair-or-one rule as the CSS, and
  swapped out of band with the `<style>` block on a palette change. The
  manifest's own colors are the bundled `ffm`'s: it cannot see a member.
- **`viewport-fit=cover`, and the shell pads itself in** by
  `--safe-t/r/b/l` (`env(safe-area-inset-*)`, zero on a desktop). The
  statusline grows by the home indicator so its surface reaches the edge,
  and the full-screen overlays stop above `--foot-h` (statusline + inset),
  not `--status-h`. `fitViewport` zeroes `--safe-b` while the keyboard is
  up, which covers the home indicator anyway.
- **The shell is `100dvh`**, never `100vh`: installed on an iPhone, the page
  starts below the system's status bar while `100vh` is the whole screen, and
  the statusline sat under the bottom edge.
- **Status bar style `default`**, not `black-translucent`: the translucent
  one draws the page under white status-bar text, which vanishes on a light
  palette.
- **An installed iOS app keeps its own cookies**, apart from Safari: it signs
  in once on its own, and starts from its own light/dark and hide cookies.

## Server-side rules

- **The focused member is data, not visual state.** It decides who a new
  transaction is attributed to. `gt` itself is visual (Alpine), but `a` / `i` /
  `:add` must **send the focused member id as a parameter**. Never keep a
  server-side session idea of "current member" — it drifts the moment the app is
  open in two windows.
- **It lives in the URL — `?pane=<member_id>` — which is the one place that
  does not drift.** A URL is per-window by construction, so two windows can
  sit in two different panes without either lying to the other, and a refresh
  comes back where you were instead of where you started. `paint()` writes it
  with `replaceState` (not `pushState`: Back should not walk one member at a
  time), and it writes **above** the empty-pane return, because which member
  is focused has nothing to do with whether they have rows this month.
- **A month change keeps `?pane=`.** `[`, `]`, `T`, the arrows and the
  picker all go through one `gotoPeriod`, which builds the URL from
  `location` — a server-rendered `?period=` href cannot carry a pane that
  moved client-side, and dropped you back on your own. It asks first when
  drafts are parked, since the page load would drop them.
- **With no `?pane=`, you land on your OWN pane, never pane one.** The first
  thing a member sees must be their own ledger; everyone but the first member
  was being shown somebody else's. The server resolves it —
  `?pane=` → the session member → 0 — and an id naming nobody falls through
  the same chain rather than erroring, because a stale bookmark is not worth a
  failure page.
- **A revoked session leaves its open pages at once.** "Sign out other
  devices", an admin's "Sign out everywhere", a reset, turning a member off
  and `:logout` all call `Live::sessions_revoked`; every `/live` stream then
  checks its **own** session, and only the page whose session is gone gets a
  `signedout` event and goes to `/login?out=1`, which says why. The ping names
  nobody — a session id is the cookie, so it never travels. A revoke from
  another process (the console's `reset-link`) cannot ping, so each stream
  also re-checks every 30 seconds. The existing Server-Sent Events channel
  was enough: this signal only runs server to page, so no WebSocket.
- **A long Save names its steps over the same stream.** A `progress` event
  — trimming, shrinking, unlocking, reading, asking the assistant, filing —
  tagged with a job id the page sent as `X-FFM-Job` (`web::fidget`), forwarded
  only to the acting member's streams, and drawn as a fidget-style box above
  Save. Words only, never bill content. See `design/spec.md`'s The loader.
- **Sessions are server-side, not a bare signed cookie.** A `sessions` table
  (`id`, `member_id`, `created_at`, `last_seen_at`, `label`) backs an opaque
  `HttpOnly` session cookie, so a single lost/stolen device can be logged out
  without rotating a secret that would log out the whole household. Sized for a
  LAN/Tailscale-only deployment: long-lived sliding sessions (`last_seen_at`
  moves at most once a minute, so a request is not a write), no lockout or rate
  limiting, `SameSite=Lax` plus checking HTMX's own `HX-Request` header is enough
  CSRF defense at this scale. Revisit all of this if this is ever exposed to the
  open internet.
- **The `HX-Request` CSRF check is for *authenticated* routes only.** `/login`,
  `/setup`, `/invite/:token`, `/lang` and `/theme` are plain form POSTs and never
  carry that header, so a blanket middleware would reject all five and make the
  app impossible to enter. They rely on `SameSite=Lax` alone — sound here, because a
  forged login POST gains an attacker nothing, `/lang` has no security value, and
  setup and invite-accept are single-use by construction. **This is written down
  because the obvious "fix" is to apply the check everywhere.**
- **The check accepts `Sec-Fetch-Site: same-origin` as well as `HX-Request` —
  an amendment, made while building it.** Requiring the HTMX header alone would
  mean a control has to be HTMX *before* it can be safe, which is backwards: a
  plain `<form>` is the more robust thing, and `/logout` and `/preview` are
  plain forms. Browsers attach `Sec-Fetch-Site` to form posts and a page on
  another origin cannot forge it, so it proves the same fact. A request with
  neither header is allowed through, because refusing would break a client with
  no way to comply and `SameSite=Lax` is still standing behind it.
- **No self-registration, and no `/signup` route.** Household membership is
  closed. An admin (role 0) issues a single-use invite link; the person accepting
  it sets their **own** name and password. An admin never types somebody else's
  password, and there is no initial password to hand over. First run is `/setup`,
  which exists only while `members` is empty.
- **A member can be turned off, or — with no rows — trashed; never erased.**
  Turned off: cannot sign in, and their pane and its money are hidden **from
  the month they were turned off** onward, while earlier months still show
  them. It is a stretch in `member_inactive`, closed (not deleted) when they are
  turned back on, so the months they were away stay hidden and the
  materializer never backfills them. The trash (`members.deleted_at`) takes
  only a member with no transactions and no rules — it is for a mistake — and
  keeps their name taken so Restore is safe. Every account action refuses
  yourself, the last admin who can sign in, and a member who is not there;
  see `design/spec.md`'s Turning a member off.
- **A pending link keeps its raw token, and only while pending** — the
  reversal of "the token is never stored", so `:members` shows every pending
  link's full URL (`location.origin` + path) with Copy, instead of showing a
  new one once. Wiped on accept, revoke or expiry; links last 5 days and an
  expired one is revoked on its own.
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
- **The sole admin's own account is reachable from the console, not by hand-
  editing the database — an amendment to "recovery is editing the database by
  hand".** `ffm reset-link <name>` issues the *same* link `:members` issues.
  It deliberately does not set a password: there stays exactly one code path
  that writes `password_hash` (the accept form), the member still picks their
  own, and the reset keeps everything the web path pays — every session
  revoked, two entries in the append-only log. A `--set-password` flag was
  rejected on both counts, plus the obvious one: a password on `argv` lands in
  shell history and in `ps`. The console has no member behind it, so
  `member_invites.created_by` is nullable (NULL = the console) and the log
  stamps `source = 'system'` with a note rather than naming an admin who
  wasn't there — this is the one place the app grants the ability to act as
  somebody else, so it must not be able to do it anonymously. Issuing an
  **invite** from the console is deliberately not offered: deciding who joins
  the household is the admin's call, and a `CHECK` says so.
- **Mutations follow one optimistic flow.** Submit → insert an animated skeleton
  row immediately → the server's fragment replaces it with either the real row or
  an error row carrying a **Try Again** that resubmits the original action.
  - **The skeleton is real markup already in the DOM**, revealed by HTMX's own
    `htmx-request` class via `hx-indicator`, with the new row swapped in
    `beforebegin` of it. No JavaScript builds a placeholder, and its geometry
    therefore cannot drift from a real row's.
  - **A mutation returns the totals it moved**, out of band, alongside the row.
    A row that lands while the pane's balance stays put is the ledger
    disagreeing with itself on screen. They are computed by the same
    `ledger::pane` call a page load uses, so privacy filtering and the numbers
    cannot come apart.
  - **The message travels in a response header** (`HX-Trigger`'s `ffm:say`),
    not in the body — a swap should not have to include chrome in order to
    say something. The page shows it as a **toast** and keeps it in the
    **bell** (`say()`, the one place a message is shown); the statusline's
    message cell reads back the last command typed instead. See
    `design/spec.md`'s Toasts and the bell.
  - **An error row is HTTP 200.** HTMX swaps a successful response by default,
    and the error row *is* the answer; signaling failure in the status code
    means the person sees nothing at all.
- **Every soft delete, restore, reorder and re-attribution is undoable.** `u`
  must restore the exact prior state, including both ledgers for a
  re-attribution.

---

## Development workflow

- **`mise install` sets up the toolchain** from `mise.toml`: the standalone
  `tailwindcss` binary, `just`, `cargo-watch`, `sqlx-cli`, `python`, and `zig`
  with `cargo-zigbuild` for the Linux releases. There is no Node — it was only
  ever there to run Tailwind. Rust is pinned by `rust-toolchain.toml` (rustup),
  including `rust-analyzer` and the two musl targets, not by mise. Left to the
  system: `sqlite3`, `poppler` (`pdftoppm`/`pdftotext`, `brew install poppler`),
  `qpdf` (password-protected PDFs and trimming pages, `brew install qpdf`),
  Ghostscript (optional, shrinking PDFs, `brew install ghostscript`) and
  Safari's `safaridriver`.
- **A dev server says it is one, on every screen** (`crate::run`). The
  modes look identical otherwise, and a dev server on a copy of the database —
  or one three weeks in the future — is exactly where a real bill gets typed
  into the wrong ledger.
  - **PROD** is a release build; **dev** a debug build on loopback;
    **dev-lan** one bound to an interface the house can reach — decided by
    what the listener actually bound, not the flag that asked. A moved clock
    (`just dev 2026-10-15`) adds its date.
  - A non-PROD build wears a hazard-striped `DEV` / `DEV-LAN` /
    `DEV · 15/10/2026` badge beside the household's name — row 1 at every
    width, never moved into the profile menu the way the admin toggle was —
    on `/login` too, and prefixes the tab title (`[dev 15/10] …`). A phone
    drops the badge's year.
  - **PROD wears nothing**, and says so only in `:profile`'s first line and
    the boot log (`ffm v0.0.1 · PROD`): a badge that is always there stops
    being read, and the danger runs one way. The **version number** is not that
    label: every build shows it at the statusline's far right (`v0.0.3`,
    `-dev` on a dev build), because "which version is this?" is a question
    about any server, while the DEV badge still marks only the dev ones.
  - The badge speaks in ink, not a semantic color: the accent, pos/neg and
    info all already mean something.
- **`just release` builds a release; `just release-approve` tags it.**
  - `just release` writes `dist/v0.0.N/`: one folder and `.tar.gz` per
    platform — macOS arm64, Linux x86_64, Linux arm64 — each holding the
    binary, `static/` (only what a page loads),
    `ffm.example.toml`, and `start.sh` + `README.md` from `release/`. Plus
    `SHA256SUMS` and the `COMMIT` it was built from.
  - **The Linux builds are static musl, cross-compiled from the Mac** with
    cargo-zigbuild — zig is the C compiler for the bundled SQLite. That was
    chosen over a container building against an old LTS glibc: a static
    binary has no glibc to match at all, and nothing needs a Linux VM here.
    The runtime dependencies left are poppler and qpdf, run as programs, not
    linked — and qpdf only for a locked PDF, so a server without it still
    takes every other file (`start.sh` warns).
  - **The number is the last `v0.0.*` tag + 1**, written into Cargo.toml
    before the build so the binary reports it (`crate::version()` — the boot
    log and `:profile`'s first line; `-dev` on a `cargo run` build). Running
    `just release` again before approving **reuses** the number, so tags
    stay gapless.
  - **Both refuse a dirty tree** (the version bump aside), and
    `release-approve` refuses when HEAD has moved since the build: the tag
    must name exactly the code the tested binary was built from. It commits
    `release: v0.0.N` and tags; nothing is pushed.
  - **`just release` first runs `just tidy`**: when `target/` is over 30 GB
    (`FFM_TARGET_MAX_GB`) it runs `cargo clean -p ffm`, which keeps the
    dependencies. Cargo never deletes a superseded build of ffm, and every
    version bump supersedes them all: `target/` reached 189 GB, and its
    incremental cache broke linking under `just dev-lan` until cleaned.
- **Git hooks, versioned in `.githooks/`, on after `just hooks`** (once per
  clone — git config is not versioned). `just check` is two halves now:
  - **pre-commit runs `just check-fast`** — formatting, the offline build
    (a query changed without `just prepare`) and the greps. Seconds, so it is
    not worth skipping.
  - **pre-push runs `just check-slow`** — clippy with `-D warnings`, and
    `cargo test`.
  - `--no-verify` skips either. They check the working tree, not only what is
    staged: a partially staged file is checked as it stands on disk, on
    purpose, rather than juggling stashes. They were added after 16 files had
    drifted from `cargo fmt` unnoticed.
- `just dev` — Tailwind build, then `cargo watch` recompiling and restarting on
  save. `just css` in a second pane for CSS-only iteration.
- **`just dev` and `just dev-lan` first stop an earlier dev loop on their
  port** (`_free-port`), printing each step. A forgotten server in another
  pane once answered every request while the new one died on bind, so a
  `just dev 2026-10-15` looked like it worked and showed the real today.
  - **The port alone is not enough to look at.** An old cargo watch whose
    server had already exited held nothing, then restarted that server the
    instant the new loop's startup wrote a file, and won the port a second
    before the new server bound. So every cargo watch running `-x run` in
    this checkout with the same `FFM_ADDR` port is stopped too — **paused
    (SIGSTOP) first**, so it cannot restart anything mid-cleanup.
  - The server gets SIGTERM, which it answers by checkpointing the WAL, and
    SIGKILL only if it still holds the port after 8 seconds.
  - Anything on the port that is not an ffm binary is refused, never killed;
    a watcher on another port, or a `cargo watch -x test`, is left alone.
- `just dev 2026-10-15` (and `just dev-lan 2026-10-15`) — the same loop as of
  that date, on a **fresh copy** of `ffm.db` and the vault in `./demo`
  (gitignored), made once per invocation. The demo materializes that month's
  rows and stamps its edits with that date, which must never reach the real
  database — so it never runs on it.
- **The browser reloads once the server restarts** — `web::dev_reload`, behind
  the opt-in `dev-reload` feature (which is what `just dev` turns on). The
  page's `EventSource` on `/_dev/reload` receives the process's boot id, and
  reloads only when a reconnect brings a **different** one; keep-alives every
  15s hold the stream open through a proxy. Not `#[cfg(debug_assertions)]`,
  and not a default feature: see the gotcha below for why both are wrong.
  It replaced `tower-livereload` — see the gotcha on reloading on reconnect.
- **The same feature sends `Cache-Control: no-cache` on `/static`**, so a
  rebuilt `build.css` lands on a plain reload. Without it Safari reused a
  stale stylesheet from its heuristic cache and a CSS fix looked like it had
  not landed, while the template beside it (not cached) had.
- **Elsewhere a page's stylesheet and scripts are kept for a year** —
  amended from "a production build leaves caching alone". A page names them
  through `web::assets::url` (`/static/css/build.css?v=<hash of the file>`),
  and only a URL whose hash is the file's *current* one is sent
  `immutable`, so a deploy is a new URL for exactly the files it changed.
  Fonts and icons carry no hash and keep the browser's own caching. The
  response is gzip or brotli unless it is a PDF, a font, an image or an
  event stream (`compression()` in `src/web/mod.rs`).
- **The keymap is `static/js/ledger.js`, and nothing in it is templated.**
  It was ~200 KB inline in `pages/ledger.html`, sent again on every month
  change. What the server decides per page — the pane, the member's
  settings, the catalog's words — is the page's `window.FFM` block, read as
  `FFM.<name>`; **a new value goes there**, never as `{{ … }}` in the file,
  where it would reach the browser as braces (`just tpl-check` refuses it).
  Editing the keymap now needs a reload, not a `cargo build` — the `FFM`
  block still does.
- `just db` creates the database and applies every migration with the plain
  `sqlite3` CLI — which exists to break a bootstrap deadlock: `sqlx::query!`
  checks queries against a real schema at **compile** time, but
  `sqlx::migrate!()` only applies migrations at **startup**. `just seed` puts a
  household in it. Neither needs `cargo-sqlx` installed.
- **Dev-only UI sandbox router**, also behind `#[cfg(debug_assertions)]`: renders
  every template — including success, error and skeleton states — against dummy
  data, so UI and CSS can be iterated without touching the real database. Keep it
  in sync with the row-state table above; it is how that table stays honest.
- **Test UI in Safari AND Chrome before calling it done.** The household uses
  Safari; Chrome is only where the automation is convenient. A fix checked in
  one browser is a guess about the other — the `P` carousel's zoom passed in
  Chrome three times while doing nothing (or the wrong thing) in Safari. Say
  which browsers were actually checked.
  - **Chrome**: the Claude-in-Chrome tools. **Its key presses may not reach
    the page at all** — the tab runs in the background, and a `keydown`
    listener saw nothing — so dispatch keys from inside the page (fine for
    unshifted keys; see the Shift gotcha below) and let Safari's native keys
    carry the real-keyboard check. Its screenshot coordinates are
    scaled against the real viewport — convert from `getBoundingClientRect`
    before clicking, or a click meant for a button lands on the backdrop.
    Its tab reports `visibilityState = hidden`, so a plugin (the PDF viewer)
    can take seconds to paint: measure sizes in JS rather than trusting an
    early screenshot.
  - **Safari**: WebDriver through `safaridriver` (needs Safari → Settings →
    Developer → "Allow remote automation", already enabled on this Mac).
    `safaridriver -p 4444`, `POST /session`, then `/url`, `/cookie` with a
    trial `ffm.sid`, `/execute/async`, element `/click`, key `/actions`, and
    `/screenshot`. Native clicks and key presses — not synthetic events.
  - **Never against the live database.** Copy `ffm.db` and `receipts/` to a
    scratch dir and run a second process on its own port with `FFM_DB_PATH`
    / `FFM_RECEIPTS_ROOT` / `FFM_ADDR`, signing in with a session row
    inserted into the copy. Stop it by its port (`lsof -tiTCP:<port>`), never
    `pkill -f target/debug/ffm` — that also matches the `just dev` server.

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
  **Language & copy (i18n)** section. **What stays English is so on purpose**
  — identifiers a member types back, the mode badge, product and browser
  names; stored English (log notes, device labels) is worded at display; and
  `just i18n-check` plus a both-languages test on `?` keep the rest out. The
  date picker is the browser's own, in the browser's order; beside it the
  server echoes the date with its month named (*1 mai 2026*).
- **Sample/seed data stays Brazilian-household Portuguese on purpose** — bill
  names like "Água · Sabesp", bucket labels like "Mesada". That's content a
  household typed into its own ledger, not app copy: it never passes through the
  catalog, it stays Portuguese when the UI language is English, and it must not
  appear in any `.ftl` file. Anything the *app itself* says is a catalog message.
- **Copy is written from the user's side of the screen.** Name things by what
  people recognize, active voice, a control says exactly what happens. Errors
  explain what went wrong and how to fix it.
- **US spelling, always** — in UI copy, identifiers, comments and docs alike:
  `color`, `center`, `behavior`, `neighbor`, `gray`, `canceled`, `labeled`,
  `installment`, `-ize`/`-yze`. The one exception is an already-applied
  migration file: SQLx checksums it, so even a comment edit breaks startup
  against an existing database. (`aria-labelledby` is the HTML spelling, not
  a British one — leave it.)
- **Never expose internal ids in UI copy.** The old `↩ #1042` exception went
  away with the superseding restore row — the change log speaks in times,
  actors and verbs, not row ids.

---

## Gotchas already paid for

These cost real debugging time in the prototype. Do not re-introduce them.

- **A modifier pressed on its own fires its own keydown, and a keymap that
  clears state on every key will eat the keystroke before it.** Holding Shift
  to type `T` sends `Shift` first; the dispatcher cleared the pending prefix
  and the pending count on every key it saw, so `g` + Shift-`T` lost the `g`
  and `T` arrived bare. **`gT` never worked on a real keyboard**, nor `zR`,
  `zM`, or any count before a shifted key — `2J` moved one row, not two. The
  fix is one line: return early for `Shift`/`Control`/`Alt`/`Meta`/
  `CapsLock`/`AltGraph`.
- **Which is why a synthetic `KeyboardEvent` is not a test of a keymap.** It
  sends only the final key; a real keyboard sends the modifier first. Every
  shifted binding in this app passed automated checks for months while being
  broken in the browser. When a binding involves Shift, dispatch the modifier
  keydown too — `press('g'); press('Shift', true); press('T', true)` — or
  press it by hand.
- **A prefixed binding must be resolved before any unprefixed one, and not
  by line order.** This was written wrong four times in one keydown chain:
  `gg` lost to the arm that arms `g`, `gu1` and `zz` lost the same way, and
  **`za` lost to `a` and opened the add form instead of toggling a fold** —
  a documented key that never once worked. Four instances of one bug is a
  shape problem, not four mistakes. Every prefixed pair now lives in a single
  `prefixed(buffer, key, count)` function consulted *before* the chain, and an
  armed prefix consumes its second key even when the pair means nothing, so
  `zq` drops the prefix rather than falling through to `q`.
- **A keymap's count branch will eat a prefix's digit, twice.** `g1`/`g2`/`g3`
  and `gu1`/`gu2`/`gu3` are bindings whose last key is a *digit*, and the
  count that makes `5j` work matches digits first. Guard it on the buffer
  being **empty**, never on a named prefix: the first fix tested
  `buffer !== 'g'`, which was right until `gu` existed and then silently
  broke `gu1` the same way. The sibling bug is the prefix key itself
  consuming a pending count — `2gt` must still mean tab 2 — so arming a
  prefix reads no count at all.
- **`htmx:beforeSwap` fires on the swap TARGET, not on the element that
  triggered the request.** A listener on a form inside the target never hears
  it. This cost a silent bug: the "keep the draft when a save is refused"
  handler sat on the form, never ran, and the statusline said *"your edit is
  still here"* about a draft that had just been thrown away. Anything that
  has to read a form's own values before it is swapped away belongs on a
  container outside the target — which, for drafts, is the same shell they
  already live on.
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
- **A check digit is a one-in-ten filter, not proof.** The first candidate
  whose DV passed became the bill, candidates were tried in *sorted* order,
  and Vivo's bill draws its Pix QR code as thirty rows of `0`/`1` characters
  in its text layer: one 44-digit window of one row passed mod 11, sorted
  ahead of the real `8466…` linha, and the row read R$ 10.111.000,00 due
  30/05/2022 — and bound the rule as a bank slip, so the right bill would
  then have been refused as the wrong kind. Check the digits the standard
  fixes (a cobrança's currency `9`, an arrecadação's product `8`) before the
  DV, try candidates in the order the bill prints them, and skip runs of only
  0s and 1s. See `design/spec-bill-import.md` §5.
- **Two path builders that agree today are a vault the app cannot see
  tomorrow.** v0.0.5 computed a row's file path in five places: uploads from
  the rule's bucket, the ledger's mark from the row's own. They agreed until a
  rule's bucket was renamed — then every upload landed where the rule said,
  the ledger looked where the row said, and the row showed no bill however
  many times it was uploaded (Damha III, in production). A path is decided in
  `receipts::slot_of` and nowhere else; a new reader takes a `Slot` from it,
  never a bucket from whatever row it happens to have.
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
- **`just db` must write `_sqlx_migrations` itself, or startup re-runs the
  migration.** Applying the schema with the `sqlite3` CLI leaves SQLx's own
  bookkeeping table empty, so the migrator at startup cheerfully tries to
  `CREATE TABLE` everything again and dies. The recipe inserts the row with the
  same **SHA-384 over the migration file** that SQLx computes, so the two agree.
  Change a migration and the checksums disagree on purpose — that is SQLx saying
  `just db-reset`, which is safe only while no household database exists.
- **An `HX-Trigger` header carrying UTF-8 arrives mangled.** HTTP header values
  are bytes and browsers read them as latin-1, so a statusline message built
  from a household's own bill name comes back as `added "Padaria Â· Manhã"`.
  The fix is not to strip the character: JSON already has `\uXXXX`, so escape
  every non-ASCII char when building the header and `JSON.parse` puts it back.
  Any header this app ever builds from household content needs the same.
- **Askama compiles templates into the binary.** Editing a `.html` and
  reloading proves nothing until `cargo build` has run — `just dev` watches for
  it, but a hand-started `cargo run` does not. Half an hour went into "the
  keymap stopped working" that was an old template still in a running process.
- **A browser with forced dark mode rewrites colors, and it is not a bug in
  the page.** A plain `<div style="background:#F6F3ED">` computed to
  `rgb(24,26,27)` in one Chrome profile, which made the palette swatches and
  the month picker's warning dots look broken. Verify with a clean headless
  render (`--headless --screenshot` on a minimal HTML file) before changing a
  color — this has now cost time twice.
- **iOS shows its keyboard only for a `focus()` made synchronously inside a
  tap, and keeps it only when focus moves to another text field in the same
  gesture.** A `focus()` after `$nextTick` or a timeout is silently a focus
  with no keyboard. So the phone's key trap is focused in the `NORMAL` tap
  itself, `:` and `/` focus their field at once while it is up (`focusBar`,
  which first undoes `x-show`'s `display: none`), and the key strip's buttons
  act on `pointerdown.prevent` so a tap never takes focus from the field
  holding the keyboard. See `design/spec.md`'s *On a phone*.
- **`x-show` has not been applied yet when `$nextTick` runs.** Alpine queues
  the DOM mutation behind its own scheduler, so for a frame after a fold key
  the `.foldrows` are still laid out in their *previous* state — and in a
  background tab, longer. Anything that reads a position **out of the layout**
  in that gap gets the old answer: `zc` on the last row of a three-row fold
  left the cursor index pointing four folds further down, so the next `j`
  landed somewhere unrelated. `$nextTick`, `queueMicrotask` and `setTimeout(…,
  0)` were all measured and all still saw the stale DOM. The fix is not a
  longer wait — it is to stop asking the DOM: fold visibility comes from
  `this.folds`, which is correct synchronously, and `offsetParent` decides
  only what the model does not know about. **A cursor derived from layout is
  a cursor derived from a race.**

- **A cursor that is an index, over a list the server re-renders, is a bug
  waiting for a second keypress.** `J` moved the row and left the cursor on
  the index, which after the swap named the row that had taken the vacated
  slot — so the next `J` moved that one back and two rows traded places
  forever. The row under the cursor is identified by **id**, never by
  position, across anything that re-renders: the element the cursor pointed
  at does not survive the swap. The same shape is why `rows()` reads fold
  state from the model rather than from layout — both are the cursor being
  asked to mean something after the ground moved under it.
  **And a count masks it**: `5J` is one request and one swap, so it looked
  right in every test until somebody pressed `J` five times instead.

- **Alpine merges a component's data and its methods into one object**, so a
  method named after a data property silently replaces it. `pane: 0` (the
  focused pane's index) and a later `pane(url)` helper meant `panes[this.pane]`
  started indexing an array with a *function*: `j`, `k`, `dd`, `gl` and every
  other row-based keystroke stopped working at once, with nothing in the
  console. `just js-check` now fails on a name declared twice in a component.
- **Askama escapes the `json` filter's output too, which kills an inline
  `<script>`.** `{{ x|json }}` inside a script tag renders `&#34;` for every
  quote, the whole block fails to parse, and *everything* in it stops working —
  in this case the entire keymap, silently, with nothing in the console about
  the template. The pairing is `{{ x|json|safe }}`: the `json` filter already
  escapes the characters that could end the script tag, so `safe` suppresses
  the *wrong* escaping rather than all of it. `just tpl-check` enforces it.
- **A quote-blind TOML comment stripper eats hex colors.** `line.replace(/#.*$/, '')`
  cuts `accent = "#c4a7e7"` in half at the color's own `#`. The real `toml` crate
  handles this — which is a reason not to hand-roll a config reader.
- **`@click.outside` fires for the click that opens a file picker.** The
  `+` menu was `x-show="attaching" @click.outside="attaching = null"`, and
  `attaching` was also the row the picker was for — so *Add bill*'s
  `$refs.picker.click()`, a click on an input outside the menu, emptied it
  before the dialog opened, and the file picked afterwards was dropped with no
  request and no message. Every pick from the `+` menu, on every row, for as
  long as it existed; a drop on the row calls `upload()` directly, which is
  why nothing looked broken. One flag must not mean both "a popup is open"
  and "what the next event is for": the picker's row is `pickFor` now.
- **An SSE event with empty `data` is never dispatched.** The first
  `signedout` was `data("")`: curl printed it, the browser dropped it, and
  the page read the stream closing as a lost connection and reconnected —
  signed out, and still showing the ledger. Every event carries a payload.
- **A message sent just before blocking work waits for that work.** The box
  above Save named *Reading the bill* only once the bill had been read: the
  handler sent the step, then ran `pdftotext` (and `qpdf`) synchronously on its
  tokio worker. Sending wakes the `/live` stream's task, tokio parks a task
  woken that way in the current worker's own LIFO slot, and no other worker
  can steal from it — so the event left only when the handler next awaited.
  A child process belongs in `spawn_blocking` (`import::read_bill_off`,
  `unlock_for`) anyway; this is how it shows when it is not.
- **Two writers at startup and one of them fails — and the materializer
  waits an hour to try again.** Moving the vault's upkeep after the bind put
  its re-filing beside the materializer's first pass, and that pass died with
  `database is locked` (517, `SQLITE_BUSY_SNAPSHOT`): a transaction that has
  read cannot wait out a writer that committed under it, so `busy_timeout`
  does not help. A restart across a month boundary would have shown the new
  month without its recurring rows. `materializer::spawn` now says when its
  first pass is over, and the upkeep and the catch-up wait for it. **Any new
  startup work that writes goes in that same task, after them.**
- **Watch class-name collisions.** An `app.cfg` state class silently matched a
  leftover `.cfg { display: grid; align-items: start }` rule and broke the whole
  shell width. Namespace state classes (`is-`/`has-` prefix) and delete dead CSS.
- **A fixed-width key column overflows onto its neighbor.** Size it
  `minmax(64px, max-content)` so long commands fit their own box.
- **Hardcoded `#fff` on semantic fills** breaks on the fifth theme, not the first.
  Use `var(--paper)`.
- **Fluent wraps every placeable in invisible BiDi isolation marks
  (U+2068/U+2069) by default.** In a mono statusline those render as phantom
  width, breaking the `.st.grow` ellipsis measurement and any string comparison
  in a test. Call `set_use_isolating(false)` on the bundle.
- **A plural only Portuguese needs must still go through Fluent.** `build.rs`
  chose the plural path by looking at the English message alone, and rendered
  a pt-BR selector's variants as nothing: the selection cell read *"3 de 5 "*
  for as long as it existed, and only the English tests ran. It now takes the
  plural path when any locale has a selector; a test in `ui::i18n` renders the
  Portuguese.
- **A pre-auth template has no member row.** Every layout context must carry
  `me: Option<&Member>`, and the `.auth` layout must not read `layout_*` or
  `theme_name` — otherwise the login page either won't compile, or worse,
  compiles against defaults that silently diverge from a real member's.
- **`[target.'cfg(debug_assertions)'.dependencies]` does not gate anything.**
  Cargo accepts the section, emits a warning, and links the crate in *every*
  profile — so a dev-only dependency written that way looks gated and ships.
  Use an optional dependency behind a feature. Keep that feature **opt-in**
  rather than default-on, so a forgotten flag fails as "livereload didn't
  reload" (noticed in seconds) instead of "livereload shipped in release"
  (never noticed).
- **A dev reload that fires on any reconnect reloads every time a proxy cuts
  an idle stream.** `tower-livereload` (0.10) sent one `init` event and then
  not a byte, and its script reloaded on the first `init` after any error.
  Behind the HTTPS proxy that fronts `just dev-lan`, an idle stream is cut at
  60s (`ERR_HTTP2_PROTOCOL_ERROR` at ~65s in the HAR), so the page reloaded
  about once a minute on a server that never restarted — taking parked drafts
  with it. Loopback `just dev` has no proxy, which is why it only ever showed
  on `dev-lan`. **Symptom to recognize:** the reload stream fails, a new one
  starts, a document load follows, and the terminal shows no rebuild. The fix
  is two things, both needed: reload only on a **changed boot id**, and send
  keep-alives. (Before 0.10 the same library reload-looped against hyper 1.x
  — a long-poll that completed at once read as a restart. One more reason it
  is gone.)
