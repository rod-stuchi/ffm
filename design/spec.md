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
  — and is surfaced through the file badges below, not through the status
  mark, regardless of whether anything has been scheduled or paid.
- **Installment progress is a bare `{paid}/{total}` badge** (e.g. `7/12`), not
  baked into the title — the same meta slot the due-date and recurring glyphs
  already occupy.

### File controls (the file rail)

Up to three small controls sit in their own slot between the name/meta block
and the amount — not in the meta line (too small for what these need to
carry), and not merged into the status mark (the mark stays a pure fact about
state; opening a file is an action, kept visually separate on purpose).

```
 0   ✓   Energia · Enel SP              [1] [3]            R$ 343,62
         🏁 11/07/2026 · ↻                                 ▲ +20%

 1   ⏱   Água · Sabesp                  [1] [2]            R$ 108,40
         ⚠ agendado, sem comprovante · venceu 18/07

 2        Mercado · Pão de Açúcar                          R$ 658,90
         13/07/2026                                          ▼ -6%
```

- **Fixed numbering by kind, always**: `1` is the bill, `2` is scheduled, `3`
  is the receipt — never renumbered based on what happens to exist on a given
  row. `g1` means "show me the bill" on every row, tracked or not, which is
  what makes it learnable.
- **Only rendered when that file exists.** A row with a bill and a receipt but
  no scheduled file shows `[1] [3]` — nothing sits where `2` would be. No
  ghost/hollow placeholder; the keybinding still works purely off kind, not
  position.
- **Neutral stroke icon per kind, not a colour-coded emoji.** Bill, scheduled
  and receipt each get their own small matching-weight line-icon (a
  document, a clock-in-a-box, a checked receipt) rendered in `currentColor` —
  legible in every theme, and never competing with `pos`/`neg`/`accent` for
  attention the way a full-colour emoji glyph did.
- **The number is a real `<kbd>` built into the control itself**, not a
  floating notification-style bubble pinned to its corner — same key-cap
  styling used everywhere else in the app (surface-2 fill, bottom-weighted
  border, mono font). It doesn't just label the control, it reads as "press
  this," the same trick the gutter's relative line numbers already pull off.
- **Compact by default, labelled on the focused row.** Every control is
  icon-and-keycap only until its row is focused; the focused row's controls
  expand in place to add the file's name (`Bill`, `Scheduled`, `Receipt`), so
  the one row you're actually looking at spells itself out without every row
  in the ledger paying that width cost. Narrow screens stay icon-only
  regardless of focus — there's no room to spare there.
- **Sized as a deliberate step down from the status mark**, not by box
  height alone: the mark is a solid 26px disc, the read of the row's single
  most important fact. The file rail's controls share that 26px height but
  as slim, low-contrast outline pills with a 14px icon — visually lighter and
  narrower, so they stay legible without competing with the mark for the eye.
  `↻`/`🔒` stay exactly as small and inline as they've always been in the
  meta line. Three tiers, each weight saying how important the fact is.
- **Kept visually separate from the amount.** The file rail carries its own
  margin beyond the row's normal grid gap — wider than before — so it never
  reads as fused to the value it sits next to: opening a file and reading the
  amount are two different facts, not one glued group.
- **Absent for any row with nothing to show** — not a reserved empty box. The
  row's middle column is already flexible (`1fr`), so the amount column stays
  flush against the row's right edge whether the file rail is present or not;
  nothing about existing alignment changes.
- **Clicking a control opens that file in a new browser tab.** That default
  is unchanged — see the Recurring rules section of `CLAUDE.md` for why no
  custom file renderer is built for it. `P` (below) is a second, additive
  way to look at a row's files without leaving the ledger; it doesn't
  replace opening a file in its own tab, which is still one keystroke away.
- **The bill control shows a page indicator when its rule's barcode spans
  several periods** (see Recurring rules' `spans_period` in `CLAUDE.md`) —
  `Bill · p.4` in its label, rather than a bare "Bill" that would read as
  this month's own file when it's actually page 4 of a shared one. An
  occurrence outside the current window's page count gets the same
  warning treatment as `scheduled overdue` (`fbadge-warn`) instead of the
  neutral stroke styling every other file control uses — the badge itself
  is the first place this gets flagged, before anyone opens `P` or the
  file.

### File preview carousel (`P`)

`P` opens a carousel over the focused row's own files — bill, scheduled,
receipt — without leaving the ledger behind it.

- **Same fixed numbering as `g1`/`g2`/`g3`, reused rather than
  reinvented.** `1`/`2`/`3` inside the carousel jump straight to that file
  kind; pressing a number for a kind that doesn't exist on this row is the
  same no-op-with-a-statusline-message `g1`/`g2`/`g3` already give,
  not a new error path.
- **`h`/`l` step to the previous/next file that actually exists.** Clamped
  at either end, not wrapping — the first file's "previous" button and the
  last file's "next" button are both visibly disabled rather than looping
  back around.
- **Full-viewport, not a dialog-sized modal** — every other modal in this
  app is a centred `min(560px, 100%)` box, and this is the one deliberate
  exception. The actual job is reading a boleto's barcode digits, or
  holding a phone up to its QR code to scan it — both need the file
  genuinely large on screen, which a 560px dialog can't give.
- **Zoom (`+`/`-`, `0` to reset) scales the preview**, with floating
  controls in the stage's corner — the same placement any image viewer's
  zoom control uses, chosen so it doesn't compete with the row of file-nav
  buttons for space. Each file starts back at 100% when you flip to a
  different one; a carried-over zoom level would be a surprise applied to
  an unrelated file, not a convenience.
- **The stage shows the file's icon (the same stroke icon the file rail
  already uses, just bigger) and its computed path — not the file's actual
  bytes.** This is still not a custom file renderer; "Open in new tab" sits
  in the footer for when the real content is actually needed.
- **A row with zero files refuses outright** — `P` says "no files on this
  row" rather than opening an empty carousel with nothing to look at.
- **A bill whose rule spans several periods (`spans_period`) shows its page
  prominently in the stage**, not just in the file rail's small label —
  "One file, 10 pages — this month is page **4**" sits right under the
  icon, since the whole point of opening the carousel for this file kind
  is confirming which page is actually owed this period. A page outside
  the current window's count shows the same warning colour and copy as
  the file rail's `fbadge-warn` state: "page 11 — outside this file's 10
  pages, a new upload is needed for this month" — the carousel and the
  badge always agree, never one flagging a problem the other stays quiet
  about.

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
| `R` | edit the recurring rule behind it — or, on a row that isn't recurring yet, create one starting here |
| `gd` | show due/cadence/receipt details for the focused row *(proposal, not in the original spec)* |
| `p` | toggle private *(proposal, not in the original spec)* |
| `P` | preview the row's files in a carousel — see Files below *(proposal, not in the original spec)* |

### Mouse
| Action | Result |
|---|---|
| Click a row | focus it — the same cursor `j`/`k` move, just placed by pointing instead |
| Double-click a row | `e` — edit this occurrence |
| Hover a recurring row | show its due/cadence/receipt details — the same info `gd` shows for the focused row, keyboard-only |
| Click a file control | open that file, new tab — the same action as `g1`/`g2`/`g3` |

- **`gd` is that same details popup with no mouse involved.** Hovering was
  the only way to see a row's due date, cadence and receipt path before this
  — no keyboard path existed at all. `gd` anchors the popup to the focused
  row instead of a pointer position (there isn't one), reuses the exact same
  panel and content, and is dismissed the same way any hover artifact is —
  the very next keypress, of any kind, closes it, per the rule below. It's a
  no-op with a statusline message on a non-recurring row — the same
  treatment `R` already gets there — but works the same as hover on any
  recurring row, tracked or not.

- **Keyboard motion always wins over a stale hover.** The moment any key
  moves the cursor, the row the mouse happens to be resting over stops
  painting its hover highlight, and the recurring-row tooltip is dismissed
  outright — only the real (`.row.focus`) cursor stays lit. Without this,
  hovering one row and then walking away with `j`/`k` leaves two different
  rows highlighted in two different ways at once (possibly with the old
  row's tooltip still floating on screen, detached from anything current),
  and neither reading ("where my mouse is" vs. "where my cursor is") is more
  correct than the other, which is exactly the ambiguity a modal ledger
  cannot afford. Any real mouse movement clears the suppression immediately
  — hover and the tooltip both come right back the instant the mouse is the
  thing actually moving again.

### Find
| Key | Action |
|---|---|
| `/` | search the ledger — incremental, regex |
| `n` / `N` | next / previous match, wrapping |
| `Esc` | clear the search highlight |

### Fold
| Key | Action |
|---|---|
| `za` | toggle the fold under the cursor |
| `zo` / `zc` | open / close it explicitly |
| `zR` / `zM` | open / close every fold in the panel |

### Files
| Key | Action |
|---|---|
| `g1` | open the bill for the focused row, new tab *(proposal, not in the original spec)* |
| `g2` | open the scheduled-payment file *(proposal, not in the original spec)* |
| `g3` | open the receipt *(proposal, not in the original spec)* |
| `P` | preview the row's files in a carousel, without leaving the ledger *(proposal, not in the original spec)* |
| `1` / `2` / `3` | inside the carousel — jump straight to that file |
| `h` / `l` | inside the carousel — previous / next file |
| `+` / `-` | inside the carousel — zoom in / out |
| `0` | inside the carousel — reset zoom to 100% |

Not stock Vim — a genuine FFM extension of the `g`-prefix "goto" vocabulary
already in use (`gg`, `gt`), not a new pattern. Pressing a number for a file
that doesn't exist on that row is a no-op with a statusline message, same
treatment `R` already gets on a non-recurring row: *"no bill on file for this
row"*, *"no scheduled payment on file for this row"*, *"no receipt on file
for this row"*.

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
| `:group bucket` | group by category · `:group none` to flatten |
| `:set rnu` | gutter: relative · absolute · off |
| `:theme gruvbox` | colourscheme (writes `ffm.toml`) |
| `:currency usd` | household currency, admin-only (writes `ffm.toml`) |
| `:config` | open `ffm.toml` beside the ledger |
| `:preview` | admin only — toggle previewing the app as a non-admin member |
| `?` | keymap |

### Modal fields
| Key | Action |
|---|---|
| `Ctrl-H` / `Ctrl-J` / `Ctrl-K` / `Ctrl-L` | move focus between a modal's fields, left / down / up / right |
| `Enter` | trigger the modal's primary action — Save on the edit-occurrence form |
| `Esc` | cancel, no changes written |

- **Directional, not tab order.** These move by where a field actually sits
  on screen, not by DOM sequence — so a field grid (amount / day / member,
  say) navigates the way it visually reads, without the form's markup order
  having to match its layout order. Plain `Tab`/`Shift-Tab` still walk the
  DOM order for anyone who reaches for that instead.
- **The amount comes before the description in the edit-occurrence form.**
  It's the field most edits actually touch, so it's both the first thing on
  the form and the one that starts focused and pre-selected — type over it
  immediately, no click or tab required.
- **`Enter` never reaches a destructive confirmation.** It fires whatever
  the modal's own primary button does — Save on the edit-occurrence form,
  Save/Create on the rule form, Close on a modal with nothing to save
  (help) — but the delete-confirmation modal has no primary button for it
  to find, on purpose. Its only "yes" stays the explicit `y` keypress (see
  Undo above), so `Enter` can never become an accidental way to confirm a
  delete.
- **The rule form's End date and Total installments stay in sync**, per
  CLAUDE.md's Recurring rules section — typing one recomputes the other
  (assuming monthly cadence; that's the only cadence actually wired today).
  End date alone, with installments left blank, is the correct way to give
  an ordinary open-ended bill a stopping point without implying it was ever
  a fixed-length loan.
- **A tracked rule's "Barcode covers multiple months" checkbox reveals two
  more fields**, "Pages in this file" and "First page covers" —
  `spans_period`'s `window_size`/`window_start`, per CLAUDE.md's Recurring
  rules section. The same two fields serve both shapes the feature
  supports: filled in once for a fixed-length loan's one-time file, or
  re-entered whenever a cyclic bill's next cycle actually sends a new one
  — there's no separate "renew" control, typing a fresh pair of values
  *is* the renewal.
- **Due date moved from the occurrence form to the rule form.** The
  edit-occurrence form's Day field only renders at all for a non-recurring
  row; a recurring row's due day is edited on the rule (`R`) instead, so
  there's exactly one control for that fact, never two disagreeing about
  which one a future materialized period actually follows.
- **Bucket is the identical combobox in both forms** — same per-member
  datalist, same "pick one of yours or type a new one" behaviour, in the
  occurrence form and the rule form alike. The rule form's Bucket starts
  blank when creating a new rule specifically (not when editing an
  existing one), regardless of what the anchor transaction was tagged
  with before it became a rule.
- **Day only ever holds digits.** Any edit that would leave it holding
  anything else is rejected outright, snapping back to the field's last
  valid value rather than to some filtered version of what was just typed.
  That distinction matters the moment the rejected edit also erases
  existing text — select all and type one letter over "15", say — where
  filtering the new value down to digits has nothing left to recover; only
  remembering the value from before the edit gets it back.
- **Amount is a live mask, not a filtered text field.** The user only ever
  types digits — there's no decimal separator to type at all, because the
  field inserts it for them, along with thousands grouping, as they type.
  Typing `1234567` becomes `12.345,67` (or `12,345.67`, `USD`-configured
  households; see Currency, below) one keystroke at a time, the same way a
  calculator or POS terminal reads digits right-to-left into cents. The
  field's real value is tracked as an integer count of minor units, not
  parsed back out of the formatted string on save — the display and the
  saved value can't disagree with each other because the display is
  computed from the same integer that gets saved.
- **A datalist's native suggestion popup is browser chrome, not page DOM** —
  closing the modal while it's open (`Enter` on the Bucket field) blurs the
  focused field first, on purpose, before the field is torn down. Skipping
  that step can leave the popup visually stuck on screen with nothing left
  to anchor to, since removing its input from under it isn't the same thing
  as the browser's own "dismiss the suggestions" trigger, which is losing
  focus.
- **`Enter` on the bucket field itself never saves the form — it only
  blurs it.** That key is inherently double-booked on a combobox: it's also
  how you confirm the highlighted `<datalist>` suggestion, and browsers
  don't agree on which meaning wins. Chromium swallows that keystroke inside
  its own suggestion-list handling before this app ever sees it; Safari
  doesn't, so the same Enter that was only meant to pick a bucket value also
  fell through and saved the whole form — closing it before the rest of the
  edit was even reviewed. Carving this one field out of the generic
  "Enter saves" rule sidesteps the disagreement outright rather than trying
  to detect it: Enter here always just confirms the field and blurs; a
  second Enter, now from anywhere else, saves for real.

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

### Preview as non-admin (admin only)

An admin can toggle their own session's admin rights off, to see the app the
way any other member actually experiences it — badge click (`ADMIN` ↔
`PREVIEW`) or `:preview`, reversible any time, no confirmation needed since
nothing about it is destructive or hard to undo.

- **Deliberately not called "impersonate", and not built as one.** The admin
  considered and rejected impersonating a specific other member (viewing as
  *that* person, in their words: "it will not impersonate another user") —
  this is the same session, same identity, same pane, same edits, own private
  rows still fully visible throughout. The only thing toggled is whether
  admin-only visibility and actions are currently in effect for that one
  session, not a session swap to someone else's account.
- **What actually changes**: other members' `private` rows go back to being
  genuinely hidden — not greyed out, not present-but-redacted, absent from
  the row list, the pane's totals, and any group subtotal exactly the way
  they'd be absent for a real non-admin, so nothing about the numbers can
  leak a hidden row's existence by not adding up. Admin-only config
  (`ffm.toml` — `:config`, `:currency`) becomes inaccessible; the `ffm.toml`
  button itself disappears rather than sitting there disabled, since a real
  non-admin never sees a control for a file they can't open.
- **What doesn't change**: the admin's own pane, own rows, own private
  transactions — ownership already grants visibility independent of role,
  same as it does for every other member.
- **`j`/`k` (and any other row-stepping motion) skip a hidden row rather than
  landing on it** — the same mechanism a closed `:group bucket` fold already
  uses to keep the gutter and focus index correct while stepping over rows
  that aren't rendered. A hidden row is skipped, never a dead stop.

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

### Multi-page barcode — one file, several months (`spans_period`)

Opt-in per rule, off by default. Some bills arrive as a single document
covering several periods at once instead of a fresh file every month:

- A city property tax office sending one PDF with 10 barcodes for a
  Jan–Oct cycle, reissued (new file, values readjusted) at the start of
  the next cycle.
- A bank issuing one document with a loan's entire run of installments up
  front — that one file lasts for the rule's whole life; there's no
  "next cycle" to reissue for.

Both shapes share the same mechanism: `spans_period`, `window_size`
(pages in the current file) and `window_start` (which period is page 1).
`{month}_barcode.{ext}` stops being computed per period when this is on —
every occurrence inside the window points at the identical uploaded file,
and the app derives the page from `window_start` and the occurrence's own
period, not from a separately-stored per-occurrence number. `scheduled`
and the definitive receipt stay genuinely per-period regardless — sharing
one bill document doesn't mean sharing one payment.

- **The window doesn't have to span the rule's entire life.** A
  fixed-length rule (`total_installments` set) gets exactly one window,
  sized to match, that outlives nothing — once it's done, it's done, so
  there's never a second file to upload. A cyclic bill gets a **new**
  window every cycle: re-entering `window_size`/`window_start` when that
  cycle's document actually arrives is the whole "I got a new file"
  action — there's no separate upload flow modelled for any of the three
  file kinds yet (see Upload, below), and this doesn't need one either.
- **Independent of the still-open cyclic/seasonal rule design** (a
  `months`-of-year allow-list — see CLAUDE.md's Open items). This doesn't
  wait on that: it only needs a start period and a page count to compute
  from, regardless of what eventually decides which months are active.
  IPTU already demonstrates the *filtering* half (a `months` allow-list in
  the mock's seed data) independently of the *shared-file* half decided
  here — the two compose without either depending on the other.
- **An occurrence outside the current window's page count is flagged, not
  silently wrong** — the same warning channel as scheduled-overdue, shown
  both on the file badge (`fbadge-warn`) and in the `P` preview carousel:
  *"page 11 — outside this file's 10 pages, a new upload is needed for
  this month."* That message is the cue a fresh file needs uploading, not
  a bug to chase down.
- **The rule form's "Barcode covers multiple months" checkbox** sits right
  under "Track receipt on disk" (only shown once that's on), revealing
  "Pages in this file" and "First page covers" — the same two fields
  whether you're setting up a loan's one-time window or a tax's next
  yearly one.

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

## Categorization & grouping

`bucket` and `bill_name` are free-text fields on any transaction, autocompleted
per member (see Receipt tracking & files above for how tracked rules also use
them, plus `instance`, to compute storage paths). Grouping turns them into a
view.

- **The bucket field is a native combobox** (an `<input>` paired with a
  `<datalist>`), not a `<select>`. A real `<select>` would force one of the
  member's existing values and make typing a brand-new one impossible; a
  plain text input would drop the "pick one you've already used" affordance
  autocomplete is there for. The datalist is exactly "existing value or type
  a new one" with no extra markup — the same widget browsers already use for
  address/search autocomplete.

### Folds, not a new widget

`:group bucket` switches a panel's ledger from flat to grouped, rendered as
real Vim folds — see the Fold keymap above (`za`/`zo`/`zc`/`zR`/`zM`). Fold
state is pure Alpine visual state — the server renders the full grouped,
subtotaled fragment regardless of what's folded; folding only shows or hides
what's already in the DOM. `j`/`k` skip a closed fold's contents and land on
its header, same as real Vim.

### What a fold header looks like

A framed container (same visual language as the existing `.card` component),
one per `bucket`, holding the bucket name, a row count, and a **subtotal that
stays visible whether the fold is open or closed**. Deliberately not
colour-coded — `bucket` is open-vocabulary and unbounded, while a theme's
palette is a fixed 11 tokens with `pos`/`neg`/`accent`/`info` already spoken
for; a frame plus a label scales to any number of categories without risking a
collision or inventing a hash-based colour scheme for marginal gain.

Rows with no `bucket` set land in an **`Uncategorized`** group — grouping never
silently drops a row. Nesting stops at `bucket` — no nested `bill_name`
sub-folds.

### This is the report, not a separate screen

Because the subtotal never disappears, `zM` (close every fold) turns the
ledger straight into a per-category summary — Consumption R$ 1.556,92,
Subscriptions R$ 59,90, and so on — with zero additional screen to build.
Browsing and summarizing are the same mechanism at two zoom levels.

### Interaction with sort and reorder

Grouping is a derived view exactly like `ledger.sort = "date"`/`"amount"` — the
moment rows are re-clustered by category, `J`/`K` refuse with the same message
pattern: `ledger.group = "bucket" — reorder needs group = "none"`.

### Default fold state is personal

Whether a newly-opened grouped panel starts fully expanded or fully collapsed
is a `member_settings` radio choice (`group_fold_default`), not a shared
`ffm.toml` setting — same reasoning as theme and line numbers: it's taste, not
process config.

---

## Unresolved issues across months

An unresolved issue is a tracked occurrence overdue with no receipt —
`pending` past its due date, or `scheduled overdue` — checked household-wide
across the current calendar year, filtered per-viewer by the existing privacy
rule (a private row only counts for its owner and the admin).

### Surfaced as a badge, not a message

- A small dot (`--neg` token, the same warning colour already used for
  `scheduled overdue`) overlays the month-picker trigger button whenever any
  month this year, other than the one currently open, has an unresolved
  occurrence.
- The same dot appears on each flagged month's `.mcell` inside the picker
  popup, so opening the picker shows exactly which months, not just that
  something, somewhere, is wrong.
- No toast, no banner, no first-load message. The dot is just there the
  moment the page renders, for as long as the issue stays unresolved —
  ambient like every other status in this app, not a one-shot notification
  that gets dismissed and then forgotten.
- Clicking a flagged cell jumps to that month — the picker's existing
  behaviour, unchanged. No new command.

### Defaults

The app opens to the current month on first load, always — never the
last-viewed month, never January.

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

[locale]
currency = "BRL"        # "BRL" | "USD" | "EUR" | "GBP" — see below
```

### Currency (`[locale] currency`)

Household-wide, admin-only — deliberately not in `member_settings` next to
theme. Every member's transactions feed the same shared totals (INs/OUTs/NET,
balances), so a per-member currency would mean either fake, unbacked
exchange-rate conversion between members' views, or two members seeing
different-looking numbers for the exact same total. One value, same tier as
`storage`/`layout`, is what keeps that coherent — see `CLAUDE.md`'s
Conventions section for the fuller reasoning.

- **A closed set, not a free-typed code.** `BRL` | `USD` | `EUR` | `GBP`
  today, the same "closed set, real published values" posture as the
  bundled themes — extendable later, not user-invented.
- **Choosing a currency chooses its whole conventional number format as one
  preset** — symbol, decimal separator, thousands separator, all bundled.
  Picking `USD` switches the household to dot-decimal/comma-thousands
  (`$1,234.56`), not just the glyph; there's no "keep comma-decimal but
  show `$`" combination on offer, because that isn't how any of these
  currencies are actually written.
- **Dates are unaffected.** `dd/mm/yyyy` stays fixed regardless of currency
  — this setting is scoped to money formatting only, not a full locale
  switch.
- **The edit-occurrence modal's amount field is a live mask keyed off this
  same value** — the user only ever types digits (see Modal fields,
  above); the decimal/thousands separators it inserts, and the static
  currency-symbol prefix shown beside the field, both come from whichever
  currency is currently configured. Every other money display in the app
  (row amounts, pane totals, the household header) reads the same single
  value, so there's exactly one place that ever needs to change if this
  setting changes — nothing about currency formatting is hardcoded
  per call site.

### Schema — `member_settings` (per member, DB-backed)

```
theme_name        text     -- any of the 11 bundled palettes
theme_overrides   json     -- open-ended: any of the 11 tokens, same
                            -- #[serde(flatten)] HashMap<ThemeToken, Color> as
                            -- before, decoded from JSON instead of TOML
line_numbers      text     -- "relative" | "absolute" | "none"
ledger_group      text     -- "none" | "bucket"
group_fold_default text    -- "expanded" | "collapsed"
```

`theme_overrides` stays the one **open-ended** field: a strongly-typed key enum
still makes an unknown token a real error naming the valid tokens — only the
source format changed (JSON column vs. TOML file), not the validation.

---

