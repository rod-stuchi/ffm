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
| `scheduled overdue` | warning-icon glyph, warning color | tracked, scheduled-payment file exists **past** due date, no receipt yet — an unconfirmed promise |
| `pending` | ring, filling green → amber → red | tracked, neither a receipt nor a scheduled-payment file exists yet |
| `plain` | hollow dot | untracked, not marked paid — no filesystem proof either way |
| `manual paid` | filled disc, distinct shade from `paid` | untracked — user asserted paid, no file to prove it |
| income | filled dot, `+ R$`, positive color | sums into the pane header |
| focused | accent left rail, gutter `0`, tinted ground | cursor position |
| selected | `--info` inset bar | `x` — a marker for a batch op, never a delete |
| private | lock-icon glyph, full color when set, faint when not | hidden from other members, never from admin |
| deleted | dimmed, strikethrough, deletion date | soft delete, row retained; `u` clears the mark on this same row |
| skeleton | shimmer bars, **identical geometry** | in flight |
| busy | dimmed, the logo's three houses lit in turn over it | a request about this row is in flight — see The loader, below |
| error | red ground, message + **Try Again** | server said no, nothing was written |
| editing | the row opens into its own form, Cancel / Save at its foot | `e` — see Edit fields |
| rule open | accent outline, the rule pane's arrow pointing at it | `E` — see Edit fields |
| unsaved | dashed accent border + `unsaved · e to resume` tag | a draft left behind when the cursor moved on |
| placeholder | dashed, empty "New recurring bill / income" row | `A` / `I` — where the new rule's row will land |
| expected | ring toward the expected day, `+` at its center | income, not yet received — the same elapsed-time arc a bill's due date drives |
| received | filled positive disc + received date | income that landed |
| estimated | `~R$ …` in a quieter color, no delta | a varying income's amount is projected from history until confirmed — the mark still carries arrival, the tilde carries "this number isn't a fact yet" |
| orphaned | the bill's own mark and files, ↻ dim, popup says *rule deleted on {date}* in place of the cadence | its rule was deleted. Deleting a rule stops future periods and never rewrites recorded ones: the row keeps `rule_id`, its files (they live at the rule's taxonomy) and its tracked status, and still counts toward the unresolved badge — it only stops claiming to recur. `E` says there is nothing to open |

**`?` shows every mark beside what it means**, under a *Marks* section in the
keymap modal. The samples are drawn by `partials/mark.html` — the partial the
row itself includes — from real `Mark` values, so the legend cannot show a
mark the ledger does not draw. The words are catalog messages; the `~R$`
sample goes through `fmt::money`.

### Density

`member_settings.density`, picked in `:profile` from three cards that each
draw what they do — Gmail's picker. Written onto the shell as
`data-density`; `row.css` holds the three presets as one set of custom
properties (`--row-py`, `--entry-py`, `--gut-pt`), and that is the one place
they live.

| density | between rows | a row | the file rail |
|---|---|---|---|
| `default` | 6px | taller | every empty slot as its own `+1` / `+2` / `+3`, dashed, in its numbered place — straight to that slot's picker, as `gu1` is |
| `comfortable` | 2px | as the ledger always was | only the files that exist, and the `+` menu on the focused row |
| `compact` | none | one line: the meta line goes, the delta sits beside the amount, and the rows touch with a hairline between | as `comfortable` |

- **`default` is a value**, Gmail's name for its roomiest — it never means
  "follow the household". Everybody, existing members included, starts on
  `comfortable`, which is exactly the screen they already had.
- **Compact gives up the meta line and nothing else.** The mark carries the
  state, the rail the files, the amount the money; a parked draft still shows
  by its dashed border. What it loses is the second reading — the due date,
  the paid date, `7/12` — which is what makes it compact.
- **The skeleton reads the same numbers**, and drops its second bar in
  compact, so its geometry still matches the real row at every density.
- **The `+` slots are rendered at every density and shown by CSS**, so the
  density is the attribute and nothing else. A read-only pane shows none, and
  a file in the air hides them for the drop targets — the rules the `+`
  already follows.
- **Writing it refreshes the page**, like every layout value: the server
  renders the attribute, so the first frame is already right.
  `?split=profile` keeps the pane open across it.
- **The household seeds it** (`default_density`, starting at `comfortable`),
  with the palette and the layout — see the household schema.

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
- **Cancel and Save are their own row, at the right edge, Save last, a size down.** They
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
| busy | the loader, while a request with no row or pane to show itself on is in flight — a setting, `u`, a settings pane opening. After the message, the one cell that grows, so its coming and going moves nothing to its right |
| selection | `x`: `3 of 5 selected · Σ R$ 588,20` — how many of the **focused pane's** rows are selected, and their sum written like the pane header (`Σ + …` for incomes only; `in · out · net` for a mix, `~` on a projection). Rendered by the server from the same pane query as the header; hidden while nothing in the pane is selected |
| buffer | the pending count and key prefix, so a half-typed `3d` or `g` is visible rather than silently remembered |
| position | `4/7` in the focused pane |
| sort | `manual` · `date` · `amount` — **clickable, cycles**, the same as `s` |
| group order | `First seen` · `A–Z` · `By cost` — only while the pane is grouped; **clickable, cycles**, the same as `S` |
| line numbers | `rnu` · `nu` · `nonu` — **clickable, cycles** |
| version | the running build — `v0.0.3`, or `v0.0.4-dev` on a dev build. Last, quiet, and **kept at every width**, where the cells before it step aside below 700px: a phone is where "which version is this?" gets asked |

**On a phone the household and the member cells give way** with an ellipsis,
while the mode badge and the version keep their size. They did not fit beside
the badge on their own — 404px of cells on a 400px screen — so the bar used to
run off the edge before the version was ever added.

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

**One table, and everything that names a command reads it.**
`web::commands::ALL` holds each command's name, aliases, who is offered it,
the closed set its argument comes from and a catalog message saying what it
does. The bar's resting hint, Tab completion, the contextual hint and `?`'s
Type section are all rendered from it — `?` had kept advertising
`:add 50 mercado` long after `:add` was abandoned, because it was a fifth
hand-kept list. An argument's values come from the same constants its write
validates against (`ThemeName::ALL`, `Currency::ALL`,
`settings::LEDGER_SORTS`, …), so Tab cannot complete to a value the server
then refuses.

**The bar teaches itself.**

- **The hint follows what you type.** At rest it is the vocabulary. With a
  command typed it says what that command does and the values it takes —
  `:set — line numbers: relative · absolute · off · rnu | nu | nonu` — and
  with part of one, the commands that start that way.
- **Tab completes** a command's name, then its argument, from the table:
  one candidate is filled in; several are cycled Vim-wildmenu style, the one
  on show lit in the hint, Shift-Tab going back. A name that takes an
  argument gets its space, so the next Tab completes that. Only closed sets
  are completed — `:grep`'s term is anybody's guess.
- **Ctrl-p / Ctrl-n** (and the arrows) walk the last 30 commands, newest
  first, and what was typed before the walk began is a prefix, as in Vim:
  `:th` then Ctrl-p steps through past `:theme` lines only. Refused commands
  are kept too — the usual reason to recall one is to fix it. The history is
  per member, in this browser (`localStorage`): it is a convenience, like a
  shell's, and two people on one laptop keep their own. Outside the bar
  Ctrl-p means nothing: the months are `[` / `]`.

**Admin-only commands are absent for a non-admin**, and for an admin
previewing as one — the same rule that removes the Household button rather
than disabling it. `preview` is the exception and follows the *role* rather
than whether its powers are in effect: it is the toggle itself, the same
category as the badge, and dropping it while previewing would say there is no
way back.

### The loader

On a slow link every gesture that asks the server something — `e`, Save,
Cancel, `gl`, `E`/`A`/`I`, `J`/`K`, `p`, `dd`, `:profile` — used to show
nothing until the answer landed, which reads as a key that did not work.

- **The logo's three houses, lit one after another** (`partials/loader.html`).
  Only the houses: the mask behind the middle one has a global id, and a
  second copy would collide with the logo's.
- **Next to the action, never over the screen.** A row action dims the row
  and puts the houses on it; the rule editor and the change log show them in
  their header — and opening either shows them on the focused row, because
  the empty column the pane will fill is not where you are looking. The
  carousel shows them in the middle of its stage. Anything with no closer
  place — a setting, `u`, a settings pane — uses the statusline. No dialog, no
  backdrop: a request in flight is no reason to stop reading the ledger.
- **150ms before it shows**, and the dimming with it — a delayed zero-length
  CSS animation, not a timer. On the household's LAN almost every answer is
  back first, so nothing flickers; on a slow link it is there almost at once.
- **One mechanism.** Every htmx request passes through `htmx:beforeRequest`
  and ends on its XHR's `loadend` — not `htmx:afterRequest`, which is
  dispatched on the requesting element, and a row swapped out by its own
  answer is no longer in the document to bubble from. The two plain `fetch`
  uploads wrap themselves in `busyFetch`. A background request (the
  selection sum, the month picker, the months read-back) opts out with
  `data-busy="none"` or `'none'`.
- **A dropped connection says so** in the statusline, since a loader that
  vanishes with nothing changed is the same confusion again.
- **A long Save names its steps**, in a small translucent box above it —
  fidget.nvim's shape: *Keeping the pages you chose*, *Shrinking the file*,
  *Opening the locked PDF*, *Reading the bill*, *Asking codex to read it*,
  *Checking what it read*, *Filing it*. Each line spins while it runs and
  ticks when the next starts; after success the box fades, and on a refusal
  or a question it goes at once, because the statusline or the dialog says
  what happened.
  - **Over the `/live` stream, not a WebSocket.** It only runs server to
    page, and the stream is already open on every page. The page makes a
    random job id per Save and sends it as `X-FFM-Job` on each request the
    Save makes (the trim view's file build, then the upload), so one box
    covers the chain; a step goes out as a `progress` event tagged with that
    id (`web::fidget`).
  - **Your steps reach only your pages, and only the page that asked shows
    them.** A stream forwards only its own member's steps, so another member
    never learns an upload is happening; a second tab ignores a job it did
    not start.
  - **Words, never data.** No amount, name or line of the bill travels this
    way: the steps are catalog messages, in the member's language.
  - **Only steps really done are named.** The trim view's size line builds the
    trimmed and shrunk file as you choose pages, so a Save usually starts at
    *Reading the bill*; trimming and shrinking show only when the Save has to
    do them itself.
  - The houses loader in the button stays: with no `/live` there is no box,
    and nothing else changes.
- **The edit row is locked while its Save is in flight** — `inert`, the same
  as the pre-auth forms: nothing is typed into a value already sent, and a
  second Enter cannot post twice. Lifted when the answer lands, with focus
  back where it was if the form is still there.
- **Esc on the edit row, and parking a draft, ask the server for nothing.**
  They used to re-fetch the read-only row — a round trip, and on a slow link
  a visible wait, for markup that had not changed. `e` keeps the row it
  replaces and those put it back. The kept copy is dropped the moment it stops
  being true: a save, or a write from inside the form (removing a file), after
  which they fetch the row as before.
- **The pre-auth pages show it in the button that was pressed.** `/login`,
  `/setup`, `/invite` and the language and light/dark switches are plain
  form posts answered by a whole new page — no htmx, no row, no pane — so a
  `submit` listener puts the houses where the key-cap was (or, on a button
  without one, over its faded label). `--paper` on the accent fill, like any
  text there. Alt-L and Alt-T use `requestSubmit()` so they show it too, and
  a page restored from the back/forward cache clears it.
- **And the form goes `inert` while its post is in flight** — its fields dim
  and stop taking focus, clicks and keys, and a second Enter cannot post
  twice. Not `disabled`: a disabled field is left out of the post, an inert
  one is still sent, and the browser builds that list after the `submit`
  handler has run.

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
  date. Three-quarters amber reads as "still room, but not much" with no legend. **The day you are on counts as begun**: due on the 18th, the 1st reads
  1/18. Counting only completed days drew every ring as an empty circle on the
  1st — the day a household opens the new month — the same as no ring at all.
- **"The same line" means `(member_id, title)` in the previous period.**
  Unambiguous for a recurring row — it is the same rule — but a one-off has no
  rule, and the mockup shows deltas on one-offs (`Mercado ▼ −6%`), so the
  question is real. Exact string match, same member, previous period, first
  hit. It is what a household means by "the same line", it needs no extra
  column, and it degrades the right way: rename the line and the delta
  disappears, which is arguably correct — it is not that line any more.
- **The MoM delta is derived, not stored.** Compare each row to the same line in
  the previous month (a window function). Red ▲ up, green ▼ down, gray dash for
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
  text. An emoji font renders differently — sometimes in full color, at a
  different weight — on every OS, which fights the "one visual system"
  point of the rest of this table.
- **Recurring and private are always both rendered, dimmed when off,
  instead of appearing only when true.** Unlike the status icons above
  (mutually exclusive with each other) or the file rail (a control is
  absent when that file doesn't exist), recurring and private are
  independent per-row booleans that can be on, off, or both at once — a
  fixed two-icon pair with opacity carrying the fact reads at a glance
  without the row's width jumping around as flags toggle. Active is full
  opacity in the fact's own color (`--accent` for recurring, `--info`
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
- **Only rendered when that file exists** — at `comfortable` and `compact`
  density. A row with a bill and a receipt but no scheduled file shows
  `[1] [3]` — nothing sits where `2` would be. No ghost/hollow placeholder;
  the keybinding still works purely off kind, not position. **`default`
  density is the one exception, on purpose** (see Density, below): it is the
  roomy, explicit setting, and there every empty slot of an attachable row
  shows as its own dashed `+2`, in its numbered place.
- **Neutral stroke icon per kind, not a color-coded emoji.** Bill, scheduled
  and receipt each get their own small matching-weight line-icon (a
  document, a clock-in-a-box, a checked receipt) rendered in `currentColor` —
  legible in every theme, and never competing with `pos`/`neg`/`accent` for
  attention the way a full-color emoji glyph did.
- **The number is a real `<kbd>` built into the control itself**, not a
  floating notification-style bubble pinned to its corner — same key-cap
  styling used everywhere else in the app (surface-2 fill, bottom-weighted
  border, mono font). It doesn't just label the control, it reads as "press
  this," the same trick the gutter's relative line numbers already pull off.
- **Compact by default, labeled on the focused row.** Every control is
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
- **The bill control shows a page indicator when its rule's bill covers
  several months** (see Recurring rules' `spans_period` in `CLAUDE.md`) —
  `Bill · p.2` in its label: the page this month's own code is printed on in
  the one shared file, rather than a bare "Bill" that would read as this
  month's own file.
- **Same fixed numbering as `g1`/`g2`/`g3`, reused rather than
  reinvented.** `1`/`2`/`3` inside the carousel jump straight to that file
  kind; pressing a number for a kind that doesn't exist on this row is the
  same no-op-with-a-statusline-message `g1`/`g2`/`g3` already give,
  not a new error path.
- **`h`/`l` step to the previous/next file that actually exists.** Clamped
  at either end, not wrapping — the first file's "previous" button and the
  last file's "next" button are both visibly disabled rather than looping
  back around.
- **The whole viewport, edge to edge** — not a box on a dimmed backdrop,
  which it was until it was noticed that an 1100 × 760 stage kept a margin
  of ledger around a file opened to be seen large. With no backdrop left to
  click away on, an `esc` button beside the zoom controls closes it, the same
  word the rule pane's close button uses.
- **Except the statusline, which stays visible below it** — and below the
  trim view, which shares the rule. It is the one place the app speaks, and
  covering it meant a bill refused on the trim view's Save was refused in
  silence. Both overlays end at `--status-h` (30px, `tokens.css`), the
  statusline's own fixed height.
- **Full-viewport, not a dialog-sized modal** — the few other modals left
  in this app (confirmations, the keymap) are centered `min(560px, 100%)`
  boxes, and this is the one deliberate exception. The actual job is reading a boleto's barcode digits, or
  holding a phone up to its QR code to scan it — both need the file
  genuinely large on screen, which a 560px dialog can't give.
- **Zoom (`+`/`-`, `0` to reset) scales the preview**, with floating
  controls in the stage's corner — the same placement any image viewer's
  zoom control uses, chosen so it doesn't compete with the row of file-nav
  buttons for space. Each file starts back at 100% when you flip to a
  different one; a carried-over zoom level would be a surprise applied to
  an unrelated file, not a convenience.
- **The stage shows the file itself**, with its kind, icon and computed
  path in a caption above it. It used to show only the icon and the path —
  "not the file's actual bytes" — which could not do the one job this modal
  is full-viewport for. **A PDF's pages are drawn by the server** with
  `pdftoppm` at 300 dpi (`GET /tx/{id}/file/{kind}/page/{n}`) and stacked as
  images, fitted to the width; a photo is shown as itself and a text file as
  text. The stage is one fragment per file (`…/view`), and a bill shared
  across months scrolls to this month's page. `+`/`-`/`0` set the pages'
  width, so zoom is one mechanism for every type and every browser. The
  browser's own PDF viewer in a frame was tried first and dropped: CSS zoom
  scaled its toolbar instead of the page, a fresh frame with `#zoom=` came
  up blank in Chrome, and widening the frame only zoomed while the viewer
  stayed fitted to the width — in Safari `+` slid the page sideways and `-`
  did nothing.
- **A page opens fitted to its height, `f` fits it to the width.** Fitted to
  the width, a tall statement opened zoomed in, a third of its first page on
  screen and no key to move down it — the carousel took every key and used
  none of them for scrolling. Now the whole page shows first; `f` flips to
  the width for reading it, `j`/`k` scroll, and `H`/`M`/`L` put the page on
  show at its top, middle or bottom, as Vim's do for the screen. `J`/`K`
  go a whole page down or up, landing on the next page's top — at fit height
  one page per press, which `j`'s small steps took seven of.
  `+`/`-`/`0` zoom on top of either fit, which is kept from file to file
  while the carousel is open and starts at height every time it opens. The
  fit is CSS: the stage is a size container and a page is
  `100cqh` tall, so nothing is measured in script. A photo fits the same
  way; a text file has no page, and only scrolls. The trim view's large
  page has the same keys and switch — one set of keys wherever a page is
  an image — drawn at 150 dpi there so fit width stays readable.
- **300 dpi, not 150.** An image has the resolution it was drawn at: a
  statement drawn small in PDF points (Inter's is 446 pt wide) came out
  929 px at 150 dpi and was stretched 2–4× to fill the stage — blurry beside
  the same file in a tab. At 300 an A4 page is ~2480 px and that statement
  ~1860 px. A page costs ~2.5× the bytes and the time (~1 MB, ~0.8 s for an
  A4 bill), which the household's LAN does not notice.
- **`v` shows a PDF in the browser's own viewer instead** — a *PNG ⇄ PDF*
  switch in the corner, beside the zoom buttons, only on a PDF: one `.tog`
  between the two names, the side on show in the ink. The viewer
  is the file itself in an `<embed>`: sharp at any size, text selectable, a
  shared bill opened on its page with `#page=N`. It zooms itself, so the
  carousel's `+`/`-`/`0` step aside while it shows, and once clicked into it
  takes the keyboard — `Esc`, `h`/`l` and `v` work again after a click
  outside it. That is why it is offered and never the default: the frame
  this section rejected above failed as the *only* view, because the
  carousel's zoom had to drive it; as an opt-in it is free to zoom itself.
  **`P` opens on the member's side** — `member_settings.pdf_view`, a
  `:profile` radio, `png` by default (amended from "every `P` opens on the
  pages", which made a member who reads every bill in the viewer press `v`
  each time). `v` flips it while the carousel is open, across the row's
  files, and that flip is not remembered after it closes.
- **The caption and the file-nav row are one low height** (`--cbar-h`,
  34px), with the nav's buttons a size down: the bars frame the file, and
  every pixel they take is the file's. An
  SVG drawing of each page (`pdftocairo`) was tried as a third way and did
  not fit as well as either.
- **The bottom bar's left end shows the file's size** — `412 KB`,
  `1,2 MB`, in the household's numbers (`fmt::bytes`, as `:prune` and the
  trim view write it). The server reads it off the disk with the stage
  (`data-size` on the fragment), so it is the stored file's size in every
  view, and it changes as `h`/`l` move to another file. It sits at the same
  end of the bar as *Trim* on purpose: the size is what decides whether a
  PDF is worth shrinking. Out of the flow beside the centered buttons; on a
  phone, the row's first item — and there the bar wraps onto a second line,
  since the stage clips whatever overflows and would cut its last button off.
- **A row with zero files refuses outright** — `P` says "no files on this
  row" rather than opening an empty carousel with nothing to look at.
- **The file on show can be removed from here** — a Remove button beside
  "Open in tab", and `dd`, the row's own delete gesture pointed at the file.
  Both ask first, in the same confirmation shape as the row's `dd` (only `y`
  confirms), raised above the carousel so the file stays in view behind the
  question. It is the existing soft delete into the member's trash, so `u`
  puts it back. The carousel drops the file only once the row fragment that
  comes back no longer shows it, and closes when the last one goes.
- **A PDF on show can be trimmed or shrunk from here** — a *Trim / shrink*
  button beside Remove, and `t`. It opens the view every PDF upload passes
  through (see Trim and shrink, under Receipt tracking & files), on the
  stored file; saving replaces it, and the carousel then shows the new copy.
  On a photo or a text file the button is absent and `t` says why.
- **A PDF on show can take more pages** — *Add barcode* (`b`) and *Append
  PDF* (`a`), beside Trim, on a PDF only. Both open **the same trim view** on
  the longer file, so nothing is filed until Save there: the pages are seen
  first, any can be dropped, and Save replaces the stored file — the old one
  to the trash, `u` brings it back. A locked PDF being appended is opened
  with the rule's password, as every upload to the row is.
  - **`b` draws a barcode from a code** typed or pasted into a small box (47
    or 48 digits as printed, or the 44 the bars encode). Every check digit
    and the digits the standard fixes are checked first, and a code that
    fails is refused with the reason — a typo must never become a scannable
    barcode for somebody else's bill. The new last page matches the size of
    the page it follows (A4 if that cannot be read): the bars, Interleaved 2
    of 5 at the full width inside the margins (`services::itf`), the 44
    digits under them, and above, what the code says — kind, value,
    company or bank, segment, due date — with a footer naming the row and
    that ffm made it from a typed code. A PDF written by hand, standard fonts
    only (`services::barcode_page`), no dependency.
  - **The page is a readable bill.** The code is printed as text, so the
    app's own reader finds it in the page, exactly as on a bill a company
    issued — Save files it through the same bill checks.
  - **`a` puts another PDF's pages last** (`qpdf --empty --pages`).
- **A bill covering several months (`spans_period`) opens on this month's
  page** — the page its own code is printed on, scrolled to as the carousel
  opens, and passed to the browser's viewer as `#page=N`.
- **One way to change month, whatever asked.** `[`, `]`, `T`, the `‹ ›`
  arrows and a picker cell all go through `gotoPeriod`, which builds the
  URL from `location`: `?pane=` and `?split=` survive the page load — the
  arrows' plain hrefs dropped the pane, landing you back on your own — and
  `?row=` goes. The hrefs stay for a page without JS and for a Cmd-click to
  a new tab.
- **The cursor follows its row to the new month.** It used to come back on
  row 1 every time. Each month's row is a different transaction, so what is
  carried is what spans months: the **rule** first — it survives a rename,
  Nubank to Nubank&Co — then the **exact title**, the "same line" a one-off
  has; with neither there, the **same position**, clamped to the last row.
  A closed fold holding the row opens, and there is no blink: this is
  ordinary navigation, not a search hit. It is kept in `sessionStorage`
  (`ffm.follow`), not the URL — a title is household content — read once
  for the month it was kept for, and dropped, so a refresh starts on row 1.
  A `:grep` hit's `?row=` is more specific and wins.
- **Parked drafts are asked about first.** Changing month is a page load,
  which drops every parked draft. With any parked — an open form is parked
  first, so a changed one counts and an unchanged one just closes — the
  first press says *changing month loses 2 unsaved drafts — ] again to go
  anyway*, and only the same request again goes. Any other key lets the
  question lapse, as `u`'s does.
- **Today is the server's** (`data-today`, from `clock::today()`), never the
  browser's: a dev server running as of another date must agree with
  itself. `T` on the current month says so and stays.
- **`Ctrl-P` / `Ctrl-N` were the first binding and were never built.** Chrome
  keeps `Ctrl-N` for a new window on Linux and Windows, where a page cannot
  take it, so they are dropped rather than wired.

### Act
| Key | Action |
|---|---|
| `J` / `K` | move the row — or every selected row — down / up |
| `s` | row order: `manual` → `date` → `amount`, and round again — `S` does the same for groups |
| `3J` | move it three places — a count past the end lands on the end |
| `ga` | **re-attribute** — give the row to another member, picked from a list: their pane's number (`ga2`), or `j`/`k` + `Enter`. Admin only |
| `a` | add a one-off spend |
| `i` | add a one-off income |
| `x` | toggle selection for a batch operation — the statusline counts and sums what is selected in the focused pane |
| `e` | edit this occurrence |
| `A` | create a recurring bill — opens the rule pane, with a placeholder row at the cursor |
| `I` | create a recurring income — the same pane, money the other way |
| `E` | edit the recurring rule behind the focused row; no-op with a statusline message on a plain row |
| `gd` | show due/cadence/receipt details for the focused row *(proposal, not in the original spec)* |
| `gl` | change log for the focused row — who changed what, when — in a side pane *(proposal, not in the original spec)* |
| `p` | toggle private *(proposal, not in the original spec)* |
| `m` | mark paid by hand — an untracked recurring bill only; "received" on an income. Refused, with the reason, on a tracked row (its receipt proves it) and on a one-off |
| `P` | preview the row's files in a carousel — see Files below *(proposal, not in the original spec)* |

### Mouse
| Action | Result |
|---|---|
| Click a row | focus it — the same cursor `j`/`k` move, just placed by pointing instead |
| Tap a row (phone) | the same as a click — a tap is a click, nothing separate to learn |
| Click a fold header | put the cursor on it **and** toggle the fold — `za` from the header, so closing a fold by mouse never leaves the cursor inside it |
| Long-press a row (touch) · right-click (mouse) | the row menu — **Edit** `e`, **Edit rule** `E` when the row has a rule, **Mark paid** `m` when the row takes the manual tick, and **Move to…** `ga` for an admin |
| Hover a recurring row | show its due/cadence/receipt details — the same info `gd` shows for the focused row, keyboard-only |
| Click a file control | open that file, new tab — the same action as `g1`/`g2`/`g3` |

- **The row menu is the touch half of the keymap, and it replaced
  double-click.** Double-click was the promise here, and on a phone it cannot
  keep it: double-tap is the browser's zoom, `dblclick` does not fire
  reliably, and nothing on screen says it exists. A menu can say what is on
  offer, and it grows with the phases to come — delete, private, files, the
  log — by adding a line. Each item runs the method its key runs, with the
  cursor already on the row, and wears that key as a `<kbd>`: the menu
  teaches the keys rather than replacing them, the way the profile menu
  teaches the command bar. An item that does not apply is **absent**, not
  disabled — no *Edit rule* on a plain row or on an orphan — and on somebody
  else's read-only ledger there is no menu at all, only the statusline's
  read-only message, since every item so far writes.
  - **The long press is timed by hand** (500ms, touch and pen only), because
    iOS Safari never fires `contextmenu` for one — it opens its own text
    selection instead, which `-webkit-touch-callout` and `user-select` turn
    off on rows, on touch screens only. Moving the finger more than a few
    pixels, or the browser taking the gesture as a scroll, cancels it, so
    dragging the ledger never opens a menu. The click the lift sends is
    swallowed: the press has already done its work.
  - **Only on a row.** A fold header, the pane's chrome and the open edit
    form keep the browser's own right-click menu — in a text field that is
    where copy and paste live.
  - Esc closes it; any other key closes it and then does its own job, and a
    cursor move off the row closes it too.

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
  color and icon (see the row-state table above). A full sentence
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
  thing actually moving again. **"Real" is load-bearing:** a browser fires
  `mousemove` when the page scrolls under a pointer that never moved — which
  is exactly what `j` past the edge of the view does — so the suppression is
  cleared only when the pointer's screen position actually changes. (The CSS
  half of this, `body:not(.kbd-nav) .row:hover`, existed for a long time with
  nothing setting the class, so the hovered row stayed lit beside the cursor.)

### On a phone — the keyboard

A phone shows its keyboard only for a focused text field, so on an iPhone the
whole keymap was out of reach: there was no field to focus.

- **Tap `NORMAL` for the keyboard.** On a touch screen the statusline's mode
  badge wears a small keyboard glyph and is a button: a tap brings the
  keyboard up, another (or the keyboard's Done) puts it away. On a desktop
  nothing changes — the glyph is not drawn, and a click is harmless.
- **The keys go through an invisible field** (`.keytrap`), excluded from
  `key()`'s `typing` guard, so what is typed there is a ledger key and goes
  through the one dispatcher. Nothing is ever inserted into it. Predictive
  text sends `Unidentified` for a key and the character on `input` instead;
  that is caught and sent on as the key it is, and the `Unidentified` itself
  never reaches `key()`, where it would drop a pending `g` or count.
- **A key strip above the keyboard — `esc`, `ctrl`, `:`, `/`** — while it is
  up. The phone's keyboard has no Esc and no Ctrl; `:` and `/` open a bar in
  one tap. `ctrl` is sticky for the next key (`ctrl` then `u` is the trash)
  and is lit while armed; `esc` disarms it. The buttons act on `pointerdown`
  and never take focus, or the keyboard would drop.
- **The focus has to happen inside the tap.** iOS shows its keyboard only for
  a `focus()` made synchronously in the gesture, and keeps it only when focus
  moves to another text field in the same gesture. So with the keyboard up,
  `:` and `/` focus their field at once (`focusBar`) rather than after Alpine's
  next tick, and closing either bar hands focus straight back to the trap.
- **The shell follows the visible screen.** iOS shrinks only the visual
  viewport for its keyboard; a `100vh` shell put the statusline and the
  command bar underneath it. While the keyboard is up the shell is sized to
  `visualViewport.height` (`--app-h`).
- **Known limit:** after a form field took the keyboard (`e`, the add row),
  saving or canceling closes the keyboard with the field; tap `NORMAL` again.
  Focus moving to a checkbox or a button (the trash's list) does the same.

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
| `S` | order the groups: first seen → A–Z → by cost, and round again |

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
| `j` / `k` | inside the carousel or the trim view — scroll the page down / up |
| `f` | inside the carousel or the trim view — fit height ⇄ fit width |
| `H` / `M` / `L` | inside the carousel or the trim view — the top / middle / bottom of the page on show |
| `J` / `K` | inside the carousel or the trim view — the next / previous page, at its top |
| `gg` / `G` | inside the carousel — the top of the first page, the end of the last · inside the trim view — the first / last page |
| `dd` | inside the carousel — remove the file on show; confirms with `y`, never `Enter` |
| `t` | inside the carousel — trim or shrink the PDF on show (see Trim and shrink, below) |
| `b` | inside the carousel — add a last page to the PDF on show: the barcode drawn from a typed code, and what the code says |
| `a` | inside the carousel — append another PDF's pages to the one on show |
| `v` | inside the carousel — the PDF on show in the browser's own viewer, and back to the page images |

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
| `u` | undo your last change — in the focused pane, in the month on screen |
| `U` | redo — put back what `u` just took |
| `Ctrl-u` | the trash — every deleted row and removed file, each restorable on its own · `:trash` opens it too |
| `Esc` | clear count, buffer, selection and search |

- **Undo restores the deleted row in place.** There is no second,
  superseding row and no `↩ #id` marker — that earlier design is reversed
  (see CLAUDE.md's Data model). One `dd` followed by `u` leaves exactly one
  row in the ledger, undeleted and focused. Both the delete and the restore
  are entries in the change log, which is where the chain lives now.

#### Undo and redo (`u` / `U`)

`u` used to reach your last change *anywhere*: another month, another pane,
last week — and said only `undone`, so finding out what a stray press had
done meant opening `gl`. It was also the one gesture that could overwrite
somebody else: undoing my 10:00 edit put my old amount back over Ana's 11:00
one. And there was no redo.

- **Scoped to what is in front of you.** `u` and `U` reach only the viewer's
  **own** changes, to rows in the **focused pane**, in the **month on
  screen**. To undo something in August, go to August. A non-admin in another
  member's pane is refused as read-only, like every other write key; an admin
  in Ana's pane reaches the admin's own changes there and never Ana's — `u`
  means "undo what I did", in whoever's pane it is pressed. A re-attribution
  is reachable from the pane the row left as well as the one it went to.
- **Redo is an undo of an undo.** Every undo is already its own entry, with
  `undo_of` and the full diff, and reversing it re-applies what it undid
  through the same code — so there is no second implementation of any verb.
  Depth along the `undo_of` chain says what an entry is: 0 an original, odd
  an undo, even a redo. `u` takes the newest at even depth, `U` the newest at
  odd depth, and only one newer than your newest original here: **a fresh
  change clears the redo stack**, as it does in Vim. A trash restore is a
  fresh change.
- **A change somebody else has changed since is skipped, and named.** A later
  change still in effect, by another member, to the same row (or the same
  file slot) means yours is no longer the row's state to reverse: `u` leaves
  it as they made it, says so — *Ana changed "Padaria" since — left as they
  made it* — and reverses your change before it. Skipping rather than
  refusing is also what keeps `u` from getting stuck on one change forever.
  The materializer and the system are nobody's work and never overtake.
- **A change from before today asks first.** The first `u` says what it would
  undo and from when — *u again to undo: edited "Mercado", from 12/09/2026* —
  and only the very next key, `u` again, about the same change, does it. The
  server re-checks the id, so a stale confirmation cannot reverse something
  else.
- **It says what it did, where you are looking.** *undone: edited "Mercado" ·
  amount R$ 12,00 → R$ 10,00 · U redoes* — read off the entry the step just
  wrote, formatted as the log pane formats it, two fields at most. The cursor
  goes to the row and it blinks once.
- **Parts of another step never stand alone.** A row's files moved with it,
  and a live file a swap moved aside (`swapped aside`), follow the entry they
  belong to in both directions and are never picked on their own.
- **Not Ctrl-R.** Vim's redo reloads the page on Linux, and a reload drops
  every parked draft. `U` is free on every platform and pairs with `u` the
  way the rest of this keymap pairs lowercase with uppercase.

#### The trash (`Ctrl-u` / `:trash`)

`u` is a stack: it walks back through *your* last actions, in order. Getting
back a row deleted three actions ago meant undoing the two newer, unrelated
ones first. The trash is the other shape — everything that is gone, a
checkbox each, restored in any combination.

- **Only deletions.** Deleted rows and removed files are independent of one
  another, so restoring any subset in any order is well defined. An older
  *edit* is not: undoing it out of order would overwrite whatever was edited
  since. Edits, reorders, privacy and re-attribution stay on `u`'s stack.
- **Read by ledger, not by who deleted.** Everything gone from a ledger the
  viewer may write (`Viewer::may_write`) — a member's own, every ledger for an
  admin, their own again under preview. A row an admin deleted from Ana's
  ledger is Ana's to bring back.
- **A deleted row stands for its files.** The files that went with it are not
  listed on their own; the row says `+ 2 files`, and restoring it brings them
  back, exactly as `u` on the delete does. A file removed *before* the row was
  deleted reappears as its own entry once the row is live again.
- **An older file swaps with the current one.** Restoring a replaced bill
  into a slot that holds a newer file moves the newer one into the trash as
  `superseded` — the same thing replacing a file does — rather than refusing.
  Two versions of one slot checked at once are refused: which one belongs on
  the row is the person's call, not the server's.
- **A restore from here is an ordinary action, not an undo.** It is logged
  as `restore` / `file_upload` with no `undo_of`, so `u` right afterwards puts
  back what was just restored, and then keeps walking back through history
  — within the pane and month on screen, like any `u`.
- **`Ctrl-u`, not `U`.** It opened on `U` first; `U` became redo. On Linux
  and Windows Ctrl-U is the browser's view-source, which the keymap takes
  over — and if it ever slips past, a source tab opens and nothing is lost.
- **`u` swaps too.** Reaching a file delete whose slot has since been filled
  used to fail with "occupied" — and since `u` never skips a change it could
  not undo, every `u` after it failed the same way. It now swaps, and the file
  it moves aside points `undo_of` at the same change, so the next `u` does not
  swap them back.
- **The bottom split, the `:grep` shape** — one list, grouped by the day of
  the deletion, newest first, the newest 200. Not remembered in `?split=`:
  like `:grep`, it is a list for a moment, not a setting being watched.
- **A real form.** The checked boxes are unsubmitted form values, like a
  draft; the server re-checks every pick. `Ctrl-u` opens it with the keyboard on
  the first entry: `j`/`k`, `gg`/`G`, `x` or Space checks, `X` checks or
  clears the whole day under the cursor and `gx` the whole trash (all checked
  → none, otherwise → all), `Enter` restores what is checked, `Esc` closes. No confirmation — a restore is undone by `u`,
  so the consequence is obvious from the action.
- **The answer** is the trash again without what left it, every pane of the
  month on screen that a restored row or file landed in, with its totals, and
  a statusline naming how many rows and files came back — and how many landed
  in other months, where there is nothing on screen to show it.

### Type
| Key | Action |
|---|---|
| `:` | open the command bar |
| `:grep vinho` | search every month — a result list, not a filter over this one · bare `:grep` reopens the last search |
| `:trash` | deleted rows and removed files, to bring back — the same pane `Ctrl-u` opens |
| `:prune` | admin only — byte-identical files in the vault; remove the copies in the trash, for good |
| `:sort date` | row order: manual · date · amount |
| `:group bucket` | group by category · `:group none` to flatten |
| `:set rnu` | gutter: relative · absolute · off |
| `:theme gruvbox` | colorscheme (writes your own settings row) · bare `:theme` opens the picker |
| `:lang en` · `:lang pt-BR` | UI language — yours alone; dates and money don't move |
| `:currency usd` | household currency, admin-only (writes the household row) |
| `:profile` · `:me` | your own settings, beside the ledger |
| `:household` | household settings, admin-only · `:config` is an alias |
| `:members` | admin only — members, and issue an invite link |
| `:invite` | admin only — open `:members` with the create form |
| `:logout` | end this session |
| `:preview` | admin only — toggle previewing the app as a non-admin member |
| `:help` · `:h` | the keymap — the same modal `?` opens |
| `?` | keymap |

**`:add`, `:in` and `:move` are not here, and will not be.** They were
listed for a long time and never built, which is how this table came to
advertise three commands the app answers *"not a command"* to. They are
abandoned rather than deferred — see `TODO_2.md` §2 for the reasoning, kept
so their absence is not read as an oversight and re-opened.

The short version: each is a second spelling of a gesture that already
works (`a`, `i`, `ga`), so nothing is unreachable without them,
and each costs more than a keystroke is worth — `:add 50 mercado` is a
second place money is parsed out of a string, in an app whose currency
design is *exactly one place formats and parses money*, and `:move ana` has
to resolve a typed name and then route a private row through the
warning the `ga` picker already shows. `:invite`'s label argument goes with
them; the pane's own create form takes it directly.

**`just cmd-check` now reads this table, in both directions.** A row here
whose command has no `case` in the dispatcher fails the build, and so does a
command the dispatcher handles that no row here names. It already guarded the
command *bar* the same way — every name and alias in the command table
(`web::commands::ALL`) must be handled, every command handled must be in that
table, and every `(try X)` in the catalogs must exist — so the surfaces are
held against one another and none can drift alone.

Reading it **both** ways is the part worth keeping. A promise nobody kept is
the bug that started the recipe, and it is the loud one: somebody types the
command and the app refuses. A command nobody wrote down is the quiet one,
and this table had that too — `:grep` worked from the day it was built and
was named only in prose, three sections away, so the one place a person looks
up commands did not list it. The check found it on its first run.

It scans the whole row, not just the key cell, because an alias is usually
named in the description — `:config` is one, and it is a real command.

### Edit fields
| Key | Action |
|---|---|
| `Ctrl-H` / `Ctrl-J` / `Ctrl-K` / `Ctrl-L` | move focus between a form's fields, left / down / up / right — past the form's edge, back to the ledger, keeping the draft |
| `Ctrl-j` / `Ctrl-k` | from the ledger, into the bottom split; inside it, `Ctrl-k` off the top field comes back out |
| `zz` | zoom the bottom split, half ↔ nearly full |
| `Enter` | save — the form's primary action |
| `Esc` | cancel: discard the draft, nothing written — one press in every form, from inside a field too |

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
stops generalizing the moment income can recur too. **`i` no longer means
"add before the current row."** `a` adds below; above is `a` then `K`. That
gesture was in the keymap table but never in the code (`a` and `i` ran
identical logic), so nothing working was removed — but it is a deletion, not
a rename.

**Neither edit form is a modal.** The forms outgrew a centered 560px dialog
(the rule form especially), and a dialog hides the very ledger you're
editing. So:

- **`e` edits in place.** The focused row opens up into its own form — the
  same fields, with Cancel (`Esc`) and Save (`Enter`) at the bottom right of
  the row, Save last — every form's actions follow that one rule (CLAUDE.md,
  Editing), the rule pane's included, where Delete sits apart at the far
  left — and the rest of the ledger stays where it was.
- **`A` / `I` / `E` open the rule pane** — a column of its own in the ledger's
  grid, right after the member pane being edited, pushing any later panes
  further right. *Right after the pane being edited* is load-bearing and was
  got wrong once: a single mount at the end of the stage looks identical with
  two members and is plainly wrong with three, putting Rodrigo's rule editor
  past Miguel at the far edge. Every pane carries its own mount and the
  focused one is the target — which is also where `gl` goes, since it is
  about a row too. (The settings surfaces and `:grep` do **not**: they are
  about the household, and they open in the full-width split at the foot of
  the ledger. See Configuration §Surface.) The ledger scrolls sideways when
  the panes don't all fit rather than hiding one: a household can have more
  than two members, and the mount column never shrinks to make room.

  A small arrow on the pane's left edge sits level with the row being edited
  and follows it as the ledger scrolls; the row itself keeps an accent
  outline while its rule is open. The offset is measured in the browser and
  written into a custom property the stylesheet reads — where a row sits is
  not something the server can know.

  **The arrow always points at the row; the pane moves only when the arrow
  alone cannot reach it.** A rule editor is tall enough to cover most of the
  ledger, so its arrow travels inside it and the pane stays at the top. The
  change log often is not — one entry is a few hundred pixels against a row
  a thousand below — so it slides down to meet the row rather than pinning
  its arrow to its own bottom edge and pointing at nothing. Both panes are
  aimed by the same code; the log used to be left out of it entirely and got
  the CSS fallback, an arrow level with its own header.
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
- **`Ctrl-H/J/K/L` past the form's edge is the keyboard way out — and the
  way to close a form keeping what was typed.** Esc discards; there is no
  separate "park" key. Off the top or bottom of an inline row, or of an `E`
  pane, the cursor moves to the row above or below, which parks the draft:
  the row is marked `unsaved · e to resume` (or `· E to resume` for a rule)
  and `e`/`E` picks it back up. Off the left edge of the rule pane, focus
  returns to the ledger with the pane still open until the cursor actually
  moves — and moving it parks the pane the same way. `A`/`I` have no rule
  yet to park, so their top and bottom edges do nothing. Sideways never
  jumps rows — with no field left or right on this row, that's the edge.
  A park key was considered and dropped: `Ctrl-C` is copy and `Ctrl-Z` is
  undo on Linux, inside the very fields it would fire from.
- **Which form a key belongs to is decided by its container, not by whether
  focus is in a text field.** With focus on Save or a toggle, a "typing"
  check says no, and `Ctrl-j` fell through to the ledger's own `Ctrl-j`
  and opened the settings split. These keys had been in this table since
  the mock, which implemented them; the app never did until the port.
- **A rule draft is measured against the pane as it loaded**, not against
  each field's default: a `<select>` has no `defaultValue`, and a field
  `x-model` rewrites on load would read as edited by nobody. Restoring one
  fires the events `x-model` listens for, toggles first, so the fields they
  reveal exist and the path preview follows.
- **Still modal, on purpose:** the `dd` delete confirmation, the
  `ga` member picker, the `?` keymap and the `P` file
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
- **A tracked rule's "Barcode covers multiple months" switch has no fields
  of its own** — each month takes the code due in it, read off the file when
  it is attached (see One bill file, several months, under Receipt tracking &
  files). Under the switch, a read-back says what the attached file holds:
  *10 installments · 01/2026–10/2026 · R$ 121,62 each*. It replaced *Pages in
  this file* and *First page covers*, which could not describe a real IPTU.
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
  layout above, under the member's `{slug}` folder. A tracked rule with no
  bucket shows the missing segment by name rather than as `//`.
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
    kinds stay recognizable at a glance.
- **The barcode upload field is a dropzone, not a file `<input>` styled to
  look like one** — attaching a file there is what derives `client_code`
  on save (see Bill import, below), and it's optional in both `A` and `E`:
  a rule can be created and saved with tracking on and no file yet, the
  same way an amount or due date can be filled in later.
- **A bound rule shows a "Bill on file" box**, above the upload field —
  issuer, barcode kind, company code, client code and card (*ending 7766*),
  each only when the bill carried it, with its own **Unlink** button and no
  input box. Never the amount or the due date: they change every period,
  and this box is about the identity that does not — the one a later bill is
  checked against, and asked about as *"wrong row?"* when it disagrees. A
  bill dropped in `A` with no barcode says so under the list: *read from
  its text*. Every value
  came off a bill (see Bill import, below), never off a keyboard, so there
  is nothing here to edit; a rule with no bill yet shows no box at all, the
  same "absent, not a blank field" treatment `instance` gets. In `A` the
  same box appears the moment a bill is dropped, before Create, saying it
  will be attached when the rule is created — so what you see is exactly
  what `E` will show afterwards. **Unlink** clears the binding only — it
  doesn't close the pane, doesn't touch `tracked`/`bucket`/`billName`, and
  exists specifically for "the wrong file got attached the first time":
  drop a corrected one right after, in the same still-open dropzone.
- **The `e` form lists the row's files**, each with Open and **Remove** — the
  one place besides the `P` carousel a file is removed. Remove is its own
  request targeting only the form's file strip, never part of the form's
  submit, so the draft typed around it survives the swap. No confirmation:
  it is a soft delete undone with `u`, and the button says exactly what
  happens. Removing a file does not bump the row's `version`, so Save still
  goes through afterwards.
- **A new due day reaches this month's row while it is unpaid.** Saving
  the rule restamps the current month's occurrence (clamped to the month's
  length) unless it is confirmed — a receipt or payslip for a tracked rule, the
  manual tick for an untracked one — logs it as an edit of the row's due date
  that `u` reverses, and the statusline says *this month's row now due
  05/10/2026*. Earlier months stay as recorded, and the notebox says so.
- **Due date moved from the occurrence form to the rule form.** The
  edit-occurrence form's Day field only renders at all for a non-recurring
  row; a recurring row's due day is edited on the rule (`E`) instead, so
  there's exactly one control for that fact, never two disagreeing about
  which one a future materialized period actually follows.
- **Bucket is the identical combobox in both forms** — same per-member
  datalist, same "pick one of yours or type a new one" behavior, in the
  occurrence form and the rule form alike. The rule form's Bucket starts
  blank when creating a new rule specifically (not when editing an
  existing one), regardless of what the anchor transaction was tagged
  with before it became a rule.
- **A tracked rule has a PDF password field, with an eye to show it.** It
  sits under Bill name / Instance, for spend and income alike — payslips are
  locked as often as statements. Every upload to the rule's rows is opened
  with it and stored unlocked; a file it does not open is refused, *nothing
  was saved* (see CLAUDE.md's Recurring rules). The field is a text input
  masked by `-webkit-text-security`, **not `type="password"`**: a real
  password field makes Safari and Chrome offer to save a login on every rule
  save and autofill the member's own ffm password into it, both wrong for a
  bank file's key. The eye (`showPw`) is visual state and never part of a
  draft; the value, having a `name`, parks with the rest. The log says only
  *PDF password · off → on*, never the value.
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
  closing a form while it's open (saving, canceling, or collapsing the
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
- **Re-attribution is `ga` and a name, not a direction.** `J` reorders —
  cosmetic, affects nobody's totals. Re-attribution moves money between two
  people's INs/OUTs and changes who owns the row. It was `Ctrl-H`/`Ctrl-L`,
  which kept it off `J`/`K` but put it one modifier away from `H`/`L` — the
  keys pressed most to move between panes — and made a distant member a walk
  through every pane between. `ga` ("give to") opens a list of the household
  instead, each member under their pane's number, the owner left out: two
  keys to ask, a third to choose, at any household size. `Esc` backs out with
  nothing written. `Ctrl-H`/`Ctrl-L` now only say *rows move to another
  member with ga now*. The row menu offers it as *Move to…*.

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
  place the cursor themselves, because `autofocus` is not honored reliably
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
- Re-attributing a row asks **who**, never **whether**: `ga` opens a picker, and
  naming the member is the deliberate step. It is fully undoable and the
  statusline says where it went.
- On a **private** row the picker says so before the choice: handing a hidden
  transaction to another member makes *them* the owner, so the person who hid it
  stops seeing it. That is invisible in the gesture, so it is written above the
  names — and picking one is the confirmation, with no second `y` dialog.

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
  language and color scheme. The closed-household rule is untouched — an admin
  still decides who gets a link — but nobody ever types somebody else's
  password, and there is no initial password to hand over on a sticky note.
- **The login identifier is `members.name`, not an email.** `name` is already
  declared as "display name + login identifier"; `email` is nullable and plays
  no part in signing in. There is no mail sender in this app.
- **A name is stored lowercase and shown capitalized.** `/setup` and an
  invite store what was typed trimmed and lowercased (`db::auth::normal_name`),
  and signing in, a taken-name check and the console's `reset-link` all match
  ignoring case — `Rodrigo`, `rodrigo` and `RODRIGO` are one person, and a
  Shift key is never why somebody cannot sign in. Everywhere a person reads a
  name it goes through `ui::fmt::name`, which capitalizes every word:
  `maria clara` → *Maria Clara*. Members stored before this keep their names
  as typed — they still sign in and still take their name, and read the same.
  The comparison is done in Rust, not with SQLite's `lower()`, which folds
  only ASCII.
- **Signing a device out reaches it while it is open.** Any revoke — "Sign
  out other devices", "Sign out everywhere", a reset, a member turned off,
  `:logout` on another tab — pings the live channel; each open page's stream
  checks its own session server-side, and the page whose session is gone is
  sent to `/login` at once, where a note says *this device was signed out —
  from another device, or by an admin*. Nothing else on any page moves. A
  console reset (another process) is caught by a 30-second re-check instead.
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
- **Another member's ledger is read-only to a non-admin.** They see its shared
  rows and cannot change them, its rules or its files; the pane header carries
  a `read-only` tag, it has no add row and no `+` file control, and every write
  key answers *"read-only: this is Ana's ledger"*. Re-attributing a row is an
  admin's gesture even on your own row. An admin writes anywhere — until they
  preview as a non-admin, when the same rule applies to them.
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
no month, and no member. One new layout, `.auth` — a centered card on
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
- The color-scheme picker is a real `<fieldset>` of radios, so `←`/`→` walk it
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

It collects, in three labeled sections:

- **Your household** — household name, currency, default language. The name is
  a real need, not decoration: `/login`'s heading, the statusline's left cell
  and the invite page's "Join …" all need a word for the household, and
  hardcoding "FFM" is worst on the one page whose job is *am I on the right
  server*.
- **You** — name (with "you'll sign in with this" said plainly next to it),
  password, confirm password, color scheme.
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

**A new password is judged as it is typed** — on `/setup` and on the invite
and reset page, the same two partials (`auth/pwmeter.html`,
`auth/pwmatch.html`):

- **Under Password, a bar and a word** — *Weak*, *Medium*, *Strong* — that
  grows and changes color as you type, in the due ring's colors: `--neg`,
  the accent as its amber, `--pos`. Below the minimum it says the server's
  own sentence instead, *Use at least 8 characters.*
- **Under Confirm, whether the two match** — silent while Confirm is still
  a prefix of the password, since "do not match" three characters in is
  nagging, not news.
- **It is advice, not a rule.** The server refuses exactly one thing, fewer
  than `MIN_PASSWORD` characters, and the meter's "too short" reads that
  same constant, so the two cannot disagree about it. Weak / medium / strong
  past that is an estimate — length times the bits each character can carry,
  with a repeat or a run (`aaaa`, `1234`) counted as a quarter of one — and
  nothing refuses a weak password. It is computed in the page because the
  alternative, sending each keystroke of a password to the server, is worse.
  With JS off the fields work exactly as before.
- **Editing a field clears its server error**, which answered the last
  submission and would otherwise sit there contradicting the live line.

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

The token is 32 random bytes, base64url in the URL, and **SHA-256 in the
database** is what a link is looked up by. Argon2 is deliberately *not* used:
there is nothing to brute-force in 256 bits of randomness, and a slow hash on
every invite-page GET is a self-inflicted denial of service. Recorded here
because the obvious "fix" is to make it match how passwords are hashed.

**The raw token is also kept — while the link is pending, and only then**
(`member_invites.token`, migration 0009). It is set to NULL the moment the link
is accepted, revoked or expires. This reverses "the token itself is never
stored", which is what made a link *shown once*: an admin who closed the pane
before copying it could only revoke it and issue another. Keeping it gives
nothing away — whoever can read `ffm.db` can already add a member directly — and
it is what lets every pending link be shown and copied again.

Links **expire after 5 days** (`LINK_DAYS`). It is a LAN app and the link is
handed over in person or over a chat app; the number is named so the page has
something to render. **An expired link is revoked on its own** — `revoked_at`
set to the moment it expired, its token wiped — and leaves the pending list: a
link that no longer works is not pending anything.

The accept form asks for name, password, confirm, language and color scheme —
the same field grid as `/setup`'s second section, which is the point: one form,
two entry points. Above it sits the context (*"Rodrigo invited you to Casa
Stuchi"*) and below it the expiry, in mono.

- **Editing the name here is free; later it isn't.** `members.slug` is the
  receipt vault's folder name (`{slug}`), so renaming a member after
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

**`:members` is where an admin issues one** — the full-width split at the foot
of the ledger, like `:household`. It has three tabs, **Active · Inactive ·
Trash**, each with its count; which one shows is visual state, and the server
opens on the one the member acted on was in.

- **Active** lists the members with their role and signed-in devices, a
  create form (an optional *for whom* label, a role, and a button), and the
  **pending links**. Each pending link is its own row: who it is for, when it
  expires, and the **full URL** in a read-only mono input with **Copy** and
  **Revoke**. A link just created or reset lands in its own row, lit with the
  accent and selected — there is no separate "shown once" box any more.
  - **The URL is the address the admin is looking at.** The page renders the
    path and the field prefixes `location.origin`: the domain and port the
    admin reached this server by, right behind a proxy or over https too,
    with no Host-header guessing.
  - **Copy** uses `navigator.clipboard` where it exists, and falls back to
    select-and-copy: a plain-http LAN address is not a secure context, so the
    clipboard API is simply absent there.
  - *"Links stop working 5 days after they are created"* sits beside the list.
- **Inactive** — members turned off: *inactive since 09/2026*, and **Turn
  back on**.
- **Trash** — members removed, each with **Restore**.

`:invite` opens the pane on the create form.

A `.notebox` beside the form states the principle so it is visible at the moment
it applies: *"You never type anyone else's password. They set it on the link,
along with their name."*

Like every other admin-only control, the create form **disappears** under
preview-as-non-admin rather than sitting there disabled.

### Turning a member off, and the trash

Somebody leaves the household — a kid moves out — and their history is still
the household's. Somebody was added by mistake. Those are two different things,
and they get two different gestures.

**Turn off** (the Inactive tab):

- **They cannot sign in.** Every session and any open reset link go at once;
  `/login` with the right password says *"This account is turned off. Ask an
  admin to turn it back on."* — only somebody who knows the password learns it,
  and their next step is not to retry. A wrong password gets the usual message.
- **Their pane is hidden from the month they were turned off onward**, and in
  those months **their rows count in no total** — the pane query is what the
  totals and the month picker's NETs sum, so a hidden pane takes its money with
  it. **The months before still show them.** `:grep` finds nothing there,
  the unresolved badge ignores it, and nothing can be added to it.
- **It is a stretch, not a flag** (`member_inactive`: from the month turned
  off, until the month turned back on). Turning them back on in November keeps
  September and October hidden, and **the materializer never backfills** a
  bill into a month inside a stretch — reconciling from `start_date` would
  otherwise do exactly that. Turned back on in the same month, the stretch is
  empty and hides nothing.
- Logged as an `edit` on `entity = member` with one `status` field,
  `active` → `inactive` and back.

**Move to trash**:

- **Only a member with no rows at all** — no transaction and no rule, deleted
  or not. It is for a mistake; a member with history is turned off instead, and
  the pane offers only that. The server refuses it either way.
- Sets `members.deleted_at`, which every sign-in and pane query already
  filtered on. **Their name stays taken** while they are there
  (`members.name` is UNIQUE), which is what makes **Restore** safe. Logged as
  `delete` / `restore` on `entity = member`.

**Guards**, checked on the server for every account action — reset and sign
out included, which trusted the template alone before: **not on yourself**,
**not the household's last admin who can still sign in**, and **a member who
exists**. The pane leaves an action out where it does not apply, rather than
disabling it.

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
(`{slug}`). Recovering a password must never orphan somebody's filing.

The accept form asks for **password and confirm only**. No name — renaming is a
vault move once files exist, and this person is not new. No language or color
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
everywhere**. The new link lands in the pending list like an invite, lit and
selected, with the same full URL and Copy button — no new widget. `:reset <name>` is not bound; the
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
  link."* Not security theater — this document already declines rate limiting on
  the grounds that the network is the boundary. It is that "we have no rodrigo"
  and "wrong password" lead to the same next action, and the copy rule says tell
  people what to do about it. On failure the name is kept, the password cleared,
  and focus returns to the password field.
- **No color-scheme picker here.** A palette chosen by an anonymous visitor has
  nowhere to live, and a returning member's real palette is in their settings
  row and would override it three seconds later. Language and light/dark only —
  both of which genuinely apply to this page. The page *is* drawn in the
  palette of whoever last signed in on this device (`ffm.palette`, see Language
  & copy), so signing out does not look like leaving for another app. `/setup` and `/invite` *do* get
  the full picker, because on those two pages it is a form field writing
  `member_settings`, not chrome.
- **On a phone the card is not vertically centered.** It sits at `12vh` from the
  top: centering a card in the viewport puts it underneath the software keyboard
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
  genuinely hidden — not grayed out, not present-but-redacted, absent from
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
  relabeled *"Mark as received manually"*. Same one exception, same reason:
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

- The row reads `~R$ 4.133,33` in a quieter color. The mark carries whether
  the money moved; the tilde carries whether the number is a fact — one
  channel per fact, as everywhere else in this document.
- **The pane's INs and Balance, and the household's IN and NET, all carry
  the `~`** when any component is a projection. Leaving projections out of
  the totals would make a future month's NET useless; putting them in
  unmarked would present a guess as a fact.
- **The totals' words are the catalog's, not literals** — `in` / `out` /
  `net` / `bal` in English, `entradas` / `saídas` / `saldo` in Portuguese —
  in the app bar, every pane header, the month picker's year and the
  selection cell alike. Stored lowercase, one word per idea; the bar and the
  picker uppercase them in CSS, so the case is where a word sits, not what it
  says. Each word stays with its figure (`white-space: nowrap` on the pair):
  a pane at its floor, in a language with longer words, moves the pairs under
  its name whole rather than splitting `R$` from its digits.
- **The MoM delta inverts.** The arrow is direction, the color is whether
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
  labeled **Source**, the due date **Expected day**, installments
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
and cursor-follows behavior the `A`/`I`/`E` rule editor uses, so nothing new has
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
- **A bill that moves the row's amount says so in its own entry** —
  `import`, read *"read from a bill · amount R$ 883,34 → R$ 131,45"* — next
  to the attach entry rather than folded inside it, because the amount is the
  one change a person needs to see. A due date the bill replaced is a field
  on the same entry, so it is never a silent change either. It is undoable
  like an edit, and the first `u` after a drop puts the old amount and date
  back before the second takes the bill away. (See CLAUDE.md's Recurring rules for why the bill wins even
  over a typed amount.)
- **Expanding an entry** shows `old -> new` per field, formatted by `kind`,
  with an em dash for empty.
- **One field is shown inline; only two or more fold.** Expanding to reveal
  a single line is a poor trade, and one field is the common case — a privacy
  toggle, a reorder, a re-attribution and a single edit each produce exactly
  one. Such an entry carries no `+` and no field count either: *"edited · 1
  field"* above the line it describes says nothing the line does not. Delete
  and restore have no fields at all and have never offered anything to open.
- **Read-only.** No drafts, no save. `Esc` closes.
- **While it is open, the log takes `j`/`k` — and `gg`/`G` with them.**
  Nothing is stolen by that: moving the *ledger* cursor closes the log, so
  ledger navigation is already meaningless while it is up — which is also why
  there is no key to focus the pane, since there is nothing to focus away
  from. The cursor starts on the first entry and is marked the way a focused
  row is, with an accent rail and a tinted ground.

  `gg` and `G` are the same motions they are in the ledger, pointed at the
  entries: first and last. A long history is exactly where they earn their
  keep, since the pane lists newest first and the oldest entry is the one you
  would otherwise scroll for. **At either end they scroll the body all the
  way, not just far enough** — `scrollIntoView`'s `nearest` stops the moment
  the entry is visible, which leaves the day heading above the first entry
  hidden and the padding below the last one cut off. Both read as "it did not
  quite go there".
- **It opens showing its own top**, and that takes two things, neither of
  them obvious. The pane is **bounded by the viewport and scrolls inside
  itself**: unbounded it grew to fit its entries — forty of them came to
  2387px in a 777px stage — so reading the log scrolled the *stage*, which
  carries the pane's header and first entries away with it. And because the
  stage scrolls vertically while the log is a column inside it, a stage
  already scrolled down to the focused row hides the pane's header under the
  app bar before a single key is pressed; opening therefore scrolls the stage
  back **up** far enough to show the pane's top, and never down — a pane
  sitting lower was put there by the arrow to meet its row, which is
  deliberate.
- **Its entries answer the fold keys**, because an entry *is* a fold — a
  header with collapsible content. `za`/`zo`/`zc` act on the entry under the
  cursor and `zR`/`zM` on all of them, exactly as they do for `:group bucket`
  folds; which set they act on is whichever is in front of you. `Enter`
  toggles too, for the half of this app's userbase that does not use Vim —
  the same reasoning that makes `?` open the keymap.
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
{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}.{ext}           # definitive receipt
{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}_scheduled.{ext} # scheduled-payment proof (optional)
{slug}/{bucket}/{bill_name}[/{instance}]/{year}/{month}_barcode.{ext}   # the bill/invoice itself
```

- **The member folder is `{slug}`** — it was `{member_uuid}-{slug}`. Names
  and slugs are both `UNIQUE` and a member cannot be renamed, so the uuid
  protected against nothing and made the one folder a person browses first
  unreadable. Existing folders are moved once at startup, with the stored
  paths that point into them (see CLAUDE.md's Recurring rules).

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
its own warning-colored state (see the row-state table above) rather than
silently reading the same as "nothing has happened yet."

**Uploading the receipt supersedes the scheduled file automatically.** The
system moves `{month}_scheduled.{ext}` into `.delete/` the instant
`{month}.{ext}` lands — the promise is resolved, only the definitive proof is
left standing. This is a system-triggered soft delete, logged exactly like a
manual one — a `file_delete` entry in the change log (below) carrying
`old_path`, `deleted_at`, `deleted_by` — plus a `reason` of `superseded`
rather than `manual`, so `u` on the receipt upload restores the scheduled
file too and the audit trail is honest about why it moved.

### One bill file, several months (`spans_period`)

Opt-in per rule, off by default. Some bills arrive as a single document
covering several months instead of a fresh file every month:

- A city's IPTU: one PDF holding the single payment (*ÚNICA*, in full with
  a discount) and ten installments, reissued each year.
- A bank's carnê with a loan's whole run, which lasts the rule's life.

**Each month takes the code due in it.** A utility code carries its due date
in its free field and a bank slip in its date factor, so attaching the file
reads every code in it (`services::installments`) and keeps one per month in
`rule_bill_code`, with the page it is printed on. There is nothing to type:
no page count, no first month. Only the **bill** is shared — the scheduled
proof and the receipt stay per month, because each installment is still
paid on its own.

- **Every covered month is filled when the file arrives**, with its own
  amount, due date and code — one `import` entry per row, so `u` reverses
  each. A month the materializer creates later is filled from its code when
  it appears.
- **The installments, not the single payment.** Two codes due in one month —
  the ÚNICA and installment 1 — resolve to the one whose value repeats in the
  file. With the toggle **off**, the bill is read like any other bill: its
  first code, which on an IPTU is the ÚNICA, paid at once.
- **Every covered month shows the one file**, stored on the row it was
  dropped on: slot 1 opens it on that month's page (*Bill · p.2*), in `P`
  as in a new tab. A month the file has no code for shows nothing from it.
- **Codes with no date** go to the rule's active months (`months`), in the
  order the file prints them, with a value that appears only once left out —
  so a single payment does not take January's place.
- **A new file retires the last one's codes**; history keeps the codes it
  was billed with.
- **Reversed: the page window.** The first design asked for *Pages in this
  file* and *First page covers* and derived "page N is month N". A real IPTU
  holds eleven codes on three pages, so neither field could describe it, and
  the file's path was never stored — no other month ever showed the bill.
  Both fields are gone from the pane; the toggle alone says the file covers
  several months, and a read-back under it says what the attached file holds
  (*10 installments · 01/2026–10/2026 · R$ 121,62 each*).

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

### Files changed by hand — watched, recorded, shown live

The vault is meant to be filed by hand, and `paid` was always read from the
disk on render, so the ledger was never *wrong* after a hand edit. It was
**silent**: no record of the file, nothing in the log, and the open pages
found out on their next load. A file deleted by hand even stayed "live" in
`receipts`, and the trash offered to restore what was gone.

- **One function decides** — `receipts::reconcile_row`, run per row by the
  watcher, the startup catch-up and the tests. It compares the disk with the
  live `receipts` rows, per kind, and never touches a file:

  | on disk | live row | meaning | recorded as |
  |---|---|---|---|
  | a file at the row's computed path | none | filed by hand | a `receipts` row (`uploaded_by` = the row's member) and `file_upload`, system, *found on disk — filed by hand* |
  | at neither the computed nor the stored path | yes | removed by hand | the row marked deleted (`manual`) and `file_delete`, system, *removed from disk by hand* |
  | anything else | | agrees | nothing |

  The **stored** path is what keeps a rule rename, a re-attribution or an
  amount change — which move the computed path, never the file — from
  reading as a removal.
- **No list of the app's own writes.** An upload, a soft delete or a restore
  updates its `receipts` row in the same handler that touches the disk, so by
  the time the watcher looks the two agree and nothing is logged.
- **The watcher** (`notify`: FSEvents, inotify) watches the whole vault, and
  acts on a path once it has been quiet for **1.5s** — a Finder copy is a
  burst. A path names a member (its first folder) and a month (its year folder
  and the file name's first two digits); that member's tracked rows in that
  month are reconciled, so the bucket and bill name never have to be read
  back out of a slug. `.delete/`, dotfiles and half-downloaded names are
  ignored; a whole folder going reconciles the whole member.
- **A hand-filed receipt does not supersede the scheduled file** the way an
  upload does. The watcher records; it never rearranges somebody's filing.
- **Startup catches up**, on its own task after the bind — the server answers
  from the first moment — and logs `files · N found on disk, M removed by hand
  while away`.
- **The trash leaves a file removed by hand out**: nothing went through
  `.delete/`, so there is nothing to bring back.
- **Open pages update live, over Server-Sent Events** (`GET /live`). An event
  is `row` with `{id, member, period, disk}` — ids, never HTML: a row is
  rendered per viewer, so each page that has it on screen re-fetches it
  (`GET /tx/{id}`) as itself, totals included. The row open in `e` is never
  swapped under the form; its kept copy goes stale, the statusline says so,
  and Esc shows the new one. Every affected row is pushed, so another
  member's upload through the app shows live too; *updated from disk* is said
  only for a hand change. The stream ends on shutdown, or graceful shutdown
  would wait on it forever.

### Pruning duplicates (`:prune`)

Every delete, replace and supersede moves a file into `{slug}/.delete/`, and
nothing ever leaves — so the trash fills with exact copies of files that are
still live, or of each other. `:prune`, admin only, lists every group of
byte-identical files in the vault, and removes the trash's copies the admin
checks.

- **It is the one place a file leaves the disk**, and that is defensible only
  because what goes is an exact copy of a file that **stays**: no bytes are
  lost. The server proves it again at the moment of removal, against a fresh
  scan rather than the page, and refuses the whole batch if any pick is not a
  copy in a member's trash (`{slug}/.delete/…`, no `..`), is no longer a
  duplicate, or would leave a group with no copy at all.
- **Size first, then blake3.** Only sizes two files share are hashed, so a
  vault of unique PDFs costs one `stat` each. Dotfiles are skipped; `.delete`
  is the one hidden folder walked.
- **Every group is listed**, the way `fd … | sha1sum | uniq` shows them:
  groups with a copy in the trash first, by the space their copies take, then
  the groups made only of live files — the same PDF filed on two rows is worth
  seeing. A live file has no checkbox; only a copy in the trash has one.
- **Each line says what the file is**, read off `receipts`: *live · Moto CG
  09/2026 · Bill*, *trash · … · deleted 18/09/2026*, or *not on record* for a
  file the app never filed.
- **Checked one at a time, a group at a time, or all at once** — `x`, `X`
  on the group under the cursor, `gx` on the whole list; all checked → none,
  otherwise → all, as in the trash. In a group with no live copy the server
  marks one copy to keep (`data-keep`), and `X`/`gx` never check it, so
  either is always a batch the server accepts; it can still be checked by
  hand, and the last-copy refusal stands behind that. **The group header**
  is the heading of what follows — the fingerprint, the count and the size
  in the ink, bold, with a rule under it — not a faint label.
- **It asks first, with `y`.** Enter (or Remove) raises the confirmation
  `dd` uses — only `y` goes, Esc backs out — because this is the one action
  `u` cannot take back.
- **A pruned copy is retired**, like a bill that moved to another row: a
  `file_unlink` noted *removed for good — a duplicate kept elsewhere*, with
  the removed `path` and the `kept` one, so the trash, `u` and every restore
  leave it alone (`receipts::RETIRED`). A copy with no record is logged too,
  with `entity_id = 0` — no `receipts` row has that id — and its path.
- **Folders the removal empties go with it**, as they do when a file moves
  into the trash.

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
- **Delete (of any of the three) is a soft delete.** The file moves into the
  member's trash, `{slug}/.delete/`, under the same path it had and named
  after its own row (`09_barcode.r42.pdf`); never removed from disk, and
  folders the move empties are removed so the live tree holds only what is
  live. Deleting a **row** moves its files the same way, and one `u` restores
  the row and its files together. The DB
  change log records the move (`old_path`, `deleted_at`, `deleted_by`,
  `reason`) so `u` restores it exactly — the same one audit mechanism every
  other change goes through, not a second one for files.

### Attaching a file — four gestures, one upload path

Phase 7 built the server side — three endpoints, one per kind, with the
supersede and the soft delete. What was left open was the gesture. There are
four, and they all end in the same request:

- **The `+` control on the focused row's file rail.** The rail still shows
  only files that *exist* — no ghost placeholders — so attaching lives behind
  one control rather than three empty slots pretending to be files. Clicking
  it opens a menu of the three kinds, anchored to the control itself because
  the ledger scrolls sideways and a centered menu would point at nothing.
  Picking a kind closes the menu and opens the file picker for that row; the
  row the picker is for is kept apart from whether the menu is open (see
  CLAUDE.md's gotchas for how sharing one flag dropped every pick silently).
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
- **Dropping a bill anywhere else on the page** — slot 1 only, on the row
  of the rule whose binding recognizes it (see Bill import, below). A
  scheduled-payment proof or a receipt carries nothing that names its rule,
  so those still go on their row.

**Three distinct actions, never one upload plus a question**, in all three
gestures: each kind has its own fixed name on disk and its own effect on
state, so asking afterwards would be asking about something already decided.

**An income row has no slot 2** in any of them — no menu entry, no drop
target, and `gu2` refuses with the message `g2` already gives.

### Trim and shrink — every PDF, before it is filed

All three gestures, and `A`/`E`'s bill dropzone, stop a PDF at one more
view before it reaches the vault: the pages as a strip of thumbnails, the
focused one large beside it, and a size line — *987 KB → 78 KB −93%*. It
does what the household used to do by hand before uploading: `qpdf` keeps
the pages that matter, Ghostscript (`/ebook`) shrinks the rest. A photo or a
text file skips it. `t` in the `P` carousel opens the same view on a PDF
already stored.

- **Full viewport, like `P`**, because the reason to look is to tell an
  advert from the page with the barcode, which needs the page large.
- **Keys:** `h`/`l` (or ←/→) move between pages, `x` or Space keeps or
  removes the one in focus, `s` switches the shrink, `Enter` saves and `Esc`
  cancels. The large page is read as in the carousel: fitted to height
  first, `f` for width, `j`/`k` (or ↑/↓) scroll, `H`/`M`/`L` top / middle /
  bottom, `+`/`-`/`0` zoom — so `j`/`k` no longer change page here. A click on a thumbnail focuses it; its button
  keeps or removes it. Removing the last page is refused — a file with no
  pages is not a file.
- **The size line is worked out by the server**, on every change (debounced),
  from the same files Save will send: what is kept and whether to shrink are
  visual state, the arithmetic is not. Results are cached per selection, so
  flipping a choice back is instant. A shrink can take Ghostscript several
  seconds on a long statement, so the figure on show dims and the loader sits
  beside it — not in the statusline, which the view covers.
- **Shrink is on by default, and absent without `gs`** on the server. A
  shrink that comes out no smaller is dropped and the line says so.
- **A bill covering several months (`spans_period`) has no page toggles** —
  page N is month N — and a line in their place says why. Shrink still works.
- **Save sends the trimmed file through the upload it was headed for, and
  the upload shrinks it last**, right before it is stored. Every check reads
  the file as trimmed and **not** shrunk — the barcode, the rule's recipe, the
  binding, the amount and the duplicate fingerprint — because Ghostscript can
  re-encode a PDF's text, and a bill read after it was not always the same
  bill. The shrunk copy the size line built is the one stored, so Save is no
  slower; when there is none (`A`'s Create, after the view closed) the shrink
  runs then, and the progress box says so. A bill whose barcode page was
  removed is refused with the usual message and the view **stays open** to
  put it back; anything else closes it. From `P`, saving replaces the stored
  file — the old copy goes to the trash as `superseded`, the carousel shows
  the new one, and `u` brings the old one back.
- **Esc files nothing**, and the staged copy is deleted. A staged file nobody
  saved or canceled is swept after a day.

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
- **A bill dropped anywhere on the page finds its own row — by the rule's
  binding, never by guessing.** (It was "a panel drop binds nothing — a
  notifier", which was never built.) Only rules a bill has already bound
  are candidates, and a rule matches only when the bill names its account
  or issuer — client code, CNPJ, card, company code (`import::names_rule`).
  The row is that rule's in the bill's due-date month, or the month on
  screen when the bill shows none. One match goes straight to the trim view,
  which says *for Nubank · 01/2026*; several open a picker shaped like `ga`'s
  — a number picks, `j`/`k` + Enter, Esc files nothing; none says *no rule
  knows this bill yet*. A bill already filed says so before the trim view.
  A row in another month is not on screen, so the statusline says where it
  went. Binding itself still happens only in `A`/`E` or on a row's slot 1.
- **A bill that cannot be confirmed, or that contradicts the rule, is a
  question — never a dead end.** *"Wrong row? this bill is from Cartão ·
  Inter, but Nubank is bound to Cartão · Nubank — attach it anyway?"*, or
  *"nothing on this bill confirms it is Nubank's…"*. The modal is the `dd`
  shape: only `y` attaches; Enter, Esc and any other key leave it out with
  nothing written — so the trim view's second Enter can never attach it, and
  the trim view stays open on "no" to fix. A contradiction wears the warning
  edge; an unconfirmed bill does not. `y` skips only that check — a
  duplicate is still refused — and the log notes what was overridden. See
  CLAUDE.md's Recurring rules.
- **A fatura with no barcode is read from its text** by its rule's recipe:
  the statusline says *read R$ 8.843,57 due 09/01/2026 from the bill's
  text*, and the same file dropped on another month is *already on
  01/2026*, by its text fingerprint.
- **A rule's first bill teaches it to read the rest.** When the bill leaves
  something open, the household's assistant (`:household`) reads page 1 —
  redacted — and the answer is a question: *Teach Nubank to read its bills?*
  with what it read listed (issuer and CNPJ, amount, due date, account,
  card), `y` *Keep and attach*; anything else attaches without it. From then
  on every month is read here, instantly; a redesigned layout that stops the
  recipe reading asks again. In `A` the Bill on file box lists what was
  read, and Create keeps it.
- **A bill removed from one row can go on another.** A row holds a bill
  only while it holds the file: attaching the same PDF elsewhere takes it
  over, and the statusline says *moved from Moto CG 09/2026*. The old row's
  copy in the trash is retired — no longer offered by `U` or `u`, kept on
  disk. See CLAUDE.md's Recurring rules.
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
  asserted — with different colors/shades of the same glyph, never the same
  mark (see the row-state table above).
- **Setting it**: the *Paid — marked by hand* toggle in the `e` form, `m` on
  the focused row, or *Mark paid* in the row menu — three gestures, one
  column, each asking `tx::manual_paid_applies` on the server. The toggle and
  the menu item are **absent** on a row the tick does not apply to, not
  disabled; `m` there says why. Logged as an `edit` of the `paid` field
  ("edited · paid by hand · no → yes"), undone by `u` like any edit.

### Not modeled yet

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
what's already in the DOM.

**A fold's header is a cursor position**, open or closed — the same thing a
closed fold is in Vim, a line you can put the cursor on. `j`/`k` step over a
closed fold's contents in a single press because its rows are not positions
while it is closed, and `za`/`zo`/`zc` act on the fold the cursor is in *or
on*.

That last part is load-bearing and was got wrong: with only rows as
positions, closing a fold hid the very rows the cursor was standing on, so
the cursor fell into the *next* fold and `zo`/`za` could never reopen the one
you had just closed. **A fold could be closed and never reopened** except by
`zR`. There was no position representing a closed fold, which is precisely
what Vim has.

**Closing a fold takes the cursor with it.** `zc` (or `za`, or `zM`) from a
row *inside* a fold leaves the cursor on that fold's header, the way Vim
does — it does not stay behind on a line that is no longer shown. This has
to be explicit because the cursor is an index into the visible positions,
and collapsing a fold shortens that list underneath it: left alone the index
kept its number and quietly meant a different line, so `zc` on the last row
of a three-row fold followed by `j` landed four folds further down. `zR` and
`zo` are the mirror case and keep the cursor exactly where it is, since
nothing it could be standing on disappears.

A row action on a header — `e`, `dd`, `p`, `gl`, `g1`, `x`, `J`/`K`,
`ga` — says *"that is a group, not a row"* rather than doing
nothing at all, the same treatment every other misplaced key gets. Relative
line numbers stay counted among **rows**: a header carries no gutter, so
counting it would leave a hole where nothing is printed; the cursor sitting
on one simply means no row reads `0`.

### What a fold header looks like

A framed container (same visual language as the existing `.card` component),
one per `bucket`, holding the bucket name, a row count, and a **subtotal that
stays visible whether the fold is open or closed**. Deliberately not
color-coded — `bucket` is open-vocabulary and unbounded, while a theme's
palette is a fixed 11 tokens with `pos`/`neg`/`accent`/`info` already spoken
for; a frame plus a label scales to any number of categories without risking a
collision or inventing a hash-based color scheme for marginal gain.

Rows with no `bucket` set land in an **`Uncategorized`** group — grouping never
silently drops a row. Nesting stops at `bucket` — no nested `bill_name`
sub-folds.

### Which group comes first — `S`

Three orders, cycled with `S`, per member (`member_settings.group_order`):

| order | groups by |
|---|---|
| **First seen** (default) | where each bucket's first row sits in the pane's own sort — what grouping always did |
| **A–Z** | bucket name, ignoring case and accents, so *Água* and *Dívidas* file where a Brazilian household expects |
| **By cost** | the subtotal, biggest spend first; a group that nets to income comes after every cost |

- **`Uncategorized` is last in all three.** It is a catch-all, not a
  category, so the order only ever decides among buckets somebody chose.
  First seen alone used to put it first whenever an unfiled row led the
  pane — one income at the top pulled it above everything.
- **A tie keeps first-seen order** — every sort is stable.
- **Three gestures, one row:** `S`, the statusline cell beside sort, and the
  *Groups ordered* radio in `:profile`. The order is decided when the server
  renders the pane, so writing it refreshes the page, like sort.
- **`S` on a flat pane refuses**, in the sort refusal's shape:
  `ledger.group = "none" — S orders groups; :group bucket first`.

### This is the report, not a separate screen

Because the subtotal never disappears, `zM` (close every fold) turns the
ledger straight into a per-category summary — Consumption R$ 1.556,92,
Subscriptions R$ 59,90, and so on — with zero additional screen to build.
Browsing and summarizing are the same mechanism at two zoom levels.

### A change that moves a row re-renders the pane

In a grouped pane — and under `sort = date` / `amount` — where a row sits
follows from what it says. Saving an edit, adding a row, or undoing an edit
can therefore move it: a new bucket belongs in another fold, a new amount
elsewhere in the order. Such an answer is the **whole pane**, not the one row
swapped back in place, with the cursor put back on the row by id (the same
way `J`/`K` keep it on the row they moved). "Venda" filed under `Abc` used to
stay under `Sem categoria` until a reload.

The pane fragment and the page render **one partial**
(`partials/pane_body.html`) from one value (`web::ledger::PaneView`). They
were two copies, and the fragment's only knew the flat layout — every answer
that re-rendered a grouped pane (a rule save included) flattened it.

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

- A small dot (`--neg` token, the same warning color already used for
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
  behavior, unchanged. No new command.
- **The trigger's dot stays current without a reload.** The page draws it
  once; `GET /months/dot` re-renders just the trigger button, from the same
  partial (`partials/month_trigger.html`), at three moments:
  - **after a mutation that can move the answer** — files, row deletes and
    restores, undo/redo, privacy, re-attribution, rule writes, members turned
    off and on. Those routes are listed once, in one group, behind one layer
    (`live::mark_dot`) that adds `ffm:dot` to a successful non-GET's
    `HX-Trigger`, merged with whatever the handler said there. No handler has
    to remember.
  - **after any live event, in any month** — a receipt filed by hand in
    August is exactly what clears a dot seen from September, so the live
    listener refreshes the dot before it filters by the month on screen.
  - **when the tab becomes visible again** — midnight makes bills overdue
    with nothing else happening. Deliberately unguarded: only the button is
    swapped, so an open `e` form, a rule pane or the add row are never in its
    way, and whatever is typed in them stays.

  Debounced (a burst of events is one fetch), quiet (no loader), and per
  viewer like the page.

### The picker is also the year

Each cell carries its month's NET under the label, compact and signed —
`+20k`, `−3,4k`, `~+13k` — and the popup's foot sums the year so far: IN,
OUT and NET in full, the monthly average, and how many months carry a dot.
With `zM` this is the second "report" that needed no report screen.

- **Gray when fine, `--neg` when not.** A deficit month is the thing to find;
  twelve green numbers would only be noise around it. The sign is in the text
  either way, and the `~` leads when any part of the month is a projection.
- **Two marks, two facts.** The month on screen wears the accent — "you are
  here" — and today's month the `--info` bar along its foot. You can be
  looking at March in September.
- **A future month says `—`, never "projected".** The materializer never
  writes past the current month and the pane query reads only rows that
  exist, so a future NET would be a zero nobody earned. (The mock's
  "projected" label promises a rule-based projection this app does not yet
  make.) The summary covers only the months that have happened.
- **The numbers are fetched when the picker opens**, from `GET /months`, and
  on every open. A NET is the real pane query for every member, vault
  included — a tracked income only stops being an estimate when its payslip
  is on disk — so it is a dozen pane reads: fine for a popup somebody asked
  to see, waste on every ledger load for one that is usually closed. The page
  renders the same partial with labels, dots and marks only, holding the NET
  line open at full height so nothing shifts when the numbers land.

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
the whole problem, and SQLite already serializes concurrent writes where a file
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

**These three are read once at startup and are not watched.** (The receipt
vault's *contents* are — see Files changed by hand; its *location* is not.) The earlier
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

All three tiers are read in a **full-width split at the foot of the ledger**
— not a modal, not a full-screen buffer, and no longer a vertical split:

- `:household` (admin only) — the household row as a form, with the operator's
  three paths read-only beneath it.
- `:profile` (or `:me`) — the **session's** `member_settings` as a form. Never
  the focused pane's: focusing your spouse's pane must not let you edit their
  preferences.
- `:config` survives as an **alias for `:household`**, so muscle memory and the
  `?` keymap entry don't break. `:household` is the real name; nothing is called
  after a file any more.

**It was a vertical split, and the reasons given for that were three.** Two
were load-bearing and survive unchanged; the third was a convenience, and in
practice it cost more than it saved:

| the recorded reason | now |
|---|---|
| A full-screen buffer hides the ledger, and `layout.panes` is precisely the setting you need to *watch* take effect | **survives** — the split takes half the ledger area, so the top half stays visible and live. This is why it is half and not full, and why the ledger behind it is **not** dimmed |
| A modal needs a backdrop, scroll-lock and a focus trap, and it implies a settings dialog with an OK button — a second, pending copy of the truth | **survives** — no backdrop, no scroll-lock, no focus trap, still no Save button; every control writes on `change` |
| A vsplit is just a second pane; `H`/`L` already means "move between panes" | **does not** — it put a settings form in the member panes' row, wedged between two people's ledgers, at a width that fits neither a form nor a result list |

A 420px column is the wrong shape for these surfaces because they are **not
about one member and not about one row**. They are about the household. So:
full width, at the bottom, over the ledger — fzf-shaped.

- **It floats over the bottom of the ledger**, and a shadow on its top edge
  says so. The stage keeps `padding-bottom` equal to the split's height while
  it is open, so the focused row can always be scrolled clear of it.
- **The groups flow right and wrap** — a wrapping grid, `repeat(auto-fill,
  minmax(320px, 1fr))`, with `.sect` headings spanning the row so each starts
  a band. Not CSS multi-column: in a fixed-height box that overflows sideways
  instead of scrolling, and it reads one stream down-then-right, which is the
  wrong order for independent setting groups. A 1400px-wide form with one
  400px column of fields and a lot of emptiness is not an improvement on a
  420px column.
- **Controls that have to line up sit in a `.band`, which spans the row.**
  A band is two grid rows — every label in the first, every control in the
  second — so a label that wraps to two lines pushes the whole band's
  controls down together instead of leaving its own input a line below its
  neighbors'. That was the Layout group before: a radio set, a two-up
  `.fgrid` and a lone field, three independent items whose only alignment
  was luck.
  - **Each field inherits the band's rows with `subgrid`**, which is what
    makes the label row sized by the tallest label in the band rather than
    by each field alone. It degrades correctly to one column: a field still
    spans two rows, so the fields stack label-over-control rather than every
    label piling into row one.
  - **A radio group in a band is a labeled `radiogroup`, not a
    `fieldset`/`legend`.** A legend is rendered outside its fieldset's own
    layout box — measured, not assumed — so it cannot be a row of the band's
    grid, and the control under it lands a line off from every other control
    there. The grouping and its name are the same to a screen reader, and
    each group is still scoped to its own form, which is what keeps one
    checked radio per setting rather than one per pane.
  - **A hint travels with its control**, inside the control's row, rather
    than taking a row of its own. A row of its own would line every hint in
    the band up with every other — and in a band holding one short radio set
    beside a tall swatch grid, that put the radios' hint a hundred pixels
    below the radios, level with the bottom of the swatches.
  - **A control keeps its own height.** A subgrid does not inherit the
    band's `align-items`, so a `<select>` stretched to fill its row and the
    currency box came out twice the height of the text input beside it. An
    `<input>` never showed it, having an intrinsic height of its own.
  - **A toggle stays outside the band.** It carries its own label inline, so
    it has nothing to put in the band's label row and would only widen the
    row it joined.
- **`:grep` is deliberately not columned.** It is a result list; columns
  would make the reading order zigzag. It gets the full width as one wide
  list, which is what stops its rows truncating.
- **`:grep` opens with the keyboard on the first hit, and picking one goes
  there.** `j`/`k`/`gg`/`G` walk the hits, `Esc` closes, and `Enter` (or a
  click) lands on the hit's month, its member's pane and the row itself —
  cursor on it, one blink, fold opened if it was closed — and closes the
  list. In the month on screen that is a cursor move; elsewhere it is a page
  load carrying `?row=`, which `init()` lands on and then drops from the URL
  so a refresh does not blink again. Each hit stays a real link, so a
  Cmd-click still opens it in a tab of its own. While a hit has focus the
  list owns the keys, as a field would — otherwise `j` walked the ledger
  cursor behind it too.
- **A bare `:grep` reopens the last search**, re-run so the hits are fresh,
  with the keyboard on the hit picked last. With none, it says it needs a
  term, as before. The term lives in `sessionStorage`: it has to survive the
  page load picking a hit in another month causes, and it is per window for
  the reason `?pane=` is — two windows are two searches. That is still not
  `?split=`: a refresh does not reopen the list.
- **`zz` zooms** the split between half the ledger area and nearly all of it
  — worth it for `:grep` with forty hits. **Every drawer's head says so**: a
  maximize button beside `esc` (`partials/split_actions.html`, one partial
  for all six) writes the same `splitZoom` the key does, and its title names
  the key — *Maximize · zz*. Closing resets it. The rule pane and the log
  don't carry it: they are columns beside a row, with nothing to grow into.
- **`Ctrl-j` drops into the split and `Ctrl-k` off its top field comes back
  out**, the mirror of the rule pane's `Ctrl-h` past its left edge, rotated
  onto the axis the split sits on. Inside it, `Ctrl-h/j/k/l` move between
  controls by where they sit, the same as in every other form. `Esc` closes it in one press **including from inside a
  field**, which is the one place the `typing` guard yields; everywhere else
  `Esc` still means "cancel this edit".

**What stays a column beside its row:** the rule editor (`A`/`I`/`E`) and the
change log (`gl`). Both point at a row — the editor with an arrow level with
it, the log by closing when the cursor leaves it. A pane that is about one
row belongs beside that row.

This deviates from `design/ffm-settings-mockup.html:137`, which puts the
settings pane as a **left** column beside the whole panels grid
(`minmax(300px,420px) 1fr`). That mockup is normative, so the deviation is
recorded here rather than left to be discovered.

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

-- What a new member's own settings START as (migration 0010). Seeded, never
-- inherited: read once, when somebody joins.
default_theme                  text  -- a bundled palette; starts as the founder's
default_layout_panes           text  -- "auto" | "split" | "tabs"
default_layout_min_split_width int   -- the same bounds as the member columns
default_layout_min_width       int
default_layout_max_width       int
default_layout_gap             int
default_density                text  -- "default" | "comfortable" | "compact" (0011)
```

**The household seeds a new member's palette and layout; it never overrides
them.** `:household`'s *For new members* section sets the six `default_*`
values above, with the very controls `:profile` uses (one pair of partials,
pointed at the household's columns), held to the very same bounds (one
validator per value, whoever writes it).

- **Seeded, not inherited.** They are copied into a member's own row when the
  invite is accepted — read inside the accept's transaction — and never read
  for that member again. Changing a default therefore moves nobody's screen,
  which is also why those writes skip the refresh the rest of `:household`
  causes. The alternative, member columns that follow the household until
  changed, was rejected: an admin's tweak would rearrange somebody else's
  screen under them.
- **The invite page opens its palette picker on the household's palette.**
  The newcomer still picks; a refused form comes back on what they picked.
- **The founder sets the first one.** `/setup` stores the admin's own palette
  as the household's default, so a new household starts looking like its
  first member; the layout defaults keep the schema's values.
- **A member adopts them only by asking** — `:profile`'s *Use the household's
  defaults* copies all six into their own row, which stays theirs. The button
  is there only while their values differ, absent rather than disabled.

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
layout_min_width        int   -- px floor per pane; 0 = no floor
layout_max_width        int   -- px cap per pane; 0 = unlimited
layout_gap              int   -- px between panes
ledger_sort             text  -- "manual" | "date" | "amount"
ledger_search_scope     text  -- "pane" | "household"
ledger_show_delta       bool
ledger_group            text  -- "none" | "bucket"
group_fold_default      text  -- "expanded" | "collapsed"
density                 text  -- "default" | "comfortable" | "compact"; see Density
pdf_view                text  -- "png" | "pdf": which side `P` opens a PDF on (0015)
default_visibility      text  -- NULL = follow the household's policy,
                              -- else "household" | "private"
```

- `theme_overrides` stays the one **open-ended** field: a strongly-typed key
  enum still makes an unknown token a real error naming the valid tokens — only
  the source format changed (JSON column vs. TOML file), not the validation.
- `default_visibility`'s nullable tri-state is rendered honestly in `:profile`:
  three radios, the first meaning `NULL` and labeled with what it currently
  resolves to.
- **`layout_min_width` and `layout_max_width` are both rendered into the
  page's own `<style>`**, not carried in a stylesheet. The floor arrived
  first as a constant in `ledger.css`, which was the wrong place for it: how
  narrow a pane may get is a fact about the screen somebody is sitting at,
  exactly like every other `layout_*` value, and a number in a stylesheet is
  one the household cannot reach. Below the floor a row's file rail and
  amount collide with the title, so the stage scrolls sideways instead of
  compressing further — three unreadable panes are worse than two readable
  ones and a scroll. `0` means *no floor*, the same real choice `0` means in
  the cap. Tabs mode lifts it: one pane has nothing to share the stage with,
  so a floor there would only overflow a narrow viewport.
- **`layout_min_split_width` is still rendered into a single media query
  server-side**, from the session member's row instead of from the file. The rule
  is unchanged — `auto` resolves in CSS, never on the server and never in Alpine
  — only the source of the number moved. The pre-auth `.auth` layout has no
  member row and must not read any `layout_*` value.
- **Light/dark is *not* in this table.** Palette is a personal preference and
  belongs to the member; light-versus-dark is a property of the *device* — the
  same person uses a bright laptop and a dark phone — so it lives in the
  `ffm.theme` cookie. Keeping the two separate is what makes a pre-auth page
  themeable at all: `/login` can honor your light/dark without knowing who you
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
localizing a number, rather than merely discouraged from it.

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
- `ffm.palette` carries the palette of whoever last signed in on this device,
  so the sign-in page after `:logout` looks like the app it came from. Written
  from `member_settings.theme_name` at sign-in, on a palette change and at
  sign-out; read only on the pre-auth pages; `ffm` when absent or unknown.
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
| two counts in one message | *"10 installments · 01/2026–10/2026"* (a count and a range) |
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
