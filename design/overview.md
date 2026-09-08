# Family Finances Manager (ffm)

## Purpose

This project replaces a shared household spreadsheet used to track finances — incomings, spends, and recurring payments. The spreadsheet has three recurring pain points this system is meant to eliminate:

- Every month, a new worksheet is copied and manually cleaned up.
- The spreadsheet is used jointly with my wife (both of us input spends/incomings).
- We regularly compare a month against the previous one to spot spending changes.

## Goals

- **No more manual month copies.** "Months" are not physical copies of anything — they're just a time-sliced view over a single, continuous transactions table. Recurring payments are defined once and automatically materialized into the ledger (e.g. on app start or a scheduled task) instead of being re-entered or copy-pasted.
- **Shared, per-user household use.** Transactions are attributed to a user, so the app can show the household total or filter down to either person's spending individually.
- **Open sharing by default, with opt-in privacy.** Transactions are visible to all household members by default. A user can mark a transaction as hidden from other members, but never from the admin user (the household member with role 0), who can always see everything.
- **Built-in month-over-month comparison.** Comparing this month to previous ones is a first-class query (e.g. via SQL window functions), not a manual diffing exercise.
- **Optional receipt tracking for recurring bills**, filed under a small user-defined taxonomy — a required category "bucket" (e.g. "Consumption"), a required bill name (e.g. "Power"), and an optional instance to disambiguate the same bill across two properties or accounts (e.g. two houses, each with its own power bill — skipped when there's only one) — rather than a hand-written path/regex pattern. The taxonomy is a per-member, autocompleted tag cloud — scoped to the logged-in member's own past categories, not shared across the household, since a kid's allowance categories and a parent's utility categories have nothing to do with each other — and it fully determines a deterministic file path per period, so there's nothing to author or traverse. Tracking applies for the recurring transaction's whole active period, defined by a start-date and an end-date (end-date may be null, meaning still ongoing). See `design/spec.md` for the full mechanics, including per-member storage layout and in-app upload.
  - Each period can carry up to three files, each a separate fact: the **bill itself** (informational — line items, amount, due date, as issued by the company), an **optional scheduled-payment proof** (only relevant if the payment was set up ahead of time rather than paid same-day), and the **definitive receipt**, whose presence alone means **paid** — read straight off the filesystem, not a database flag, since the vault is meant to be browsable and manipulable by hand.
  - A scheduled payment left unconfirmed past its due date — a promise with no receipt yet — is its own warning state, not lumped in with "nothing filed yet." Uploading the receipt resolves it automatically, moving the now-superseded scheduled file aside.
  - Recurring bills without tracking opted in have no filesystem proof either way, but do get an optional **manual** paid checkbox, visually distinct from a file-verified paid mark. A *tracked* rule never gets a manual override — the whole point of opting into tracking is that a checkbox stops being sufficient evidence.

## Stack

- **Rust (Axum, SQLx, Askama)**
  - **Axum** — the web framework; mature, stable, maintained by the Tokio team.
  - **SQLx** — database access with compile-time checked queries against the real schema; a renamed/removed column fails the build instead of failing at runtime.
  - **SQLite (WAL mode)** — a single-file database, fast enough and simple enough for a 2-user household system, with no separate database process to run or maintain.
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
- **Recovery creates a new entry.** Restoring a deleted transaction inserts a new row rather than reverting the status on the deleted one, so the deleted entry's history is never overwritten. The new entry references the entry it supersedes, so each transaction carries a traceable chain of prior versions — similar to how a git commit points back to its parent.
- **Rolling backups.** The SQLite database file is backed up on a rolling two-month window.
- **Receipts follow the same rule.** Deleting one moves the file into a sibling `.delete/` folder rather than removing it from disk, with a DB row recording the move so it can be undone — no exception to "nothing is destroyed" just because it's a file instead of a row.

## Accounts & Access

- **Members are real accounts** with a password, not just an attribution label — an admin (role 0) creates each one; there's no self-registration, matching how `ffm.toml` itself is admin-only.
- **Sized for a LAN/Tailscale-only deployment.** Sessions are long-lived and server-side revocable; there's no rate limiting or lockout, since the network boundary — not the login form — is the real defense at this scale. See `design/spec.md` for the session model in full, including why it isn't designed for open-internet exposure as-is.
- **Personal preferences (theme, line-number style) live per-member in the database**, set once at onboarding and editable any time — not in `ffm.toml`, which stays a single shared, admin-owned file. Household members can't disagree over one file if the setting isn't in the file.

## Interaction Model (UX)

The app is designed keyboard-first, Vim-inspired, while remaining fully usable with a mouse for anyone who doesn't want to learn the shortcuts:

- A global command bar, opened with `/` or `:`, accepts short commands like `add 50 groceries publix` for fast data entry without leaving the keyboard.
- Ledger navigation uses `j`/`k` to move between rows.
- Rows display line numbers, configurable as **none**, **relative**, or **incremental** — defaulting to **relative**, so counted motions like `5j` / `7k` can jump straight to a row without stepping through it one line at a time.
- `x` toggles selection on the current row (like checking an item in a to-do list) for future batch operations — it does not delete anything.
- `a` / `i` open the add-entry input, vim-style (insert after / insert before the current row).
- `dd` (vim-style double keystroke) deletes the current row, gated behind a confirmation modal — no destructive action fires from a single keystroke. This is still a soft delete under the hood (see Data Safety).
- `e` edits the selected transaction occurrence; `R` edits the underlying recurring bill definition it was generated from, keeping the same lowercase/uppercase "this occurrence vs. the broader rule" split used elsewhere in Vim.
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
