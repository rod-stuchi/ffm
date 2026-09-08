## The row component

`templates/partials/row.html`, rendered from a `LedgerRow` struct. The status mark,
delta arrow and meta glyphs are **pure functions of fields** — put them as methods
on the struct, not as logic in the template.

Each visual channel encodes exactly one fact, so a row never has to be read to be
understood:

| State | Mark | Meaning |
|---|---|---|
| `paid` | filled green disc + checkered-flag date | tracked, receipt file found on disk |
| `scheduled` | calendar glyph over the ring | tracked, scheduled-payment file exists, due date not yet passed |
| `scheduled overdue` | calendar glyph, warning colour | tracked, scheduled-payment file exists **past** due date, no receipt yet — an unconfirmed promise |
| `pending` | ring, filling green → amber → red | tracked, neither a receipt nor a scheduled-payment file exists yet |
| `plain` | hollow dot | untracked, not marked paid — no filesystem proof either way |
| `manual paid` | filled disc, distinct shade from `paid` | untracked — user asserted paid, no file to prove it |
| income | filled dot, `+ R$`, positive colour | sums into the pane header |
| focused | accent left rail, gutter `0`, tinted ground | cursor position |
| selected | `--info` inset bar | `x` — a marker for a batch op, never a delete |
| private | 🔒 glyph | hidden from other members, never from admin |
| deleted | dimmed, strikethrough, deletion date | soft delete, row retained |
| restored | `↩ #id` | new row superseding the deleted one |
| skeleton | shimmer bars, **identical geometry** | in flight |
| error | red ground, message + **Try Again** | server said no, nothing was written |

### Specifics that carry meaning

- **The due ring is elapsed time**, not a countdown — from period start to due
  date. Three-quarters amber reads as "still room, but not much" with no legend.
- **The MoM delta is derived, not stored.** Compare each row to the same line in
  the previous month (a window function). Red ▲ up, green ▼ down, grey dash for
  unchanged.
- **Skeleton geometry must match the real row exactly**, so nothing shifts when
  the server's fragment lands.
- **Error copy says what happened and that nothing was written.** No apologies.
  "Try Again" resubmits the original action.
- **`manual paid` exists only where there's no file to contradict it.** A rule
  with tracking on never shows this mark — it shows
  `paid`/`scheduled`/`scheduled overdue`/`pending`, full stop. The manual
  checkbox is the one place in the whole receipt model where a human assertion
  stands in for evidence, and it's reserved for the case where no evidence is
  even possible.
- **The bill/invoice file (`barcode`) never changes this state column at all.**
  It's informational — line items, amount, due date, as issued by the company
  — shown as its own small "bill attached" glyph regardless of whether
  anything has been scheduled or paid.

---

## Keymap

Lowercase acts on the **occurrence**, uppercase on the **rule or the window**.
Nothing destructive fires from a single keystroke.

### Move
| Key | Action |
|---|---|
| `j` / `k` | row down / up |
| `5j` `7k` | counted motion |
| `gg` / `G` | first / last row |
| `H` / `L` | focus member pane, left / right |
| `gt` / `gT` | next / previous member tab (tabs mode) |
| `2gt` | jump straight to tab 2 |
| `Ctrl-P` / `Ctrl-N` | previous / next month |

### Act
| Key | Action |
|---|---|
| `J` / `K` | move the row — or every selected row — down / up |
| `3J` | move it three places |
| `Ctrl-H` / `Ctrl-L` | **re-attribute** the row to the member pane left / right |
| `a` / `i` | add after / before the current row |
| `x` | toggle selection for a batch operation |
| `e` | edit this occurrence |
| `R` | edit the recurring rule behind it |
| `p` | toggle private *(proposal, not in the original spec)* |

### Find
| Key | Action |
|---|---|
| `/` | search the ledger — incremental, regex |
| `n` / `N` | next / previous match, wrapping |
| `Esc` | clear the search highlight |

### Undo
| Key | Action |
|---|---|
| `dd` | delete — opens a confirmation |
| `y` | confirm in the delete modal |
| `u` | undo the last change |
| `Esc` | clear count, buffer, selection and search |

### Type
| Key | Action |
|---|---|
| `:` | open the command bar |
| `:add 50 mercado publix` | a spend, in one line |
| `:in 2400 freelance` | an incoming |
| `:move ana` | re-attribute to another member |
| `:sort date` | row order: manual · date · amount |
| `:set rnu` | gutter: relative · absolute · off |
| `:theme gruvbox` | colourscheme (writes `ffm.toml`) |
| `:config` | open `ffm.toml` beside the ledger |
| `?` | keymap |

### Deliberate deviations from Vim

- **`?` opens the keymap, not reverse search.** `N` already walks a `/` search
  backwards, and `?`-as-help is the one key a non-Vim user will try. Half this
  app's userbase does not use Vim. One-line swap in the keydown switch if this
  is ever reversed.
- **`J` moves the row, not join-lines.** There is nothing to join in a ledger,
  and `j` moves me / `J` moves the thing is a clean extension of the
  lowercase/uppercase rule.
- **`Ctrl-H` / `Ctrl-L` are separate from `J` / `K` on purpose.** `J` reorders —
  cosmetic, affects nobody's totals. `Ctrl-L` re-attributes — it moves money
  between two people's INs/OUTs and changes who owns the row. Same-shaped
  gesture, very different blast radius; they must not share a keystroke you can
  overshoot into.

### Confirmation rule

**Confirm when the consequence is not obvious from the action — not merely when
the action writes something.**

- `dd` confirms: deletion reads as destructive even though it is a soft delete.
- Re-attributing an ordinary row does **not** confirm: it is fully undoable and
  the statusline says where it went.
- Re-attributing a **private** row **does** confirm: handing a hidden transaction
  to another member makes *them* the owner, so the person who hid it stops seeing
  it. That is invisible in the gesture.

---

## Auth & sessions

Sized deliberately for a LAN/Tailscale-only deployment — the network boundary
does most of the real work, so the auth layer stays simple on purpose.

- `members` gains `password_hash` (argon2) and keeps `role` (0 = admin).
- **No self-registration.** An admin creates a member row and sets its initial
  password — household membership is closed, the same way `ffm.toml` is
  admin-only.
- **A real `sessions` table**, not a bare signed cookie carrying the member id
  directly: `sessions(id, member_id, created_at, last_seen_at)`. The cookie
  carries only the opaque session id (`HttpOnly`, `SameSite=Lax`, `Secure` if TLS
  sits in front). Losing one device means revoking one row, not rotating a
  secret that logs out the whole household.
- **Long-lived, sliding sessions.** No lockout or rate limiting — disproportionate
  for a closed household network. Revisit all of this the day this is ever
  exposed to the open internet; it is not designed for that today.
- **CSRF**: `SameSite=Lax` plus checking HTMX's own `HX-Request` header on
  state-changing routes. No token scheme — not proportionate at this scale.
- **Focused member ≠ logged-in member.** `H`/`L`/`gt` pick which pane is
  focused, which decides who a *new transaction* is attributed to — that stays
  visual/Alpine-adjacent per the existing rule. Login (the session) decides whose
  `member_settings` and private rows you can see or edit. Never conflate the
  two: focusing your spouse's pane must never let you touch their settings or
  their hidden rows.

---

## Receipt tracking & files

No regex, no traversal, no user-authored pattern. A tracked rule's file
location is fully computed from a small taxonomy picked from a per-member
autocomplete — never typed as a path or a pattern.

### Taxonomy

Three free-text fields on the rule, each suggested from the logged-in member's
own previously-used values — a personal tag cloud, not a rigid category table
(no separate `categories` entity; just `SELECT DISTINCT ... WHERE member_id = ?`
over that member's rules). Scoped per member on purpose: a kid's allowance and
school-cafeteria categories have no business showing up in a parent's utility
dropdown, or vice versa. Two members can independently use the same string
(e.g. both typing "Consumption") — it's just never suggested across accounts,
and it has no effect on storage either way, since files already live under
each member's own folder regardless of tag overlap:

- `bucket` — **required**, e.g. "Consumption"
- `bill_name` — **required**, e.g. "Power"
- `instance` — **optional.** Disambiguates the same bill name across two
  properties/accounts, e.g. "company_001" / "House 8898". Skipped entirely
  when there's only one — "Power" alone is enough, no need to invent a name
  for a distinction that doesn't exist yet.

### Storage layout — fully deterministic

```
{member_uuid}-{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}.{ext}           # definitive receipt
{member_uuid}-{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}_scheduled.{ext} # scheduled-payment proof (optional)
{member_uuid}-{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}_barcode.{ext}   # the bill/invoice itself
```

- `{month}` is zero-padded `01`–`12`; the `{year}` folder exists specifically so
  the same month doesn't collide across years. `{instance}` is only present in
  the path when the rule has one.
- Every path segment is slugified (lowercase, spaces → `_`). Renaming a rule's
  `bucket`/`bill_name`/`instance` must move its existing files to the new
  computed path — it isn't just a label change.
- `bucket`/`bill_name`/`instance` may not slugify to `delete` or start with `.`
  — reserved, since `.delete/` is where soft-deleted files go (see below).

### Three file kinds, three separate facts

- **`{month}_barcode.{ext}` — the bill itself**, as issued by the company (line
  items, quantities, amount, due date). Purely informational — its presence
  never changes the paid/scheduled state, it's just proof of what's owed.
- **`{month}_scheduled.{ext}` — optional.** Proof a payment was set up ahead of
  the due date. There is no requirement to ever have one — a same-day payment
  out of balance skips straight to the receipt, no scheduling step to record.
- **`{month}.{ext}` — the definitive receipt.** Its presence is `paid`, full
  stop, regardless of whether a `scheduled` file ever existed.

A scheduled file with no receipt and a due date already in the past is not
generic `pending` — it's a promise that hasn't been confirmed kept, and gets
its own warning-coloured state (see the row-state table above) rather than
silently reading the same as "nothing has happened yet."

**Uploading the receipt supersedes the scheduled file automatically.** The
system moves `{month}_scheduled.{ext}` into `.delete/` the instant
`{month}.{ext}` lands — the promise is resolved, only the definitive proof is
left standing. This is a system-triggered soft delete, logged exactly like a
manual one (`old_path`, `deleted_at`, `deleted_by`) plus a `reason` of
`superseded` rather than `manual`, so `u` on the receipt upload restores the
scheduled file too and the audit trail is honest about why it moved.

### Paid is still "a matching file exists on disk" — checked directly, not by traversal

The filesystem stays the source of truth on purpose, not the database: the
vault is meant to be human-browsable and manipulable directly (backups, manual
tidying, cross-checking at tax time), and a DB flag that could silently
disagree with what's actually in the folder is exactly the failure mode this
app avoids everywhere else (`ffm.toml` vs. `:theme`, months as views not
copies, and so on). Checking is now a single `exists()` call per kind, on one
exact path per occurrence — no directory walk, since the path is computed, not
searched for. This also fully retires the earlier concern about excluding
`.delete/` from a traversal: there's no traversal left to exclude it from.

A DB row still accompanies every upload — uploader, upload time, kind
(`receipt` | `scheduled` | `barcode`), original filename as typed by the user,
extension — but purely as metadata/audit for undo. It does not decide
paid/not-paid; the disk does.

### Extensions

Same closed, case-insensitive set as before: `pdf`, `jpg`, `png`, `txt`. The
stored file's extension mirrors whatever was actually uploaded.

### Upload

- Scoped to recurring-rule occurrences only.
- **Three distinct actions — add barcode / add scheduled / add receipt** —
  rather than one upload plus a question, since each kind has its own fixed
  name and its own effect on state.
- The file is renamed to the deterministic convention on disk; the user's
  original filename is kept as metadata only, for their own reference.
- **Delete (of any of the three) is a soft delete.** The file moves into a
  `.delete/` subfolder beside where it lived; never removed from disk. The DB
  row records the move (`old_path`, `deleted_at`, `deleted_by`, `reason`) so
  `u` restores it exactly.

### Manual paid, for untracked rules only

- A rule with tracking **never turned on** has no filesystem trail — so, and
  only so, it gets an optional manual `paid` checkbox. This is the sole manual
  tick in the whole receipt model.
- A **tracked** rule never gets this checkbox. "Paid only when a matching file
  exists" is absolute for it — ticking a box is not a substitute for evidence,
  which is the entire reason tracking was opted into.
- Visually distinguish the two "paid" marks — file-verified vs. manually
  asserted — with different colours/shades of the same glyph, never the same
  mark (see the row-state table above).

### Not modelled yet

- A `months` allow-list and a tracking-window `interval` (e.g. a tax tracked
  only January–October). ffm's rule already has a start/end date; a
  months-of-year filter on top of that is not yet designed against the new
  taxonomy.

---

## Configuration — `ffm.toml` + per-member settings

Two tiers, split by who legitimately needs to change what:

- **`ffm.toml`** — process-level, **admin-only**. Lives in the repo, versioned
  with the code, watched at runtime (`notify` crate). Parsed with `toml` +
  `serde`, `#[derive(Deserialize)]`, `#[serde(default)]`.
- **`member_settings`** — one row per `member_id` in SQLite. Personal display
  preferences that would otherwise make household members fight over one shared file:
  theme, line-number style. Set at onboarding, edited with `:theme`/`:set` or a
  `:profile` vsplit form — same commands as before, different destination.

Why split it this way: `ffm.toml`'s "no settings dialog, the file writes back
and is the single source of truth" rule only holds if there's one person per
file. Theme is taste, not process config — Natália's `:theme rosepine`
overwriting Rodrigo's `:theme nord` in a shared file is the file *disagreeing
with itself*, exactly the failure mode the single-source-of-truth rule exists
to prevent. Moving personal prefs into a `member_id`-keyed row sidesteps it
entirely — SQLite already serializes concurrent writes, `ffm.toml` doesn't.

### The single-source-of-truth rule (still holds, per tier)

**There is no settings dialog for either tier.** `:set`, `:sort`, `:theme` and
the clickable statusline cells **write back into the file or the row directly**.
If a setting could be changed in two places that disagree, that tier stops
being the source of truth for it.

### Surface

Both tiers open as a **vertical split** beside the ledger — not a modal, not a
full-screen buffer. `:config` opens `ffm.toml` (admin only); `:profile` (or
`:me`) opens the current session's `member_settings` as an HTML form instead of
a text buffer, since it's DB-backed, not file-backed — same vsplit mechanic,
same "one way to change it" guarantee, just not a literal file on that side.
Reasons a vsplit at all, so this is not relitigated:

- A full-screen buffer hides the ledger, and `layout.panes` is precisely the
  setting you need to *watch* take effect.
- A modal needs a backdrop, scroll-lock and a focus trap, and it implies a
  settings dialog with toggles — a second way to change one setting.
- A vsplit is just a second pane. `H`/`L` already means "move between panes". In
  HTMX it is one swap changing the grid from `1fr` to `360px 1fr`.

### Invalid config

**Keep the last valid config and say which line is wrong.** Do not fall back to
defaults mid-session — that silently changes behaviour the user did not ask for.
Error format: `✗ line 8 · layout.panes must be "auto", "split", "tabs" — got "tab"`.
Serde already reports the failing field; wire it to the line. Applies to
`ffm.toml`; `member_settings` gets the equivalent per-field validation on
submit.

### Schema — `ffm.toml` (process-level, admin-only)

```toml
[storage]
db_path = "./ffm.db"
receipts_root = "/disks/vault/ffm/receipts"   # per-member folders generated under here
backup_dir = "./backups"

[layout]
panes = "auto"          # "auto" | "split" | "tabs"
min_split_width = 900   # px; below this, auto → tabs
max_width = 900         # px cap per pane; 0 = unlimited
gap = 16                # px between panes

[ledger]
sort = "manual"         # "manual" | "date" | "amount"
search_scope = "pane"   # "pane" | "household"
show_delta = true
show_recurring = true

[privacy]
default_visibility = "household"   # "household" | "private"
```

### Schema — `member_settings` (per member, DB-backed)

```
theme_name        text     -- any of the 11 bundled palettes
theme_overrides   json     -- open-ended: any of the 11 tokens, same
                            -- #[serde(flatten)] HashMap<ThemeToken, Color> as
                            -- before, decoded from JSON instead of TOML
line_numbers      text     -- "relative" | "absolute" | "none"
```

`theme_overrides` stays the one **open-ended** field: a strongly-typed key enum
still makes an unknown token a real error naming the valid tokens — only the
source format changed (JSON column vs. TOML file), not the validation.

---

