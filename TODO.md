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

## Phase 4 ★ — auth, sessions, the pre-auth shell, `Viewer`

**Privacy filtering is the most expensive retrofit on the list**: a `WHERE`
clause on every read *and* a predicate inside every total, because a hidden row
must not leak by making the numbers fail to add up.

- `/setup` (one sectioned form, one POST, one transaction), `/invite/:token`,
  `/login`, `/lang`. No `/signup`, ever.
- `sessions` table + opaque `HttpOnly` cookie. `HX-Request` CSRF check on
  **authenticated** routes only — a blanket middleware breaks all four plain
  form POSTs and makes the app impossible to enter.
- `Viewer` carries member id, role and whether preview-as-non-admin is on.

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
