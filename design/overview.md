# Family Finances Manager (ffm)

## Purpose

This project replaces a shared household spreadsheet used to track finances — incomings, spends, and recurring payments. The spreadsheet has three recurring pain points this system is meant to eliminate:

- Every month, a new worksheet is copied and manually cleaned up.
- The spreadsheet is used jointly with my wife (both of us input spends/incomings).
- We regularly compare a month against the previous one to spot spending changes.

## Goals

- **No more manual month copies.** "Months" are not physical copies of anything — they're just a time-sliced view over a single, continuous transactions table. Recurring payments are defined once and automatically materialized into the ledger — by an in-process task that runs at startup and hourly thereafter, reconciling which periods are missing rather than tracking a "last run" marker — instead of being re-entered or copy-pasted.
- **A recurring bill's amount can carry forward, or not — the household's choice, per bill.** Most bills (rent, internet, IPTU) barely change month to month, so materializing a new period can seed its amount from last period's actual value, still fully editable. A bill that's genuinely different every time, like a credit card statement, can opt out and start blank instead — auto-filling a wildly different number would be actively wrong, not just unhelpful.
- **Shared, per-user household use.** Transactions are attributed to a user, so the app can show the household total or filter down to either person's spending individually.
- **Open sharing by default, with opt-in privacy.** Transactions are visible to all household members by default. A user can mark a transaction as hidden from other members, but never from the admin user (the household member with role 0), who can always see everything.
- **Built-in month-over-month comparison.** Comparing this month to previous ones is a first-class query (e.g. via SQL window functions), not a manual diffing exercise.
- **Optional receipt tracking for recurring bills**, filed under a small user-defined taxonomy — a required category "bucket" (e.g. "Consumption"), a required bill name (e.g. "Power"), and an optional instance to disambiguate the same bill across two properties or accounts (e.g. two houses, each with its own power bill — skipped when there's only one) — rather than a hand-written path/regex pattern. The taxonomy is a per-member, autocompleted tag cloud — scoped to the logged-in member's own past categories, not shared across the household, since a kid's allowance categories and a parent's utility categories have nothing to do with each other — and it fully determines a deterministic file path per period, so there's nothing to author or traverse. Tracking applies for the recurring transaction's whole active period, defined by a start-date and an end-date (end-date may be null, meaning still ongoing). See `design/spec.md` for the full mechanics, including per-member storage layout and in-app upload.
  - Each period can carry up to three files, each a separate fact: the **bill itself** (informational — line items, amount, due date, as issued by the company), an **optional scheduled-payment proof** (only relevant if the payment was set up ahead of time rather than paid same-day), and the **definitive receipt**, whose presence alone means **paid** — read straight off the filesystem, not a database flag, since the vault is meant to be browsable and manipulable by hand.
  - A scheduled payment left unconfirmed past its due date — a promise with no receipt yet — is its own warning state, not lumped in with "nothing filed yet." Uploading the receipt resolves it automatically, moving the now-superseded scheduled file aside.
  - Recurring bills without tracking opted in have no filesystem proof either way, but do get an optional **manual** paid checkbox, visually distinct from a file-verified paid mark. A *tracked* rule never gets a manual override — the whole point of opting into tracking is that a checkbox stops being sufficient evidence.
- **Categorization is universal, grouping is free.** `bucket` and `bill_name` are plain tags on any transaction — recurring or one-off, tracked or not — not just receipt-tracking plumbing. Grouping a panel by `bucket` renders as real Vim folds, and because a fold header always shows its subtotal whether open or closed, collapsing everything (`zM`) already reads as the monthly category report. No separate summary screen to build.
- **Unresolved bills don't get lost in an old month.** The app always opens to the current month, and if a tracked bill anywhere earlier in the current calendar year is overdue with no receipt, a small badge on the month picker flags it — ambient, not a notification that has to be dismissed, and one click away from being resolved via the same month navigation that already exists.
- **Recurring rules can be open-ended or a fixed run.** A subscription or utility recurs indefinitely; something like a car loan financed in installments has a real end. `total_installments` gives it a countdown that stays in sync with an editable end date — distinct from a seasonal bill that goes quiet part of the year and returns annually (still an open design question, see `CLAUDE.md`'s open items).

## Stack

- **Rust (Axum, SQLx, Askama)**
  - **Axum** — the web framework; mature, stable, maintained by the Tokio team.
  - **SQLx** — database access with compile-time checked queries against the real schema; a renamed/removed column fails the build instead of failing at runtime.
  - **SQLite (WAL mode)** — a single-file database, fast enough and simple enough for a household-scale, multi-user system, with no separate database process to run or maintain.
  - **Askama** — HTML templates that are compile-time checked against the Rust structs passed into them; a template referencing a field that no longer exists fails `cargo build`.
- **HTMX, Tailwind, Alpine.js**
  - **HTMX** — drives all client/server interactivity by swapping HTML fragments returned by the server; no JSON API layer to build or keep in sync.
  - **Tailwind CSS** — utility-first styling with no custom stylesheets to write, so there are no global CSS rules to collide between pages/components.
  - **Alpine.js** — small, declarative library for purely visual/ephemeral UI state (toggles, modals, keybindings) that never touches real data.

## Architecture Principles

- **Hypermedia over the wire.** The server returns HTML fragments, not JSON. There is no client-side data model to keep in sync with the server — the database is the single source of truth, and the UI is just its rendering.
- **Compile-time safety at every boundary.** SQLx checks queries against the schema at build time; Askama checks templates against the structs passed to them at build time. A backend data change that breaks the UI is caught by `cargo build`, not discovered at runtime.
- **Strict division of responsibility** between the three moving parts of the frontend:
  - **Rust / database** owns real data, validation, and anything related to money.
  - **HTMX** owns anything that needs to talk to the server (fetching or persisting data).
  - **Alpine.js** owns purely visual state only (open/closed, selected row, keystroke capture) — no business logic ever lives in Alpine.

## Data Safety

- **No hard deletes.** There is no destructive delete operation — every piece of information can be recovered. "Deleting" a transaction marks it with a status and a deletion date rather than removing the row.
- **Recovery restores the same row.** Undoing a delete clears the deletion mark on the row itself — it does not insert a second, superseding row. That earlier design left two ledger rows for one transaction after a single delete-and-undo, which is a permanent cost paid in the view people read every day for a chain almost nobody followed. The history lives in the change log instead (below), which is both more complete and out of the way.
- **Every change is recorded.** An append-only change log captures who changed what and when — amounts, names, buckets, privacy, deletes, restores, re-attributions, file uploads — and is read per row with `gl`. Nothing in it is ever rewritten or removed.
- **Rolling backups.** The SQLite database file is backed up on a rolling two-month window.
- **Receipts follow the same rule.** Deleting one moves the file into a sibling `.delete/` folder rather than removing it from disk, with a DB row recording the move so it can be undone — no exception to "nothing is destroyed" just because it's a file instead of a row.

## Accounts & Access

- **Members are real accounts** with a password, not just an attribution label. There's no self-registration and no signup page: an admin (role 0) issues a single-use invite link, and the person accepting it sets their **own** name, password, language and colour scheme. Household membership stays closed — an admin still decides who gets a link — but nobody ever types somebody else's password, and there's no initial password to hand over on a sticky note. First run is a one-time setup page that creates the household and its first admin.
- **Sized for a LAN/Tailscale-only deployment.** Sessions are long-lived and server-side revocable; there's no rate limiting or lockout, since the network boundary — not the login form — is the real defense at this scale. There's deliberately no self-service password reset either — with no mail sender there'd be nowhere to send a token — so a forgotten password is recovered by an admin issuing a one-time reset link. That link only sets a password; the member keeps their name, their language and their filing. Because it also hands the admin a way to sign in as that person, issuing one signs them out everywhere and is written into the change log under the admin's name, rather than happening quietly. See `design/spec.md` for the session model in full, including why it isn't designed for open-internet exposure as-is.
- **The interface speaks English or Brazilian Portuguese, per member.** Language changes the *words* and nothing else: dates stay `dd/mm/yyyy` and money follows the household's currency regardless, so a Portuguese interface in a dollar household shows Portuguese copy and `$1,234.56`. Before you log in, the sign-in page follows the browser's own language preference.
- **Settings live in three tiers, split by who legitimately needs to change what.** A small file holds what an operator sets once — where the database, the receipt vault and the backups live. Everything genuinely shared by the household (currency, default language, default visibility) is one admin-owned record. Everything that's taste rather than policy — theme, line numbers, layout, row order, search scope — is a record per member, set when they join and editable any time. A shared file that two people both want to edit is the file disagreeing with itself; a record per member can't have that argument.

## Interaction Model (UX)

The app is designed keyboard-first, Vim-inspired, while remaining fully usable with a mouse for anyone who doesn't want to learn the shortcuts:

- A global command bar, opened with `/` or `:`, accepts short commands like `add 50 groceries publix` for fast data entry without leaving the keyboard.
- Ledger navigation uses `j`/`k` to move between rows.
- Rows display line numbers, configurable as **none**, **relative**, or **incremental** — defaulting to **relative**, so counted motions like `5j` / `7k` can jump straight to a row without stepping through it one line at a time.
- `x` toggles selection on the current row (like checking an item in a to-do list) for future batch operations — it does not delete anything.
- `a` adds a one-off spend and `i` a one-off income; `A` and `I` create the recurring versions of each. Lowercase acts on the single occurrence, uppercase on the rule behind it — the same split Vim uses, applied to money out and money in as a two-by-two grid.
- `dd` (vim-style double keystroke) deletes the current row, gated behind a confirmation modal — no destructive action fires from a single keystroke. This is still a soft delete under the hood (see Data Safety).
- `e` edits the selected transaction occurrence; `E` edits the underlying recurring rule it was generated from, keeping the same lowercase/uppercase "this occurrence vs. the broader rule" split.
- `u` to undo.
- `Ctrl-P` / `Ctrl-N` move to the previous/next month view.
- `H` / `L` (uppercase) move focus between family-member panels, left/right, like Vim window navigation. Early idea (see `FFM_001.pdf`): each member has their own panel with an INs/OUTs header and a ledger below, and the currently focused panel is visually highlighted — full screen layout is still just a sketch at this point.
- All standard actions are also available as clickable UI elements — the keyboard layer is an accelerator, not a requirement.

Mutations follow a consistent optimistic-UI flow: submitting an action immediately inserts an animated skeleton row; when the server responds, the skeleton is replaced either by the real row (success) or by an error row with a "Try Again" button that resubmits the original action (failure).

## Development Workflow

The local dev loop is designed to stay close to a modern hot-reload experience without a JS build step:

- **Tailwind CLI** runs in `--watch` mode, regenerating CSS as templates change.
- **`cargo-watch -x run`** recompiles and restarts the server on every save.
- **`tower-livereload`** (dev-only, gated behind `#[cfg(debug_assertions)]`) auto-refreshes the browser once the server restarts.
- A **dev-only UI sandbox router**, also gated behind `#[cfg(debug_assertions)]`, renders templates (including success/error/skeleton states) against dummy data — letting UI and CSS be iterated on directly without touching the real database.
