# FFM — decisions in full

The long form of decisions that `CLAUDE.md` states in brief. Each section was
moved here **verbatim** from `CLAUDE.md` when that file outgrew the
150k-character limit Claude Code loads; `CLAUDE.md` keeps the rule and its key
names and points here. A section here is exactly as decided as its summary
there — **read it before changing what it covers, and amend both** when a
decision moves. "Above", "below" and "the gotcha on …" inside a moved section
still point into `CLAUDE.md`.

Find a section with `rg -n '^##' design/decisions.md`.

## Recurring rules — bills and their files

### One bill, several months (`spans_period`)

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

### Recognizing, binding and reading a bill

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

### The vault, watched and live

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

### PDF passwords

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

### Trim and shrink

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

### The `P` carousel

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
  server-side by `pdftoppm`** (poppler, already required for `pdftotext`)
  and shown as images; a photo is itself, a text file is text.
  **As JPEG, kept by the browser, and loaded as they come into view**
  (`services::pdfpage`): PNG's encoder was the cost — 0.3–2.5 s a page, against
  45–190 ms — the page URL carries the file's version (`?v=`) so a reopen asks
  for nothing, and only the page `P` opens on loads at once.
  - **At the size it is shown, 150 dpi or 300** — amended from "300 dpi,
    every page". 300 was for a statement drawn small in PDF points, stretched
    at fit width; but `P` opens at fit height, where 150 covers a page on a
    laptop and on a phone, and an A4 page at 300 decodes to ~33 MB against
    ~8 MB — a 10-page statement scrolled through held 332 MB of pixels. The
    page asks for 300 (`?dpi=300`) once its laid-out width outgrows what 150
    covers: fit width on a large screen, a zoom, a bigger window
    (`sharpen` in `ledger.js`). A page already drawn is raised only near the
    view, the rest as they come near, so a zoom redraws what is looked at.
    Only the two, so a page is drawn at most twice.
  - **Kept on the server too, three drawn at a time.** A drawn page is kept
    in `ffm.pages/` beside the database — never in the vault, which is
    browsed by hand — named after the file's path, size and modified time,
    so a replaced file is a new name; a sweep drops drawings over 30 days
    old, at startup and daily. A second device, or the installed app beside
    Safari, draws nothing. At most three `pdftoppm` run at once
    (`pdfpage::DRAWING`), the trim view's included: a row's three statements
    started twenty-odd together.
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
  only warmed in the browser's cache, which the pages do reach now (`?v=`,
  `immutable`): a stage kept in the DOM also keeps its scroll, zoom and
  decoded pages, and shows at once.

### Deleting files

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

## The ledger bar

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

## Installed on a phone (PWA)

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

## Development workflow

### A dev server says it is one

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

### Releases

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

### Freeing the dev port

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

### Browser tests (`just e2e`)

- **`just e2e` runs the browser tests, in a container** — Playwright and
  TypeScript, Chromium and WebKit, on data `ffm-anon` makes up. `E2E_SEED=123`
  reproduces a run's data; `just e2e-report` serves the report and traces;
  `just e2e-clean` drops this worktree's build volume. See `scripts/e2e.sh`.
  - **Apple's `container` on macOS, `podman` on Linux**, and only flags both
    have. Its builder asks for Rosetta, so without Rosetta
    `~/.config/container/config.toml` needs `[build] rosetta = false` and a
    service restart; the script says so. It also needs `--dns` here: the
    gateway's resolver does not answer.
  - **Worktrees cannot collide.** Nothing is published, and the container, the
    build volume and the report are named after the worktree. An Apple volume
    is a block device on one VM, so each worktree has its own Cargo registry
    too; the image is tools only and shared.
  - **A run sees an allow-list, never the repository**: a worktree can hold
    `ffm.db`, `receipts/`, `.env` and `_backups/`, not all of it ignored by
    git, so `stage` copies named paths and the image's build context is
    `e2e/image/`. **A new top-level folder the build needs goes in that list.**
  - **The data is `seeds/e2e.sql`, shuffled per run.** `ffm-anon` needs a source
    database, so the seed is fictional and holds every state a row can be in;
    `tests/e2e_seed.rs` fails when a migration breaks it. Names and amounts
    differ per seed, so **a spec never hardcodes them**: it finds its row with
    `app.sql`, by what the row is.
  - **Each test has its own server on a copy of the data**, on tmpfs, because
    `u` depends on who did what. `test.use({ today: '2026-10-15' })` moves its
    clock, which is why the binary is a debug build. Sessions are already in
    the data, so only the sign-in specs go through the form. `/setup` exists
    only with no members, so its spec takes the `blank` fixture: a server on an
    empty database, not the shuffled data.
  - **`vim(page, 'gT')` presses Shift on its own first**, as a keyboard does —
    the case a synthetic event cannot test. Playwright types with no delay, so
    a key sent the instant something opens is a real race, not test noise.
    The one race that is the test's own: WebKit can land a pre-auth page's
    `autofocus` in the middle of a `fill`, so a form's helper calls
    `autofocused(page)` first.
  - **Linux WebKit is close to Safari and is not Safari.** It does not replace
    the check below, and a WebKit-only failure wants checking in real Safari:
    `x-show` lands over a second late there, which real Safari does not do.
