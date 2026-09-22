## The row component

`templates/partials/row.html`, rendered from a `LedgerRow` struct. The status mark,
delta arrow and meta glyphs are **pure functions of fields** — put them as methods
on the struct, not as logic in the template.

Each visual channel encodes exactly one fact, so a row never has to be read to be
understood:

| State | Mark | Meaning |
|---|---|---|
| `paid` | filled green disc + flag-icon date | tracked, receipt file found on disk |
| `scheduled` | calendar-icon glyph over the ring | tracked, scheduled-payment file exists, due date not yet passed |
| `scheduled overdue` | warning-icon glyph, warning colour | tracked, scheduled-payment file exists **past** due date, no receipt yet — an unconfirmed promise |
| `pending` | ring, filling green → amber → red | tracked, neither a receipt nor a scheduled-payment file exists yet |
| `plain` | hollow dot | untracked, not marked paid — no filesystem proof either way |
| `manual paid` | filled disc, distinct shade from `paid` | untracked — user asserted paid, no file to prove it |
| income | filled dot, `+ R$`, positive colour | sums into the pane header |
| focused | accent left rail, gutter `0`, tinted ground | cursor position |
| selected | `--info` inset bar | `x` — a marker for a batch op, never a delete |
| private | lock-icon glyph, full colour when set, faint when not | hidden from other members, never from admin |
| deleted | dimmed, strikethrough, deletion date | soft delete, row retained; `u` clears the mark on this same row |
| skeleton | shimmer bars, **identical geometry** | in flight |
| error | red ground, message + **Try Again** | server said no, nothing was written |
| editing | the row opens into its own form, Cancel / Save at its foot | `e` — see Edit fields |
| rule open | accent outline, the rule pane's arrow pointing at it | `E` — see Edit fields |
| unsaved | dashed accent border + `unsaved · e to resume` tag | a draft left behind when the cursor moved on |
| placeholder | dashed, empty "New recurring bill / income" row | `A` / `I` — where the new rule's row will land |
| expected | ring toward the expected day, `+` at its centre | income, not yet received — the same elapsed-time arc a bill's due date drives |
| received | filled positive disc + received date | income that landed |
| estimated | `~R$ …` in a quieter colour, no delta | a varying income's amount is projected from history until confirmed — the mark still carries arrival, the tilde carries "this number isn't a fact yet" |

### The add form (`a` / `i`)

It opens inside the pane, never in a modal, and **stays open after a save** —
a household entering the week's spending types straight into the next one.

- **It says which it is adding.** The same open form serves `a` and `i`, and
  the only thing that changes between them is the kind, so a heading is the
  only thing that can say so: *Add a spend* / *Add an income*, from the
  catalog, switching with the form's own `kind`.
- **It keeps the row's two columns.** The description starts where every
  description starts and the amount ends where every amount ends, because the
  form mirrors `.row`'s `34px 1fr` and `.entry`'s `26px 1fr auto` exactly. It
  was a flex strip before, lining up with nothing above it.
- **The frame is dashed**, which is what this ledger already means by "not a
  row yet" — the same language the `A`/`I` placeholder row uses.
- **Save and Cancel are their own row, at the right edge, a size down.** They
  end the gesture rather than being part of the thing being typed, so they do
  not sit in the field strip competing with it for width.
- **The amount is the live mask**, per Edit fields below: only digits are
  typed and the separators are inserted right-to-left into minor units. The
  element carries the integer in `data-cents` and the text is always painted
  from it, so the two cannot drift; what is submitted is the formatted string,
  which `Cents::parse` reads back to exactly that integer. One parse on the
  way in, rather than a second field to keep in step.
- **A sign says the direction before the words do** — `+` in `--pos`, the
  token an income row's amount already wears, and `−` in the plain ink a
  spend's amount is written in. Not the accent: the accent means "you are
  here", and a minus in red would say something the ledger does not.

**All three amount fields carry the same mask** — the add row, the inline
edit row and the rule pane. It is a plain function rather than a method on
the ledger component, because the rule pane is its own Alpine component and
reaching a parent's method through scope chaining is a subtlety not worth
resting every money field on. Each seeds from `data-cents`, the row's own
stored integer, and never from the text rendered beside it — an edit form
that re-parsed its own display would be exactly the drift the mask exists to
prevent. It also fixed what the rule pane used to show: the amount was
rendered as `343.62` with a dot, which is not how a BRL household writes it.

### The foot of the shell: the command bar, then the statusline

Both are **in flow at the bottom of the shell**, not pinned with
`position: fixed`. `.stage` takes the leftover height, so they sit against the
bottom of the viewport whatever the ledger contains, and the stage scrolls
inside itself when the rows outgrow it.

This was wrong in both directions at once and the two halves disagreed: the
statusline simply followed the content and floated wherever the panes ended,
while the command bar was `position: fixed; bottom: 30px` — pinned above a
30px statusline that was not there. Two bars, two different ideas of where
the bottom was.

**The command bar sits ABOVE the statusline**, which is the mock's order and
not Vim's. Vim puts the command line on the last row; this app does not,
because the mode badge lives on the statusline and flips `NORMAL` → `COMMAND`
as you type — which only reads if that row is the one that stays put.

The statusline carries, left to right:

| cell | what it says |
|---|---|
| mode | `NORMAL` · `COMMAND` · `SEARCH`. Accent for normal — the "you are here" token — and `--info` while typing, which already means "what I picked" |
| household · period | which ledger, which month |
| member | the **session's** member and `(admin)` when admin powers are actually in effect, so it reads as a member under preview-as-non-admin |
| message | the same channel the pre-auth pages use |
| buffer | the pending count and key prefix, so a half-typed `3d` or `g` is visible rather than silently remembered |
| position | `4/7` in the focused pane |
| sort | `manual` · `date` · `amount` — **clickable, cycles** |
| line numbers | `rnu` · `nu` · `nonu` — **clickable, cycles** |

The last two are the single-source rule made visible: a command, a form
control and a statusline cell are three gestures writing **one** row. Clicking
them writes `member_settings` exactly as `:set` and `:sort` do, and the page
re-renders because the gutter and the row order are decided server-side.

The `:` bar carries the command vocabulary on its right. Those are
identifiers a member types back, not prose, so they never pass through the
catalog — the same rule the sort-refusal message follows.

**The hint lists exactly what the dispatcher handles**, and `just cmd-check`
fails the build when it does not. Its first version was copied from the mock
and advertised `add`, `in`, `move`, `undo` and `help`, none of which exist —
a hint that offers commands the app then refuses is worse than no hint. (The
first three are in the Type table above and genuinely unbuilt; see
`TODO_2.md`.)

**Admin-only commands are absent for a non-admin**, and for an admin
previewing as one — the same rule that removes the Household button rather
than disabling it. `preview` is the exception and follows the *role* rather
than whether its powers are in effect: it is the toggle itself, the same
category as the badge, and dropping it while previewing would say there is no
way back.

### The gutter, and which half of it the server can decide

`member_settings.line_numbers` is `relative` | `absolute` | `none`, and the
three are not decided in the same place:

- **`absolute` and `none` are entirely the server's.** The row index and "no
  numbers at all" are both known at render time, so the query builds them and
  nothing on the client touches them.
- **`relative` is measured from the cursor, and the cursor is Alpine's.** The
  server still renders it correctly for the first frame — the cursor always
  starts on the first row, so relative-to-row-0 is the right answer before any
  key is pressed — and `paint` rewrites the numbers as the cursor moves. Same
  posture `layout = auto` takes: correct before the first paint, no round
  trip, and no business logic in Alpine (the distance from a row to the cursor
  is a fact about the cursor).

This was a real bug: the pane query hardcoded `Gutter::Absolute`, so
`relative` and `none` were settings you could pick with nothing happening.
The cause was a missing field rather than wrong arithmetic — `RowCtx` carried
sort, currency and the delta toggle but never the one setting the gutter is
about.

**`none` narrows the gutter's track; it never removes the cell.**
`display: none` on a grid child reflows its siblings and crushes the row —
a gotcha this codebase has already paid for.

**Changing any setting the server renders from refreshes the page.** Line
numbers, row order, grouping, the delta column and every layout value are
decided per render, so writing the row and swapping nothing leaves the
screen disagreeing with the setting. Language and theme were already doing
this; the list was simply too short. What stays a quiet write is what the
next render reads anyway without changing what is on screen: the fold
default, the search scope and the privacy default.

### Specifics that carry meaning

- **The due ring is elapsed time**, not a countdown — from period start to due
  date. Three-quarters amber reads as "still room, but not much" with no legend.
- **"The same line" means `(member_id, title)` in the previous period.**
  Unambiguous for a recurring row — it is the same rule — but a one-off has no
  rule, and the mockup shows deltas on one-offs (`Mercado ▼ −6%`), so the
  question is real. Exact string match, same member, previous period, first
  hit. It is what a household means by "the same line", it needs no extra
  column, and it degrades the right way: rename the line and the delta
  disappears, which is arguably correct — it is not that line any more.
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
- **The meta line's icons are stroke SVGs, not an emoji font** — the flag
  (paid), calendar-clock (scheduled/pending) and warning triangle
  (scheduled overdue) reuse the exact same line-icon language as the file
  rail's bill/scheduled/receipt icons (unfilled paths, `currentColor`,
  matching stroke weight), just sized down to sit inline with the meta
  text. An emoji font renders differently — sometimes in full colour, at a
  different weight — on every OS, which fights the "one visual system"
  point of the rest of this table.
- **Recurring and private are always both rendered, dimmed when off,
  instead of appearing only when true.** Unlike the status icons above
  (mutually exclusive with each other) or the file rail (a control is
  absent when that file doesn't exist), recurring and private are
  independent per-row booleans that can be on, off, or both at once — a
  fixed two-icon pair with opacity carrying the fact reads at a glance
  without the row's width jumping around as flags toggle. Active is full
  opacity in the fact's own colour (`--accent` for recurring, `--info`
  for private, unchanged from before); inactive is the same icon at low
  opacity in `--ink-3` — present, but deliberately not competing for
  attention. `ledger.show_recurring = false` still removes the recurring
  icon outright, household-wide — that config already means "we don't
  use this concept here," not "mute it."

### File controls (the file rail)

Up to three small controls sit in their own slot between the name/meta block
and the amount — not in the meta line (too small for what these need to
carry), and not merged into the status mark (the mark stays a pure fact about
state; opening a file is an action, kept visually separate on purpose).

```
 0   ✓   Energia · Enel SP              [1] [3]            R$ 343,62
         🏁 11/07/2026 · ↻ 🔒                                ▲ +20%

 1   ⏱   Água · Sabesp                  [1] [2]            R$ 108,40
         ⚠ agendado, sem comprovante · ↻ 🔒 venceu 18/07

 2        Mercado · Pão de Açúcar                          R$ 658,90
         13/07/2026 · ↻ 🔒                                    ▼ -6%
```

(The icons above are ASCII stand-ins for the actual line-icon SVGs — see
Specifics that carry meaning, above. Row 2's `↻ 🔒` render faint/dim, since
that row is neither recurring nor private; they're still there, just not
standing out.)

- **Fixed numbering by kind, always**: `1` states the amount, `2` is the
  promise, `3` is proof the money moved — never renumbered based on what
  happens to exist on a given row. Only the *label* changes with which way
  the money goes: slot 1 is a spend's `Bill` and an income's `Invoice`, slot
  3 a `Receipt` or a `Payslip`. Slot 2 is spend-only — you don't schedule
  money arriving — so `g2` on an income row is a no-op with a message
  (*"no scheduled payment on an income row"*), joining the family a missing
  file already belongs to. `g1` and `g3` therefore mean one consistent thing
  on every row in the ledger, which is what makes them learnable.
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
  The recurring/private icons stay exactly as small and inline as they've
  always been in the meta line — see Specifics that carry meaning, above, for
  why they're now a fixed pair rather than emoji that appear only when true.
  Three tiers, each weight saying how important the fact is.
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
- **Full-viewport, not a dialog-sized modal** — the few other modals left
  in this app (confirmations, the keymap) are centred `min(560px, 100%)`
  boxes, and this is the one deliberate exception. The actual job is reading a boleto's barcode digits, or
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
| `a` | add a one-off spend |
| `i` | add a one-off income |
| `x` | toggle selection for a batch operation |
| `e` | edit this occurrence |
| `A` | create a recurring bill — opens the rule pane, with a placeholder row at the cursor |
| `I` | create a recurring income — the same pane, money the other way |
| `E` | edit the recurring rule behind the focused row; no-op with a statusline message on a plain row |
| `gd` | show due/cadence/receipt details for the focused row *(proposal, not in the original spec)* |
| `gl` | change log for the focused row — who changed what, when — in a side pane *(proposal, not in the original spec)* |
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
  treatment `E` already gets there — but works the same as hover on any
  recurring row, tracked or not.

- **The row's own status text stays short; the full explanation lives in
  this popup.** `scheduled overdue`'s meta line is just `overdue since
  {date}` — enough to scan at a glance, backed by the mark's own warning
  colour and icon (see the row-state table above). A full sentence
  (`scheduled, due date passed, no receipt yet`) sits in the popup instead,
  next to the `Due`/`Cadence`/`Receipt` fields it belongs with — the same
  glance-vs-read-on-demand split every other on-row status already makes,
  rather than growing the meta line into a paragraph every row pays for.

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
| `gu1` / `gu2` / `gu3` | **attach** a bill / scheduled / receipt to the focused row |
| `P` | preview the row's files in a carousel, without leaving the ledger *(proposal, not in the original spec)* |
| `1` / `2` / `3` | inside the carousel — jump straight to that file |
| `h` / `l` | inside the carousel — previous / next file |
| `+` / `-` | inside the carousel — zoom in / out |
| `0` | inside the carousel — reset zoom to 100% |

`gu` is "goto upload", a second step of the same `g`-prefix vocabulary rather
than a new pattern — and it deliberately reuses `g1`/`g2`/`g3`'s numbering, so
one set of numbers means one thing whether you are opening a file or adding
one. On a row with nothing to file under it is the same no-op-with-a-message
treatment as everything else in this family, and `gu2` on an income row is
refused for the same reason `g2` is: you do not schedule money arriving.

Not stock Vim — a genuine FFM extension of the `g`-prefix "goto" vocabulary
already in use (`gg`, `gt`), not a new pattern. Pressing a number for a file
that doesn't exist on that row is a no-op with a statusline message, same
treatment `E` already gets on a non-recurring row: *"no bill on file for this
row"*, *"no scheduled payment on file for this row"*, *"no receipt on file
for this row"*.

### Undo
| Key | Action |
|---|---|
| `dd` | delete — opens a confirmation |
| `y` | confirm in the delete modal |
| `u` | undo the last change |
| `Esc` | clear count, buffer, selection and search |

- **Undo restores the deleted row in place.** There is no second,
  superseding row and no `↩ #id` marker — that earlier design is reversed
  (see CLAUDE.md's Data model). One `dd` followed by `u` leaves exactly one
  row in the ledger, undeleted and focused. Both the delete and the restore
  are entries in the change log, which is where the chain lives now.

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
| `:theme gruvbox` | colourscheme (writes your own settings row) |
| `:lang en` · `:lang pt-BR` | UI language — yours alone; dates and money don't move |
| `:currency usd` | household currency, admin-only (writes the household row) |
| `:profile` · `:me` | your own settings, beside the ledger |
| `:household` | household settings, admin-only · `:config` is an alias |
| `:members` | admin only — members, and issue an invite link |
| `:invite ana` | admin only — create an invite link and open `:members` on it |
| `:logout` | end this session |
| `:preview` | admin only — toggle previewing the app as a non-admin member |
| `?` | keymap |

### Edit fields
| Key | Action |
|---|---|
| `Ctrl-H` / `Ctrl-J` / `Ctrl-K` / `Ctrl-L` | move focus between a form's fields, left / down / up / right — past the form's edge, back to the ledger, keeping the draft |
| `Enter` | save — the form's primary action |
| `Esc` | cancel: discard the draft, nothing written |

**Creation is a two-by-two grid, not four ad-hoc keys.** Lowercase acts on
the occurrence, uppercase on the rule; `a`/`A` are money out, `i`/`I` money
in, and `e`/`E` edit either side of that split:

|  | one-off (occurrence) | recurring (rule) |
|---|---|---|
| **spend** | `a` · `:add 50 mercado` | `A` |
| **income** | `i` · `:in 2400 freelance` | `I` |
| **edit** | `e` | `E` |

This replaces `r` (create rule) / `R` (edit rule), which were bound that way
because a barcode binding has to happen at a bill's creation — a reason that
stops generalising the moment income can recur too. **`i` no longer means
"add before the current row."** `a` adds below; above is `a` then `K`. That
gesture was in the keymap table but never in the code (`a` and `i` ran
identical logic), so nothing working was removed — but it is a deletion, not
a rename.

**Neither edit form is a modal.** The forms outgrew a centred 560px dialog
(the rule form especially), and a dialog hides the very ledger you're
editing. So:

- **`e` edits in place.** The focused row opens up into its own form — the
  same fields, with Cancel (`Esc`) and Save (`Enter`) at the bottom of the
  row — and the rest of the ledger stays where it was.
- **`A` / `I` / `E` open the rule pane** — a column of its own in the ledger's
  grid, right after the member pane being edited, pushing any later panes
  further right. *Right after the pane being edited* is load-bearing and was
  got wrong once: a single mount at the end of the stage looks identical with
  two members and is plainly wrong with three, putting Rodrigo's rule editor
  past Miguel at the far edge. Every pane carries its own mount and the
  focused one is the target — which is also where `gl` and the settings
  vsplits go, since `H`/`L` already means "move between panes". The ledger scrolls sideways when they don't all fit rather
  than hiding a pane: a household can have more than two members. A small
  arrow on the pane's left edge sits level with the row being edited (and
  follows it as the ledger scrolls); the row itself keeps an accent outline
  while its rule is open.
- **`A` / `I` get a placeholder row.** A brand-new rule has no row yet for the
  arrow to point at, so a dashed "New recurring bill" row appears at the
  cursor; Create turns it into the real row in that same spot, Cancel
  removes it.
- **Leaving an edit keeps it as a draft.** Moving the cursor off a row
  being edited — `j`/`k`, a click, a month change, `H`/`L`, anything —
  collapses it back to read-only. If nothing was changed the draft just
  goes away; if something was, the row stays marked (dashed accent border,
  an `unsaved · e to resume` tag) until the draft is saved or discarded, and
  the statusline counts them. Several can be parked at once; `e`/`E` on a
  marked row picks its draft back up exactly as it was left.
- **`Ctrl-H/J/K/L` past the form's edge is the keyboard way out.** Off the
  top or bottom of an inline row, the cursor moves to the row above or
  below (parking the draft); off the left edge of the rule pane, focus
  returns to the ledger with the pane still open until the cursor actually
  moves. Sideways never jumps rows — with no field left or right on this
  row, that's the edge.
- **Still modal, on purpose:** the `dd` delete confirmation, the
  private-row re-attribution confirm, the `?` keymap and the `P` file
  carousel. Those are confirmations and full-screen viewers, not forms —
  there's nothing to type, and a confirmation *should* stop you.

- **Directional, not tab order.** These move by where a field actually sits
  on screen, not by DOM sequence — so a field grid (amount / day / member,
  say) navigates the way it visually reads, without the form's markup order
  having to match its layout order. Plain `Tab`/`Shift-Tab` still walk the
  DOM order for anyone who reaches for that instead.
- **The amount comes before the description in the edit-occurrence form.**
  It's the field most edits actually touch, so it's both the first thing on
  the form and the one that starts focused and pre-selected — type over it
  immediately, no click or tab required.
- **`Enter` never reaches a destructive confirmation.** In a form it saves;
  in a modal it fires the modal's own primary button — Close on the keymap,
  Re-attribute on the private-row confirm — but the delete-confirmation
  modal has no primary button for it to find, on purpose. Its only "yes" stays the explicit `y` keypress (see
  Undo above), so `Enter` can never become an accidental way to confirm a
  delete.
- **Field order puts what names the bill first:** Amount and Bill name, then
  Bucket and Cadence, then the dates (Start / Due, End / Installments).
- **On/off settings are toggle switches, not tick boxes** — "track receipt on
  disk", "no barcode file", "covers multiple months", "hidden from other
  members" all read as states rather than actions, and a pill toggle says
  that in less space than a checkbox plus its label.
- **A create form is the same field grid as an edit form, minus the two per-occurrence
  noteboxes** (the path box and "past occurrences are left as recorded") —
  there's no real row behind it yet. `Esc`/Cancel removes the placeholder
  row too: nothing has been created to cancel out of.
- **Cadence offers monthly, bimonthly, every 3 / 4 / 6 months and yearly.**
  Only the list is real so far: End date ↔ Installments still computes in
  whole months, so anything other than monthly is a label today.
- **Seasonal months are an expression, not twelve toggles.** A rule that only
  runs part of the year, every year (IPTU Jan–Oct; school fees Feb–Dec) gets
  a "only some months" switch, and with it on, one mono field:

  | typed | means |
  |---|---|
  | `1-10` | January to October |
  | `1,2,3,5,6` | Jan, Feb, Mar, May, Jun |
  | `^11,^12` | every month except November and December |
  | `1-10,^5` | January to October, except May |
  | `^6-8` | every month except June, July, August |

  **`-` is always a range and `^` is always an exclusion.** A leading `-` for
  "except" parses just as well, since the range separator is always infix —
  it was rejected because it gives one idea two spellings, and an error
  message can then say plainly what `^` does. **Exclusions alone imply every
  month**, so `^11,^12` needs no `1-12,` in front of it; requiring an
  explicit base would make the shortest useful expression the longest one.
  An expression that excludes everything is an error, not an empty rule: a
  rule that runs in no month would materialize nothing forever, with nothing
  on screen to tell it apart from one that has not reached its start date.

  Toggles were the alternative and are a mouse gesture in a keyboard-first
  app. The field is mono, like every other thing in this app read character by
  character — a regex, a path, an invite URL.

- **The months read-back is rendered by the server, on every keystroke.** A
  notebox under the field lights the months it resolved to and says so in
  words (`10 months a year · Nov, Dec skipped`), or shows the parse error
  with no months lit. It is an `hx-get`, not JavaScript, for the reason the
  theme picker already gives: a preview computed in the browser is a second
  implementation of the parser, free to disagree with the one that actually
  stores the value — and this one decides which months a bill exists in at
  all. It sits with the computed receipt path and the income estimate box,
  which are the same idea.

- **The rule form's End date and Total installments stay in sync**, per
  CLAUDE.md's Recurring rules section — typing one recomputes the other
  (assuming monthly cadence; that's the only cadence actually wired today).
  End date alone, with installments left blank, is the correct way to give
  an ordinary open-ended bill a stopping point without implying it was ever
  a fixed-length loan.
- **A tracked rule's "Barcode covers multiple months" checkbox reveals two
  more fields** the moment it's ticked, "Pages in this file" and "First page covers" —
  `spans_period`'s `window_size`/`window_start`, per CLAUDE.md's Recurring
  rules section. The same two fields serve both shapes the feature
  supports: filled in once for a fixed-length loan's one-time file, or
  re-entered whenever a cyclic bill's next cycle actually sends a new one
  — there's no separate "renew" control, typing a fresh pair of values
  *is* the renewal.
- **A tracked rule's "This bill has no barcode file" checkbox hides the
  upload control and the multi-page checkbox entirely** — set once, for
  bills paid by wire transfer / PIX / TED that will never have a barcode to
  attach. Decided in `A`'s form at creation, correctable later in `E`'s,
  never inferred from anything else about the rule.
- **The computed receipt path is shown live, in two lines** — the real path
  as it stands (`consumption/condominio/house_33a/2026/09.pdf`) above a dim
  reminder of what makes it up (`bucket / bill name / instance / year /
  month.ext`). It recomputes on every keystroke in Bucket, Bill name or
  Instance, so renaming taxonomy shows where the files will move before the
  rule is saved. It includes the **bucket** segment, matching the storage
  layout above; the mock still omits the `{member_uuid}-{slug}` prefix.
- **"Amount in filename" appends it to the month** —
  `{month}_{currency}{amount}.{ext}`, so `09.pdf` becomes
  `09_brl122.94.pdf`. Off by default, per tracked rule.
  - **The currency code is what keeps the month readable.** A bare
    `09_9_99` reads as three equal parts — is that September, or the ninth
    of something? `09_brl9.99` can't be misread, and it records which
    currency the number was in, which matters because currency is
    household-wide config that can change (see Currency, below) while old
    filenames stay as written.
  - **Nothing in the name needs shell quoting.** Only letters, digits, `.`,
    `_`, `-` and `/` — no brackets, no spaces — because the vault is meant
    to be browsed and moved around by hand (`cp .../09_brl9.99.pdf` just
    works).
  - **The amount is plain: dot decimal, no thousands separator**
    (`1234.56`, never `1.234,56` or `1,234.56`). The displayed
    amount follows the household's currency format; the filename
    deliberately doesn't, so a stored name never depends on a setting that
    can be changed later.
  - **No amount yet means no suffix** — the name stays `{month}.{ext}` and
    picks the suffix up once the amount is known. In practice that gap only
    happens when the barcode parse couldn't read the value (see
    `design/spec-bill-import.md` §3's `Open`/`Reference` cases and §8's
    extraction-failure dialog), since an imported bill normally arrives with
    its amount already filled in. Writing a placeholder like
    `09_brl0.00.pdf` would put a number in the filename that was never
    owed.
  - **The app's own suffixes are untouched** — `{month}_scheduled.{ext}`
    and `{month}_barcode.{ext}` keep their plain `_` form, so the three file
    kinds stay recognisable at a glance.
- **The barcode upload field is a dropzone, not a file `<input>` styled to
  look like one** — attaching a file there is what derives `client_code`
  on save (see Bill import, below), and it's optional in both `A` and `E`:
  a rule can be created and saved with tracking on and no file yet, the
  same way an amount or due date can be filled in later.
- **A rule with a `client_code` on file shows it read-only**, above the
  upload field — a plain "Client code: 0123456789-0" line with its own
  **Unlink** button, no input box. The code itself is populated only by
  attaching a file (see Bill import, below), never typed by hand, so
  there's nothing here to edit directly; a rule with no file attached yet
  shows no line at all, the same "absent, not a blank field" treatment
  `instance` already gets when a rule doesn't need one. **Unlink** clears
  just the code — it doesn't close the pane, doesn't touch `tracked`/
  `bucket`/`billName`/`pattern`, and exists specifically for "the wrong
  file got attached the first time": drop a corrected one right after,
  in the same still-open dropzone.
- **Due date moved from the occurrence form to the rule form.** The
  edit-occurrence form's Day field only renders at all for a non-recurring
  row; a recurring row's due day is edited on the rule (`E`) instead, so
  there's exactly one control for that fact, never two disagreeing about
  which one a future materialized period actually follows.
- **Bucket is the identical combobox in both forms** — same per-member
  datalist, same "pick one of yours or type a new one" behaviour, in the
  occurrence form and the rule form alike. The rule form's Bucket starts
  blank when creating a new rule specifically (not when editing an
  existing one), regardless of what the anchor transaction was tagged
  with before it became a rule.
- **Numeric fields refuse a bad keystroke outright — value and selection
  untouched.** Day, Due date, Installments and Pages only ever hold digits,
  and Amount only ever takes typed digits. A letter is rejected *before* it
  lands (`beforeinput`), so typing "a" over a selected "59,90" leaves
  "59,90" selected, rather than blanking the field (the money mask would
  otherwise see "no digits" and repaint it empty) or snapping the caret to
  the end. A paste into Amount goes through as long as it carries digits
  ("R$ 12,34" is fine). Anything that slips past that (autofill) reverts to
  the field's last valid value with the selection put back where it was.
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
  closing a form while it's open (saving, cancelling, or collapsing the
  row) blurs the focused field first, on purpose, before the field is torn down. Skipping
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
  to detect it: Enter here always just confirms the field, blurs it and
  moves focus to Save; a second Enter saves for real.

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

### A rejected form says which box

**A message belongs next to the field it is about.** Every form in this app —
`/setup`, `/invite`, the inline edit row, the rule pane — renders **both**:
the message under its own field with the field marked, and a summary in the
card's `.warnbox`.

Two places for one message is a real cost, paid on purpose: the field message
locates the problem, and the summary is what a screen reader announces on
submit (`role="alert"`) and what makes a long form scannable. They are
generated from one `FieldErrors`, so the duplication is in the rendering and
never in the source.

- **Every field is checked, not just the first that fails.** A form that
  rejects one box per round trip makes somebody submit four times to find out
  four things. The checks run in the order the fields sit on the page, so the
  summary reads top to bottom.
- **Focus moves to the first bad field**, rendered server-side as `autofocus`
  on exactly one input — no JavaScript. A clean load keeps whatever autofocus
  it normally has, which is why every one of those is conditional rather than
  two elements competing for one cursor. (The rule pane and the edit row
  place the cursor themselves, because `autofocus` is not honoured reliably
  on a fragment the browser inserts, and both arrive as fragments.)
- **A cross-field rule is reported on the field you would retype.** "The two
  passwords do not match" lands on Confirm, not on Password — and only once
  the password is long enough, or a short password says two things about
  itself at once.
- **`/login` is the deliberate exception.** It gives one message for both a
  wrong name and a wrong password, which is a decision with a reason (see
  Signing in, below): the two lead to the same next action, and naming the
  field would say which half was right. It already does the other half of
  this rule — focus comes back to the password.
- **The settings panes are not forms in this sense.** They write on `change`
  and have no submit, so there is no rejected-submission moment; a bad value
  keeps the field's previous value and the pane's footer says what was wrong.

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
- **No self-registration.** Household membership is closed. There are exactly
  three ways in, and **no `/signup` route exists** — not even a 404 handler that
  mentions one:

  | route | when it works | who it's for |
  |---|---|---|
  | `/setup` | only while `members` is empty | the first admin, once |
  | `/invite/:token` | single-use, admin-issued, expires | a new member |
  | `/login` | always, once a member exists | everyone, thereafter |

- **The invitee sets their own password.** This reverses the earlier "an admin
  creates a member row *and sets its initial password*". An admin issues a
  one-time link; the person who accepts it picks their own name, password,
  language and colour scheme. The closed-household rule is untouched — an admin
  still decides who gets a link — but nobody ever types somebody else's
  password, and there is no initial password to hand over on a sticky note.
- **The login identifier is `members.name`, not an email.** `name` is already
  declared as "display name + login identifier"; `email` is nullable and plays
  no part in signing in. There is no mail sender in this app.
- **A real `sessions` table**, not a bare signed cookie carrying the member id
  directly: `sessions(id, member_id, created_at, last_seen_at, label)`. The
  cookie carries only the opaque session id (`HttpOnly`, `SameSite=Lax`,
  `Secure` if TLS sits in front). Losing one device means revoking one row, not
  rotating a secret that logs out the whole household. `label` is derived from
  the User-Agent so `:profile` can name the device it is offering to revoke.
- **Long-lived, sliding sessions.** No lockout or rate limiting — disproportionate
  for a closed household network. Revisit all of this the day this is ever
  exposed to the open internet; it is not designed for that today.
- **CSRF**: `SameSite=Lax` plus checking HTMX's own `HX-Request` header on
  state-changing routes. No token scheme — not proportionate at this scale.
  **The `HX-Request` check applies to *authenticated* state-changing routes
  only.** `/login`, `/setup`, `/invite/:token` and `/lang` are plain form POSTs
  and never carry that header, so a blanket check would reject all four and
  make the app impossible to enter. They rely on `SameSite=Lax` alone, which is
  sound here: a forged login POST accomplishes nothing for an attacker, `/lang`
  has no security value, and `/setup` and invite-accept are single-use by
  construction, so a replay self-limits. **This is written down on purpose** —
  left implicit, somebody eventually "fixes" the header check into a blanket
  middleware and breaks signing in.
- **There is no *self-service* password reset**, and that is a decision, not an
  omission: no mail sender exists, so there is nowhere to send a token. An
  admin issuing a reset link is the recovery path — see Forgotten passwords,
  below. The admin's own password is the one with nobody above it, which is why
  `/setup` asks for it twice; with a second admin, either can reset the other,
  and with only one, the recovery path is the console — see The console,
  below. **That replaces "recovery is editing the database by hand"**, which
  was never really available: `password_hash` is Argon2, so there is no hash a
  person can type into `sqlite3`, and getting back into your own ledger meant
  writing a program.
- **Focused member ≠ logged-in member.** `H`/`L`/`gt` pick which pane is
  focused, which decides who a *new transaction* is attributed to — that stays
  visual/Alpine-adjacent per the existing rule. Login (the session) decides whose
  `member_settings` and private rows you can see or edit. Never conflate the
  two: focusing your spouse's pane must never let you touch their settings or
  their hidden rows.

### The pre-auth shell

All three pre-auth pages share a layout that **does not exist anywhere else in
this app**: every current surface lives inside the full-viewport `.app` grid
(`.topbar` / `.tabbar` / `.stage` / `.status`), and pre-auth there is no ledger,
no month, and no member. One new layout, `.auth` — a centred card on
`var(--paper)`, a thin chrome strip, and the existing statusline.

What it reuses unchanged, so the app looks like itself before you are in it: the
11 design tokens and their light/dark triple definition, `.card`, `.f`/`.f .k`
labels, `input.i` with the **accent focus rail already used everywhere else**,
`.btn`/`.btn.pri`, `kbd` key-caps, `.warnbox`, `.notebox`, and the statusline
message channel (`say(msg, bad)`) — so there is one way the app talks to you on
both sides of the login. Archivo for headings and labels; **JetBrains Mono for
anything read character by character** — a login name, a receipts path, an
invite URL, a currency sample.

What it deliberately drops: the `NORMAL` mode badge (a mode badge with no modes
is a lie), the command bar, the tab bar and the month picker.

One layout note worth stating because it is load-bearing: **the chrome strip is
last in the DOM and painted first with `order: -1`.** That way `Tab` from the
last field reaches the submit button rather than the language switcher, with no
`tabindex` juggling. And **the `.auth` layout must never read `layout_*`** —
there is no member row to read it from. The auth card is sized in CSS.

### Pre-auth is not modal — and that is the existing rule, not an exception

Plain `Tab` / `Shift-Tab` / `Enter`. No `NORMAL` mode, no `:`, no `/`, and `?`
is unbound.

This is not a carve-out from the app's Vim-first identity; it is the rule this
document already follows. The inline edit row, the rule pane and the command bar
all take literal text, and moving between *their* fields is `Ctrl-h/j/k/l`, not
`j`/`k` (see Edit fields, above). A login page is 100% form, so the existing
rule yields plain `Tab`. Two further reasons it could not be otherwise: a modal
layer's premise is that `NORMAL` is the resting state, which means the first
character somebody types into a name field gets eaten as a motion — and on
`/invite`, the person typing has provably never used this app.

What is still guaranteed from the keyboard:

- `Enter` submits from any field. Each pre-auth page has exactly one submit, so
  there is no ambiguity about which button it finds — the same care the
  delete-confirmation modal takes in the other direction.
- **`Alt-L` cycles the language, `Alt-T` cycles light → dark → system.** Alt
  combinations rather than bare letters, so nothing is swallowed mid-typing.
  Rendered as `kbd` caps beside each control at ≥ 700px and dropped below, the
  same treatment the tab bar's `gt · gT` hint already gets.
- The colour-scheme picker is a real `<fieldset>` of radios, so `←`/`→` walk it
  natively. Do not build a grid of buttons and then reimplement arrow keys.

The keyboard-first identity still shows: every primary button carries a real `⏎`
key-cap in the same styling used throughout the app.

### First run — `/setup`

Reachable **only** while `members` is empty; afterwards it redirects to
`/login`. Conversely `/login` on an empty database redirects here, so there is
no state in which the app is running and unreachable.

**One sectioned form, one POST** — not a wizard. A wizard would need somewhere
to keep the first step's answers, and there is nowhere: no household row, no
member, therefore no session, and holding them client-side is exactly the
client-side data model this architecture forbids. Carrying them in hidden inputs
is the same single form with extra round trips and extra ways to land half
configured. One POST is one SQLite transaction that creates
`household_settings` + `members` + `member_settings` + `sessions`, or creates
nothing at all.

It collects, in three labelled sections:

- **Your household** — household name, currency, default language. The name is
  a real need, not decoration: `/login`'s heading, the statusline's left cell
  and the invite page's "Join …" all need a word for the household, and
  hardcoding "FFM" is worst on the one page whose job is *am I on the right
  server*.
- **You** — name (with "you'll sign in with this" said plainly next to it),
  password, confirm password, colour scheme.
- **Storage · `ffm.toml`** — `db_path`, `receipts_root` and `backup_dir`, shown
  **read-only** with a line saying to edit the file and restart. They are the
  operator's, not the admin's. Showing them lets the admin confirm the vault is
  where they think it is *before* a single receipt is filed, without creating a
  second place the value can be set.

Two copy decisions worth keeping:

- **The currency options show their format, not their ISO code** — `Real · R$
  1.234,56`, `US dollar · $1,234.56`. Picking a currency picks its whole number
  format as one preset (see Currency, below); the label should say so rather
  than letting the household discover it later.
- A note under the language field states the separation directly: *"Dates are
  always dd/mm/yyyy. Money follows the currency you pick. Language only changes
  the words."*

Success sets the session cookie and lands on the ledger. It never says "now
sign in with the password you just typed".

### Invites — `/invite/:token`, and `:members`

**A link, not an account somebody else made for you.** The `members` row is
created when the invite is *accepted*, not when the link is issued, which is why
`password_hash` stays `NOT NULL`: a pre-created, password-less member would make
"a NULL hash cannot authenticate" a rule enforced by a single `WHERE` clause
that somebody eventually forgets. The admin's real need — telling two open links
apart — is served by a `label` on the invite row instead.

*(The cost of that choice, recorded so it can be revisited on evidence rather
than taste: an invited member has no pane until they accept, so an admin cannot
attribute transactions to them in advance. If that turns out to matter — a
child's allowance being tracked before they ever log in — it is a small additive
change later, not a reason to weaken the schema now.)*

The token is 32 random bytes, base64url in the URL, **SHA-256 in the database;
the token itself is never stored.** Argon2 is deliberately *not* used: there is
nothing to brute-force in 256 bits of randomness, and a slow hash on every
invite-page GET is a self-inflicted denial of service. Recorded here because the
obvious "fix" is to make it match how passwords are hashed.

Links **expire after 7 days** by default. It is a LAN app and the link is handed
over in person or over a chat app; the number is named so the page has something
to render.

The accept form asks for name, password, confirm, language and colour scheme —
the same field grid as `/setup`'s second section, which is the point: one form,
two entry points. Above it sits the context (*"Rodrigo invited you to Casa
Stuchi"*) and below it the expiry, in mono.

- **Editing the name here is free; later it isn't.** `members.slug` is the
  receipt vault's folder name (`{uuid}-{slug}`), so renaming a member after
  files exist is a vault move. At accept time there are no files, which makes
  this the one moment it costs nothing — so the field is editable here and gated
  behind a move afterwards.
- **A used, expired or unknown token renders a page, never an error.** A
  `.warnbox` and **no form at all** — a disabled form invites retrying: *"This
  invite has already been used. If that wasn't you, ask Rodrigo to revoke it."*
  / *"This invite expired on 27/09/2026. Ask Rodrigo for a new link."* Each with
  a single way onward, to `/login`.
- **Name-taken needs copy, because it is reachable** (`members.name` is
  `UNIQUE`): *"Somebody already uses that name. Pick another — names are how
  everyone signs in, so they have to be unique."* Everything typed is preserved
  except the passwords.

**`:members` is where an admin issues one** — a vsplit pane, not a modal,
reusing the same 420px column, `esc` header and footer status line the rule pane
and the change log already use. It lists members with their role, lists pending
invites with their link and expiry, and has a create form at its foot: an
optional *for whom* label, a role, and a button. The new link is shown **once**,
in a read-only mono input with a Copy button, with a note saying to create a new
one if it is lost. `:invite [label]` is the shorthand that creates one and opens
the pane focused on it.

A `.notebox` beside the form states the principle so it is visible at the moment
it applies: *"You never type anyone else's password. They set it on the link,
along with their name."*

Like every other admin-only control, the create form **disappears** under
preview-as-non-admin rather than sitting there disabled.

### Forgotten passwords — an admin-issued reset link

**Changing a password you still know is not this.** That lives in `:profile`
(current / new / confirm) and needs no admin. This section is only about the
case where the password is *gone*.

**A reset reuses the invite mechanism rather than inventing a second one.**
Both are a one-time, admin-issued, expiring token that lands you on a form
which sets a password; the only real differences are whether a `members` row
exists yet and which fields the form asks for. So `member_invites` carries a
nullable `member_id`:

| `member_id` | what the link is | accepting it |
|---|---|---|
| `NULL` | an invite | **creates** the `members` row |
| set | a password reset | **sets `password_hash`** on that existing row |

**A reset is not an invite pointed at somebody who already exists**, and it
matters that this is written down: an invite's accept path *inserts*, and
`members.name` is `UNIQUE`, so a member trying to recover through one would hit
the name-taken error and could only get back in by choosing a *different* name.
That would also rewrite their `slug`, and `slug` is their receipt-vault folder
(`{uuid}-{slug}`). Recovering a password must never orphan somebody's filing.

The accept form asks for **password and confirm only**. No name — renaming is a
vault move once files exist, and this person is not new. No language or colour
scheme — they already have both, and a reset has no business resetting taste.

**At most one live reset link per member**, enforced by a partial unique index.
A second *Reset password* click replaces the first rather than leaving two
working links for one account.

#### It is logged, and it signs that member out

A reset hands the admin the ability to sign in **as** that member and write to
the ledger under their name. That cuts against the line this document draws
around preview-as-non-admin — *no session swap, no writes attributed to anyone
else* — so it is made accountable rather than quietly allowed:

- **It is a change-log entry**, `action = password_reset`, `entity = member`,
  with `entity_id` the member reset and `member_id` the admin who did it. The
  log is append-only, so the record cannot be tidied away afterwards.
- **Issuing it revokes every session that member has** — one delete per row in
  `sessions`, which is the reason that table exists. They are signed out, so a
  takeover is something they *notice* rather than something they could only
  infer.

Neither is optional. An admin already sees every `private` row by virtue of
role 0, so a reset grants no new *visibility* — what it grants is the ability
to act as somebody else, and that is exactly the thing this app otherwise
refuses to let anyone do silently.

#### Where the admin does it

In `:members`, expanding a member reveals **Reset password** beside **Sign out
everywhere**. The new link is shown **once**, in the same read-only mono input
with a Copy button that a pending invite already uses — no new widget, and the
same "create a new one if it's lost" note. `:reset <name>` is not bound; the
row action is the only entry point, so the capability is visible to whoever
holds it rather than known only to whoever remembers a command.

Like every other admin-only control, it **disappears** under
preview-as-non-admin rather than sitting there disabled.

### The console — `ffm reset-link <name>`

The one account nobody else can reset is the sole admin's. `ffm reset-link
<name>` is the way back into it, run on the machine the process runs on:

```
$ ffm reset-link rodrigo

✓ reset link for rodrigo · expires 28/09/2026

  http://192.168.0.12:3000/invite/nQ7…
```

- **It issues the same link `:members` issues**, and accepting it lands on the
  same page. It does **not** set a password, which is the whole shape of it:
  there stays exactly one code path that writes `password_hash`, so a change
  to how passwords are stored cannot reach one and miss the other; the member
  still picks their own, as they would have; and a password passed on `argv`
  would sit in the shell's history file and in `ps` output for every user on
  the machine.
- **It costs what the web path costs** — every session revoked, a
  `password_reset` and a `sessions_revoked` entry in the append-only log.
  Skipping either would be the same power, unrecorded.
- **The log says the console did it, not an admin.** There is no member behind
  the command, so `member_invites.created_by` is nullable and the entries
  stamp `source = 'system'` with the note *"reset link issued from the
  console"*. Naming an admin who wasn't there would be a lie in the one record
  built to catch exactly this action. (`read_link`'s join to the issuer is a
  `LEFT JOIN` for this reason — an inner one made a console link read as
  `Unknown`, which is the worst possible answer: a valid token, on a page
  telling you to ask an admin, when being unable to reach one is why you ran
  the command.)
- **Only a reset, never an invite.** Deciding who joins the household is the
  admin's call, not the operator's, and a `CHECK` enforces it: a row with no
  `created_by` must name a `member_id`.
- **Access to the binary and the database file is the authority here**, and it
  grants nothing that reading `ffm.db` directly did not already. It is not a
  privilege escalation, it is the same privilege made usable.
- **The name is exact**, matched against `members.name` — the identifier
  people sign in with. Resolving `rod` to the nearest match would be the wrong
  kind of helpful when the thing being reset is an account; an unknown name
  lists the household's members instead.
- **Argument parsing is hand-rolled**, no argument-parser dependency: there
  are two commands and one argument between them, and `ffm --help` is a
  `const &str`.

---

### Signing in — `/login`

Two fields, `Name` and `Password`, in that order, with `Name` focused on load.
The card's heading is the household name and its subtitle says what this is.

- **One error message for both a wrong name and a wrong password**: *"That name
  and password don't match. Check the name, or ask an admin for a new invite
  link."* Not security theatre — this document already declines rate limiting on
  the grounds that the network is the boundary. It is that "we have no rodrigo"
  and "wrong password" lead to the same next action, and the copy rule says tell
  people what to do about it. On failure the name is kept, the password cleared,
  and focus returns to the password field.
- **No colour-scheme picker here.** A palette chosen by an anonymous visitor has
  nowhere to live, and a returning member's real palette is in their settings
  row and would override it three seconds later. Language and light/dark only —
  both of which genuinely apply to this page. `/setup` and `/invite` *do* get
  the full picker, because on those two pages it is a form field writing
  `member_settings`, not chrome.
- **On a phone the card is not vertically centred.** It sits at `12vh` from the
  top: centring a card in the viewport puts it underneath the software keyboard
  the moment a field takes focus.

---

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
  leak a hidden row's existence by not adding up. Admin-only settings become
  inaccessible — `:household` and `:currency` refuse, and `:members` with them —
  and the **Household** button disappears from the toolbar rather than sitting
  there disabled, since a real non-admin never sees a control for a pane they
  can't open.
- **What doesn't change**: the admin's own pane, own rows, own private
  transactions — ownership already grants visibility independent of role,
  same as it does for every other member.
- **`j`/`k` (and any other row-stepping motion) skip a hidden row rather than
  landing on it** — the same mechanism a closed `:group bucket` fold already
  uses to keep the gutter and focus index correct while stepping over rows
  that aren't rendered. A hidden row is skipped, never a dead stop.

---

## Income

Income has always been in the ledger — `kind = 'in'`, a positive amount,
summed into the pane header — but without a rule, recurrence or files. It
now uses the same recurring-rule machinery as a bill, pointed the other way:
`I` creates one, `E` edits it, `i` adds a one-off.

### Its own axis: arrival, not payment

A bill's status column answers *did the money leave*. Income answers *did it
arrive*, with two states of its own (see the row-state table above):

- **`expected`** — not yet received. The mark is the identical elapsed-time
  ring a bill's due date drives, counting toward the rule's **expected day**
  instead, with the same green → amber → red arc. Past the expected day it
  goes fully red, and a tracked one counts toward the unresolved-issues
  badge exactly as an overdue bill does.
- **`received`** — the money landed: the filled positive disc income rows
  already used, plus the date.

Income never shows `paid`, `scheduled` or `scheduled overdue`. Those are
spend states and stay that way.

### Confirming it needs no new verb

- **Tracked income is confirmed by its payslip on disk** — the same single
  `exists()` check, the same absolute "no manual tick for a tracked rule".
- **Untracked income uses the manual tick** untracked rules already get,
  relabelled *"Mark as received manually"*. Same one exception, same reason:
  there is no filesystem evidence to contradict it.

### A varying amount is projected

A rule with **Amount varies** set can't seed a period from its own number —
a commission or a freelance invoice lands differently every month. Until the
period is confirmed, the row shows an **estimate** derived from history.

- **Basis is per rule**: `lowest` | `average` | `last`, over the last N
  confirmed periods (default N = 3). **`lowest` is the default**, on
  purpose: for income the dangerous error is over-estimating, which produces
  a shortfall the household then has to absorb. Bills are the mirror image,
  which is why their carry-forward default points the other way.
- **Only confirmed periods count.** An unconfirmed month is skipped, never
  folded in — two unconfirmed periods back to back would otherwise project a
  guess from another guess.
- **A rule's first occurrence has nothing behind it** and falls back to the
  rule's own stated amount — the identical carve-out bills already have.
- **The estimate is computed, never stored.** What persists is the rule's
  baseline or the real confirmed figure; the projection is derived on read,
  so it follows new history rather than freezing an old guess, and it never
  writes a spurious "amount changed" entry into the change log.

### Saying so, everywhere the number goes

- The row reads `~R$ 4.133,33` in a quieter colour. The mark carries whether
  the money moved; the tilde carries whether the number is a fact — one
  channel per fact, as everywhere else in this document.
- **The pane's INs and Balance, and the household's IN and NET, all carry
  the `~`** when any component is a projection. Leaving projections out of
  the totals would make a future month's NET useless; putting them in
  unmarked would present a guess as a fact.
- **The MoM delta inverts.** The arrow is direction, the colour is whether
  it's good news: more money out is bad, more money in is good, so a raise
  never paints itself red. A recurring income gets a delta; a one-off and an
  unconfirmed projection don't — the latter would compare a guess to a fact.

### Where it sits, and what it's called

- **A new income lands at the top of its pane** — where a household reads
  what came in. That's a starting sort key, not a pin: `J`/`K` move it
  anywhere afterwards, and `:sort date`/`amount` treat it like any other
  row. A hard "income always floats" tier was rejected: it would contradict
  being able to move it, and it would quietly override an explicit sort.
- **The form is the same field grid with income wording** — `bill_name` is
  labelled **Source**, the due date **Expected day**, installments
  **Payments**. Same columns, same computed path
  (`bucket / source / year / month.ext`); no parallel fields.
- **The whole barcode apparatus is absent.** No "no barcode file" switch, no
  client code, no dropzone, no multi-page window — money arriving has no
  boleto to scan. See `design/spec-bill-import.md`, which is spend-only.
- **The estimate box mirrors the computed-path box**: the projected figure
  bright on the first line, the months it came from dim on the second, both
  recomputing live as the basis or window changes — before anything is
  saved, exactly like the receipt path.

---

## Change log (`gl`)

Every change to a transaction, a rule or a file is recorded. `gl` on the
focused row opens its history in a side pane — the same 420px column, arrow
and cursor-follows behaviour the `A`/`I`/`E` rule editor uses, so nothing new has
to be learned to read it.

### Tables

Two, not one. A single wide table would either repeat the actor and
timestamp once per changed field, or bury the diff in a JSON blob nothing can
query.

```
change              -- one row per user action ("a save")
  id                INTEGER PK
  at                TEXT     -- ISO timestamp
  member_id         INTEGER  -- who did it: the session's member, never the focused pane
  source            TEXT     -- 'user' | 'import' | 'materializer' | 'system'
  action            TEXT     -- create · edit · delete · restore · reattribute ·
                             -- reorder · privacy · rule_create · rule_edit ·
                             -- file_attach · file_unlink · file_upload ·
                             -- file_delete · import · materialize ·
                             -- password_reset · sessions_revoked
  entity            TEXT     -- 'transaction' | 'rule' | 'file' | 'member'
  entity_id         INTEGER
  rule_id           INTEGER  -- nullable; set on transaction changes so a rule's
                             -- whole history is one query
  period            TEXT     -- 'mm/yyyy'; null for rule-level changes
  undo_of           INTEGER  -- nullable FK -> change.id, for an undone action
  note              TEXT     -- nullable, one line ('superseded by receipt upload')

change_field        -- one row per field actually touched; none for delete/restore
  change_id         INTEGER FK
  field             TEXT     -- 'amount' | 'title' | 'bucket' | 'instance' | ...
  old_value         TEXT     -- storage-neutral: minor units for money, ISO dates
  new_value         TEXT
  kind              TEXT     -- 'money' | 'date' | 'day' | 'bool' | 'text' | 'path'
```

- **`kind` travels with the value**, so the pane formats old/new the way the
  rest of the app does without inferring from the field name — and a stored
  amount never depends on the household's *current* currency setting,
  the same reasoning the receipt filename's plain amount follows.
- **`rule_id` on transaction changes** makes "this row plus the rule behind
  it" one query, which is exactly what the pane shows.
- **`undo_of` makes an undo a first-class entry**, not a silent rewrite: the
  log reads "deleted", then "restored · undo of 18:39".
- **Append-only.** Never updated, never deleted from — the same rule the
  ledger itself follows. It is the app's one audit mechanism; the file
  upload/soft-delete records described under Receipt tracking are entries in
  it, not a parallel table.
- **`entity = 'member'` covers account actions, not ledger ones** —
  `password_reset` and `sessions_revoked`, where `entity_id` is the member
  acted on and `member_id` is the admin who acted. These are the one place an
  admin can affect somebody else's ability to sign in, so they belong in the
  same append-only record as everything else rather than in a side channel.
  They carry no `change_field` rows: a password has no old value worth
  recording, and writing one down would be worse than useless.

### The pane

```
History · Energia · Enel SP · 09/2026        [ this row | rule | all ]
---------------------------------------------
today
  14:32  Rodrigo   edited · 3 fields                     >
           amount     R$ 340,66 -> R$ 352,10
           bucket     Consumption -> Utilities
           due day    15 -> 18                     (rule)
  14:05  Rodrigo   barcode attached · 1 field
           client code  -- -> 325124615-7
  09:12  system    materialized · 09/2026
yesterday
  18:40  Ana       re-attributed
  18:39  Rodrigo   restored                 undo of 18:39
  18:39  Rodrigo   deleted
```

- **Grouped by day, newest first.** Time · actor · verb on the entry line,
  plus a field count when there is a diff to open.
- **Rule-level entries sit inline in date order**, tagged `rule`, so the
  story reads chronologically; the chips narrow to `this row` / `rule` /
  `all`. A file entry (attach, unlink, upload, delete) counts as the row's
  own history — it belongs to that occurrence, it just isn't the transaction
  record itself.
- **Expanding an entry** shows `old -> new` per field, formatted by `kind`,
  with an em dash for empty.
- **Read-only.** No drafts, no save. `Esc` closes; `Ctrl-h` hands focus back
  to the ledger (matching the rule pane's left-edge behaviour) with the pane
  still open until the cursor actually moves; `j`/`k` walk the entries and
  `Enter` toggles one.
- **It closes when the cursor leaves the row** — the same single check in
  the render loop that parks an open edit draft.
- **A row with nothing recorded still opens the pane**, saying so. An empty
  history is an answer, not an error worth refusing on.

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
manual one — a `file_delete` entry in the change log (below) carrying
`old_path`, `deleted_at`, `deleted_by` — plus a `reason` of `superseded`
rather than `manual`, so `u` on the receipt upload restores the scheduled
file too and the audit trail is honest about why it moved.

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
app avoids everywhere else (months as views rather than copies, one row per
member for a personal preference rather than one shared file, and so on). Checking is now a single `exists()` call per kind, on one
exact path per occurrence — no directory walk, since the path is computed, not
searched for. This also fully retires the earlier concern about excluding
`.delete/` from a traversal: there's no traversal left to exclude it from.

A DB row still accompanies every upload — uploader, upload time, kind
(`receipt` | `scheduled` | `barcode`), original filename as typed by the user,
extension — but purely as metadata/audit for undo, recorded as a
`file_upload` entry in the change log (below) rather than in a separate
upload table. It does not decide paid/not-paid; the disk does.

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
  change log records the move (`old_path`, `deleted_at`, `deleted_by`,
  `reason`) so `u` restores it exactly — the same one audit mechanism every
  other change goes through, not a second one for files.

### Attaching a file — three gestures, one upload path

Phase 7 built the server side — three endpoints, one per kind, with the
supersede and the soft delete. What was left open was the gesture. There are
three, and they all end in the same request:

- **The `+` control on the focused row's file rail.** The rail still shows
  only files that *exist* — no ghost placeholders — so attaching lives behind
  one control rather than three empty slots pretending to be files. Clicking
  it opens a menu of the three kinds, anchored to the control itself because
  the ledger scrolls sideways and a centred menu would point at nothing.
- **Dropping a file on the row.** A row drop infers nothing: the row is
  identified by where you let go. What a drop cannot say by itself is *which
  of the three kinds* it is, and guessing that from the filename is exactly
  what a panel drop was downgraded to a notifier for — so while a file is
  over the ledger, every attachable row's rail splits into three aimed
  targets (`1` `2` `3`, the same numbers), and the drop carries the answer.
  The targets are rendered always and revealed by CSS off a class on the
  shell; built on demand they would flicker into place under a pointer
  already moving toward them.
- **`gu1` / `gu2` / `gu3`** — straight to that slot, no menu.

**Three distinct actions, never one upload plus a question**, in all three
gestures: each kind has its own fixed name on disk and its own effect on
state, so asking afterwards would be asking about something already decided.

**An income row has no slot 2** in any of them — no menu entry, no drop
target, and `gu2` refuses with the message `g2` already gives.

### Bill import — attached inside the rule's own form

Binding a barcode file to a rule is an explicit action in the rule's own form — never
inferred by matching a dropped file against existing rows. An earlier design
had a panel drop parse the file and guess which rule it belonged to (by
issuer/period/value proximity, or by matching the dropped filename against a
row's title); both were rejected as too fragile — a real bill's filename or
content shouldn't have to line up with what's already in the ledger for the
household to trust the result. Full parsing design (FEBRABAN barcode
decoding, issuer profiles) lives in `design/spec-bill-import.md`; what's
UI-facing:

- **The only place a barcode file is ever attached is the rule's own
  dropzone** — `A`'s upload field at creation, or `E`'s afterward. Both are
  scoped to one specific, already-identified rule, so there's no matching
  left to get wrong; see Edit fields, above, for the dropzone and its
  read-only Client code / Unlink controls.
- **Dropping a file onto a member's panel binds nothing.** It's downgraded
  to a passive nudge: if that panel has recurring, tracked rules still
  missing a barcode (and not marked "no barcode file"), the statusline
  names them and points at `E`; otherwise the dropped file is discarded
  with no message at all. The panel still decides member attribution for
  a *new* rule (`A`/`I`'s form is created against whichever panel is
  focused, same as `a`/`i`/`:add`) — it just never decides which rule a
  dropped file belongs to, because a drop no longer resolves to a rule at
  all.
- **A successfully-attached file may carry a `client_code`** — the
  subscriber's own account number, read from the bill's text (never the
  barcode). It shows up read-only in the rule editor once a rule has one;
  see Edit fields, above.

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
household-wide setting — same reasoning as theme and line numbers: it's taste,
not household policy, and two members can disagree about it without either being
wrong.

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

## Configuration — three tiers

Split by who legitimately needs to change what. This is a **reversal** of the
earlier two-tier split: `ffm.toml` used to hold layout, ledger, privacy and
locale settings as well, and those have all moved into the database.

| tier | where | who | what |
|---|---|---|---|
| **operator** | `ffm.toml`, or env vars | whoever runs the process | `db_path`, `receipts_root`, `backup_dir` |
| **household** | `household_settings`, one row | admin only, via `:household` | name, currency, default language, default visibility, `show_recurring` |
| **member** | `member_settings`, one row per member | each member, via `:profile` | language, theme, line numbers, layout, ledger view, own privacy default |

Why it ended up this way: the old `ffm.toml` rule — *the file writes back and is
the single source of truth* — only holds if there is one person per file. Theme
is taste, not process config, and Natália's `:theme rosepine` overwriting
Rodrigo's `:theme nord` in a shared file is the file **disagreeing with itself**:
exactly the failure mode the single-source-of-truth rule exists to prevent. Once
that argument is admitted for theme it applies unchanged to line numbers, sort,
search scope, the delta toggle and every layout threshold — all of which
describe *a person at a screen*, not a process. A `member_id`-keyed row sidesteps
the whole problem, and SQLite already serialises concurrent writes where a file
does not.

What stayed household-wide is what cannot coherently differ between members:
currency, because every member's transactions feed the same shared totals; and
`show_recurring`, because that setting means "we don't use this concept here",
not "mute it for me".

### What's left in `ffm.toml` — the entire file

```toml
[storage]
db_path       = "./ffm.db"
receipts_root = "/disks/vault/ffm/receipts"   # per-member folders generated under here
backup_dir    = "./backups"
```

Each key is also readable from `FFM_DB_PATH` / `FFM_RECEIPTS_ROOT` /
`FFM_BACKUP_DIR`, with the environment winning, so a container deployment needs
no file at all.

**These three are read once at startup and are not watched.** The earlier
`notify`-watched-at-runtime design is dropped, because it is now false for
everything left in the file: `db_path` cannot change under a running process
without swapping the database out from beneath open transactions, and
`receipts_root` cannot move without relocating the whole vault. The process
validates them at boot — the receipts root must exist and be writable — and
exits with a clear message if it can't.

`db_path` in particular **cannot** live in the database, for the obvious reason:
you can't read a setting out of a database you haven't opened yet. That is the
whole justification for this tier continuing to exist.

### The single-source-of-truth rule — still holds, and what it actually forbids

**No setting is changeable in two places that could disagree.** `:set`, `:sort`,
`:theme`, `:currency`, `:lang` and the clickable statusline cells all write
directly into the one row that owns the setting, and the `:household` /
`:profile` panes render controls over those same rows.

The older wording of this rule — *"there is no settings dialog for either
tier"* — needs restating, because a form in a vsplit looks exactly like the thing
it appeared to forbid. What the rule protects, in its own words, is that *"if a
setting could be changed in two places that disagree, that tier stops being the
source of truth for it"*. The prohibited thing is **duplication**, not a form:

- **The duplication that motivated it is gone.** The `ffm.toml` textarea was the
  source of truth for `layout`/`ledger`/`privacy`/`locale`, and `:set`/`:theme`
  and the statusline cells wrote *into that text* precisely so there would only
  ever be one copy. Those keys are now columns. A form, a command and a
  statusline cell are three gestures writing one row.
- **What keeps these panes from being dialogs in spirit is that they have no
  Save button.** Every control writes on `change`, the pane's footer confirms,
  and the statusline echoes. There is no pending state, so there is no moment
  when the form and the row disagree. A Save/Cancel pair would manufacture
  exactly that moment — **that** is what this rule should be read as forbidding.
- **The `ffm.toml` textarea is retired, not relocated.** For the three keys that
  remain, a live-applying editor would be a lie: it would accept input and
  silently not apply it, which is worse than any dialog. They are shown
  **read-only** at the foot of `:household` and on `/setup`, with their resolved
  absolute paths and a line saying to edit the file and restart.

### Surface

All three tiers are read in a **vertical split** beside the ledger — not a modal,
not a full-screen buffer:

- `:household` (admin only) — the household row as a form, with the operator's
  three paths read-only beneath it.
- `:profile` (or `:me`) — the **session's** `member_settings` as a form. Never
  the focused pane's: focusing your spouse's pane must not let you edit their
  preferences.
- `:config` survives as an **alias for `:household`**, so muscle memory and the
  `?` keymap entry don't break. `:household` is the real name; nothing is called
  after a file any more.

Reasons a vsplit at all, so this is not relitigated:

- A full-screen buffer hides the ledger, and `layout.panes` is precisely the
  setting you need to *watch* take effect.
- A modal needs a backdrop, scroll-lock and a focus trap, and it implies a
  settings dialog with an OK button — a second, pending copy of the truth.
- A vsplit is just a second pane. `H`/`L` already means "move between panes". In
  HTMX it is one swap changing the grid from `1fr` to `360px 1fr`.

Both panes frame their groups with the existing `.card` visual language, so a
settings group reads like a ledger group rather than like a new widget.

### Invalid config

- **`ffm.toml`, at startup**: say which line is wrong and refuse to start.
  Format: `✗ line 3 · storage.receipts_root does not exist — /disks/vault/ffm`.
  Serde reports the failing field; wire it to the line. The older "keep the last
  valid config, don't fall back to defaults mid-session" rule no longer has a
  mid-session case to protect, since the file isn't watched — but the *reason*
  behind it still applies to the DB tiers.
- **`household_settings` / `member_settings`, on submit**: per-field validation,
  the field keeps its previous value, and the pane's footer says what was wrong
  in the same `✗ …` dialect. Every constraint is also a `CHECK` in the schema, so
  a bad value cannot reach the table even if a handler forgets.

### Schema — `household_settings` (one row, admin-only)

```
name                  text     -- shown on /login and in the statusline
locale_currency       text     -- "BRL" | "USD" | "EUR" | "GBP" — see below
default_language      text     -- "en" | "pt-BR"; used pre-login and for a
                               -- member who hasn't chosen
default_visibility    text     -- "household" | "private"; a member may override
ledger_show_recurring bool     -- false = "we don't use this concept here"
```

`CHECK (id = 1)` in the migration makes "there is one household" a schema fact
rather than a convention.

### Schema — `member_settings` (per member, DB-backed)

```
language                text  -- "en" | "pt-BR". Words only; see Language & copy
theme_name              text  -- any of the 11 bundled palettes
theme_overrides         json  -- open-ended: any of the 11 tokens, same
                              -- #[serde(flatten)] HashMap<ThemeToken, Color> as
                              -- before, decoded from JSON instead of TOML
line_numbers            text  -- "relative" | "absolute" | "none"
layout_panes            text  -- "auto" | "split" | "tabs"
layout_min_split_width  int   -- px; below this, auto → tabs
layout_max_width        int   -- px cap per pane; 0 = unlimited
layout_gap              int   -- px between panes
ledger_sort             text  -- "manual" | "date" | "amount"
ledger_search_scope     text  -- "pane" | "household"
ledger_show_delta       bool
ledger_group            text  -- "none" | "bucket"
group_fold_default      text  -- "expanded" | "collapsed"
default_visibility      text  -- NULL = follow the household's policy,
                              -- else "household" | "private"
```

- `theme_overrides` stays the one **open-ended** field: a strongly-typed key
  enum still makes an unknown token a real error naming the valid tokens — only
  the source format changed (JSON column vs. TOML file), not the validation.
- `default_visibility`'s nullable tri-state is rendered honestly in `:profile`:
  three radios, the first meaning `NULL` and labelled with what it currently
  resolves to.
- **`layout_min_split_width` is still rendered into a single media query
  server-side**, from the session member's row instead of from the file. The rule
  is unchanged — `auto` resolves in CSS, never on the server and never in Alpine
  — only the source of the number moved. The pre-auth `.auth` layout has no
  member row and must not read any `layout_*` value.
- **Light/dark is *not* in this table.** Palette is a personal preference and
  belongs to the member; light-versus-dark is a property of the *device* — the
  same person uses a bright laptop and a dark phone — so it lives in the
  `ffm.theme` cookie. Keeping the two separate is what makes a pre-auth page
  themeable at all: `/login` can honour your light/dark without knowing who you
  are.

### Currency (`household_settings.locale_currency`)

Household-wide, admin-only — deliberately not in `member_settings` next to
theme. Every member's transactions feed the same shared totals (INs/OUTs/NET,
balances), so a per-member currency would mean either fake, unbacked
exchange-rate conversion between members' views, or two members seeing
different-looking numbers for the exact same total. One value on the household
row is what keeps that coherent — see `CLAUDE.md`'s Conventions section for the
fuller reasoning.

- **A closed set, not a free-typed code.** `BRL` | `USD` | `EUR` | `GBP`
  today, the same "closed set, real published values" posture as the
  bundled themes — extendable later, not user-invented.
- **Choosing a currency chooses its whole conventional number format as one
  preset** — symbol, decimal separator, thousands separator, all bundled.
  Picking `USD` switches the household to dot-decimal/comma-thousands
  (`$1,234.56`), not just the glyph; there's no "keep comma-decimal but
  show `$`" combination on offer, because that isn't how any of these
  currencies are actually written. `/setup`'s currency options therefore show
  the format rather than the ISO code: `Real · R$ 1.234,56`.
- **Dates are unaffected**, and so is language. `dd/mm/yyyy` stays fixed
  regardless of currency, and the UI language has no bearing on number format —
  three independent axes, never fused. See Language & copy, below.
- **It is set on `/setup` and edited in `:household`.** The earlier claim that
  currency "isn't a signup-flow setting, because there is no self-registration"
  no longer holds: there is a first-run setup page now, and the currency has to
  be chosen before the first amount is typed.
- **The edit-occurrence form's amount field is a live mask keyed off this
  same value** — the user only ever types digits (see Edit fields,
  above); the decimal/thousands separators it inserts, and the static
  currency-symbol prefix shown beside the field, both come from whichever
  currency is currently configured. Every other money display in the app
  (row amounts, pane totals, the household header) reads the same single
  value, so there's exactly one place that ever needs to change if this
  setting changes — nothing about currency formatting is hardcoded
  per call site.

---

## Language & copy (i18n)

The app ships **English and Brazilian Portuguese**, English by default. This
replaces the earlier "i18n support planned but not built yet" placeholder.

### Three independent axes, never fused

This is the trap the whole design is shaped around:

| axis | source | changes |
|---|---|---|
| date format | **fixed** | nothing. Always `dd/mm/yyyy` |
| money format | `household_settings.locale_currency` | symbol, decimal and thousands separators |
| language | `member_settings.language` | **the words, and only the words** |

A `pt-BR` UI in a `USD` household shows Portuguese copy and `$1,234.56`. A member
switching to English does not move a single decimal point. There is no "locale"
in this app that bundles the three together, and adding one would break the
household-wide-currency argument above the moment two members disagreed.

### Catalogs: Fluent files, compile-time generated accessors

Source of truth is `i18n/en.ftl` and `i18n/pt-BR.ftl`, with `en` as the
reference locale.

The hard constraint is this project's compile-time-safety invariant: *a backend
change that breaks the UI must fail `cargo build`, not surface at runtime.* A
template calling `t("login.password")` fails at **runtime** — a string key is
exactly the kind of unchecked reference this codebase refuses everywhere else.
But that is an argument about the **lookup**, not the file format, so the two are
separated:

**`build.rs` parses both `.ftl` files with `fluent-syntax` and generates a typed
accessor.** For each message id in `en.ftl` it emits one method on `Catalog`,
kebab-case → snake_case, with a parameter per placeable:

```rust
impl Catalog {
    pub fn login_error_mismatch(&self) -> Cow<'static, str>;
    pub fn ledger_sort_refuses(&self, sort: &str) -> String;
    pub fn rows_moved(&self, n: i64, dir: &str) -> String;
}
```

- **The build fails** on an id present in one locale and missing from the other,
  on a placeable set that differs between locales for the same id, and on a
  duplicate id. `cargo:rerun-if-changed` on `i18n/`.
- Messages with no selector compile to a plain `match self.lang { … }` — no
  runtime catalog, no ICU machinery, and a borrowed `&'static str` for the
  constant case. Only messages that actually use a plural selector fall back to
  a lazily-built `FluentBundle`, where Fluent's CLDR rules do the work properly.
- Arguments are typed by convention: `$n` / `$count` → `i64`; everything else →
  `&str`, because money and dates arrive **already formatted**.

Askama templates then call methods — `{{ t.login_password() }}` — and Askama
type-checks method existence and arity against the struct at build time. Renaming
a message id breaks `cargo build` in every template that used it, which is the
invariant restored.

**Rejected, with reasons, so this isn't relitigated:**

- **`fluent-templates`** — convenient, but its macro resolves ids at runtime,
  reintroducing precisely the failure mode being designed out.
- **gettext `.po`** — the msgid *is* the English sentence, so every copy tweak
  invalidates the translation, and `xgettext`/`msgmerge` becomes a build-chain
  dependency for a two-locale household app.
- **Plain TOML/JSON key-value** — nearly right, since code is being generated
  either way, but it loses CLDR plural selectors, and a hand-rolled selector
  syntax would be invented the first time a count appeared. Fluent's syntax is
  what we would end up writing; `fluent-syntax` is the parser.
- **A compile-time `t!("key")` proc-macro** — genuinely compile-safe, but worse
  go-to-definition, worse error messages, and no way for Askama to check argument
  types. Generated methods dominate it.

### The build enforces "language never touches numbers"

`build.rs` **rejects any `.ftl`** containing Fluent's `NUMBER()` or `DATETIME()`
builtins, a literal `dd/mm/yyyy`-shaped date pattern, or a currency glyph. Money
and dates are formatted by `fmt::money(cents, Currency)` and
`fmt::date(NaiveDate)` — neither of which takes a language — and their output is
passed *into* a message as a `&str`. The catalog is structurally incapable of
localising a number, rather than merely discouraged from it.

### Where the pre-login choice lives: a cookie

**A cookie, not `localStorage`.** The server renders the page, so the language
has to be known before the first byte. `localStorage` is readable only after JS
runs, which leaves two bad options: render English and have JS re-translate the
DOM — a client-side data model, forbidden outright — or have JS read storage and
trigger a reload, which is a flash of the wrong language plus a round trip on
every visit. A cookie arrives **with the request**, so the first byte is already
correct with zero JS. This is the identical argument this project already makes
for resolving `layout = auto` in CSS rather than on the server or in Alpine:
correct before the first frame, no round trip.

- `ffm.lang`, `Path=/`, `SameSite=Lax`, `Max-Age=31536000`, `HttpOnly` — only the
  server ever reads it, so `HttpOnly` is strictly better and costs nothing.
- `ffm.theme` carries light/dark/system the same way, which is what removes the
  last need for a pre-paint inline script.
- **Pre-login resolution**: `ffm.lang` → `Accept-Language` (parsed with quality
  values, matched against the supported set) → `household_settings.default_language`
  → `en`.
- **Post-login `member_settings.language` is authoritative, full stop.** The
  cookie is refreshed to match at session creation and whenever the two
  disagree, so the next visit's login page greets you in your own language. It
  is never *read* on an authenticated request.
- **The switcher is a plain form.** `<form method="post" action="/lang">` with
  one submit button per language and a hidden `next` carrying the current path;
  the handler sets the cookie and redirects. It works with JS disabled and with
  HTMX absent. Post-login there is no chrome switcher — language lives in
  `:profile` and in `:lang en|pt-BR`, which writes the row, refreshes the cookie
  and responds `HX-Refresh: true`, because every string on the page changed and a
  partial swap would be dishonest about that.

### What needs real plurals

Not a hypothetical — the existing copy already needs them, which is why a flat
key-value file was rejected:

| need | existing copy |
|---|---|
| cardinal plural | `2 rows moved down` versus a single named row |
| a count of drafts | the statusline's parked-`unsaved` count |
| two counts in one message | *"page 11 — outside this file's 10 pages"* |
| user content interpolated | `"{title}" moved down`, `not a command: {cmd}` |
| identifiers interpolated | `ledger.sort = "date" — reorder needs sort = "manual"` |
| pre-formatted money/date | `~R$ 4.133,33`, `expires 27/09/2026` |
| a ratio, not a plural | `7/12` installments, `3/7` search matches — plain mono text, no message at all |

Two rules that fall out of that table:

- **Interpolated user content stays HTML-escaped.** Askama escapes `{{ }}` by
  default; `|safe` is never applied to catalog output, or a bill titled with a
  `<script>` tag becomes one.
- **Interpolated setting values are identifiers, not words.** `manual` and
  `date` in the sort-refusal message are the literal config values a member
  would type back; the sentence around them is translated, they are not.

### Seed data is household content, not app copy

The existing rule survives verbatim and gets sharper now that there is a real
catalog to contrast it against: *"Água · Sabesp"*, *"Mesada"*, bucket labels like
*"Consumption"* are **what a Brazilian household typed into its own ledger**.
They never pass through the catalog, they stay in Portuguese when the UI language
is English, and they must not appear in any `.ftl` file.

The corollary makes it testable: the dev-only UI sandbox router renders every
template **in both locales**. That is the same "this is how the table stays
honest" argument already made for the row-state table, applied to the catalog —
a message that only reads correctly in English is found by looking, not by
waiting for a household to complain.

---
