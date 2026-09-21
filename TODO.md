# FFM — implementation phases

The working plan, kept in the repo so it survives a session. `CLAUDE.md` is the
*working agreement* (what has been decided and why); this file is the *order of
work* (what is built, in which sequence, and what each phase must not skip).

★ marks a **cross-cutting** phase: retrofitting it means touching every handler
written before it. Those are deliberately early even where a later phase looks
more exciting.

---

## Phase 1 — foundation + the row component ✅

Done, in five commits: `f7f037c` (schema completion + project foundation + the
typed Fluent catalog), `b3a4b0e` (domain types, `ui::fmt`, the palette table,
`LedgerRow`), `996cc63` (the row template, the CSS port, the dev-only sandbox),
`571daa3` (self-hosted Archivo + JetBrains Mono), `202d799` (six deferred
decisions recorded, Tailwind wired, lib/bin split).

State at the end of it: `just check` green — 68 tests, zero clippy warnings, the
schema applies inside a transaction, no hardcoded colour below `tokens.css`. No
database, no auth: every row state renders from a pure fixture.

---

## Phase 2 — config, the pool, and a read-only ledger ✅

The shortest path from *the row looks right* to *the row is true*. Done:

- **`src/config.rs`** — `ffm.toml`'s three keys, each also readable from
  `FFM_DB_PATH` / `FFM_RECEIPTS_ROOT` / `FFM_BACKUP_DIR`, **environment wins**.
  Read once at startup, never watched. A failure names the line to edit
  (`✗ line 4 · storage.receipts_root does not exist — /disks/vault/ffm`) and the
  process exits rather than starting half-configured. The vault must already
  exist and be writable — it is deliberately *not* created, because an empty
  vault conjured at a mistyped path is how a household finds out months later
  that its receipts went somewhere else. `backup_dir` *is* created: it is ours.
- **Boot log** — `ffm · timezone America/Sao_Paulo · today 21/09/2026`, with the
  zone name read from `TZ` or `/etc/localtime`.
- **`src/db/mod.rs`** — the pool, with `foreign_keys(true)` and
  `journal_mode(Wal)` in the connect options (neither can live in a migration),
  plus `synchronous=NORMAL` and a 5s busy timeout.
- **`just db` / `db-reset` / `seed`** — `db` is idempotent and writes
  `_sqlx_migrations` itself, so the startup migrator agrees.
- **`src/db/ledger.rs`** — the pane query, carrying the three facts a single row
  cannot know about itself.
- **`seeds/household.sql`** — Casa Stuchi, three members, two months.
- **`tests/ledger.rs`** — the query against a real file and the real seed.

**Offline query data is committed.** `.sqlx/` holds every checked query's
result shape, so `SQLX_OFFLINE=true cargo check` builds with no database in
sight — which is what CI and a clean checkout do, and it is in `just check` as
`offline-check` so it cannot quietly rot. Change a query, run `just prepare`,
commit what it writes. Without this the fix under pressure is dropping to
unchecked `sqlx::query`, which loses the invariant the whole stack was chosen
for.

**Deliberately deferred inside the row, and marked in the code:** file presence
(phase 7 — so a tracked row honestly reads `pending`) and the income estimate
(phase 8 — so a varying income shows its baseline rather than a wrong guess).
Privacy filtering is phase 4 and the pane query does **not** filter; there is a
test saying so, because the filter has to land where the totals are computed
too, or a hidden row leaks by making the numbers not add up.

## Phase 3 ★ — the change-log write path ✅

**Built before the first mutation, on purpose.** The log is append-only, so an
entry that was not written at the time can never be honestly backfilled: the
timestamp would be a lie and the actor a guess.

`src/db/changes.rs`. Three things are shaped by the type system rather than left
to a comment, because all three are otherwise a `CHECK` that only fires at
runtime — by which point a handler has already been written wrong:

- **An entry and the row it describes commit together or not at all.**
  `Change::write` takes `&mut Transaction`, never the pool. There is no way to
  record something that did not happen.
- **Action and entity are not independent.** A separate action enum per entity
  (`TxAction` / `RuleAction` / `FileAction` / `MemberAction`), so
  `password_reset` on a ledger row cannot be spelled. `RuleAction::Delete` maps
  to the plain `delete` verb — there is deliberately no `rule_delete`.
- **A user action has an actor.** `Actor::User(i64)` carries the id rather than
  leaving `member_id` an `Option` a handler forgets. `Materializer` and `System`
  structurally have none.

`Value` carries the `kind` **with** the value (`money` | `date` | `day` | `bool`
| `text` | `path`) and stores it neutrally — minor units, ISO dates — so the
`gl` pane formats it without inferring from the field name, and a logged amount
never depends on the currency the household has configured *today*. `diff()`
records a field only when it actually moved.

`tests/changes.rs` — six tests through a real database, since most of what makes
this table trustworthy is enforced by SQLite: the append-only triggers really
abort, a rollback loses the entry *and* the row, an undo points at what it
reversed while the original stays, every action the API can spell is one the
schema accepts — and a hand-written bad pair is refused, so that last test is
not vacuous.

## Phase 4 ★ — auth, sessions, the pre-auth shell, `Viewer` ✅

**Privacy filtering was the reason this came before any mutation work**: it is a
`WHERE` clause on every read *and* a predicate inside every total, because a row
filtered out of the list but still counted in the balance announces its own
existence.

- **`domain::viewer::Viewer`** — member id, role, preview. In `domain` rather
  than `web` because the same answer has to be given by the query and by the
  sum. `is_admin()` folds preview in, so preview mode is one flag instead of a
  hundred call sites to remember; `sees_private_of()` grants visibility by
  ownership independent of role, which is what makes preview a *view* rather
  than an impersonation.
- **Privacy is applied once, in the SELECT**, and the totals are summed from the
  rows it returned. There is no second predicate to keep in step because there
  is no second predicate.
- **`db::auth`** — Argon2 passwords, SHA-256 invite tokens (deliberately *not*
  Argon2: nothing to brute-force in 256 bits, and a slow hash on every
  `GET /invite/:token` is a self-inflicted DoS), a real `sessions` table with
  sliding `last_seen_at`, and `sessions.preview` so one browser can preview
  while another does not.
- **`/setup`** — one sectioned form, one POST, one transaction creating
  `household_settings` + `members` + `member_settings` + `sessions` or nothing.
  The freshness check runs *inside* that transaction, so two browsers racing the
  form cannot both create an admin.
- **`/invite/:token`** — the members row is created on **accept**, which is why
  `password_hash` stays `NOT NULL`. Single-use; a used, expired or unknown token
  renders a page with no form at all.
- **Resets share the mechanism** (`member_invites.member_id` non-null) and ask
  for password + confirm only. Issuing one revokes every session that member has
  and writes `password_reset` + `sessions_revoked` to the change log — phase 3
  existing first is what made that possible.
- **`/login`**, one error message for both halves; **`/logout`**; **`/lang`** and
  **`/theme`** as plain form posts setting cookies and redirecting, so the first
  byte of the next page is already right with zero JS.
- **The `.auth` shell**, ported from the mockup: chrome strip last in the DOM and
  painted first with `order:-1` so Tab reaches Submit, `safe center` so a tall
  card stays reachable, `12vh` on a phone so the card is not under the keyboard,
  the 11-palette picker as a real `<fieldset>` of radios. It reads no `layout_*`
  value — there is no member row before you sign in. Shared components went to
  `static/css/ui.css`, which the app shell will reuse.
- **CSRF** — `SameSite=Lax` plus a same-origin check on authenticated
  state-changing routes only. See the amendment in `CLAUDE.md`: the check also
  accepts `Sec-Fetch-Site`, so a plain `<form>` can be safe without being HTMX
  first.

`tests/auth.rs` — 13 tests: setup is genuinely once-only, the seeded household
cannot be signed into, one device revokes without touching the others, an invite
creates nobody until it is accepted, a taken name is refused but the link
survives, a reset renames nobody and is logged, a second reset click replaces the
first, and — the two that matter most — a private row is absent from the list
*and* the totals, and an admin sees everyone until they preview.

## Phase 5 ★ — mutations and the optimistic-UI flow ✅

A **response-shape contract**, fixed once so every later phase inherits it
rather than inventing its own.

- **`domain::sortkey`** — fractional indexing, so a move writes *one* row
  instead of renumbering everything after it. The invariant it rests on is
  written down and asserted: a key never ends in the lowest character, because
  such a key has nothing below it inside its own prefix. Tested by inserting
  into the same gap a thousand times.
- **`db::tx`** — add, soft delete, restore, undo. Every one opens a transaction
  and writes the row *and* its change entry inside it. A spend appends; income
  goes to the top of its pane, which is a starting position rather than a pin.
- **`u` walks back through history** rather than toggling one change: a
  reversal points at what it reversed, and an already-reversed change is spent.
  Scoped to the asking member — `u` means "undo what *I* just did".
- **`web::tx`** — one fragment for every mutation: the row, the totals it moved
  (out of band), and the statusline message in an `HX-Trigger` header. An error
  row says what happened *and that nothing was written*, at HTTP 200 so HTMX
  actually swaps it.
- **The skeleton is real markup already in the DOM**, revealed by HTMX's own
  request class. No JavaScript builds a placeholder, so its geometry cannot
  drift from a real row's.
- **HTMX and Alpine are vendored** in `static/js/`, like the fonts: a household
  on a LAN should not need the open internet to open its own ledger.
- **The Alpine boundary is cut correctly on the first try**, which
  `CLAUDE.md` warns about: `x-data` sits on the shell, outside every
  `hx-target`, so a swap cannot destroy the state parked inside it.
- **A first slice of the keymap**, because a mutation you cannot reach is not
  finished: `j`/`k`/`G`, `H`/`L`, `a`/`i`, `dd` → confirm → `y`, `u`, `Esc`.
  Verified by driving a real browser, not only by unit test.

`tests/mutations.rs` — 7 tests: a spend appends while income goes to the top
with no key renumbered, an add records what it created, one `dd` plus one `u`
leaves exactly one row, `u` walks back rather than toggling, undo is scoped to
its member, a second `dd` on the same row writes no second entry, and a failed
insert leaves *nothing* in the log.

Two bugs the browser found that no test would have: a UTF-8 `HX-Trigger` header
read as latin-1 (`Padaria Â· Manhã`), and totals that sat still while a row
landed. Both are now part of the contract, and both are in `CLAUDE.md`.

## Phase 6 — recurring rules and the materializer ✅

- **`db::rules`** — create, edit, soft delete, and the per-member bucket cloud
  the combobox reads. A rule is created **from scratch**: there is no "turn
  this row into a recurring one", because a barcode binding has to happen at
  creation or not at all. `end_date` and `total_installments` are kept in sync
  by one function in Rust, so the form's JavaScript and the materializer cannot
  disagree about when a twelve-month loan from March ends (February).
- **`services::materializer`** — reconcile per rule, **no watermark**. For each
  live rule, the periods between its start and `min(current_month, end_date)`
  with no occurrence get one. Runs at startup *and* hourly, which is what
  catches a process that was down across a month boundary.
  - Soft-deleted occurrences count as **present**, or the reconciler resurrects
    every deleted row once an hour and fights the household forever.
  - Never past the current month: future months stay projections.
  - `UNIQUE(rule_id, period)` makes the whole job idempotent, so the startup
    run and a tick can race harmlessly.
  - The rule owns `due_day`; materializing stamps a real date from it, clamped
    to the month — a rule due on the 31st means the 28th in February. Editing
    `due_day` changes future periods only.
  - Carry-forward seeds from the **previous period's actual value**; off, the
    row starts with no amount at all, because for a credit-card statement last
    month's number would be actively wrong.
  - Every materialized row is logged with `source = materializer` and **no
    actor**: attributing it to whoever happened to be signed in would be a lie.
- **The rule pane** (`A` / `I` / `E`) — a column in the ledger's grid that
  pushes later panes right, with the arrow on its edge. Toggle switches rather
  than tick boxes, a native bucket combobox, the live computed path in two
  lines, and end-date ↔ installments syncing both ways. Saving reconciles
  immediately, so the periods a new rule is responsible for arrive in the same
  response.
- **The response shape extends cleanly**: a row mutation answers with one row,
  a rule save answers with the pane — same out-of-band totals, same statusline
  header, one level up.

`tests/materializer.rs` (11) and `tests/rules.rs` (4). The ones worth naming:
a rule created today materializes its whole history (the case a watermark gets
wrong), a deleted occurrence is not resurrected an hour later, a due day past
the end of a month is clamped rather than skipped, editing it leaves past
occurrences alone, and a finite run stops at its installment count.

Two things the browser caught: `{{ x|json }}` inside a `<script>` is
HTML-escaped by Askama and kills the whole block — taking the entire keymap
with it, silently — and the rule pane's month has to come from the ledger, not
from the rule's own start date. The first is now `just tpl-check` and a gotcha
in `CLAUDE.md`.

The seed-purity test also earned its keep: the new-rule placeholder's label is
**app copy**, so hardcoding it in a fixture smuggled a translated string into
one. It now renders from the catalog like every other word the app says.

## Phase 7 — receipts and files ✅

What finally makes `RowStatus::Paid` real rather than a fixture: the status mark
is now decided by the disk.

- **`domain::receipt_path`** — the *one* path builder. The row renders its rail
  from it and the store writes to it; two builders that agreed on Tuesday is
  exactly how a vault ends up with files the app cannot see.
- **The two-step `exists()` check, and both steps matter.** Read `receipts.ext`,
  build the one exact path, stat it — so a file deleted by hand reads as unpaid.
  With no row, probe the four allowed extensions — so a file *added* by hand is
  still found. Skip the second and hand-filing silently stops working, which
  removes the whole reason the filesystem is authoritative.
- **Upload / open / delete**, three distinct actions rather than one upload plus
  a question. The file is renamed to the deterministic convention; the user's
  own filename is kept as metadata. Uploading the receipt **supersedes** the
  scheduled file automatically, logged with `reason = superseded` so the trail
  is honest about why it moved.
- **Nothing is destroyed**: a delete moves the file into a sibling `.delete/`,
  and `u` puts it back — undo now reaches file entries, not only transaction
  ones, because the statusline promises it does.
- **`rule_barcode_window` closes the recorded KNOWN HOLE** (migration 0003). An
  occurrence resolves against the window that covered *its own* period, so
  renewing a cyclic bill's window no longer turns last January into "page −11".
  Re-entering the two fields **is** the renewal; the old window is retired, not
  overwritten.
- **The file rail is live**: fixed numbering by kind, only the files that exist,
  `g1`/`g2`/`g3` opening in a new tab and saying so when there is nothing there.

`tests/receipts.rs` — 12 tests, including a file deleted by hand reading as
unpaid, one added by hand being found, the supersede, and the window renewal
leaving last year's pages exactly as they were.

**Provisional, and flagged as such:** the *gesture* for attaching a file. The
spec never designed one ("there's no separate upload flow modelled for this
yet"), so the focused row grows a `+` control opening a three-item menu. The
server side is settled — three endpoints, one per kind — but the affordance is
a placeholder for a real design decision, listed below.

## Phase 8 ★ — income estimates and the `~` ✅

- **`domain::estimate`** — the projection, pure and unit-tested. `lowest` is
  the default on purpose: for income the dangerous error is over-estimating,
  which produces a shortfall the household then has to absorb. `average`
  truncates, which rounds *down*, the same instinct.
- **Only confirmed periods feed it**, where confirmed means exactly what it
  means everywhere else: a payslip on disk for a tracked rule, the manual tick
  for an untracked one. An unconfirmed month is skipped, never folded in —
  otherwise two in a row project a guess from another guess. The candidate list
  reaches past skipped months so a window of three finds three *real* ones.
- **The projection is never written down.** A varying rule does not carry an
  amount forward, so the stored value stays NULL and the number on screen is
  derived on read — which is what lets it follow new history instead of
  freezing an old guess, and why it never writes a spurious "amount changed"
  entry into the log.
- **The totals include it and say so.** The pane's INs and Balance and the
  household's IN and NET carry the `~` when any component is a projection.
  Leaving projections out would make a future month's balance useless; putting
  them in unmarked would present a guess as a fact. OUTs never carry it —
  only income varies.
- **A projection gets no delta**: comparing a guess against a fact is not a
  comparison worth drawing. Confirmed, it compares like any other recurring
  income, with the colour inverted so a raise never paints itself red.
- **The rule pane's estimate box mirrors the computed-path box** — the figure
  bright on the first line, the months it came from dim on the second, both
  recomputing as the basis changes, before anything is saved.

`tests/estimates.rs` — 8 tests, including the skipped unconfirmed month, each
basis, the first-period fallback, and the totals carrying the projection rather
than the stored baseline.

One thing the browser caught that the tests did not: the pane first computed
the average **in JavaScript** and then tried to reuse one of the history's
formatted strings for it, which only works when the answer happens to be one of
the inputs — so `average` rendered as `last`. Fixed by computing all three bases
server-side with the same `domain::estimate` and `ui::fmt::money` the ledger
uses. The pane now chooses which to display and does no arithmetic and no money
formatting at all, which is where the currency convention belongs.

## Phase 9 — the settings surfaces ✅

`:profile`, `:household` and `:members` as vertical splits beside the ledger,
reusing the rule pane's column — one visual language for "a pane beside the
ledger" rather than a second thing to learn.

- **No Save button anywhere**, which is the point rather than an omission.
  Every control writes on `change`, the statusline echoes, and there is no
  pending state — so there is never a moment when the form and the row
  disagree. That is what the single-source-of-truth rule actually forbids; a
  form in a vsplit was never the problem.
- **One form per setting, not one form per pane.** Radios are grouped by `name`
  *within a form*, so a single form made the entire pane one radio group and
  let exactly one control be checked at a time. A form per setting is also what
  the pane genuinely is: a list of independent writes.
- **`MemberKey` / `HouseholdKey` are closed enums**, not string column names.
  The alternative is a handler taking a field name from a form and putting it
  into SQL — `role` and `password_hash` are not settings, and now cannot be
  spelled as ones.
- **Validation lives with the write**, in the `✗ …` dialect the pane speaks,
  and a refusal leaves the previous value alone — which falls out of not
  writing it. Every constraint is also a `CHECK`, so a bad value cannot reach
  the table even if a handler forgets.
- **The privacy default is a real tri-state**: NULL means "follow the
  household", and the first radio says what that currently resolves to rather
  than leaving somebody to guess.
- **The command bar**, because a surface you cannot reach is not finished:
  `:profile`/`:me`, `:household`/`:config`, `:members`, `:invite`, `:logout`,
  `:preview`, `:theme`, `:lang`, `:sort`, `:group`, `:currency`, `:set
  rnu|nu|nonu`. A command and a form control write the same row — three
  gestures, one place. An unknown one says `not a command: x`.
- **Admin-only refuses under preview** exactly as it would for a real
  non-admin, and `:members` offers no reset against yourself: the first session
  a reset revokes would be the one issuing it. That is what `ffm reset-link`
  at the console is for.
- **`:members`** lists who is signed in on what, shows a new link **once**
  (the token is not in the database to re-show — only its SHA-256 is), and
  keeps the reset's accountability: every session revoked, two entries in the
  append-only log.

`tests/settings.rs` — 9 tests, including a refusal leaving the previous value,
the tri-state's third state being NULL, currency switching a whole number
format rather than a glyph, and a revoked link reading as used.

## Phase 10 — the change-log pane (`gl`) ✅

A pure read of what phase 3 has been writing since before the first mutation
existed — which is why there was anything to read.

- **The same 420px column the rule editor uses**, with the arrow pointing at
  the row, so nothing new has to be learned to read it. Read-only: no drafts,
  no save, nothing here writes.
- **Grouped by day, newest first**; time · actor · verb on the entry line, with
  a field count only where there is a diff to open — delete and restore have
  none, because it is the row's *existence* that changed.
- **`kind` travels with the value**, so the pane formats `old → new` the way
  the app does without inferring anything from the field's name: money renders
  from minor units in the currency it is read under, a `day` stays a day rather
  than becoming a date, and an em dash means "nothing was there".
- **An unknown action, field or kind still renders** — falling through to its
  raw text. The log is append-only and will outlive several versions of the
  function that reads it.
- **Rule entries sit inline in date order, tagged**, so the story reads
  chronologically and the two are never confused. The chips narrow to
  `this row` / `rule` / `all`, and `rule` is absent on a row that has none.
- **An undo points at what it reversed** — "deleted", then "restored · undo of
  18:39" — and the original is still there underneath it.
- **The materializer has no actor**, and the pane names what did it rather than
  inventing a person.
- **It closes when the cursor leaves the row**, in the same render-loop check
  that would park an open draft. A pane still showing the previous row's
  history is worse than no pane.
- **A row with nothing recorded still opens it**, saying so. An empty history
  is an answer, not an error worth refusing on.

`tests/log.rs` (6) and `ui::log`'s own unit tests (6).

**A regression this phase caught, from phase 9:** Alpine merges a component's
data and its methods into one object, so the `showPane(url)` helper — then
called `pane(url)` — silently replaced the `pane: 0` index. Every row-based
keystroke stopped working, with nothing in the console. `just js-check` now
fails on a name declared twice in a component, and the guard was verified by
reintroducing the bug.

## Phase 11 — sort, grouping/folds, `/`, `:grep`

Needs a populated ledger to be worth building. `:grep` is FTS5; `/` stays
client-side over the rendered month.

## Phase 12 — bill import

The largest isolated chunk, with zero dependencies upward, so it can slide.
`boleto.rs` is pure integer maths with the seven spec'd vectors plus the
423-mutation corruption test. See `design/spec-bill-import.md`.

## Phase 13 — the unresolved badge and ops

A query over everything above — it cannot be right until privacy, tracking and
materialization are. Plus the rolling two-month backup and a graceful shutdown
that checkpoints the WAL.

---

## Scheduled for revisit

Not phases — decisions already taken, with a date attached to the next look.

### htmx 2 → 4 · revisit between 21/01/2027 and 21/05/2027

**Decided 21/09/2026: stay on the vendored htmx 2.0.4. Re-evaluate in 4–8
months.** htmx 4.0.0 shipped 28/08/2026 and is deliberately not `latest` — npm
and the CDN keep serving 2.x until early 2027, and 2 stays supported and
maintained. There is no clock running on us: `static/js/htmx.min.js` is
self-hosted, so there is no drift, no dependency bot and no security deadline.

**Nothing in 4.0 buys this app anything.** The headline feature is `hx-live` —
declarative reactive bindings, a client-side reactive engine — which is exactly
what `CLAUDE.md`'s *hypermedia over the wire, no client-side data model*
invariant forbids. We would be upgrading for features we are not allowed to
use. The one genuine improvement, 4xx/5xx responses being swapped by default,
is already worked around and written down: an error row is HTTP 200 on purpose
(phase 5), and that workaround costs nothing.

**The argument on the other side, which is why this is dated rather than
dropped:** the htmx surface is ten attributes today and will be several hundred
once the rule pane, the change-log pane, the carousel and the settings vsplits
exist. Migrating is cheapest now and gets monotonically worse. The window above
is the trade — late enough that 4.x and its extensions have settled, early
enough that phases 8–13 have not multiplied the surface.

**What the migration would touch, as of 21/09/2026:**

| site | change |
|---|---|
| `templates/pages/ledger.html:98` | `hx-disabled-elt` → `hx-disable` (and `hx-disable` → `hx-ignore`, which we do not use — rename in that order or one migrates into the other) |
| `templates/pages/ledger.html:99` | `@htmx:after-request` → `@htmx:after:request`; every event listener renames, camelCase → colon-separated |
| `ledger.html:96-97`, `rule_pane.html:19,198` | inheritance is explicit in 4 (`hx-target:inherited`). Ours look self-contained — confirm, don't assume |
| `src/web/csrf.rs:33` | **verify `HX-Request` survives the XHR → `fetch()` move.** If it does not, the check degrades to `Sec-Fetch-Site` alone — still sound, but half the documented story, and the amendment in `CLAUDE.md` would need rewriting rather than silently becoming untrue |
| `totals.html:7,12`, `pane_rows.html:6`, `fragment.html:7` | **verify `hx-swap-oob` semantics are unchanged.** Out-of-band swaps are how every mutation returns the totals it moved; 4.0 adds `hx-partial` for multi-target, which may or may not supersede them |

The last two were not answerable from the release notes and are the load-bearing
ones — settle them first, before touching a single attribute.

**Also removed in 4**, none of which we use today, so keep it that way: `hx-ext`,
`hx-inherit`, `hx-disinherit`, `hx-history`, `hx-vars`, `hx-params`,
`hx-request`, `hx-prompt`. History navigation also changes from a DOM snapshot
to a server round-trip.

**Alpine.** htmx/Alpine compatibility is an extension in 4, not built in. This
app pairs the two tightly and already has one hard-won gotcha about a swap
destroying the Alpine state that holds parked drafts. That extension's maturity
is a gate on this migration, not a footnote to it.

**First step when the date comes:** `npx htmx.org@4.0.0 upgrade-check` walks the
templates and prints deprecated patterns with file and line numbers. Run it
before deciding anything — it is a minute, and it tells you whether the table
above is still complete.

---

## Still open, not scheduled

These are decisions, not tasks — they get scheduled when there is evidence.

1. **Concurrency.** Ana deletes a row while Rodrigo has it open in an edit form
   or parked as a draft. (The materialization half is handled:
   `UNIQUE(rule_id, period)` makes a double-insert impossible.)
2. **Seasonal / cyclic rules.** A `months`-of-year allow-list on top of
   start/end dates. Distinct from `total_installments`, which ends permanently.
3. **Split mode assumes exactly two panes.** Revisit when a household bigger
   than two is an actual case rather than a hypothetical.

4. **A form's error belongs next to the field it is about, not at the top of
   the card.** Today every pre-auth failure renders one `.warnbox` above the
   whole form: *"Use at least 8 characters."* sits at the top of `/setup` with
   nothing connecting it to Password, three sections further down and possibly
   off screen. The copy rule says an error explains what went wrong **and how to
   fix it**, and "which box do I retype" is half of that.

   What needs deciding, rather than just implementing:
   - **Per-field, card-level, or both?** A field-level message is the one that
     locates the problem; a card-level summary is what a screen reader announces
     on submit. Both is the accessible answer and two places to keep in step.
   - **Which field owns a cross-field rule.** "The two passwords do not match"
     is about a *pair* — Confirm is the likelier fix, but that is a guess.
   - **Focus.** `/login` already moves focus to the password on failure. Whether
     a rejected `/setup` should focus its first bad field is the same question
     asked once per form, and it is worth answering once.
   - **What it looks like.** The mockups only ever draw the card-level
     `.warnbox`; a per-field state (red rail on `input.i`, message under it)
     does not exist in the visual system yet and would be a new component.
   - **Where it is enforced.** Validation lives in the handler today. A field
     error means the handler has to say *which* field, so `Result<_, Vec<(field,
     message)>>` rather than `Result<_, String>` — worth settling before the
     mutation handlers in phase 5 copy the current shape everywhere.

   Raised from a real `/setup` submission (21/09/2026): password too short,
   warning at the top, both password boxes visually full because the browser's
   password manager had refilled them — so the message and the fields it
   described disagreed on screen.

5. **How a file actually gets attached is not designed.** Phase 7 built the
   whole server side — three endpoints, one per kind, with the supersede and
   the soft delete — but the spec explicitly leaves the gesture open, and the
   `+` control on the focused row is a placeholder rather than a decision.

   What needs settling:
   - **Where it lives.** A control on the row, a drop target on the row, the
     `gd` details popup, or the `P` carousel — which currently refuses on a row
     with no files, so it cannot be the only home.
   - **Drag and drop onto a *row*.** A panel drop was deliberately downgraded
     to a notifier because inferring which rule a file belongs to is fragile.
     A row drop infers nothing — the row is identified by where you dropped it
     — which is the same reasoning that makes the rule editor's dropzone fine.
   - **A keystroke, or none.** Every other action on a row has one; upload has
     no obvious letter left, and inventing `gu` for it may be worse than
     reaching for the mouse on something done a few times a month.
