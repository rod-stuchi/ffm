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

## Phase 5 ★ — mutations and the optimistic-UI flow

A **response-shape contract**, not a feature: submit → skeleton row → the
server's fragment replaces it with the real row or an error row carrying
**Try Again**. Fix the handler convention once and every later phase inherits it.

Swap boundaries matter here: the `x-data` holding parked drafts must sit on a
container *outside* every `hx-target`, or the draft dies with the swap.

## Phase 6 — recurring rules and the materializer

`A` / `I` / `E`, the rule pane, and the reconciler: per rule, find the periods
between `start_date` and `min(current_month, end_date)` with no occurrence and
insert those. No watermark. Soft-deleted occurrences count as **present**, or it
resurrects deleted rows once an hour. Never materialize past the current month.
Runs at startup *and* hourly.

## Phase 7 — receipts and files

What finally makes `RowStatus::Paid` real rather than a fixture. Computed paths,
the three file kinds, the two-step `exists()` check (read `receipts.ext`, else
probe the four extensions), soft delete into `.delete/`, and the
`rule_barcode_window` table that fixes the recorded KNOWN HOLE about a renewed
window rewriting history.

## Phase 8 ★ — income estimates and the `~`

Every money surface must accept an `estimated` flag. `fmt::money` already takes
it from phase 1, so this is plumbing rather than surgery.

## Phase 9 — the settings surfaces

`:household`, `:profile`, `:members`, invites, resets. Vsplit panes with **no
Save button** — every control writes on `change`.

## Phase 10 — the change-log pane (`gl`)

A pure read of what phase 3 has been writing all along.

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
