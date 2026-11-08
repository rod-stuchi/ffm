/* The ledger's keymap and its Alpine component — `ledger()`, on the shell in
 * templates/pages/ledger.html, and the plain functions the rule pane's own
 * component shares (the money mask, Ctrl-h/j/k/l between fields).
 *
 * It was an inline <script> in that page, ~200 KB of it sent again with every
 * month change. As a file it is the same for every page and every member, so
 * the browser keeps it (`assets::url`, `immutable`). What the server decides
 * per page — the pane to land in, the member's settings, the catalog's words
 * in their language — is the `window.FFM` block the page renders just before
 * this file, and is read from there as `FFM.<name>`. NOTHING in this file is
 * templated: a `{{ … }}` here would reach the browser as those characters,
 * and `just tpl-check` refuses one.
 */
/* The household's number format — decimals and the two separators, from
   `household_settings.locale_currency`, the one value every money surface in
   this app reads. Not the symbol: the mask writes a bare number and the
   symbol is the row's job. */
const FFM_MONEY = FFM.money;

/* The amount mask, as a plain function rather than a method.
 *
 * Three forms need it — the add row, the inline edit row and the rule pane —
 * and the rule pane is its own Alpine component. A method on `ledger()` would
 * reach it only through scope chaining, which is a subtlety to rely on for
 * something every money field in the app depends on.
 *
 * Only digits are ever typed: the separators are inserted right-to-left into
 * minor units, the way a POS terminal reads them. `1234567` becomes
 * `12.345,67` a keystroke at a time, or `12,345.67` in a USD household.
 *
 * The element carries the integer in `data-cents` and the text is always
 * painted FROM it, so the display and the value cannot drift. What is
 * submitted is the formatted string, which `Cents::parse` reads back to
 * exactly that integer — the mask only ever produces the shapes that parser
 * is built for, so this stays one parse on the way in rather than a second
 * field to keep in step.
 */
function ffmFormatMinor(units) {
  const f = FFM_MONEY;
  const s = String(units).padStart(f.decimals + 1, '0');
  const whole = s.slice(0, s.length - f.decimals);
  const frac = s.slice(s.length - f.decimals);
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, f.group);
  return f.decimals ? grouped + f.decimal + frac : grouped;
}

function ffmMoney(el) {
  if (el.dataset.masked) return;
  el.dataset.masked = '1';

  const paint = (units) => {
    el.dataset.cents = String(units);
    el.value = units ? ffmFormatMinor(units) : '';
  };
  /* Seeded from `data-cents` — the row's own stored integer — never from the
     rendered text. An edit form that re-parsed its own display would be the
     drift this mask exists to prevent. */
  paint(parseInt(el.dataset.cents || '0', 10) || 0);

  el.addEventListener('beforeinput', (e) => {
    if (!e.inputType.startsWith('insert')) return;
    const txt = e.data != null ? e.data
      : (e.dataTransfer ? e.dataTransfer.getData('text/plain') : '');
    if (!txt) return;
    /* A letter is refused BEFORE it lands. Letting it through would mean
       typing over a selected "59,90" leaves no digits, and the mask would
       repaint the field empty. A paste is allowed as long as it carries
       digits: "R$ 12,34" is fine, and the mask keeps just the digits. */
    const bad = e.inputType === 'insertFromPaste'
      ? !/[0-9]/.test(txt) : /[^0-9]/.test(txt);
    if (bad) e.preventDefault();
  });

  el.addEventListener('input', () => {
    const digits = el.value.replace(/[^0-9]/g, '');
    paint(digits ? parseInt(digits, 10) : 0);
    /* The caret belongs at the end: digits are read right-to-left, so there
       is nowhere else meaningful for it to be. */
    el.setSelectionRange(el.value.length, el.value.length);
  });
}

/* `form.reset()` puts the DISPLAY back but not the integer behind it. */
function ffmResetAmounts(form) {
  form.querySelectorAll('.amt-in').forEach((el) => {
    el.dataset.cents = '0';
    el.value = '';
  });
}

/* Ctrl-h/j/k/l inside a form: focus moves to the field that sits that way on
   SCREEN, not the next one in DOM order, so a field grid navigates the way it
   reads without the markup having to match the layout. Ported from the mock
   (design/ffm-app-mockup.html, fieldFocusMove), where it was built and never
   carried over — the spec documented a key the app did not have.

   A plain function, not a method, for the reason the money mask is one: the
   rule pane is its own Alpine component.

   Returns false when there is no field that way — the form's edge, which the
   caller turns into "back to the ledger". */
const FFM_DIRS = { h: 'left', j: 'down', k: 'up', l: 'right' };

/* Not a file input: the Bill card's is visually hidden behind its label, so
   landing on it shows no focus at all, and its box is a 1px stand-in that
   would skew every distance. Tab still reaches it.

   A rule pane section's header IS a stop, though it is a button: with a
   section closed it is the only thing of it on screen, and without it
   Ctrl-j walked straight past Files to the next row. Enter or Space on it
   opens the section. */
function ffmFields(box) {
  return [...box.querySelectorAll('input:not([type=hidden]):not([type=file]), select, textarea, .rp-sect-head')]
    .filter(el => !el.disabled && el.offsetParent !== null);
}

function ffmFieldMove(box, dir) {
  const els = ffmFields(box);
  if (!els.length) return false;
  // From wherever focus is inside the form, field or not — Save, a toggle's
  // label. From outside it, the first field.
  const from = box.contains(document.activeElement) ? document.activeElement : null;
  if (!from) { els[0].focus(); return true; }
  const a = from.getBoundingClientRect();
  const horiz = dir === 'left' || dir === 'right';
  const overlaps = (aMin, aMax, bMin, bMax) => aMin < bMax && bMin < aMax;
  const inDirection = els.filter(el => el !== from)
    .map(el => ({ el, r: el.getBoundingClientRect() }))
    .filter(({ r }) => {
      if (dir === 'right') return r.left > a.left + 4;
      if (dir === 'left') return r.right < a.right - 4;
      if (dir === 'down') return r.top > a.top + 4;
      return r.bottom < a.bottom - 4;
    });
  if (!inDirection.length) return false;
  const aligned = inDirection.filter(({ r }) => horiz
    ? overlaps(a.top, a.bottom, r.top, r.bottom)
    : overlaps(a.left, a.right, r.left, r.right));
  // Sideways only ever means "same row": with nothing left or right on this
  // row, that is the form's edge, not a license to jump to a field further
  // down.
  if (horiz && !aligned.length) return false;
  // Within the aligned pool the overlap test already did the real work of
  // "same row/column", so the cross-axis term only breaks near-ties. Weighted
  // heavily there, a wide full-row field's far-off center outscored a narrow
  // field's much smaller primary-axis distance, and down from a 1-of-3
  // column field jumped clean over the next full-width row. The unaligned
  // fallback has no such guarantee, so it leans on the cross axis much harder.
  const pool = aligned.length ? aligned : inDirection;
  const cross = aligned.length ? 0.2 : 2.2;
  let best = null;
  let bestDist = Infinity;
  for (const { el, r } of pool) {
    const dx = (r.left + r.width / 2) - (a.left + a.width / 2);
    const dy = (r.top + r.height / 2) - (a.top + a.height / 2);
    const dist = horiz ? Math.abs(dx) + Math.abs(dy) * cross : Math.abs(dy) + Math.abs(dx) * cross;
    if (dist < bestDist) { bestDist = dist; best = el; }
  }
  if (best) { best.focus(); if (best.select) best.select(); }
  return !!best;
}

/* The loader's bookkeeping. `next` is the anchor the next `htmx.ajax` should
   use — set and cleared around the one call, which is safe because htmx
   fires `beforeRequest` synchronously inside it. `counts` is per element, so
   two requests on one row do not clear each other's loader. */
const ffmBusy = { next: null, counts: new WeakMap() };

/* The read-only row an open edit form replaced, by id — the server's own
   markup, kept so Esc and parking a draft can put it back instantly instead
   of asking for it again. Nothing was written, so it is exactly what the
   server last rendered. Dropped the moment it stops being that: a save, or
   anything else inside the form that writes (removing a file). */
const ffmRowHtml = new Map();

/* What one response swapped, element by element, waiting for the ledger's
   upkeep to run once over all of it (`afterSwap`). DOM nodes, which is why
   it is not Alpine state. */
const ffmSwapped = [];

function ledger() {
  return {
    // Where the server put us: your own pane by default, or whatever `?pane=`
    // asked for. Never a hardcoded 0 — that is somebody else's ledger for
    // everyone but the first member.
    pane: FFM.pane,
    row: 0,
    adding: null,
    kind: 'out',
    confirming: null,
    /// The `ga` picker while it is open: the row, its pane, whether it is
    /// private, and which member is highlighted. Visual state — nothing is
    /// sent until a name is picked.
    giving: null,
    /// A bill dropped anywhere on the page that more than one rule
    /// recognizes: `{ file, sel }` while the picker asks which row.
    billPick: null,
    tip: null,
    /// Where the pointer last was, in screen coordinates — see `mouseMoved`.
    pointerAt: null,
    tipStyle: '',
    carousel: null,
    // `:charts` open; and the `g` of a `gg` typed inside it.
    charts: false,
    chartsBuf: '',
    /// The trim-and-shrink view while it is open: the staged token, which
    /// pages are kept, the focused page, whether to shrink, and what Save
    /// does with the finished file. Visual state — unsubmitted choices; the
    /// server draws, sizes and builds.
    prep: null,
    /// The file the carousel is asking to remove: `{ id, kind, label }`.
    removingFile: null,
    /// `b` in the carousel: the row and slot a typed code is for, while its
    /// box is open, and what is typed in it.
    codeFor: null,
    code: '',
    /// `a` in the carousel: the row and slot the file picker is for. Its own
    /// flag, never the one that says a box is open (see CLAUDE.md's gotcha
    /// about `@click.outside` and a file picker).
    appendFor: null,
    /// "Attach anyway?": `{ message, mismatch, resolve }` while the server's
    /// question is on screen. See `askAttach`.
    attachAsk: null,
    /// The progress box: `{ job, steps: [{ text, done }], leaving }` while a
    /// Save is running — see `fidgetStart`.
    fidget: null,
    fidgetTimer: null,
    /// `d` pressed once inside the carousel, waiting for the second.
    carouselBuf: '',
    /// How the carousel's page images are fitted: `height` — every page
    /// whole, how the carousel opens — or `width`, read with `j`/`k` and
    /// `H`/`M`/`L`. `f` flips it.
    pageFit: 'height',
    removeFileTitle: FFM.removeFileTitle,
    /// The carousel is showing a PDF in the browser's own viewer rather than
    /// as page images (`v`). Each `P` starts on the member's own side
    /// (`data-pdf-view`, from `:profile`); `v` flips it while the carousel is
    /// open, across the row's files.
    pdfView: false,
    /// A phone's keyboard is up for the keymap (`toggleKeyboard`) — the
    /// intent, kept while focus moves to the command bar or a form field and
    /// dropped the moment focus goes nowhere (Done, a tap elsewhere).
    keyboard: false,
    /// The key strip's `ctrl`: the next key typed is sent with Ctrl held.
    ctrlArmed: false,
    keymap: false,
    /// The keymap's search — what is typed, and "matching/total" beside it.
    kmQuery: '',
    kmCount: '',
    /// The `g` of a `gg` typed inside the keymap.
    kmBuf: '',
    monthsWord: FFM.monthsWord,
    attaching: null,
    /// The row the file picker is for. NOT `attaching`, which is whether the
    /// `+` menu is open: that menu closes on `@click.outside`, and the picker
    /// is opened with a synthetic click on an input outside it — so one flag
    /// for both was emptied before the dialog even appeared, and the file
    /// picked afterwards was dropped without a request or a word.
    pickFor: null,
    dragging: false,
    splitZoom: false,
    // The question every `u`/`U` asks first — `{ id, dir, member, period,
    // title, message, lines, yes, hint }`, every string the server's. A
    // confirmation, like `confirming` — never data.
    undoAsk: null,
    // The month a `[`/`]`/`T` (or a click) asked about, when drafts are
    // parked — answered only by the same request again. Visual, like
    // `undoAsk`: nothing is written either way.
    monthConfirm: null,
    nu: FFM.nu,
    cursorOnFold: FFM.cursorOnFold,
    grepNeedsTerm: FFM.grepNeedsTerm,
    liveFromDisk: FFM.liveFromDisk,
    // `refreshDot`'s debounce: a burst of events is one re-fetch.
    dotTimer: null,
    /// Live events waiting out their 250ms, and panes waiting for a form to
    /// close before they are re-rendered (`flushLive`).
    liveQueue: [],
    liveTimer: null,
    livePanes: {},
    liveUnderEdit: FFM.liveUnderEdit,
    addSpend: FFM.addSpend,
    addIncome: FFM.addIncome,
    // The household's number format, from the one value that owns it. Every
    // money display in the app reads the same setting, so the mask cannot
    // disagree with the rows above it.
    sort: FFM.sort,
    nextSort: FFM.nextSort,
    // The two clickable cells show a SHORT label — `rnu` / `nu` / `nonu` are
    // the words a member types back at `:set`, which is the same reason the
    // sort-refusal message names `manual` and `date` rather than translating
    // them.
    get nuLabel() { return { relative: 'rnu', absolute: 'nu', none: 'nonu' }[this.nu] || this.nu; },
    get sortLabel() { return this.sort; },
    commanding: false,
    // The `:` bar's table — names, aliases, the closed set each argument
    // comes from, and what each does — rendered from `web::commands::ALL`.
    cmds: FFM.cmds,
    cmdHintDefault: FFM.cmdHintDefault,
    // Per MEMBER, in this browser: two people on one laptop keep their own.
    cmdHistKey: 'ffm.cmds.' + FFM.me,
    // The `/` bar's own history, kept the same way.
    searchHistKey: 'ffm.searches.' + FFM.me,
    // Tab's cycle — `{ list, i, head, tail }`, `head` being the text before
    // the word being completed — and Ctrl-k's walk, `{ i, prefix, draft }`,
    // in whichever bar is open. Both end on any other key.
    wild: null,
    recallAt: null,
    logFor: null,
    logOpen: [],
    logCursor: 0,
    // Fold state is PURE visual state — the server already rendered every row
    // and its subtotal, so folding shows or hides what is in the DOM. That is
    // exactly the division of responsibility this app draws.
    //
    // Keyed by bucket (`data-fold`), and kept in `localStorage` per member
    // (`foldsKey`), so a fold closed in October is still closed in November —
    // a month change is a page load, and this map used to start empty on
    // every one — and still closed after the tab or the installed app is. `keys` are folds toggled one by one; `all` is
    // what `zM`/`zR` last said, which also reaches a bucket only another month
    // has; `foldsClosed` is the member's *Folds start* setting, for anything
    // neither has spoken about.
    folds: {},
    foldsAll: null,
    foldsClosed: FFM.foldsClosed,
    // Per MEMBER, like `cmdHistKey`: two people on one laptop fold their own.
    foldsKey: 'ffm.folds.' + FFM.me,
    // Parked drafts, keyed by row id. They live HERE — on the shell, outside
    // every hx-target — because an HTMX swap destroys the Alpine state inside
    // it, and a draft that vanished with the row it belonged to would leave
    // nothing to recover. Several can be parked at once, which is why this is
    // a map rather than one slot.
    drafts: {},
    // Parked rule-pane drafts, by rule id — `E`'s counterpart of `drafts`.
    // `ruleFor` is the open `E` pane: which row it points at, which rule.
    ruleDrafts: {},
    ruleFor: null,
    editing: null,
    selected: {},
    count: '',
    search: '',
    searching: false,
    matches: [],
    matchAt: 0,
    command: '',
    kind_: 'receipt',
    menuStyle: '',
    // The row menu (long press / right-click): `{ id, rule }` while open.
    rowMenu: null,
    rowMenuStyle: '',
    // A long press in progress, and whether one just opened the menu — the
    // click its lift sends is not a tap.
    press: null,
    pressFired: false,
    // The statusline's message cell: the last command typed, or `/`'s hit
    // count while a search is up. What the app SAYS is a toast (`say`).
    msg: '',
    lastCmd: '',
    // On screen now — `{ id, text, bad, timer }`, newest last, three at most.
    toasts: [],
    toastSeq: 0,
    // Everything said in this window, newest first — the bell's list, kept in
    // `sessionStorage` (`ffm.notes`) so a reload or a month change keeps it.
    notes: [],
    notesUnread: 0,
    bellOpen: false,
    // Requests in flight with nowhere closer to show themselves than the
    // statusline. Row and pane anchors are counted in `ffmBusy`, outside the
    // component: they hold DOM elements, which Alpine's reactivity would wrap.
    busyCount: 0,
    busyOffline: FFM.busyOffline,
    pending: '',
    notARule: FFM.notARule,
    ruleDeleted: FFM.ruleDeleted,
    paidRefusedTracked: FFM.paidRefusedTracked,
    paidRefusedOneOff: FFM.paidRefusedOneOff,
    errorNotWritten: FFM.errorNotWritten,
    prepNotPdf: FFM.prepNotPdf,
    onlyPdf: FFM.onlyPdf,
    prepKeepOne: FFM.prepKeepOne,
    prepFailed: FFM.prepFailed,
    askDeclined: FFM.askDeclined,
    prepTimer: null,
    notACommand: FFM.notACommand,
    groupOrder: FFM.groupOrder,
    groupOrderNeedsGroup: FFM.groupOrderNeedsGroup,
    groupSelectNeedsGroup: FFM.groupSelectNeedsGroup,
    unsavedTag: FFM.unsavedTag,
    unsavedRuleTag: FFM.unsavedRuleTag,
    draftsParked: FFM.draftsParked,
    draftsParkedOne: FFM.draftsParkedOne,
    monthLeaveKey: FFM.monthLeaveKey,
    monthLeaveClick: FFM.monthLeaveClick,
    monthAlreadyCurrent: FFM.monthAlreadyCurrent,
    giveNobody: FFM.giveNobody,
    giveMovedKey: FFM.giveMovedKey,
    detailsNone: FFM.detailsNone,
    noFiles: FFM.noFiles,
    fileNotTracked: FFM.fileNotTracked,
    admin: FFM.admin,
    reattributeAdminOnly: FFM.reattributeAdminOnly,
    noFile: {
      barcode: FFM.noFile.barcode,
      scheduled: FFM.noFile.scheduled,
      receipt: FFM.noFile.receipt,
    },

    rows() {
      const panes = document.querySelectorAll('.pane');
      if (!panes[this.pane]) return [];
      // A DESCENDANT selector, not a child one: a grouped pane nests its rows
      // inside folds, and a child selector quietly returns nothing there —
      // taking j, k, dd and gl with it.
      //
      // These are CURSOR POSITIONS, not rows — a fold's header is one of
      // them, exactly as a closed fold is a line in Vim.
      //
      // That is what makes a closed fold reachable at all. `foldAt` anchors
      // to the position under the cursor, and closing a fold hides its rows,
      // so without the header there was nothing left to stand on: the cursor
      // fell into the next fold and `zo`/`za` could never reopen the one you
      // had just closed. Only `zR` recovered it.
      //
      // Hidden rows are still left out, which is how `j`/`k` step over a
      // closed fold's contents in one press: there is nothing to skip if it
      // was never in the list.
      //
      // Fold membership is read from `this.folds`, NOT from layout. Alpine
      // applies the `x-show` on `.foldrows` on its own schedule — later than
      // `$nextTick`, and later again in a background tab — so for a frame
      // after a fold key the DOM still describes the previous state. An index
      // taken in that gap is a different line, silently. The model is the
      // fold's truth and it is already correct synchronously.
      //
      // `offsetParent` still decides everything else, which is the only other
      // thing that hides a row here.
      return Array.from(
        panes[this.pane].querySelectorAll('.foldhead, div[id^="row-"] .row'),
      ).filter(el => {
        const section = el.closest('.fold');
        if (section && !el.classList.contains('foldhead')) {
          return !this.closed(this.foldIdOf(section));
        }
        return el.offsetParent !== null;
      });
    },

    /// The position under the cursor, when it is a row.
    ///
    /// A fold header is a position too, and almost nothing that acts on a row
    /// means anything there — so this says so rather than doing nothing at
    /// all, which is the treatment every other misplaced key in this app
    /// already gets.
    rowUnderCursor() {
      const id = this.focusedId();
      if (!id && this.onFoldHead()) this.say(this.cursorOnFold, true);
      return id;
    },

    onFoldHead() {
      const el = this.rows()[this.row];
      return !!el && el.classList.contains('foldhead');
    },

    paint() {
      // The cursor-left-the-row checks come FIRST, before the empty-pane
      // return below: moving to a pane with no rows in it is exactly when a
      // draft would otherwise be stranded, and it is not a rare case in a
      // household where one person has nothing this month.
      // The positions are read once, while the layout is clean, and again
      // only when one of the three below may have changed them: reading them
      // after the focus class came off made the browser lay the page out an
      // extra time on every `j`.
      let rows = this.rows();
      const focused = this.idOf(rows[this.row]);
      let changed = false;
      if (this.editing && this.editing !== focused) {
        this.parkDraft(this.editing);
        changed = true;
      }
      // The `E` pane is parked the same way: its arrow points at one row,
      // and a pane about a row the cursor has left is worse than none.
      if (this.ruleFor && this.ruleFor.row !== focused) {
        this.parkRule();
        changed = true;
      }
      // The log pane closes when the cursor leaves the row it is about — one
      // check in the render loop, the same one that parks a draft. A pane
      // still showing the previous row's history is worse than no pane.
      if (this.logFor && this.logFor !== focused) {
        this.closeLog();
        changed = true;
      }
      // The row menu is about one row, too.
      if (this.rowMenu && this.rowMenu.id !== focused) this.rowMenu = null;

      // ABOVE the empty-pane return below, and deliberately: which member is
      // focused has nothing to do with whether they have rows this month. In
      // tabs mode the other panes are `display: none` and report no rows at
      // all, so left below the return the URL simply stopped tracking — every
      // tab read as the first one.
      this.trackPane();
      // The selection's cell speaks about the focused pane, so a pane change
      // (H/L/gt) is a new question. It asks the server only when the pane or
      // its selected ids actually changed — not on every j/k.
      this.refreshSelection();

      document.querySelectorAll('.row.is-focus, .foldhead.is-focus')
        .forEach(r => r.classList.remove('is-focus'));
      if (changed) rows = this.rows();
      if (!rows.length) return;
      this.row = Math.max(0, Math.min(this.row, rows.length - 1));
      rows[this.row].classList.add('is-focus');
      rows[this.row].scrollIntoView({ block: 'nearest' });
      this.renumber(rows);
      // The arrow follows the row it points at — including when a swap
      // re-renders the pane under it, which is when it would otherwise be
      // left aiming at wherever the old row used to be.
      this.aimArrow();
    },

    focusedId() {
      return this.idOf(this.rows()[this.row]);
    },

    /// A position's row id. A fold header has no row wrapper, so it has no
    /// id — which is exactly what every row action already treats as
    /// "nothing to act on".
    idOf(el) {
      const wrap = el && el.closest('div[id^="row-"]');
      return wrap ? wrap.id.slice(4) : null;
    },

    /// The ledger's upkeep after a swap: draft marks, the selection, the
    /// cursor. Once per response — htmx reports every element a response
    /// swapped one by one, the target and then each out-of-band piece (the
    /// pane's rows, its total, the tab's balance, the household's), so a
    /// `J` ran all of it thirteen times, its live echo included, and with a
    /// draft parked put thirteen on the bell's count. htmx swaps everything
    /// first and then reports it all in one go, so a microtask runs after
    /// the last of them. And only for a response that reached the ledger: the
    /// month's dot, the selection's cell or the carousel swapped on its own
    /// has no row to repaint.
    afterSwap(el) {
      if (ffmSwapped.push(el) > 1) return;
      queueMicrotask(() => {
        const els = ffmSwapped.splice(0);
        const wrap = document.querySelector('.stagewrap');
        if (wrap && !els.some(t => !t.isConnected || wrap.contains(t) || t.contains(wrap))) return;
        this.markDrafts();
        this.selAsked = null;
        this.paintSelection();
        this.paint();
      });
    },

    // --- the loader --------------------------------------------------------

    /// `htmx.ajax`, naming where its loader belongs. Most requests find their
    /// own place (`busyAnchor`); this is for the ones whose target is not
    /// where you are looking — a side pane opening into an empty mount, a
    /// move swapped into `body` — and for background requests, which pass
    /// `'none'`.
    /// `GET /live`: the server says a row changed — a file filed or removed
    /// by hand in the vault, or one another member uploaded — and this page
    /// fetches that row again, as itself, if it is on screen. The event
    /// carries an id and a month and nothing else; privacy, language and
    /// sort all stay with the fetch. `EventSource` reconnects by itself.
    listenLive() {
      if (!window.EventSource) return;
      const es = new EventSource('/live');
      // This device's session was revoked while the page was open — another
      // device's "Sign out other devices", an admin, a reset. The server says
      // so only to the page whose session is gone; go to /login now rather
      // than at the next click, and stop the stream reconnecting.
      es.addEventListener('signedout', () => {
        es.close();
        location.href = '/login?out=1';
      });
      // A step of this page's own long request — the box above Save.
      es.addEventListener('progress', (e) => {
        try {
          const p = JSON.parse(e.data);
          this.fidgetStep(p.job, p.text);
        } catch (_) { /* not ours to read */ }
      });
      // Every change, from the change log (`services::live_feed`) — a row to
      // re-fetch, or a pane where rows came, went or moved. Queued for 250ms
      // and acted on once each: a burst used to fire one request per row at
      // once, from the same source element, and htmx kept only the last —
      // so a receipt uploaded on another computer showed paid there and not
      // here.
      es.addEventListener('row', (e) => {
        let ev;
        try { ev = JSON.parse(e.data); } catch (_) { return; }
        // Any month, not only the one on screen: the dot is about the OTHER
        // months, and a receipt filed by hand in August is what clears it.
        this.refreshDot();
        this.liveQueue.push(ev);
        clearTimeout(this.liveTimer);
        this.liveTimer = setTimeout(() => this.flushLive(), 250);
      });
    },

    /// Acts on the queued live events: each pane once, each row once — a row
    /// inside a pane that is re-rendered anyway needs no fetch of its own.
    flushLive() {
      const period = this.shell().dataset.period;
      const events = this.liveQueue.splice(0);
      const rows = new Map();
      for (const ev of events) {
        if (ev.period && ev.period !== period) continue;
        if (ev.pane) {
          // Member 0 is every pane: a row moved between two of them.
          const members = ev.member
            ? [String(ev.member)]
            : [...document.querySelectorAll('.rows[id^="rows-"]')].map(r => r.id.slice(5));
          members
            .filter(m => document.getElementById('rows-' + m))
            .forEach(m => { this.livePanes[m] = true; });
        } else {
          const id = String(ev.id);
          rows.set(id, rows.get(id) || !!ev.disk);
        }
      }
      // Several rows of one pane at once — a month's folder filed by hand, a
      // re-filing — is one fetch of the pane rather than one per row, each
      // of which carried the pane's totals anyway. Not while a row there is
      // open in `e`: that holds a pane's refresh back until the form closes,
      // and the other rows need not wait for it.
      const byPane = new Map();
      for (const id of rows.keys()) {
        const pane = document.getElementById('row-' + id)?.closest('.rows[id^="rows-"]');
        if (pane) byPane.set(pane, (byPane.get(pane) || []).concat(id));
      }
      for (const [pane, ids] of byPane) {
        if (ids.length <= 3 || (this.editing && pane.querySelector('#row-' + this.editing))) continue;
        this.livePanes[pane.id.slice(5)] = true;
        if (ids.some(id => rows.get(id))) this.say(this.liveFromDisk, false);
      }
      for (const [id, disk] of rows) {
        const el = document.getElementById('row-' + id);
        if (!el) continue;
        const pane = el.closest('.rows[id^="rows-"]');
        if (pane && this.livePanes[pane.id.slice(5)]) continue;
        // The row being edited is never swapped under the form: `savedSwap`
        // would read the swap as a finished save and throw the draft away.
        // Its kept copy goes stale instead, so Esc fetches the new one.
        if (String(this.editing) === id) {
          ffmRowHtml.delete(id);
          if (disk) this.say(this.liveUnderEdit, false);
          continue;
        }
        // Its own element as the source, so one row's request never cancels
        // another's.
        this.ajax('GET', '/tx/' + id, { source: el, target: el, swap: 'outerHTML' }, 'none')
          .then(() => { if (disk) this.say(this.liveFromDisk, false); });
      }
      this.refreshPanes();
    },

    /// Re-renders the panes the live channel named. A pane with a row open in
    /// `e` waits — swapping its rows would take the form, and what is typed
    /// in it, with them — until the form closes (the `editing` watch in
    /// `init`).
    refreshPanes() {
      const period = this.shell().dataset.period;
      for (const member of Object.keys(this.livePanes)) {
        const rowsEl = document.getElementById('rows-' + member);
        if (!rowsEl) { delete this.livePanes[member]; continue; }
        if (this.editing && rowsEl.querySelector('#row-' + this.editing)) continue;
        delete this.livePanes[member];
        // The cursor stays on its row, by id: the rows around it may have
        // come, gone or moved.
        const keep = this.focusedId();
        this.ajax('GET', '/panes/' + member + '/' + period,
                  { source: rowsEl, target: rowsEl, swap: 'none' }, 'none')
          .then(() => { if (keep) this.cursorTo(keep); });
      }
    },

    /// The month picker's trigger and its dot, re-rendered alone
    /// (`GET /months/dot`) — debounced, quiet (no loader), and swapped over
    /// the button only.
    refreshDot() {
      clearTimeout(this.dotTimer);
      this.dotTimer = setTimeout(() => {
        const period = document.querySelector('.ledger-shell')?.dataset.period || '';
        if (!document.querySelector('.mnav .mlabel')) return;
        this.ajax('GET', '/months/dot?period=' + encodeURIComponent(period),
          { target: '.mnav .mlabel', swap: 'outerHTML' }, 'none');
      }, 250);
    },

    ajax(method, url, opts, anchor) {
      ffmBusy.next = anchor || 'status';
      try { return htmx.ajax(method, url, opts); }
      finally { ffmBusy.next = null; }
    },

    /// The focused row's wrapper: the anchor for anything done to the row
    /// under the cursor. The statusline when there is no row to stand on.
    focusedRow() {
      return this.rows()[this.row]?.closest('div[id^="row-"]') || 'status';
    },

    /// Where a request's loader goes, closest first: the row it is about, the
    /// side pane it came from, and otherwise the statusline.
    busyAnchor(elt) {
      const next = ffmBusy.next;
      if (next) return next === 'none' ? null : next;
      if (!elt || !elt.closest) return 'status';
      if (elt.closest('[data-busy="none"]')) return null;
      return elt.closest('div[id^="row-"]') || elt.closest('.rulepane') || 'status';
    },

    busyStart(anchor) {
      if (!anchor) return;
      if (anchor === 'status') { this.busyCount++; return; }
      const n = ffmBusy.counts.get(anchor) || 0;
      ffmBusy.counts.set(anchor, n + 1);
      if (n) return;
      anchor.classList.add('is-busy');
      const loader = document.getElementById('ffm-loader').content.firstElementChild.cloneNode(true);
      // A side pane shows it in its header, beside the title; anything else
      // lays it over itself.
      const head = anchor.querySelector(':scope > .rp-head');
      if (head) head.insertBefore(loader, head.querySelector('.rp-zoom, .rp-close'));
      else anchor.appendChild(loader);
    },

    /// Also called on an anchor a swap has already replaced — harmless, and
    /// the new element never had the class.
    busyEnd(anchor) {
      if (!anchor) return;
      if (anchor === 'status') { this.busyCount = Math.max(0, this.busyCount - 1); return; }
      const n = (ffmBusy.counts.get(anchor) || 1) - 1;
      if (n > 0) { ffmBusy.counts.set(anchor, n); return; }
      ffmBusy.counts.delete(anchor);
      anchor.classList.remove('is-busy');
      anchor.querySelectorAll(':scope > .loader, :scope > .rp-head > .loader').forEach(l => l.remove());
    },

    /// A plain `fetch` fires no htmx events, so the two that exist wrap
    /// themselves in this — the whole chain, swap included, so the loader
    /// stays until the answer is on screen.
    busyFetch(anchor, promise) {
      this.busyStart(anchor);
      return promise
        .catch(() => this.say(this.busyOffline, true))
        .finally(() => this.busyEnd(anchor));
    },

    /// Everything the app tells you: a toast, and a line in the bell. Every
    /// message goes through here — the server's `ffm:say`, the keymap's own
    /// refusals, the live channel — so this is the one place it is shown.
    say(message, bad) {
      if (!message) return;
      bad = !!bad;
      this.note(message, bad);
      this.toast(message, bad);
    },

    /// A toast lasts 4s, a refusal 8s; a tap ends either. The same words again
    /// while they are still up (a refused key pressed twice) restart that
    /// toast rather than stacking a second copy of it.
    toast(text, bad) {
      const last = this.toasts[this.toasts.length - 1];
      let t = last && last.text === text && last.bad === bad ? last : null;
      if (!t) {
        t = { id: ++this.toastSeq, text, bad, timer: 0 };
        this.toasts.push(t);
        if (this.toasts.length > 3) this.dropToast(this.toasts[0].id);
      }
      clearTimeout(t.timer);
      t.timer = setTimeout(() => this.dropToast(t.id), bad ? 8000 : 4000);
    },

    dropToast(id) {
      const t = this.toasts.find(x => x.id === id);
      if (t) clearTimeout(t.timer);
      this.toasts = this.toasts.filter(x => x.id !== id);
    },

    /// The bell's line for a message. A repeat within 2s counts on the line
    /// before (`×3`) rather than filling the list with one refusal.
    note(text, bad) {
      const now = Date.now();
      const top = this.notes[0];
      if (top && top.text === text && top.bad === bad && now - top.at < 2000) {
        top.at = now;
        top.n = (top.n || 1) + 1;
      } else {
        this.notes.unshift({ text, bad, at: now, n: 1 });
        if (this.notes.length > 50) this.notes.length = 50;
        if (!this.bellOpen) this.notesUnread++;
      }
      this.saveNotes();
    },

    /// `hh:mm`, for the bell. The arrival time in this browser: it is a
    /// record of this window's session, not of the ledger.
    noteTime(at) {
      const d = new Date(at);
      return String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0');
    },

    openBell() {
      this.bellOpen = true;
      this.notesUnread = 0;
      this.saveNotes();
    },

    clearNotes() {
      this.notes = [];
      this.notesUnread = 0;
      this.saveNotes();
    },

    saveNotes() {
      try {
        sessionStorage.setItem('ffm.notes', JSON.stringify({ notes: this.notes, unread: this.notesUnread }));
      } catch (_) {}
    },

    /// And a message said in the last moments before this page loaded is
    /// toasted again: a setting answers `HX-Refresh`, and its words would
    /// otherwise reach only the bell.
    loadNotes() {
      try {
        const saved = JSON.parse(sessionStorage.getItem('ffm.notes') || 'null');
        if (saved && Array.isArray(saved.notes)) {
          this.notes = saved.notes.slice(0, 50);
          this.notesUnread = Number(saved.unread) || 0;
        }
        this.lastCmd = sessionStorage.getItem('ffm.echo') || '';
        this.msg = this.lastCmd;
      } catch (_) {}
      const now = Date.now();
      this.notes.filter(n => now - n.at < 2500).reverse().forEach(n => this.toast(n.text, n.bad));
    },

    /// The command just run, in the statusline until the next one — kept for
    /// the window, so a command that reloads the page still shows there.
    echoCommand(raw) {
      this.lastCmd = ':' + raw;
      this.msg = this.lastCmd;
      try { sessionStorage.setItem('ffm.echo', this.lastCmd); } catch (_) {}
    },

    key(e) {
      // A MODIFIER pressed on its own is not a keystroke, and must not be
      // treated as one.
      //
      // Holding Shift to type an uppercase key fires its own keydown first,
      // and this dispatcher clears the pending prefix and the pending count
      // on every key it sees. So `g` then Shift-T lost the `g` to the Shift,
      // and `T` arrived bare: **`gT` has never worked on a real keyboard**,
      // nor `zR`, `zM`, or any count before a shifted key — `2J` moved one
      // row, not two.
      //
      // It survived every test because synthetic KeyboardEvents send only the
      // final key. A real keyboard sends the modifier first.
      if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) {
        return;
      }
      // The month question is answered only by the very next key — but not
      // a modifier (the Shift before `T`) or a digit (the 3 of a repeated
      // `3]`); anything else in between and it lapses.
      if (!['[', ']', 'T', 'Shift', 'CapsLock'].includes(e.key) && !/^[0-9]$/.test(e.key)) this.monthConfirm = null;

      // A form takes literal text, so motions stop being motions inside one.
      // This is the same rule the pre-auth pages follow, not an exception.
      //
      // `target` is not always an element — a key dispatched at the window has
      // the window as its target, and `window.matches` does not exist. Guard
      // it, or the whole keymap dies silently on the first such event.
      const el = e.target instanceof Element ? e.target : null;
      // Except the phone's key trap: it is a field only so the keyboard shows,
      // and what is typed there is meant for the ledger.
      const typing = !!el && el.matches('input, textarea, select') && !el.matches('.keytrap');
      // The `:grep` list owns its keys the way a field does — decided by the
      // container, as Ctrl-h/j/k/l below are. Its hits are links, so `typing`
      // is false there, and `j` walked the ledger cursor behind the list as
      // well as the list's own (the trash escapes this only because its rows
      // are checkboxes).
      const listing = !!el && !!el.closest('#hsplitmount .greppane a.hit');
      // Any keypress dismisses a details popup — the same way any hover
      // artifact goes away — before that key does whatever else it does.
      if (this.tip) { this.tip = null; }
      // The row menu goes the same way. Esc only closes it; any other key
      // closes it and then does its own job, so `e` straight from the open
      // menu is still `e`.
      if (this.rowMenu) {
        this.rowMenu = null;
        if (e.key === 'Escape') { e.preventDefault(); return; }
      }
      // Keyboard motion wins over a stale hover: the row the resting mouse
      // happens to be over stops painting its highlight, so only the real
      // cursor is lit. The CSS half (`body:not(.kbd-nav) .row:hover`) was
      // written long ago; nothing ever set the class, so the pointer's row
      // stayed lit beside the cursor's on every j/k. Cleared by `mouseMoved`.
      if (!typing) document.body.classList.add('kbd-nav');

      // "Attach anyway?" sits above even the trim view. Only `y` attaches;
      // every other key — Enter included — leaves the bill out.
      if (this.attachAsk) {
        if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) return;
        e.preventDefault();
        this.answerAttach(e.key === 'y');
        return;
      }
      // "Undo this?" — every `u` and `U` asks first. The same key again, or
      // `y`, does it; every other key, Enter and Esc included, leaves it. A
      // bare Shift never gets here (returned above), so the Shift of `U`
      // does not answer it.
      if (this.undoAsk) {
        e.preventDefault();
        this.answerUndo(e.key === (this.undoAsk.dir === 'undo' ? 'u' : 'U') || e.key === 'y');
        return;
      }
      // The trim-and-shrink view sits above everything, the carousel included.
      if (this.prep) {
        if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) return;
        e.preventDefault();
        this.prepKey(e.key);
        return;
      }
      // The code box is a form above the carousel: its field takes every key
      // (Enter submits it), and only Esc is the box's own.
      if (this.codeFor) {
        if (e.key === 'Escape') { e.preventDefault(); this.codeFor = null; }
        return;
      }
      if (this.removingFile) {
        // As on the row's own confirm: `y` is the only yes, Enter is unbound.
        if (e.key === 'y') { e.preventDefault(); this.doRemoveFile(); }
        if (e.key === 'Escape') { e.preventDefault(); this.removingFile = null; }
        return;
      }
      // The chart viewer owns every key while it is open, except in its own
      // from/to fields, where Esc only leaves the field. Tab and the
      // browser's own chords pass through.
      if (this.charts) {
        if (e.ctrlKey || e.metaKey || e.altKey || e.key === 'Tab') return;
        if (typing) { if (e.key === 'Escape') { e.preventDefault(); el.blur(); } return; }
        e.preventDefault();
        this.chartsKey(e.key);
        return;
      }
      // A modifier on its own is not a keystroke — it must not clear the
      // pending `d` of a `dd` (see the Shift gotcha in CLAUDE.md).
      if (this.carousel && ['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) return;
      if (this.carousel) { e.preventDefault(); this.carouselKey(e.key); return; }
      if (this.keymap) { this.keymapKey(e, el); return; }
      if (this.confirming) {
        // The only "yes" is an explicit `y`. Enter is deliberately unbound in
        // here so it can never become an accidental way to confirm.
        if (e.key === 'y') { e.preventDefault(); this.doDelete(); }
        if (e.key === 'Escape') { e.preventDefault(); this.confirming = null; }
        return;
      }
      if (this.giving) { e.preventDefault(); this.giveKey(e.key); return; }
      if (this.billPick) { e.preventDefault(); this.billPickKey(e.key); return; }
      // A form — the edit row, the rule pane, a settings pane — answers
      // Ctrl-h/j/k/l itself: they move between its fields, and past its edge
      // hand focus back to the ledger (`formEdge`). Decided by the CONTAINER,
      // not by `typing`: with focus on Save or a toggle `typing` is false, and
      // Ctrl-j there fell through to the ledger's own Ctrl-j and opened the
      // settings split instead.
      const box = el && el.closest('form.editrow, .rulepane, #hsplitmount > *');
      if (box && e.ctrlKey && !e.metaKey && !e.altKey && FFM_DIRS[e.key]) {
        e.preventDefault();
        const dir = FFM_DIRS[e.key];
        if (!ffmFieldMove(box, dir)) this.formEdge(box, dir);
        return;
      }
      // Esc closes the rule pane in one press, from inside a field too — it
      // only blurred the field before, and closing took a second Esc. It
      // discards, as it does on the `e` row; keeping a draft is Ctrl-j/k past
      // the edge. Not in the bottom split: its panes wear `.rulepane` too, and
      // this arm closed a rule pane that was not there and left the split up.
      if (box && box.matches('.rulepane') && !box.closest('#hsplitmount') && e.key === 'Escape') {
        e.preventDefault();
        el.blur();
        this.closeRule();
        return;
      }
      // Every other key with focus on a BUTTON in the rule pane or the `e`
      // row — a section's fold header, Save, Unlink, the password's eye — is
      // the form's as well, by the same container rule. `typing` says no
      // there, so `j` reached the ledger, moved the cursor off the row and
      // closed the pane under the key. Not prevented: Enter and Space still
      // press the button, and Tab still walks.
      if (box && !box.closest('#hsplitmount') && !typing && e.key !== 'Escape'
          && !e.ctrlKey && !e.metaKey) {
        return;
      }
      // `:` still opens the command bar, so a second search is one key away.
      if (listing && !e.ctrlKey && !e.metaKey && e.key !== ':') {
        if (e.key === 'Escape') {
          e.preventDefault();
          el.blur();
          this.closeSplit();
        }
        return;
      }
      if (typing) {
        if (e.key === 'Escape') {
          // Esc inside the SPLIT closes it, in one press. Everywhere else it
          // keeps meaning "cancel this edit", which is why this is a case
          // rather than a new rule: the add row and the inline edit row are
          // unchanged.
          if (el.closest('#hsplitmount')) {
            e.preventDefault();
            el.blur();
            this.closeSplit();
            return;
          }
          this.adding = null;
          el.blur();
        }
        return;
      }
      // Ctrl-j drops into the bottom split, Ctrl-k comes back out. Unbound
      // before this; Ctrl-h / Ctrl-l stay re-attribution.
      if (e.ctrlKey && !e.metaKey && !e.altKey && (e.key === 'j' || e.key === 'k')) {
        e.preventDefault();
        if (e.key === 'j') this.focusSplit();
        // Ctrl-k from anything focusable inside the split, not just a text
        // field: the close button is focusable too, and it is what `Ctrl-j`
        // lands on when a pane has no fields.
        else if (document.activeElement.closest('#hsplitmount')) {
          document.activeElement.blur();
        }
        return;
      }
      // Ctrl-H / Ctrl-L used to re-attribute, one key away from H / L. They
      // answer with where it went (`ga`) rather than nothing, for the hands
      // that still reach for them — and so Ctrl-H is not the browser's
      // history on Linux.
      if (e.ctrlKey && !e.metaKey && !e.altKey && (e.key === 'h' || e.key === 'l')) {
        e.preventDefault();
        this.say(this.giveMovedKey);
        return;
      }
      // Ctrl-u opens the trash: `u` undoes one change, `U` redoes it, and
      // Ctrl-u is the whole list. On Linux and Windows Ctrl-U is the
      // browser's view-source, which this takes over; if it ever slips past
      // (focus in the address bar), a source tab opens and nothing is lost.
      if (e.ctrlKey && !e.metaKey && !e.altKey && e.key === 'u') {
        e.preventDefault();
        this.openTrash();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      const k = e.key;
      const buffer = this.pending;
      this.pending = '';

      // A count prefixes a motion: `5j`, `3J`. Typed digits accumulate until
      // something uses them, and a leading 0 is not a count — it is `0`, which
      // this app does not bind, so it simply does nothing.
      //
      // NOT while ANY prefix is armed: there the digit is the whole binding
      // (`g1` is the bill, not "one g"), so a count branch that ran first
      // would swallow it. Tested against the buffer being empty rather than
      // against 'g' specifically — checking for one prefix by name is how
      // `gu1` silently stopped working the day `gu` was added.
      if (!buffer && (/^[1-9]$/.test(k) || (this.count && k === '0'))) {
        this.count += k;
        e.preventDefault();
        return;
      }
      // EVERY prefix is armed here, in one place, before the dispatch chain
      // below — never as an `else if` inside it.
      //
      // This is the third time the same bug was written: an arming arm placed
      // above its own consuming arm swallows the second keystroke, so `gg`,
      // then `gu1`, then `zz` could each never fire. Interleaving the two
      // kinds of arm makes that a matter of line order, which is not a thing
      // to keep getting right by hand.
      //
      // Arming must also NOT consume a pending count, or `2gt` loses its 2 on
      // the way to the `t` that wanted it.
      if (['g', 'z'].includes(k) && !buffer) {
        this.pending = k;
        e.preventDefault();
        return;
      }
      // Whether a count was actually TYPED, not just what it resolves to.
      // `n` defaults to 1, so without this `1gt` and a bare `gt` are the same
      // keystroke by the time anything reads them — and they do not mean the
      // same thing.
      const counted = this.count !== '';
      const n = parseInt(this.count || '1', 10);
      this.count = '';

      // A PREFIXED binding is resolved before any unprefixed one, always.
      //
      // Line order used to decide it, and line order kept getting it wrong:
      // `gg`, then `gu1`, then `zz`, and `za` — which lost to `a` and opened
      // the add form instead of toggling a fold. Four of the same bug in one
      // chain is a shape problem, not four mistakes.
      //
      // An armed prefix also consumes its second key even when the pair means
      // nothing: `zq` drops the prefix rather than falling through to
      // whatever `q` would have meant on its own.
      if (buffer) {
        if (this.prefixed(buffer, k, n, counted)) e.preventDefault();
        return;
      }

      // While the log is open it owns the vertical motions — see logIsOpen.
      if (k === 'j' && this.logIsOpen()) { this.logMove(n); }
      else if (k === 'k' && this.logIsOpen()) { this.logMove(-n); }
      else if (k === 'Enter' && this.logIsOpen()) { this.logToggle(this.logCursorId()); }
      else if (k === 'j') { this.row += n; this.paint(); }
      else if (k === 'k') { this.row -= n; this.paint(); }
      else if (k === 'P') { this.openCarousel(); }
      // Months: `[` back, `]` on — counted, so `3]` is three months on — and
      // `T` to today's. `T` is free bare: `gT` was resolved by `prefixed`.
      else if (k === '[' || k === ']') { this.gotoPeriod(this.shiftPeriod(k === ']' ? n : -n), k); }
      else if (k === 'T') {
        const today = this.shell().dataset.today;
        if (today === this.shell().dataset.period) this.say(this.monthAlreadyCurrent, false);
        else this.gotoPeriod(today, 'T');
      }
      else if (k === '?') { this.openKeymap(''); }
      else if (k === 'G') {
        // The log owns the vertical motions while it is open, and `G` is one
        // of them — `j`/`k` already are. Ending up at the bottom of a long
        // history is the common thing to want there, since the pane lists
        // newest first and the oldest entry is the one you scroll for.
        if (this.logIsOpen()) this.logMove(this.logEntries().length);
        else { this.row = this.rows().length - 1; this.paint(); }
      }
      else if (k === 'H') { this.pane = Math.max(0, this.pane - 1); this.row = 0; this.paint(); }
      else if (k === 'L') { this.pane = Math.min(document.querySelectorAll('.pane').length - 1, this.pane + 1); this.row = 0; this.paint(); }
      else if (k === 'a' || k === 'i') { this.open(k === 'a' ? 'out' : 'in'); }
      // Uppercase acts on the RULE, lowercase on the occurrence. A/I create
      // one from scratch — never by promoting the row under the cursor — and
      // E edits the rule behind it.
      else if (k === 'A' || k === 'I') { this.newRule(k === 'A' ? 'out' : 'in'); }
      else if (k === 'E') { this.editRule(); }
      else if (k === 'e') { this.editRow(); }
      else if (k === 'p') { this.togglePrivate(); }
      else if (k === 'm') { this.togglePaid(); }
      else if (k === 'x') { this.toggleSelected(); }
      else if (k === 'X') { this.toggleFold(); }
      else if (k === 'd') { this.pending = 'd'; }
      // `u` undoes your last change in this pane and this month; `U` redoes
      // it. Not Ctrl-R, Vim's redo: on Linux that reloads the page, and a
      // reload drops every parked draft.
      else if (k === 'u') { this.undoStep('undo'); }
      else if (k === 'U') { this.undoStep('redo'); }
      else if (k === ':') { this.openCommand(); }
      else if (k === '/') { this.openSearch(); }
      else if (k === 'n') { this.step(1); }
      else if (k === 'N') { this.step(-1); }
      // `s` orders the rows, `S` the groups: manual → date → amount, the
      // same row the statusline's sort cell, `:sort` and :profile write.
      else if (k === 's') { this.cycleSort(); }
      else if (k === 'S') { this.cycleGroupOrder(); }
      else if (k === 'J') { this.move(n); }
      else if (k === 'K') { this.move(-n); }
      else if (k === 'Escape') {
        // The file menu too: it lives on this component, and a popup that
        // ignores Escape is a popup you have to reach for the mouse to close.
        // The month picker and the profile menu keep their own `x-data`, so
        // they listen for Escape themselves.
        this.attaching = null;
        this.adding = null; this.closeRule(); this.closeLog();
        this.closeSplit(); this.clearSearch();
        // The command bar too. Its own input closes it on Escape, but only
        // when the input has focus — and if focus ever fails to land, that
        // left the bar open with no way to shut it from the keyboard.
        this.commanding = false; this.command = '';
        // Esc clears the count, the buffer and the selection too.
        this.count = ''; this.pending = ''; this.selected = {}; this.paintSelection();
      }
      else return;
      e.preventDefault();
    },

    open(kind) {
      if (this.readOnly()) return;
      this.kind = kind;
      this.adding = this.pane;
      this.$nextTick(() => {
        const pane = document.querySelectorAll('.pane')[this.pane];
        pane.querySelector('.addrow input[name="title"]').focus();
      });
    },

    /// Write the focused member into the URL, so a refresh comes back to the
    /// pane you were in rather than to your own.
    ///
    /// `replaceState`, not `pushState`: moving between panes is not a page you
    /// want Back to walk through one member at a time. It runs from `paint()`
    /// because every gesture that can change the focus already goes through
    /// there — `H`/`L`, `gt`/`gT`, a click on a pane or a tab — and only
    /// writes when the id actually changed, since `paint()` also runs on every
    /// `j`.
    trackPane() {
      const id = this.paneMemberId();
      if (!id) return;
      const url = new URL(location.href);
      if (url.searchParams.get('pane') === id) return;
      url.searchParams.set('pane', id);
      history.replaceState(null, '', url);
    },

    /// The month `delta` months from the one on screen, as `YYYY-MM`. Plain
    /// calendar arithmetic over the shell's own `data-period` — never the
    /// browser's clock, which a dev server running as of another date
    /// (`FFM_TODAY`) would disagree with. `T` reads `data-today` for that.
    /// The shell carries the month on screen and today's. Looked up rather
    /// than `$root`: `monthClick` runs from the picker's own `x-data`, where
    /// `$root` is the picker.
    shell() { return document.querySelector('.ledger-shell'); },

    shiftPeriod(delta) {
      const [y, m] = this.shell().dataset.period.split('-').map(Number);
      const i = y * 12 + (m - 1) + delta;
      return Math.floor(i / 12) + '-' + String((i % 12) + 1).padStart(2, '0');
    },

    /// Every month change: `[`, `]`, `T`, the arrows and the picker. A page
    /// load, so the URL is built from `location` — `?pane=` and `?split=`
    /// survive it, which a server-rendered href cannot promise, since the
    /// pane moves client-side — and `?row=` goes, since it means "land here
    /// once".
    ///
    /// A page load drops every parked draft, so with any parked the first
    /// request only asks, and the same request again goes. Open forms are
    /// parked first, exactly as moving the cursor off them would: a changed
    /// one counts, an unchanged one just closes. `how` is the key pressed, or
    /// 'click'.
    gotoPeriod(key, how) {
      if (this.editing) this.parkDraft(this.editing);
      if (this.ruleFor) this.parkRule();
      const n = Object.keys(this.drafts).length + Object.keys(this.ruleDrafts).length;
      if (n && this.monthConfirm !== key) {
        this.monthConfirm = key;
        const drafts = n === 1 ? this.draftsParkedOne : this.draftsParked.replace('{n}', n);
        this.say(how === 'click'
          ? this.monthLeaveClick.replace('{d}', drafts)
          : this.monthLeaveKey.replace('{d}', drafts).replace('{k}', how), false);
        return;
      }
      this.monthConfirm = null;
      const url = new URL(location.href);
      url.searchParams.set('period', key);
      url.searchParams.delete('row');
      this.keepFollow(key);
      location.assign(url);
    },

    /// A click on a month link — `‹`, `›` or a picker cell — goes through
    /// `gotoPeriod`. A modified or middle click is left to the browser, so
    /// a new tab still opens on the plain href.
    monthClick(e) {
      const a = e.target.closest('a[href*="period="]');
      if (!a || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      const key = new URL(a.href, location.href).searchParams.get('period');
      if (!key) return;
      e.preventDefault();
      this.gotoPeriod(key, 'click');
    },

    paneMemberId() {
      const pane = document.querySelectorAll('.pane')[this.pane];
      return pane ? pane.querySelector('.rows').id.slice(5) : null;
    },

    /// Whether a write gesture has to stop here, saying so if it does: the
    /// pane is somebody else's and this session may only read it. `el` is a
    /// row for a gesture aimed by pointing (a drop, a click) rather than by
    /// the cursor. The refusal text is the server's, rendered on the pane.
    readOnly(el) {
      const pane = el ? el.closest('.pane') : document.querySelectorAll('.pane')[this.pane];
      const msg = pane && pane.dataset.readonly;
      if (msg) this.say(msg, true);
      return !!msg;
    },

    /// Where a side pane goes: immediately after the focused member's pane.
    /// One rule for the rule editor, the change log and the settings panes
    /// alike — `H`/`L` already means "move between panes", so the pane you
    /// are in is the one a new column belongs beside.
    mount() {
      const id = this.paneMemberId();
      return id ? '#rulemount-' + id : '.rulemount';
    },

    newRule(kind) {
      const member = this.paneMemberId();
      if (!member || this.readOnly()) return;
      // One side pane at a time, the same rule showSplit follows. Without
      // this, opening a rule editor while `:profile` was up left both
      // standing — in different columns, since each mounts beside the pane
      // that was focused when it opened.
      this.closeSplit();
      this.closeLog();
      this.closeRule();
      this.ajax('GET', '/rules/new?member_id=' + member + '&kind=' + kind,
                { target: this.mount(), swap: 'innerHTML' }, this.focusedRow())
           .then(() => { this.aimArrow(); this.revealPaneSide(); });
    },

    editRule() {
      const rows = this.rows();
      const el = rows[this.row];
      const ruleId = el && el.dataset.ruleId;
      // A no-op with a statusline message on a plain row — the same treatment
      // `gd` gets there, rather than a different error path to learn.
      if (!ruleId) { this.say(this.notARule, true); return; }
      // An orphaned bill: its rule was deleted, so there is nothing to open.
      if (el.dataset.ruleDeleted) { this.say(this.ruleDeleted, true); return; }
      if (this.readOnly()) return;
      this.closeSplit();
      this.closeLog();
      this.closeRule();
      this.ruleFor = { row: this.focusedId(), rule: ruleId };
      this.ajax('GET', '/rules/' + ruleId + '/edit?period=' + (this.shell().dataset.period || ''),
                { target: this.mount(), swap: 'innerHTML' }, this.focusedRow())
           .then(() => {
             // The cursor left before the pane arrived: it opened for a row
             // nobody is on any more.
             if (this.ruleFor?.rule !== ruleId) { if (!this.ruleFor) this.closeRule(); return; }
             const form = document.querySelector('#rulepane form');
             if (form) form._ffmStart = this.ruleValues(form);
             this.restoreRuleDraft(ruleId);
             this.aimArrow();
             this.revealPaneSide();
           });
    },

    /// Ctrl-h/j/k/l past a form's edge. Up or down out of the `e` row moves
    /// the cursor to the row above or below — and a cursor leaving the row is
    /// what parks the draft (`paint`), so the edit is kept, marked on its row,
    /// and `e` picks it back up. The rule pane's top and bottom edges do
    /// nothing: it is a column beside the ledger, so up and down stay inside
    /// it — they used to move the cursor too, and Ctrl-k on Amount closed the
    /// pane. Left out of it is back to the ledger with the pane still open,
    /// until the cursor moves. Up out of a settings pane leaves the split.
    formEdge(box, dir) {
      const vertical = dir === 'up' || dir === 'down';
      const leave = () => document.activeElement?.blur();
      if (vertical && box.matches('form.editrow')) {
        const to = this.row + (dir === 'down' ? 1 : -1);
        // At the first or last row there is nowhere to go, and leaving the
        // form without moving would leave it open with focus nowhere.
        if (to < 0 || to >= this.rows().length) return;
        leave();
        this.row = to;
        this.paint();
      } else if (box.matches('.rulepane') && dir === 'left') {
        leave();
      } else if (box.closest('#hsplitmount') && dir === 'up') {
        leave();
      }
    },

    /// What the rule form says right now, one entry per field (a radio per
    /// option), plus each money field's integer — the mask paints from that,
    /// never from the text.
    ruleValues(form) {
      const values = {};
      form.querySelectorAll('[name]').forEach(el => {
        if (el.type === 'file') return;
        const key = el.type === 'radio' ? el.name + '=' + el.value : el.name;
        values[key] = el.type === 'checkbox' || el.type === 'radio' ? el.checked : el.value;
        if (el.classList.contains('amt-in')) values[key + '#cents'] = el.dataset.cents;
      });
      return values;
    },

    /// The `E` pane's draft, parked when the cursor leaves its row — the rule
    /// pane's `parkDraft`. An unchanged form just closes.
    ///
    /// "Changed" is measured against a snapshot taken when the pane loaded,
    /// not against each field's `defaultValue`: a <select> has none, and a
    /// field `x-model` rewrites on load would read as edited by nobody.
    parkRule() {
      const at = this.ruleFor;
      this.ruleFor = null;
      const form = document.querySelector('#rulepane form');
      if (at && form) {
        const values = this.ruleValues(form);
        const start = form._ffmStart || {};
        const changed = Object.keys(values).some(k => values[k] !== start[k]);
        if (changed) this.ruleDrafts[at.rule] = values; else delete this.ruleDrafts[at.rule];
      }
      this.closeRule();
      this.markDrafts();
    },

    /// `E` on a row whose rule has a parked draft. Toggles first, so the
    /// fields their `x-show` reveals are there, then the rest in page order —
    /// each with the event its `x-model` listens for, or Alpine's copy of the
    /// value (the path preview, the months read-back) would not follow.
    ///
    /// The draft is dropped once it is back in the form: leaving again parks
    /// it again, since it differs from what the server rendered, so a save or
    /// an Esc needs no cleanup of its own.
    restoreRuleDraft(ruleId) {
      const values = this.ruleDrafts[ruleId];
      const form = document.querySelector('#rulepane form');
      if (!values || !form) return;
      delete this.ruleDrafts[ruleId];
      const fields = [...form.querySelectorAll('[name]')].filter(el => el.type !== 'file');
      const checkable = el => el.type === 'checkbox' || el.type === 'radio';
      const keyOf = el => el.type === 'radio' ? el.name + '=' + el.value : el.name;
      fields.filter(checkable).forEach(el => {
        const v = values[keyOf(el)];
        if (v === undefined || el.checked === v) return;
        el.checked = v;
        el.dispatchEvent(new Event('change', { bubbles: true }));
      });
      this.$nextTick(() => fields.filter(el => !checkable(el)).forEach(el => {
        const v = values[keyOf(el)];
        if (v === undefined) return;
        const cents = values[keyOf(el) + '#cents'];
        if (cents !== undefined) el.dataset.cents = cents;
        if (el.value === v && cents === undefined) return;
        el.value = v;
        el.dispatchEvent(new Event('input', { bubbles: true }));
      }));
      // A field the draft changed is never left inside a closed section: the
      // pane opens Files closed when the rule has its bill, and a parked
      // rename there would otherwise come back out of sight.
      const pane = Alpine.$data(form.closest('.rulepane'));
      const start = form._ffmStart || {};
      fields.forEach(el => {
        const k = keyOf(el);
        if (values[k] === undefined || values[k] === start[k]) return;
        const sect = el.closest('[data-sect]');
        if (sect && pane?.open) pane.open[sect.dataset.sect] = true;
      });
      this.markDrafts();
    },

    closeRule() {
      this.ruleFor = null;
      this.clearMount('#rulepane');
    },

    /// Empties the mount a side pane is sitting in, rather than removing the
    /// pane element alone. A swapped-in pane brings its own <script> beside
    /// it; removing just the <aside> left that behind, one orphan per mount
    /// per open, in whichever column the pane last appeared.
    clearMount(selector) {
      const el = document.querySelector(selector);
      if (!el) return;
      const mount = el.closest('.rulemount, .hsplitmount');
      if (mount) mount.replaceChildren();
      else el.remove();
    },

    /// The statusline's two clickable cells. They write the member's own
    /// settings row — the same row `:set`, `:sort` and the `:profile` radios
    /// write, which is what keeps one setting to one source.
    cycleSort() {
      // manual → name → amount → date, decided by `Sort::next` on the
      // server: the write refreshes the page, so the next step is always
      // the one rendered with it.
      this.setting('/settings/member', 'ledger_sort', this.nextSort);
    },

    /// `S`, and the statusline cell beside sort: first seen → A–Z → by cost.
    /// The order is decided when the server renders the pane, so the write
    /// answers with a refresh, like every other setting that moves rows.
    cycleGroupOrder() {
      if (!this.groupOrder) { this.say(this.groupOrderNeedsGroup, true); return; }
      const next = { appearance: 'name', name: 'subtotal', subtotal: 'appearance' }[this.groupOrder] || 'appearance';
      this.setting('/settings/member', 'group_order', next);
    },

    cycleNu() {
      const next = { relative: 'absolute', absolute: 'none', none: 'relative' }[this.nu] || 'relative';
      this.setting('/settings/member', 'line_numbers', next);
    },

    /// Relative line numbers, measured from the cursor.
    ///
    /// The server renders them relative to row 0, which is where the cursor
    /// starts — correct before the first frame, no round trip, the same
    /// posture `layout = auto` takes. From there they are Alpine's, because
    /// the distance to the cursor is a fact about the cursor and the cursor
    /// is visual state. `absolute` and `none` are decided entirely on the
    /// server and this leaves them alone.
    renumber(positions) {
      if (this.nu !== 'relative') return;
      // Counted among POSITIONS, a fold's header included — exactly what
      // `j`/`k` walk. Counting only rows made the numbers lie: from a row
      // reading 0, `4k` landed on the header the numbers had skipped, one
      // line short of the row marked 4. A header has its own gutter cell
      // (`.fgut`), so there is no hole in the sequence either. The rows of a
      // closed fold are not positions; their hidden numbers are redrawn
      // when it opens.
      positions.forEach((el, i) => {
        const cell = el.querySelector(el.classList.contains('foldhead') ? '.fgut' : '.gut');
        if (cell) cell.textContent = String(Math.abs(i - this.row));
      });
    },

    /// Points the rule pane's arrow at the row it belongs to.
    ///
    /// Pure visual state — a measurement of where a row happens to sit, which
    /// only the browser can make — so it is Alpine's, written into a custom
    /// property the stylesheet reads. A settings pane has no row behind it
    /// and is left alone, which is why the CSS keeps a fallback rather than
    /// requiring the variable.
    aimArrow() {
      // BOTH row-anchored panes, not just the rule editor. The change log is
      // the other one, and it only ever got the CSS fallback — an arrow at
      // the pane's own header, level with nothing.
      const pane = document.querySelector('#rulepane, #logpane');
      if (!pane) return;
      const row = document.querySelector('.row.is-focus, .row.is-placeholder');
      if (!row) return;

      // Measure from an unshifted pane, or each call would compound the last
      // one's offset.
      pane.style.marginTop = '0px';
      const r = row.getBoundingClientRect();
      const p = pane.getBoundingClientRect();
      const stage = document.querySelector('.stage').getBoundingClientRect();

      // Where the arrow wants to be: level with the middle of the row.
      let want = r.top + r.height / 2 - p.top - 6;
      const reach = Math.max(12, p.height - 24);

      // **The arrow always points at the row; the pane moves only when the
      // arrow alone cannot reach it.** A tall rule editor covers most of the
      // ledger, so the arrow travels inside it and the pane stays put. A
      // short change log cannot — one entry is 300px against a row a
      // thousand pixels down — so it slides down to meet the row instead of
      // pinning its arrow to its own bottom edge and pointing at nothing.
      if (want > reach) {
        const shift = Math.min(want - reach, Math.max(0, stage.bottom - p.bottom));
        if (shift > 0) {
          pane.style.marginTop = shift + 'px';
          want -= shift;
        }
      }

      pane.style.setProperty('--arrow-top', Math.max(12, Math.min(want, reach)) + 'px');
    },

    // --- gd, P, ? ---------------------------------------------------------

    /// The mouse is the thing moving again, so hover comes back.
    ///
    /// Only a REAL move counts. A browser fires `mousemove` when the page
    /// scrolls under a pointer that never moved — which is exactly what `j`
    /// past the edge of the view does — and taking that as the mouse would
    /// undo `kbd-nav` a frame after the key set it. So the screen position
    /// must actually change.
    mouseMoved(e) {
      const at = e.screenX + ',' + e.screenY;
      if (at === this.pointerAt) return;
      const first = this.pointerAt === null;
      this.pointerAt = at;
      if (!first) document.body.classList.remove('kbd-nav');
    },

    /// `gd` is the hover tooltip with no mouse involved: the same panel over
    /// the same facts, anchored to the focused row because there is no pointer
    /// position to anchor to.
    showDetails() {
      const row = this.rows()[this.row];
      if (!row) return;
      if (this.onFoldHead()) { this.say(this.cursorOnFold, true); return; }
      if (!row.dataset.cadence && !row.dataset.receipt && !row.dataset.due) {
        this.say(this.detailsNone, true);
        return;
      }
      const r = row.getBoundingClientRect();
      this.tip = {
        status: row.dataset.status,
        due: row.dataset.due,
        cadence: row.dataset.cadence,
        receipt: row.dataset.receipt,
        ruleDeleted: row.dataset.ruleDeleted,
        progress: row.dataset.progress,
      };
      this.tipStyle = 'top:' + (r.bottom + 6) + 'px; left:' + (r.left + 40) + 'px';
      // Dismissed by the very next keypress of ANY kind — the same way any
      // hover artifact goes away.
      this.tipSticky = true;
    },

    /// `P`. A row with zero files refuses outright rather than opening an
    /// empty carousel with nothing to look at.
    openCarousel() {
      const row = this.rows()[this.row];
      if (!row) return;
      if (this.onFoldHead()) { this.say(this.cursorOnFold, true); return; }
      const files = Array.from(row.querySelectorAll('.fbadge[href]')).map(a => ({
        kind: a.getAttribute('href').split('/').pop(),
        number: a.querySelector('kbd')?.textContent || '',
        label: a.querySelector('.flabel')?.textContent || '',
        // The badge's tooltip is "Bill — {path}"; the caption already shows
        // the label, so only the path is kept.
        path: (a.getAttribute('title') || '').replace(/^.*?\u2014\s*/, ''),
        href: a.getAttribute('href'),
        svg: a.querySelector('.ficon')?.innerHTML || '',
        page: a.querySelector('.flabel')?.textContent.includes('p.')
          ? a.querySelector('.flabel').textContent : '',
        warn: a.classList.contains('is-warn'),
        // Each file's own zoom, kept while the carousel is open — coming back
        // to a file finds it as it was left, and a file not yet shown starts
        // at 100%. Its scroll needs no field: its stage keeps it.
        zoom: 1,
        // `412 KB` — from the stage fragment's `data-size`, so it is the
        // server's reading of the file on disk.
        size: '',
      }));
      if (!files.length) { this.say(this.noFiles, true); return; }
      this.pageFit = 'height';
      this.carouselBuf = '';
      // The member's own starting side (`:profile`); `v` flips it from there.
      this.pdfView = this.shell().dataset.pdfView === 'pdf';
      this.carousel = {
        id: this.focusedId(),
        files,
        at: 0,
        // The file on show has its pages in, so the row's other files may
        // load behind it — see `showFile`.
        ready: false,
        get current() { return this.files[this.at]; },
      };
    },

    /// Loads one file's stage, once — a stage per file, all kept while the
    /// carousel is open, so flipping with h/l/1/2/3 fetches nothing and each
    /// file keeps its own scroll. Zooming never reloads it; `v` does, since
    /// how it is shown is part of the key.
    ///
    /// The file on show loads first. The row's others wait for its first page
    /// to arrive (`ready`), then load out of sight: a PDF page is a `pdftoppm`
    /// run on the server, and the page being looked at should not queue
    /// behind the ones that are not. A stage switched to before then is the
    /// one on show, and loads at once. Inside a stage only the page it opens
    /// on loads at once; the rest are `loading="lazy"`, fetched as they come
    /// into view (`file_view.html`).
    ///
    /// The stages keep each file's scroll and zoom. Opening `P` again is the
    /// HTTP cache's: a page's URL carries the file's version (`?v=`) and is
    /// kept by the browser for good, so it is not drawn twice.
    ///
    /// A bill shared across months (`spans_period`) scrolls to THIS month's
    /// page: the badge already says `Bill · p.4`, and confirming which page is
    /// owed is the whole reason to open that file here.
    showFile(el, f, current, ready) {
      // Only a PDF has two ways to be shown, so only a PDF's stage reloads on
      // `v` — a photo or a text file keeps its place.
      const pdf = !!f && this.pdfView && /\.pdf$/i.test(f.path || '');
      const key = f && f.href + (pdf ? '|pdf' : '');
      if (!f || el.dataset.src === key) return;
      if (!current && !ready) return;
      const c = this.carousel;
      el.dataset.src = key;
      el.innerHTML = '';
      f.size = '';
      const page = (f.page.match(/p\.\s*(\d+)/) || [])[1];
      const query = [pdf && 'pdf=1', page && 'page=' + page].filter(Boolean).join('&');
      // Its own element as the htmx source: from one source, htmx keeps
      // only the last of several requests in flight. Its own stage as the
      // loader's place, so a stage still loading when it comes on show says so.
      this.ajax('GET', f.href + '/view' + (query ? '?' + query : ''),
                { source: el, target: el, swap: 'innerHTML' }, el)
        .then(() => {
          if (el.dataset.src !== key) return;
          f.size = el.querySelector('[data-size]')?.dataset.size || '';
          const view = el.closest('.cview');
          const owed = el.querySelector('[data-owed]');
          // Its own view's scroll, never `scrollIntoView`, which would also
          // scroll whatever holds a stage that is not on show.
          if (view && owed) view.scrollTop = Math.max(0, Math.round(this.pageTop(view, owed)));
          this.sharpen(view);
          if (!current) return;
          const settled = img => img.complete ? null : new Promise(done => {
            img.addEventListener('load', done, { once: true });
            img.addEventListener('error', done, { once: true });
          });
          // Only the page that loads at once: a lazy page off screen never
          // loads, and waiting on it would hold the row's other files back
          // for good.
          Promise.all([...el.querySelectorAll('img:not([loading="lazy"])')].map(settled)).then(() => {
            if (this.carousel === c && el.dataset.src === key) c.ready = true;
          });
        });
    },

    /// The stage of the file on show, and the box inside it that carries
    /// the fit and the zoom. Found from the model, never from the
    /// `is-current` class, which Alpine paints a tick late.
    carouselView() {
      const f = this.carousel && this.carousel.current;
      return f ? document.querySelector('.carousel .cview[data-kind="' + f.kind + '"]') : null;
    },

    carouselInner() {
      return this.carouselView()?.querySelector('.cviewin') || null;
    },

    /// The file on show is a PDF — the only kind with two ways to see it.
    pdfOnShow() {
      return !!this.carousel && /\.pdf$/i.test(this.carousel.current.path);
    },

    /// `v`: the page images ↔ the browser's own PDF viewer, for every PDF of
    /// the row. Their stages reload (`showFile` keys on it) — the file on
    /// show first, the others behind it again — and their zoom starts over.
    togglePdfView() {
      if (!this.pdfOnShow()) return;
      this.carousel.ready = false;
      this.carousel.files.forEach(f => { if (/\.pdf$/i.test(f.path || '')) f.zoom = 1; });
      this.pdfView = !this.pdfView;
    },

    /// Clamped at either end rather than wrapping: the first file's previous
    /// and the last file's next are both visibly disabled.
    ///
    /// Nothing is reset: each file keeps its own place and zoom, so going
    /// back to one finds it as it was left. That is not carrying a zoom over
    /// to an unrelated file — a file not yet shown still starts at 100%.
    ///
    /// NOT `step`: that is already the search's next-match, and Alpine merges
    /// data and methods into one object — the second would silently replace
    /// the first, and `n` would start moving through files.
    stepFile(dir) {
      if (!this.carousel) return;
      const next = this.carousel.at + dir;
      if (next < 0 || next >= this.carousel.files.length) return;
      this.carousel.at = next;
    },

    askRemoveFile() {
      const c = this.carousel;
      if (!c || !c.current || this.readOnly()) return;
      this.removingFile = { id: c.id, kind: c.current.kind, label: c.current.label };
    },

    /// Soft-deletes the file, then lets the ROW say what happened: the row
    /// fragment replaces the row, and the carousel drops the file only if its
    /// badge is really gone — a refusal leaves both exactly as they were.
    doRemoveFile() {
      const f = this.removingFile;
      this.removingFile = null;
      if (!f) return;
      // The loader goes on the carousel's stage, not the row: the row is
      // under the full-viewport carousel, and the file on show is what the
      // removal is about.
      this.ajax('POST', '/tx/' + f.id + '/file/' + f.kind + '/delete',
                { target: '#row-' + f.id, swap: 'outerHTML' },
                this.carouselInner() || document.getElementById('row-' + f.id) || 'status')
        .then(() => {
          const still = document.querySelector('#row-' + f.id + ' .fbadge[href$="/' + f.kind + '"]');
          if (still || !this.carousel) return;
          // Keyed by kind, so only the removed file's stage goes; the others
          // keep their place.
          const files = this.carousel.files.filter(x => x.kind !== f.kind);
          if (!files.length) { this.carousel = null; return; }
          this.carousel.files = files;
          this.carousel.at = Math.min(this.carousel.at, files.length - 1);
        });
    },

    carouselKey(k) {
      if (k === 't') { this.carouselBuf = ''; this.trimCurrent(); return; }
      if (k === 'b') { this.carouselBuf = ''; this.askBarcode(); return; }
      if (k === 'a') { this.carouselBuf = ''; this.pickAppend(); return; }
      if (k === 'v') { this.carouselBuf = ''; this.togglePdfView(); return; }
      // `dd` removes the file on show — the row's own delete gesture, pointed
      // at the file — and `gg` goes to the top, as in the ledger. The carousel
      // reads its own keys, so a pending `d` or `g` lives here; any other key
      // drops it, the way `zq` drops the `z`.
      const armed = this.carouselBuf;
      this.carouselBuf = '';
      if (k === 'd') {
        if (armed === 'd') this.askRemoveFile(); else this.carouselBuf = 'd';
        return;
      }
      const view = this.carouselView();
      // `gg` / `G`: the very top and the very end — page 1, and the last
      // page's foot. Text scrolls too; the browser's viewer moves itself.
      if (k === 'g') {
        if (armed !== 'g') { this.carouselBuf = 'g'; return; }
        if (!(this.pdfView && this.pdfOnShow())) this.pageEnd(view, -1);
        return;
      }
      if (k === 'G') {
        if (!(this.pdfView && this.pdfOnShow())) this.pageEnd(view, 1);
        return;
      }
      if (['1', '2', '3'].includes(k)) {
        const i = this.carousel.files.findIndex(f => f.number === k);
        // Pressing a number for a kind that is not on this row is the same
        // no-op-with-a-message g1/g2/g3 already give.
        if (i < 0) { this.say(this.noFile[{1:'barcode',2:'scheduled',3:'receipt'}[k]], true); return; }
        this.carousel.at = i;
      } else if (k === 'h') this.stepFile(-1);
      else if (k === 'l') this.stepFile(1);
      // With the browser's viewer on show, it zooms and scrolls itself.
      else if (this.pdfView && this.pdfOnShow()
               && ['+', '=', '-', '0', 'j', 'k', 'J', 'K', 'ArrowDown', 'ArrowUp', 'f', 'H', 'M', 'L'].includes(k)) return;
      else if (k === 'j' || k === 'ArrowDown') this.pageScroll(view, 1);
      else if (k === 'k' || k === 'ArrowUp') this.pageScroll(view, -1);
      else if (k === 'J' || k === 'K') { if (this.imageOnShow()) this.pageStep(view, k === 'J' ? 1 : -1); }
      else if (k === 'f') { if (this.imageOnShow()) this.flipFit(); }
      else if (k === 'H' || k === 'M' || k === 'L') {
        if (this.imageOnShow()) this.pageJump(view, { H: 'top', M: 'middle', L: 'bottom' }[k]);
      }
      else if (k === '+' || k === '=') this.setZoom(Math.min(4, this.carousel.current.zoom + 0.25));
      else if (k === '-') this.setZoom(Math.max(0.5, this.carousel.current.zoom - 0.25));
      else if (k === '0') this.setZoom(1);
      else if (k === 'Escape') this.carousel = null;
    },

    /// Whether the carousel's file is drawn as page images — a PDF on the
    /// image side, or a photo. A text file has no page to fit or jump in.
    imageOnShow() {
      const f = this.carousel && this.carousel.current;
      return !!f && !/\.txt$/i.test(f.path || '') && !(this.pdfView && this.pdfOnShow());
    },

    /// `f`. The fit is one setting for the whole carousel, so it changes on
    /// every file's stage — each keeping the page it was on (a stage not on
    /// show is still laid out, so it can be measured) and each back at 100%,
    /// since fit and zoom go together.
    flipFit() {
      const c = this.carousel;
      if (!c) return;
      const fit = this.pageFit === 'height' ? 'width' : 'height';
      document.querySelectorAll('.carousel .cview').forEach(view => {
        const f = c.files.find(x => x.kind === view.dataset.kind);
        this.keepPage(view, () => {
          if (f) f.zoom = 1;
          this.paintPages(view, fit, 1);
        }, false);
        this.sharpen(view);
      });
      this.pageFit = fit;
    },

    /// The file on show's own zoom; the others keep theirs.
    setZoom(z) {
      const f = this.carousel && this.carousel.current;
      const view = this.carouselView();
      if (!f || !view) return;
      this.keepPage(view, () => {
        f.zoom = z;
        this.paintPages(view, this.pageFit, z);
      }, true);
      this.sharpen(view);
    },

    /// Each page of a stage drawn at the resolution it is shown at: 150 dpi
    /// while that covers the page's laid-out width in device pixels (with a
    /// fifth to spare, which nobody can see), 300 once it does not — fit
    /// width on a large screen, or a zoom. The only two the server draws
    /// (`files::PAGE_DPI`, `SHARP_DPI`). The stage arrives at 150
    /// (`file_view.html`); this runs when it has loaded, on a zoom or a fit
    /// change, as a page scrolls near and when the window is resized.
    ///
    /// A page not drawn yet is pointed at what it needs now, up or down,
    /// wherever it is — lazy, it is fetched only as it comes into view. One
    /// already drawn only goes up, and only near the view: a zoom redraws
    /// the pages being looked at, not the whole statement, and the rest
    /// follow as they come near. The browser keeps showing the old drawing
    /// until the sharper one has arrived.
    sharpen(view) {
      if (!view) return;
      const box = view.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      for (const img of view.querySelectorAll('img.cpg[data-src]')) {
        const r = img.getBoundingClientRect();
        // `width` is the page's own, in points: 72 to the inch.
        const shown = r.width * dpr * 72 / (+img.getAttribute('width') || 1);
        const want = shown > 150 * 1.2 ? 300 : 150;
        const has = +img.dataset.dpi || 0;
        if (img.complete && img.naturalWidth > 0) {
          if (has >= want) continue;
          if (r.bottom < box.top - box.height || r.top > box.bottom + box.height) continue;
        } else if (has === want) continue;
        img.dataset.dpi = want;
        img.src = img.dataset.src + '&dpi=' + want;
      }
    },

    /// Every stage's — a resize changes how large each page is shown.
    sharpenAll() {
      if (!this.carousel) return;
      document.querySelectorAll('.carousel .cview').forEach(v => this.sharpen(v));
    },

    /// A stage's fit and zoom, on the DOM now — `keepPage` measures the new
    /// sizes in this same turn, before Alpine's own binding catches up (and
    /// writes the same values).
    paintPages(view, fit, z) {
      const inner = view.querySelector('.cviewin');
      if (!inner) return;
      inner.dataset.fit = fit;
      inner.style.setProperty('--z', String(z));
    },

    /// Resizes the pages — `change` sets the fit or the zoom, on the DOM too
    /// — keeping the page on show where it was. The sizes change and the
    /// scroll offset does not, so left alone the view showed whatever now sat
    /// at the old offset: on the last page, `f` landed on page 3. So the page
    /// is noted first and put back after: a fit change at its top, a zoom
    /// (`keepPoint`) with the same point of the page at the top of the view.
    /// `change` must apply the size to the DOM itself — measuring after
    /// Alpine's next flush is the race CLAUDE.md's `$nextTick` gotcha
    /// describes.
    keepPage(view, change, keepPoint) {
      const page = view && this.pageAt(view);
      let frac = 0;
      if (page && keepPoint) {
        const r = page.getBoundingClientRect();
        frac = Math.max(0, (view.getBoundingClientRect().top - r.top) / (r.height || 1));
      }
      change();
      if (!page) return;
      const box = view.getBoundingClientRect();
      const r = page.getBoundingClientRect();
      const pad = parseFloat(getComputedStyle(page.closest('.cviewin') || view).paddingTop) || 0;
      const top = view.scrollTop + (r.top - box.top);
      const to = frac > 0 ? top + frac * r.height : top - pad;
      view.scrollTo({ top: Math.max(0, Math.round(to)) });
    },

    // --- page images: the carousel's image side and the trim view's page ---
    //
    // One set of keys wherever a page is shown as an image: `j`/`k` scroll,
    // `H`/`M`/`L` put the page on show at its top / middle / bottom, as Vim's
    // do for the screen, and `f` flips fit height ⇄ fit width. Fit height is
    // how both open — a whole page at a glance; fit width is for reading one,
    // which is what these keys are for.

    /// A step of the view's own height, instantly — a held `j` repeats.
    pageScroll(view, dir) {
      if (!view) return;
      view.scrollBy({ top: dir * Math.max(40, Math.round(view.clientHeight * 0.15)) });
    },

    /// The page image under the middle of the view, or the nearest one.
    pageAt(view) {
      const pages = [...view.querySelectorAll('img.cpg, img.pz-big')];
      if (!pages.length) return null;
      const box = view.getBoundingClientRect();
      const mid = box.top + box.height / 2;
      let best = null;
      for (const p of pages) {
        const r = p.getBoundingClientRect();
        if (r.top <= mid && r.bottom >= mid) return p;
        const d = Math.min(Math.abs(r.top - mid), Math.abs(r.bottom - mid));
        if (!best || d < best.d) best = { p, d };
      }
      return best.p;
    },

    /// The scroll offset that puts a page's top at the view's top, the
    /// view's padding above it — where `J`/`K` land and a shared bill opens.
    pageTop(view, page) {
      const pad = parseFloat(getComputedStyle(page.closest('.cviewin') || view).paddingTop) || 0;
      return view.scrollTop + (page.getBoundingClientRect().top - view.getBoundingClientRect().top) - pad;
    },

    /// `gg` / `G`: the top of the first page, the foot of the last.
    pageEnd(view, dir) {
      if (!view) return;
      view.scrollTo({ top: dir < 0 ? 0 : view.scrollHeight });
    },

    /// `J` / `K`: the next / previous page, its top at the view's top — at
    /// fit height a whole page per press, landing whole, where scrolling a
    /// view's height would drift by the gap between pages. Partway into a
    /// page, `K` goes to that page's own top first, as Vim's `{` goes to the
    /// start of the paragraph it is in. Clamped at either end.
    pageStep(view, dir) {
      const page = view && this.pageAt(view);
      if (!page) return;
      const pages = [...view.querySelectorAll('img.cpg, img.pz-big')];
      const box = view.getBoundingClientRect();
      const pad = parseFloat(getComputedStyle(page.closest('.cviewin') || view).paddingTop) || 0;
      const at = pages.indexOf(page);
      const partway = page.getBoundingClientRect().top - box.top < pad - 1;
      const to = dir < 0 && partway ? page : pages[at + dir];
      if (!to) return;
      view.scrollTo({ top: Math.max(0, Math.round(this.pageTop(view, to))) });
    },

    /// `H` / `M` / `L`: the page's top at the view's top, its middle at the
    /// middle, its bottom at the bottom — measured, with the view's padding.
    pageJump(view, where) {
      const page = view && this.pageAt(view);
      if (!page) return;
      const box = view.getBoundingClientRect();
      const r = page.getBoundingClientRect();
      const pad = parseFloat(getComputedStyle(page.closest('.cviewin') || view).paddingTop) || 0;
      const top = view.scrollTop + (r.top - box.top);
      const to = where === 'top' ? top - pad
        : where === 'bottom' ? top + r.height - view.clientHeight + pad
        : top + r.height / 2 - view.clientHeight / 2;
      view.scrollTo({ top: Math.max(0, Math.round(to)) });
    },


    /// `gt` / `gT`. With no count it is the next/previous tab, wrapping; with
    /// one it is that tab by number, which is what `2gt` means in Vim.
    /// `gt` / `gT`, with Vim's own count semantics — which are **not**
    /// symmetric, and were not before:
    ///
    /// - `{count}gt` goes to tab {count}. `1gt` is the first tab, which used
    ///   to cycle forward instead, because a count of 1 was indistinguishable
    ///   from no count at all.
    /// - `{count}gT` goes {count} tabs BACK. `2gT` used to jump forward to
    ///   tab 2, the same as `2gt`, which is the one thing it cannot mean.
    ///
    /// Uncounted, both cycle and wrap.
    gotoTab(dir, n, counted) {
      const panes = document.querySelectorAll('.pane').length;
      if (!panes) return;
      if (counted && dir > 0) {
        this.pane = Math.min(Math.max(1, n), panes) - 1;
      } else {
        const steps = counted ? n : 1;
        this.pane = (((this.pane + dir * steps) % panes) + panes) % panes;
      }
      this.row = 0;
      this.paint();
    },

    // --- editing a row in place -------------------------------------------

    /// `e`. A row with a parked draft resumes it exactly as it was left.
    editRow() {
      const id = this.rowUnderCursor();
      if (!id || this.readOnly()) return;
      this.editing = id;
      const wrapper = document.getElementById('row-' + id);
      if (wrapper) ffmRowHtml.set(id, wrapper.outerHTML);
      htmx.ajax('GET', '/tx/' + id + '/edit', {
        target: '#row-' + id, swap: 'outerHTML',
      }).then(() => this.restoreDraft(id));
    },

    /// Leaving a row mid-edit collapses it back to read-only. An UNCHANGED
    /// draft just goes away; a changed one stays marked on the row until it is
    /// saved or discarded, and the statusline counts them.
    parkDraft(id) {
      const form = document.querySelector('#row-' + id + ' form');
      // No form left to read means the edit already ended some other way —
      // a save swapped it out. Clear the id rather than leaving it stuck,
      // or every later cursor move re-enters this no-op.
      if (!form) { this.editing = null; return; }
      const values = {};
      let changed = false;
      form.querySelectorAll('[name]').forEach(el => {
        values[el.name] = el.type === 'checkbox' ? el.checked : el.value;
        if (el.type === 'checkbox' ? el.checked !== el.defaultChecked
                                   : el.value !== el.defaultValue) changed = true;
      });
      if (changed) this.drafts[id] = values; else delete this.drafts[id];
      this.editing = null;
      this.restoreRow(id);
    },

    /// Put the read-only row back where the edit form is. Instant when the
    /// row `e` replaced is still known and still true (`ffmRowHtml`); a
    /// request for it otherwise.
    ///
    /// Esc used to ask the server every time, which is a whole round trip —
    /// on a slow link, a visible wait — to get back markup that had not
    /// changed. Through `htmx.swap`, so the row is processed exactly like
    /// one that came over the wire.
    restoreRow(id) {
      // Always a string: the form's own Esc and Cancel pass the id as the
      // number the template wrote, and a Map keeps 12 and "12" apart — which
      // silently missed the kept row and fell back to the request.
      id = String(id);
      const html = ffmRowHtml.get(id);
      ffmRowHtml.delete(id);
      const done = () => { this.markDrafts(); this.paint(); };
      if (html && document.getElementById('row-' + id)) {
        htmx.swap('#row-' + id, html, { swapStyle: 'outerHTML' });
        done();
        return;
      }
      htmx.ajax('GET', '/tx/' + id, { target: '#row-' + id, swap: 'outerHTML' }).then(done);
    },

    /// What becomes of an open edit when its row is swapped out from under
    /// it — a save that worked, or one the server refused.
    ///
    /// It lives on the SHELL rather than on the form because
    /// `htmx:beforeSwap` fires on the swap *target*, which is the row wrapper
    /// the form sits inside; a listener on the form never hears it. And it
    /// has to be before-swap rather than after-request, because reading a
    /// form's values requires the form to still be in the DOM.
    savedSwap(e) {
      const target = e.detail && e.detail.target;
      if (!target || !target.id || !target.id.startsWith('row-')) return;
      const form = target.querySelector('form.editrow');
      if (!form) return;
      const id = target.id.slice(4);
      // FFM-Reopen: the server is handing the FORM back, not a row — a
      // rejected save with the messages in it. The edit has not ended, so
      // nothing here should act as though it has.
      if (e.detail.xhr && e.detail.xhr.getResponseHeader('FFM-Reopen')) return;
      this.editing = null;
      ffmRowHtml.delete(id);
      // FFM-Stale means the save was REFUSED — somebody changed the row while
      // this form was open. The response is still a 200 carrying a row, which
      // is this app's error convention, so the header is what tells the two
      // apart. Nothing typed is thrown away in that case.
      if (e.detail.xhr && e.detail.xhr.getResponseHeader('FFM-Stale')) {
        this.stashDraft(id, form);
      } else {
        delete this.drafts[id];
      }
      this.$nextTick(() => { this.markDrafts(); this.paint(); });
    },

    /// Keeps a draft that could not be saved. Unlike parkDraft this does NOT
    /// ask whether anything changed: the save was refused, so every value in
    /// the form is something somebody meant, including the ones that happen
    /// to match what is on the row.
    stashDraft(id, form) {
      const values = {};
      form.querySelectorAll('[name]').forEach(el => {
        values[el.name] = el.type === 'checkbox' ? el.checked : el.value;
      });
      // The version traveled with the form and is now stale by definition.
      // Keeping it would make the retry refuse for the same reason, forever.
      delete values.version;
      this.drafts[id] = values;
    },

    restoreDraft(id) {
      const values = this.drafts[id];
      if (!values) return;
      const form = document.querySelector('#row-' + id + ' form');
      if (!form) return;
      Object.entries(values).forEach(([name, value]) => {
        const el = form.querySelector('[name="' + name + '"]');
        if (!el) return;
        if (el.type === 'checkbox') el.checked = value; else el.value = value;
      });
    },

    /// The mark itself: a dashed border and a tag saying how to pick it back
    /// up. Applied after every swap, because a swap is exactly when the mark
    /// would otherwise be lost.
    markDrafts() {
      document.querySelectorAll('.row.has-draft').forEach(r => {
        r.classList.remove('has-draft');
        r.querySelector('.drafttag')?.remove();
      });
      Object.keys(this.drafts).forEach(id => {
        const row = document.querySelector('#row-' + id + ' .row');
        if (!row) return;
        row.classList.add('has-draft');
        const meta = row.querySelector('.meta');
        if (meta && !meta.querySelector('.drafttag')) {
          const tag = document.createElement('span');
          tag.className = 'drafttag';
          tag.textContent = this.unsavedTag;
          meta.appendChild(tag);
        }
      });
      // A rule's draft marks the row its rule stands behind, with its own
      // tag — `E` resumes it, not `e`.
      Object.keys(this.ruleDrafts).forEach(rid => {
        document.querySelectorAll('.row[data-rule-id="' + rid + '"]').forEach(row => {
          row.classList.add('has-draft');
          const meta = row.querySelector('.meta');
          if (meta && ![...meta.querySelectorAll('.drafttag')].some(t => t.textContent === this.unsavedRuleTag)) {
            const tag = document.createElement('span');
            tag.className = 'drafttag';
            tag.textContent = this.unsavedRuleTag;
            meta.appendChild(tag);
          }
        });
      });
      const n = Object.keys(this.drafts).length + Object.keys(this.ruleDrafts).length;
      if (n) this.say(n === 1 ? this.draftsParkedOne : this.draftsParked.replace('{n}', n), false);
    },

    cancelEdit(id) {
      // Esc discards: nothing was written, and nothing is kept.
      delete this.drafts[id];
      this.editing = null;
      this.restoreRow(id);
    },

    togglePrivate() {
      const id = this.rowUnderCursor();
      if (!id || this.readOnly()) return;
      htmx.ajax('POST', '/tx/' + id + '/privacy', {
        target: '#row-' + id, swap: 'outerHTML',
      });
    },

    /// `m` — the manual paid tick, on an untracked recurring row only. The
    /// row says whether it applies (`data-paid`); the server decides again.
    togglePaid() {
      const id = this.rowUnderCursor();
      if (!id || this.readOnly()) return;
      const el = this.rows()[this.row];
      if (!el || el.dataset.paid === undefined) {
        this.say(el && el.dataset.ruleId ? this.paidRefusedTracked : this.paidRefusedOneOff, true);
        return;
      }
      htmx.ajax('POST', '/tx/' + id + '/paid', {
        target: '#row-' + id, swap: 'outerHTML',
      });
    },

    /// `x` — a marker for a batch operation, never a delete. On a fold's
    /// header it marks the whole fold — the one thing a header can be the
    /// target of, where every other row key says it is a group.
    toggleSelected() {
      if (this.onFoldHead()) {
        this.toggleIds(this.selectableIds(this.rows()[this.row].closest('.fold')));
        return;
      }
      const id = this.rowUnderCursor();
      if (!id) return;
      if (this.selected[id]) delete this.selected[id]; else this.selected[id] = true;
      this.paintSelection();
    },

    /// `X` — every row of the fold the cursor is in, from a row or from its
    /// header: the trash's `X` on its day, pointed at a bucket.
    toggleFold() {
      const el = this.rows()[this.row];
      const section = el && el.closest('.fold');
      if (!section) { this.say(this.groupSelectNeedsGroup, true); return; }
      this.toggleIds(this.selectableIds(section));
    },

    /// `gx` — every row in the focused pane, grouped or flat.
    toggleAllRows() {
      const pane = document.querySelectorAll('.pane')[this.pane];
      if (pane) this.toggleIds(this.selectableIds(pane));
    },

    /// The rows a group toggle covers inside `scope`: real ones, by id. A row
    /// `dd` struck through is not in the pane any more — the server leaves it
    /// out of the sum — and a placeholder or an in-flight add is not a row
    /// yet, which its id says.
    selectableIds(scope) {
      return Array.from(scope.querySelectorAll('div[id^="row-"]'))
        .filter(w => /^row-\d+$/.test(w.id) && !w.querySelector('.row.is-deleted'))
        .map(w => w.id.slice(4));
    },

    /// All selected → none, otherwise → all: the trash's and `:prune`'s rule,
    /// so a second press undoes the first and a half-selected group fills
    /// rather than empties.
    toggleIds(ids) {
      if (!ids.length) return;
      const on = !ids.every(id => this.selected[id]);
      ids.forEach(id => { if (on) this.selected[id] = true; else delete this.selected[id]; });
      this.paintSelection();
    },

    paintSelection() {
      document.querySelectorAll('.row.is-selected').forEach(r => r.classList.remove('is-selected'));
      Object.keys(this.selected).forEach(id => {
        document.querySelector('#row-' + id + ' .row')?.classList.add('is-selected');
      });
      // A fold whose every row is selected says so on its header — with the
      // fold closed, the header is the only thing on screen to show it.
      document.querySelectorAll('.foldhead').forEach(head => {
        const ids = this.selectableIds(head.closest('.fold'));
        head.classList.toggle('is-selected', ids.length > 0 && ids.every(id => this.selected[id]));
      });
      this.refreshSelection();
    },

    /// What the selection cell last asked about — the focused member and the
    /// selected ids on screen in that pane — so moving the cursor does not
    /// send a request per keystroke. A swap clears it (`afterSwap`): an
    /// edited amount changes the sum without changing which rows are selected.
    selAsked: null,

    /// The statusline's selection cell: count and sum of what `x` marked in
    /// the focused pane. This only lists ids the page already holds; the
    /// server counts, sums and formats them (`web::selection`), because money
    /// is formatted in one place and privacy is applied with the numbers.
    refreshSelection() {
      const cell = document.getElementById('stsel');
      const pane = document.querySelectorAll('.pane')[this.pane];
      if (!cell || !pane) return;
      const member = this.paneMemberId();
      const ids = Object.keys(this.selected)
        .filter(id => pane.querySelector('#row-' + id))
        .sort();
      const ask = member + '|' + ids.join(',');
      if (ask === this.selAsked) return;
      this.selAsked = ask;
      if (!ids.length) { cell.innerHTML = ''; return; }
      const period = document.querySelector('.ledger-shell')?.dataset.period || '';
      this.ajax('GET', '/tx/selection?member=' + member + '&period=' + period +
                '&ids=' + ids.join(','), { target: '#stsel', swap: 'outerHTML' }, 'none');
    },

    /// `ga` — "give to". Opens the member picker for the row under the
    /// cursor; the move itself waits for a name.
    askGive() {
      const id = this.rowUnderCursor();
      if (!id) return;
      // Handing a row to somebody writes into their ledger: an admin's
      // gesture, even on a row of your own.
      if (!this.admin) { this.say(this.reattributeAdminOnly, true); return; }
      const others = this.giveOrder().filter(i => i !== this.pane);
      if (!others.length) { this.say(this.giveNobody, true); return; }
      const row = this.rows()[this.row];
      this.giving = {
        id,
        from: this.pane,
        sel: others[0],
        title: row?.querySelector('.name > b')?.textContent || '',
        private: !!row?.querySelector('.mflag.flag-private.is-on'),
      };
    },

    /// Every pane's index, in pane order — the picker's list.
    giveOrder() {
      return [...document.querySelectorAll('.givelist .give')].map((_, i) => i);
    },

    /// Inside the picker: a pane's number picks it, `j`/`k` walk the list,
    /// Enter takes the highlighted one, Esc backs out.
    giveKey(k) {
      const g = this.giving;
      if (k === 'Escape') { this.giving = null; return; }
      if (k === 'Enter') { this.give(g.sel); return; }
      if (/^[1-9]$/.test(k)) {
        const i = Number(k) - 1;
        if (i !== g.from && i < this.giveOrder().length) this.give(i);
        return;
      }
      const step = { j: 1, ArrowDown: 1, k: -1, ArrowUp: -1 }[k];
      if (!step) return;
      const others = this.giveOrder().filter(i => i !== g.from);
      const at = others.indexOf(g.sel);
      g.sel = others[Math.max(0, Math.min(at + step, others.length - 1))];
    },

    give(i) {
      const g = this.giving;
      this.giving = null;
      const btn = document.querySelectorAll('.givelist .give')[i];
      if (!g || !btn || i === g.from) return;
      this.doMove(g.id, btn.dataset.member);
    },

    doMove(id, to) {
      const row = document.getElementById('row-' + id) || 'status';
      this.ajax('POST', '/tx/' + id + '/member', {
        target: 'body', swap: 'none', values: { to },
      }, row);
    },

    // --- folds -----------------------------------------------------------

    closed(id) {
      if (id in this.folds) return this.folds[id];
      if (this.foldsAll !== null) return this.foldsAll;
      return this.foldsClosed;
    },

    toggle(id) { this.setFold(id, !this.closed(id)); },

    setFold(id, closed) {
      this.folds[id] = closed;
      this.saveFolds();
    },

    /// Kept across closing the tab — it was per window, in `sessionStorage`,
    /// and every new tab or launch of the installed app opened on the
    /// member's setting with the folds gone. Stamped with that setting, so
    /// changing *Folds start* in `:profile` starts over from it rather than
    /// being overruled for good by a `zM` made long ago.
    saveFolds() {
      try {
        localStorage.setItem(this.foldsKey,
          JSON.stringify({ all: this.foldsAll, keys: this.folds, start: this.foldsClosed }));
      } catch (_) {}
    },

    loadFolds() {
      let old = null;
      try {
        let saved = JSON.parse(localStorage.getItem(this.foldsKey) || 'null');
        // What a window open before this change had, carried over once.
        old = !saved && JSON.parse(sessionStorage.getItem('ffm.folds') || 'null');
        if (old) saved = { ...old, start: this.foldsClosed };
        sessionStorage.removeItem('ffm.folds');
        if (saved && saved.start === this.foldsClosed) {
          this.foldsAll = typeof saved.all === 'boolean' ? saved.all : null;
          this.folds = saved.keys && typeof saved.keys === 'object' ? saved.keys : {};
        } else if (saved) {
          // Made under the other *Folds start*: gone for good, so turning
          // the setting back does not bring old folds with it.
          localStorage.removeItem(this.foldsKey);
        }
      } catch (_) {}
      if (old) this.saveFolds();
    },

    /// The fold the cursor is inside, so `za` works from a row rather than
    /// only from the header.
    foldAt() {
      const el = this.rows()[this.row];
      // `closest` covers both kinds of position: a row inside the fold, and
      // the fold's own header — which is the only thing left to stand on once
      // the fold is closed.
      const section = el && el.closest('.fold');
      return section ? this.foldIdOf(section) : null;
    },

    /// A fold's key in `this.folds`: its bucket's id, which the template
    /// writes on the section. It was `{member}-{index}` — the fold's place,
    /// which names another bucket in another month or after `S`.
    foldIdOf(section) {
      return section.dataset.fold || null;
    },

    fold(which) {
      const id = this.foldAt();
      if (!id) return;
      const anchor = this.anchorCursor();
      if (which === 'a') this.toggle(id);
      else this.setFold(id, which === 'c');
      this.settleCursor(anchor);
    },

    allFolds(closed) {
      const anchor = this.anchorCursor();
      // Every fold, including the buckets only other months have: the folds
      // toggled one by one before are forgotten, as `zM` means all of them.
      this.foldsAll = closed;
      this.folds = {};
      this.saveFolds();
      this.settleCursor(anchor);
    },

    /// The two positions a fold key could leave the cursor on: where it is,
    /// and the header of the fold it is inside.
    anchorCursor() {
      const el = this.rows()[this.row];
      const fold = el && el.closest('.fold');
      return { el, head: fold && fold.querySelector('.foldhead') };
    },

    /// `zc` on a row inside the fold leaves the cursor on the fold's header,
    /// the way Vim does — and it has to, because `this.row` is an index into
    /// the VISIBLE positions and collapsing a fold shortens that list under
    /// it. Without this the index kept its number and quietly meant a
    /// different line: `zc` then `j` from the last row of a three-row fold
    /// landed four folds further down.
    settleCursor(anchor) {
      const rows = this.rows();
      const i = rows.indexOf(anchor.el);
      this.row = i >= 0 ? i : Math.max(0, rows.indexOf(anchor.head));
      this.paint();
    },

    // --- the keymap (`?`) ------------------------------------------------

    /// `?`, and `:help` — with a term, already searched, the way Vim's
    /// `:help gt` lands on `gt`. The keys stay the modal's: j/k scroll the
    /// results at once, and `/` refines them.
    openKeymap(term) {
      this.kmQuery = term || '';
      this.kmBuf = '';
      this.keymap = true;
      this.filterKeymap();
      this.$nextTick(() => { if (this.$refs.kmbody) this.$refs.kmbody.scrollTop = 0; });
    },

    /// Closing forgets the search, so the next `?` opens on the whole keymap.
    closeKeymap() {
      this.keymap = false;
      this.kmQuery = '';
      this.filterKeymap();
    },

    /// A key with the keymap open. Its own search field takes what is typed
    /// there (Enter and Esc are the field's); everything else is the modal's.
    keymapKey(e, el) {
      if (el && el.closest('.kmfind')) return;
      // A modifier alone is not a keystroke — it must not drop the `g` of a
      // `gg` (the Shift gotcha in CLAUDE.md).
      if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) return;
      const body = this.$refs.kmbody;
      const armed = this.kmBuf;
      this.kmBuf = '';
      const k = e.key;
      let done = true;
      if (k === 'Escape' || k === '?') this.closeKeymap();
      // Synchronously, inside the key's own event: with a phone's keyboard
      // up, iOS keeps it only when focus moves to a field in the same
      // gesture (see `focusBar`).
      else if (k === '/') { this.$refs.kmfind.focus({ preventScroll: true }); this.$refs.kmfind.select(); }
      else if (e.ctrlKey && (k === 'd' || k === 'u')) {
        body?.scrollBy({ top: (k === 'd' ? 1 : -1) * Math.round(body.clientHeight / 2) });
      }
      else if (e.ctrlKey || e.metaKey || e.altKey) done = false;
      else if (k === 'j' || k === 'ArrowDown') this.pageScroll(body, 1);
      else if (k === 'k' || k === 'ArrowUp') this.pageScroll(body, -1);
      else if (k === 'g') {
        if (armed === 'g') body?.scrollTo({ top: 0 });
        else this.kmBuf = 'g';
      }
      else if (k === 'G') body?.scrollTo({ top: body.scrollHeight });
      else done = false;
      if (done) e.preventDefault();
    },

    /// Filters the keymap to what the search matches — markup the server
    /// already rendered, so it is instant and asks nothing, like `/` on the
    /// ledger. A key row matches on its key or on what it does, a legend
    /// line on its words; a section whose heading matches keeps every row
    /// (`/files` is the Files section). The match is marked in place.
    ///
    /// A regex falling back to a literal, as `/` does — which matters more
    /// here, where `?`, `+` and `[` are keys as well as metacharacters.
    /// Smartcase, as Vim's: a capital in the term makes it case-sensitive,
    /// so `J` finds the `J` row and not every `j`. Accents are set aside on
    /// both sides, so `periodo` finds "período".
    filterKeymap() {
      const body = this.$refs.kmbody;
      if (!body) return;
      const term = this.unaccent(this.kmQuery.trim()).text;
      const flags = /[A-Z]/.test(term) ? '' : 'i';
      let re = null;
      if (term) {
        try { re = new RegExp(term, flags); }
        catch { re = new RegExp(term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), flags); }
      }
      let shown = 0;
      let total = 0;
      body.querySelectorAll('.kmsect').forEach(sect => {
        // Always through `markText`, with no term too: that is what takes a
        // heading's mark back off when the search is cleared.
        const whole = this.markText(sect.querySelector('.sect'), re);
        let any = false;
        sect.querySelectorAll('.krow, .lrow').forEach(row => {
          total++;
          const cells = row.matches('.krow') ? row.querySelectorAll('kbd, span') : row.querySelectorAll('.ltext');
          // Every cell is marked (or unmarked), so the eye finds the match in
          // whichever column it is.
          let hit = !re;
          cells.forEach(c => { if (this.markText(c, re)) hit = true; });
          row.hidden = !(hit || whole);
          if (!row.hidden) { shown++; any = true; }
        });
        sect.hidden = !any;
      });
      this.kmCount = re ? shown + '/' + total : '';
    },

    /// Marks `re`'s first match in `el`'s text, in place, and says whether
    /// there was one; no regex puts the text back as the server rendered it.
    /// Built from text nodes, never markup. The match is made on the text
    /// with its accents set aside, and mapped back onto the original
    /// letters, so "período" is marked whole.
    markText(el, re) {
      if (!el) return false;
      if (el.dataset.text === undefined) el.dataset.text = el.textContent;
      const text = el.dataset.text;
      if (!re) {
        if (el.querySelector('mark')) el.textContent = text;
        return false;
      }
      const { text: folded, at } = this.unaccent(text);
      const m = re.exec(folded);
      if (!m) {
        if (el.querySelector('mark')) el.textContent = text;
        return false;
      }
      const from = at[m.index];
      const to = at[m.index + m[0].length];
      const mark = document.createElement('mark');
      mark.className = 'kmhit';
      mark.textContent = text.slice(from, to);
      el.replaceChildren(document.createTextNode(text.slice(0, from)), mark,
                         document.createTextNode(text.slice(to)));
      return true;
    },

    /// `s` with its accents taken off, and where each folded character came
    /// from: `at[i]` is the index in `s` of folded character `i`, with one
    /// extra entry for the end.
    unaccent(s) {
      let text = '';
      const at = [];
      for (let i = 0; i < s.length; ) {
        const ch = String.fromCodePoint(s.codePointAt(i));
        const bare = ch.normalize('NFD').replace(/\p{M}/gu, '');
        for (let j = 0; j < bare.length; j++) at.push(i);
        text += bare;
        i += ch.length;
      }
      at.push(s.length);
      return { text, at };
    },

    // --- search ----------------------------------------------------------

    openSearch() {
      this.searching = true;
      this.recallAt = null;
      this.focusBar(this.$refs.search);
    },

    /// A bar's field takes focus at once, inside the keydown that opened it.
    /// Two reasons, either enough. iOS keeps its keyboard only when focus
    /// moves to another field inside the same gesture, and `focusSoon`'s
    /// `$nextTick` is already outside it, so the keyboard dropped and `:`
    /// typed nothing. And on any keyboard, a key typed before the focus lands
    /// is read by the keymap: an `s` typed straight after `:` cycled the sort
    /// and wrote `ledger_sort` (real Safari, 2 of 3 on a reopened bar).
    /// The bar is still `display: none` until Alpine's `x-show` lands, and a
    /// hidden field cannot take focus, so it is shown here first; `x-show`
    /// then agrees with it. `focusSoon` stays as the fallback for a field
    /// that still could not take it.
    focusBar(el) {
      if (!el) return;
      el.closest('.cmdbar')?.style.removeProperty('display');
      el.focus({ preventScroll: true });
      if (document.activeElement !== el) this.focusSoon(el);
    },

    // --- the phone's keyboard ----------------------------------------------

    /// The `NORMAL` badge. The focus has to happen right here, synchronously
    /// inside the tap: that is the only way iOS shows its keyboard.
    toggleKeyboard() {
      const trap = this.$refs.keytrap;
      if (this.keyboard) {
        this.keyboard = false;
        this.ctrlArmed = false;
        trap.blur();
        return;
      }
      this.keyboard = true;
      trap.focus({ preventScroll: true });
    },

    /// Back to the trap after a bar closes, so the next `j` is a motion
    /// without another tap — or, with no keyboard up, simply out of the field.
    backToKeys(el) {
      if (this.keyboard) this.$refs.keytrap.focus({ preventScroll: true });
      else el?.blur();
    },

    /// A key typed into the trap. Runs before the window's `key()`, which
    /// still sees the event: this only makes sure nothing is ever inserted,
    /// and adds Ctrl when the strip armed it.
    keyTrapDown(e) {
      // Predictive text sends `Unidentified`; the character arrives on
      // `input` instead (`keyTrapInput`). `key()` must not see it, or it
      // clears a pending `g` or count for a key that was never pressed.
      if (e.key === 'Unidentified' || e.key === 'Process' || e.isComposing) {
        e.stopPropagation();
        return;
      }
      if (this.ctrlArmed && e.key.length === 1 && !e.ctrlKey) {
        e.preventDefault();
        e.stopPropagation();
        this.ctrlArmed = false;
        e.target.dispatchEvent(new KeyboardEvent('keydown',
          { key: e.key, ctrlKey: true, bubbles: true, cancelable: true }));
        return;
      }
      if (e.key.length === 1) e.preventDefault();
    },

    /// The fallback: a character that reached the field anyway is sent on as
    /// the key it is, and the field is emptied again.
    keyTrapInput(e) {
      const chars = Array.from(e.data || e.target.value || '');
      e.target.value = '';
      chars.forEach(ch => e.target.dispatchEvent(new KeyboardEvent('keydown',
        { key: ch, bubbles: true, cancelable: true })));
    },

    /// The strip's buttons: a key the phone's keyboard does not have, sent to
    /// whatever has focus — the command bar's own Esc handler when it is
    /// open, `key()` otherwise. A tap, so any focus it causes keeps the
    /// keyboard up.
    stripKey(k) {
      if (k === 'Escape') this.ctrlArmed = false;
      const at = document.activeElement && document.activeElement !== document.body
        ? document.activeElement : this.$refs.keytrap;
      at.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
    },

    /// Focus left for somewhere that holds no keyboard — nowhere (Done, a tap
    /// on the ledger, a field swapped away), or a checkbox or button (the
    /// trash's list) — so the keyboard is gone, and the strip goes with it.
    /// Focus moving between text fields (trap → command bar → trap) keeps it.
    keyFocusOut(e) {
      const to = e.relatedTarget;
      const typed = !!to && to.matches('textarea, input:not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]):not([type=file])');
      if (this.keyboard && !typed) {
        this.keyboard = false;
        this.ctrlArmed = false;
      }
    },

    /// iOS shrinks only the VISUAL viewport for its keyboard, so a shell that
    /// is `100vh` tall puts the statusline and the command bar under it. While
    /// the keyboard is up the shell is sized to what is actually visible —
    /// and the statusline drops its home-indicator padding (`--safe-b`), since
    /// the keyboard covers the home indicator and the padding would only float
    /// the bar above it.
    fitViewport() {
      const vv = window.visualViewport;
      const shell = this.shell();
      if (!vv || !shell) return;
      if (!this.keyboard) {
        shell.style.removeProperty('--app-h');
        shell.style.removeProperty('--safe-b');
        return;
      }
      shell.style.setProperty('--app-h', vv.height + 'px');
      shell.style.setProperty('--safe-b', '0px');
      window.scrollTo(0, 0);
    },

    /// Incremental, client-side and scoped to the visible month: it filters
    /// rows already rendered, which is what makes it feel instant. `:grep` is
    /// the different verb that reaches other months.
    runSearch() {
      const term = this.search;
      // The positions are read first, while the layout is clean. Read after
      // the marks below came off and went on, each read made the browser lay
      // the page out again — twice a keystroke, which was nearly all of what
      // a keystroke cost, where reading the rows' words was a few percent.
      const rows = term ? this.rows() : [];
      document.querySelectorAll('.row.is-match').forEach(r => r.classList.remove('is-match'));
      this.matches = [];
      if (!term) return;

      // A regex, falling back to a literal when it does not compile — a
      // half-typed `(` should narrow the list, not blow up.
      let re;
      try { re = new RegExp(term, 'i'); }
      catch { re = new RegExp(term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i'); }

      rows.forEach((row, i) => {
        // Title, dates, the formatted amount — and the WORDS a row's state
        // implies, so `/private` and `/paid` find what they look like they
        // should.
        const hay = [
          row.querySelector('.name b')?.textContent || '',
          row.querySelector('.meta')?.textContent || '',
          row.querySelector('.amt')?.textContent || '',
          row.dataset.ruleId && !row.dataset.ruleDeleted ? 'recurring' : '',
          row.querySelector('.mflag.flag-private.is-on') ? 'private' : '',
          row.querySelector('.disc.tone-pos') ? 'paid' : '',
        ].join(' ');
        if (re.test(hay)) {
          row.classList.add('is-match');
          this.matches.push(i);
        }
      });
      this.msg = this.matches.length ? (this.matches.length + '/' + rows.length) : this.lastCmd;
    },

    step(dir) {
      if (!this.matches.length) return;
      // Wrapping, as `n` does in Vim.
      this.matchAt = (this.matchAt + dir + this.matches.length) % this.matches.length;
      this.row = this.matches[this.matchAt];
      this.paint();
    },

    clearSearch() {
      this.search = '';
      this.searching = false;
      this.matches = [];
      document.querySelectorAll('.row.is-match').forEach(r => r.classList.remove('is-match'));
      this.msg = this.lastCmd;
    },

    /// `J` / `K`. With rows selected in the focused pane, every one of them
    /// moves, together, wherever the cursor is; with none, the cursor's row.
    /// The cursor stays on its row by id either way, so it rides along when
    /// its row was selected, and the selection — ids — is painted back on.
    move(delta) {
      const id = this.rowUnderCursor();
      if (!id || this.readOnly()) return;
      const pane = document.querySelectorAll('.pane')[this.pane];
      const ids = pane ? this.selectableIds(pane).filter(i => this.selected[i]) : [];
      const values = { delta };
      if (ids.length) values.ids = ids.join(',');
      this.ajax('POST', '/tx/' + id + '/move', {
        target: 'body', swap: 'none', values,
      }, this.focusedRow()).then(() => { this.cursorTo(id); this.paintSelection(); });
    },

    /// Put the cursor back on a row by id, wherever the swap left it.
    ///
    /// `j` moves me and `J` moves the thing — and the cursor is an INDEX, so
    /// the moment the row under it moves, that index names whichever row took
    /// the vacated slot: the one that just swapped past it. Pressing `J`
    /// again then moved THAT row back down, and the two traded places
    /// forever. A row could not be walked down the ledger a press at a time
    /// at all; it alternated with its neighbor, once per keystroke.
    ///
    /// `5J` hid this completely, because a count is one request and one swap
    /// — which is why the bug survived a keymap sweep that tested counts.
    ///
    /// The swap re-renders the whole pane, so the element the cursor used to
    /// point at is gone; the id is the only thing that survives it.
    /// One blink on the row a `u`/`U` just changed, so the eye lands where the
    /// statusline is talking about. Restarted rather than stacked when two
    /// steps land on the same row in a row.
    flashRow(id) {
      this.$nextTick(() => {
        const el = document.getElementById('row-' + id)?.querySelector('.row');
        if (!el) return;
        el.classList.remove('is-flash');
        void el.offsetWidth;
        el.classList.add('is-flash');
        el.addEventListener('animationend', () => el.classList.remove('is-flash'), { once: true });
      });
    },

    cursorTo(id) {
      this.$nextTick(() => {
        const i = this.rows().findIndex(
          el => el.closest('div[id^="row-"]')?.id === 'row-' + id,
        );
        // Not found is a real case, not a bug: a re-attribution moves the row
        // to another pane entirely. Leaving the cursor where it is, is the
        // right answer there.
        if (i >= 0) this.row = i;
        this.paint();
      });
    },

    /// A click puts the cursor where it landed — the same cursor `j`/`k`
    /// move, placed by pointing instead. A tap on a phone is the same click.
    ///
    /// Delegated from the pane rather than bound on each row: rows are
    /// swapped in and out by HTMX all the time, and a listener on the section
    /// survives every one of those swaps.
    ///
    /// The index comes from `rows()`, so a click can only land on a real
    /// cursor position — a fold header or a visible row. The skeleton, the
    /// add form and the pane's own header are not positions and move nothing
    /// (except which pane is focused). Everything a cursor move implies —
    /// parking a draft, closing `E`/`gl`, `?pane=`, relative numbers — comes
    /// from `paint()`, exactly as it does for `j`.
    ///
    /// A fold header toggles as well, and it goes through `fold('a')` so the
    /// cursor is already standing on the header when the fold closes. A
    /// header that toggled on its own left the index pointing into the fold
    /// it had just hidden.
    clickPane(e, i) {
      // The press that opened the row menu ends in a click when the finger
      // lifts. It has already put the cursor on the row; taken as a tap it
      // would do nothing worse than repaint, but it is not a second gesture.
      if (this.pressFired) return;
      // The mouse is the thing acting now, so hover comes back.
      document.body.classList.remove('kbd-nav');
      const was = this.pane;
      this.pane = i;
      const hit = e.target.closest('.foldhead, div[id^="row-"] .row');
      const at = hit ? this.rows().indexOf(hit) : -1;
      const head = at >= 0 && hit.classList.contains('foldhead');
      if (at >= 0) {
        // Already there — a click inside the open edit form, say. Nothing
        // moved, so nothing repaints.
        if (!head && was === i && at === this.row) return;
        this.row = at;
      } else if (was !== i) {
        this.row = 0;            // the pane's chrome: the same as H/L
      } else {
        return;
      }
      // A `gd` popup belongs to the row it was opened on, like a hover.
      this.tip = null;
      if (head) this.fold('a');  // fold() paints
      else this.paint();
    },

    /// The row menu: a long press on a touch screen, a right-click with a
    /// mouse. The touch half of the keymap — each item runs the method its
    /// key runs and shows that key, so the menu teaches the keys rather than
    /// standing in for them.
    ///
    /// Only on a row. A fold header, the pane's chrome and the open edit
    /// form keep the browser's own menu, which is what a right-click in a
    /// text field is for.
    ///
    /// Returns whether it took the gesture, so a long press knows to swallow
    /// the click that follows it.
    rowMenuAt(e, i, x, y) {
      const hit = e.target.closest('div[id^="row-"] .row');
      if (!hit || hit.classList.contains('is-editing')) return false;
      e.preventDefault();
      document.body.classList.remove('kbd-nav');
      this.pane = i;
      const at = this.rows().indexOf(hit);
      if (at < 0) return false;
      this.row = at;
      this.tip = null;
      this.paint();
      // On somebody else's ledger the write items are absent, and the menu
      // still opens for the read ones — looking at a row's files and history
      // is allowed there, as `gp` and `gl` already are from the keyboard.
      // Read straight off the pane rather than through `readOnly()`, which
      // would say "read-only" over a menu that just opened.
      const write = !hit.closest('.pane')?.dataset.readonly;
      const m = {
        id: this.focusedId(),
        write,
        // `editRule()`'s own test, so the item and the key cannot disagree:
        // absent on a plain row, and on an orphan whose rule was deleted.
        rule: write && !!hit.dataset.ruleId && !hit.dataset.ruleDeleted,
        // `togglePaid()`'s own test: absent where the tick does not apply.
        paid: !write || hit.dataset.paid === undefined ? null : hit.dataset.paid === '1',
        income: !!hit.querySelector('.amt .v.is-income'),
        // `askGive()`'s own tests: an admin, and somebody else to give it to.
        give: write && this.admin && document.querySelectorAll('.pane').length > 1,
        // `openCarousel()`'s own test: a row with no files refuses `gp`.
        files: !!hit.querySelector('.fbadge[href]'),
      };
      this.rowMenu = m;
      // Beside the press rather than under it, so a finger does not cover
      // what it just opened; clamped to the viewport with the menu's rough
      // size, since `x-show` has not laid it out yet to be measured. Up to
      // six items now, so the height is counted rather than guessed once.
      // Each item and its 4px gap, the rule (1px and its margins) when there
      // is a write group above it, and the menu's padding and border — as
      // measured, since a guess 18px short put the last item off-screen.
      const items = [write, m.rule, m.paid !== null, m.give, m.files, true]
        .filter(Boolean).length;
      const itemH = matchMedia('(pointer: coarse)').matches ? 40 : 35;
      const height = items * (itemH + 4) + (write ? 13 : 0) + 10;
      const left = Math.max(8, Math.min(x + 8, innerWidth - 200));
      const top = Math.max(8, Math.min(y + 8, innerHeight - height - 8));
      this.rowMenuStyle = 'top:' + top + 'px; left:' + left + 'px';
      return true;
    },

    /// A long press, timed by hand: iOS Safari never fires `contextmenu` for
    /// one, only its own text-selection callout. Touch and pen only — a
    /// mouse has a right button for this.
    ///
    /// Any real movement cancels it, and so does the browser taking the
    /// gesture for a scroll (`pointercancel`), so a finger dragging the
    /// ledger never opens a menu on the row it started from.
    pressStart(e, i) {
      this.pressFired = false;
      this.pressCancel();
      if (e.pointerType === 'mouse' || !e.isPrimary) return;
      const { target, clientX: x, clientY: y } = e;
      this.press = {
        x, y,
        timer: setTimeout(() => {
          this.press = null;
          const fake = { target, preventDefault() {} };
          if (this.rowMenuAt(fake, i, x, y)) this.pressFired = true;
        }, 500),
      };
    },

    pressMove(e) {
      if (this.press && Math.hypot(e.clientX - this.press.x, e.clientY - this.press.y) > 10) {
        this.pressCancel();
      }
    },

    pressCancel() {
      if (!this.press) return;
      clearTimeout(this.press.timer);
      this.press = null;
    },

    // --- commands --------------------------------------------------------

    openCommand() {
      this.commanding = true;
      this.wild = null;
      this.recallAt = null;
      this.focusBar(this.$refs.cmd);
    },

    /// Focus a control that is revealed by `x-show`.
    ///
    /// An element that is still `display: none` cannot take focus, and
    /// `.focus()` on one fails **silently** — there is no error and no event
    /// to wait for. `$nextTick` alone is a race: sometimes the show has
    /// landed, sometimes it has not, and when it loses, the command bar opens
    /// with the cursor still in the ledger, so what you type is read as
    /// motions. So: try after the update, and once more on the next frame if
    /// it did not take.
    focusSoon(el) {
      if (!el) return;
      this.$nextTick(() => {
        el.focus();
        // A timeout rather than requestAnimationFrame: rAF does not fire in a
        // background tab, and "the bar opened while you were looking
        // elsewhere" is exactly when the first attempt is most likely to have
        // lost the race.
        if (document.activeElement !== el) setTimeout(() => el.focus(), 0);
      });
    },


    // NOT `pane()`: that name is already the focused pane's INDEX, and a
    // method by the same name shadows it — `panes[this.pane]` then indexes an
    // array with a function and every row-based keystroke silently stops
    // working. Alpine merges data and methods into one object, so the two
    // namespaces are the same namespace.
    /// A surface about the HOUSEHOLD, not about one member or one row:
    /// `:household`, `:profile`, `:members`, `:grep`. Full width, at the foot
    /// of the ledger — a 420px column between two member panes was the wrong
    /// shape for a form and worse for a result list.
    showSplit(url) {
      this.closeSplit();
      this.closeLog();
      this.closeRule();
      this.trackSplit(url);
      // Returned, not discarded: `openColors()` has to wait for the swap
      // before it can put the keyboard on the first swatch.
      return htmx.ajax('GET', url, { target: '#hsplitmount', swap: 'innerHTML' });
    },

    /// Remember the open settings pane in the URL, so it survives the page
    /// refresh a setting forces.
    ///
    /// Several settings are rendered by the server — the gutter, the row
    /// order, every layout value — so writing one answers `HX-Refresh`, and a
    /// reload takes the pane with it: the split is a swap into an empty mount,
    /// and a fresh page has an empty mount. You changed one radio and the pane
    /// you changed it in vanished.
    ///
    /// The URL is where this belongs for the same reason `?pane=` is: it is
    /// per-window, it survives a reload, and it needs no server-side memory of
    /// "which pane was open" that two windows would immediately disagree
    /// about. Only the settings surfaces are remembered — `:grep` is a result
    /// list for a term you typed, and reopening it on every reload would be a
    /// nuisance rather than a courtesy.
    /// Where a settings pane was scrolled to, kept across the reload a setting
    /// forces — `?split=` brings the pane back, and this brings it back to the
    /// control you just changed rather than to its top. Per browser
    /// (`localStorage`), like the command history: a convenience, not a
    /// setting. Stamped with the time and used at most once, within a few
    /// seconds, so a change that did NOT reload (the palette, the fold
    /// default) never scrolls a pane opened later.
    keepSplitScroll() {
      const name = new URLSearchParams(location.search).get('split');
      const body = document.querySelector('#hsplitmount .rp-body');
      if (!name || !body) return;
      try {
        localStorage.setItem('ffm.splitScroll', JSON.stringify({ name, top: body.scrollTop, at: Date.now() }));
      } catch (_) {}
    },

    restoreSplitScroll(name) {
      let kept = null;
      try {
        kept = JSON.parse(localStorage.getItem('ffm.splitScroll') || 'null');
        localStorage.removeItem('ffm.splitScroll');
      } catch (_) { return; }
      if (!kept || kept.name !== name || Date.now() - kept.at > 15000) return;
      const body = document.querySelector('#hsplitmount .rp-body');
      if (body) body.scrollTop = kept.top;
    },

    trackSplit(url) {
      const name = { '/profile': 'profile', '/household': 'household', '/members': 'members' }[url];
      const u = new URL(location.href);
      if (name) u.searchParams.set('split', name);
      else u.searchParams.delete('split');
      history.replaceState(null, '', u);
    },

    /// The profile menu's color-scheme item. `:profile` with the cursor
    /// already on the palette — the swatches are the pane's first control, and
    /// `focusSplit` lands on its first visible input, so this needs no anchor
    /// and no second picker.
    openColors() {
      this.showSplit('/profile').then(() => this.focusSplit());
    },

    /// `Ctrl-u` / `:trash`. Opened with the keyboard already on the first item,
    /// because the only thing to do there is pick one — and the month on
    /// screen rides along, so a restore can swap the panes it lands in.
    /// `:charts` / `gc` — opened on the month on screen. The page sends only
    /// what it already holds: that month, and the ids picked with `x` in the
    /// focused pane, which the server follows across months. Everything
    /// drawn is the server's.
    openCharts() {
      const pane = document.querySelectorAll('.pane')[this.pane];
      const ids = pane
        ? Object.keys(this.selected).filter(id => pane.querySelector('#row-' + id)).sort()
        : [];
      const period = document.querySelector('.ledger-shell')?.dataset.period || '';
      let url = '/charts?period=' + encodeURIComponent(period);
      if (ids.length) url += '&selp=' + encodeURIComponent(period) + '&ids=' + ids.join(',');
      this.charts = true;
      this.chartsBuf = '';
      this.ajax('GET', url, { target: '#chstage', swap: 'innerHTML' }, 'status');
    },

    closeCharts() {
      this.charts = false;
      const stage = document.getElementById('chstage');
      if (stage) stage.innerHTML = '';
    },

    /// The viewer's keys press the stage's own links — each carries its key
    /// in `data-ck` — so a key and a click cannot disagree, and nothing here
    /// decides what is shown. Only scrolling is the page's own.
    chartsKey(k) {
      const body = document.querySelector('#chstage .chbody');
      const g = this.chartsBuf;
      this.chartsBuf = '';
      if (k === 'Escape') return this.closeCharts();
      // `gh` hides the amounts here as on the ledger: the viewer owns every
      // key, so without this the statusline's eye was the only way.
      if (g === 'g' && k === 'h') return this.toggleValues();
      if (!body) return;
      if (k === 'j' || k === 'ArrowDown') return body.scrollBy({ top: 80 });
      if (k === 'k' || k === 'ArrowUp') return body.scrollBy({ top: -80 });
      if (k === 'G') return body.scrollTo({ top: body.scrollHeight });
      if (k === 'g') {
        if (g === 'g') body.scrollTo({ top: 0 });
        else this.chartsBuf = 'g';
        return;
      }
      const el = [...document.querySelectorAll('#chstage [data-ck]')].find(a => a.dataset.ck === k);
      if (el) el.click();
    },

    openTrash() {
      const period = document.querySelector('.ledger-shell')?.dataset.period || '';
      this.showSplit('/trash?period=' + encodeURIComponent(period))
        .then(() => this.focusSplit());
    },

    /// `:order` / `gs` — the focused pane's manual order, in the month on
    /// screen, copied to other months. Opened with the keyboard in the months
    /// field, its only control. The server refuses it under a sort that is
    /// not manual and on somebody else's ledger, and the drawer then does not
    /// open; a grouped pane is fine — the folds come from the manual order.
    /// Not remembered in `?split=`: it is a gesture, not a setting.
    openOrder() {
      const member = this.paneMemberId();
      if (!member || this.readOnly()) return;
      const period = document.querySelector('.ledger-shell')?.dataset.period || '';
      this.showSplit('/order?member=' + member + '&period=' + encodeURIComponent(period))
        .then(() => this.focusSplit());
    },

    /// `:prune` — opened with the keyboard on the first copy that can go.
    /// Not remembered in `?split=`: like the trash, it is a list for a
    /// moment, and reopening it on a reload would re-hash the vault for
    /// nothing.
    openPrune() {
      this.showSplit('/prune').then(() => this.focusSplit());
    },

    /// `:grep` — opened with the keyboard already on a hit (the `at`th), since
    /// the only thing to do in a result list is pick one.
    openGrep(q, at) {
      return this.showSplit('/grep?q=' + encodeURIComponent(q)).then(() => {
        const hits = [...document.querySelectorAll('#hsplitmount a.hit')];
        const el = hits[Math.max(0, Math.min(hits.length - 1, at || 0))];
        if (!el) return this.focusSplit();
        el.focus();
        el.scrollIntoView({ block: 'nearest' });
      });
    },

    /// The last search, per TAB. `sessionStorage` rather than Alpine because
    /// picking a hit in another month is a page load, which is exactly when a
    /// bare `:grep` gets asked for; and per tab for the reason `?pane=` is in
    /// the URL — two windows are two searches. Storage can be refused (a
    /// private window), and then a bare `:grep` just says what it always said.
    lastGrep() {
      try {
        const v = JSON.parse(sessionStorage.getItem('ffm.grep') || 'null');
        return v && typeof v.q === 'string' && v.q ? v : null;
      } catch (_) { return null; }
    },

    keepGrep(q, at) {
      try { sessionStorage.setItem('ffm.grep', JSON.stringify({ q, at })); } catch (_) {}
    },

    /// Go to a `:grep` hit: its month, its member's pane, the row itself —
    /// and close the list. In the month on screen that is a cursor move with
    /// no round trip; anywhere else it is the hit's own link, whose `?row=`
    /// `init()` lands on after the load.
    gotoHit(el) {
      const hits = [...document.querySelectorAll('#hsplitmount a.hit')];
      const last = this.lastGrep();
      if (last) this.keepGrep(last.q, Math.max(0, hits.indexOf(el)));
      const period = document.querySelector('.ledger-shell')?.dataset.period;
      if (el.dataset.period !== period) {
        location.assign(el.href);
        return;
      }
      this.closeSplit();
      const i = [...document.querySelectorAll('.pane')]
        .findIndex(p => p.querySelector('.rows')?.id === 'rows-' + el.dataset.pane);
      if (i >= 0 && i !== this.pane) { this.pane = i; this.row = 0; }
      this.landOn(el.dataset.id);
    },

    /// Put the cursor on a row by id, and blink it once so the eye finds it.
    /// A row inside a closed fold is not a cursor position (`rows()` leaves it
    /// out), so its fold opens first — landing on the fold's header instead
    /// would answer a question nobody asked.
    landOn(id) {
      this.openFoldOf(id);
      this.cursorTo(String(id));
      this.flashRow(String(id));
    },

    /// Open the fold a row sits in, if it is closed: a row inside a closed
    /// fold is not a cursor position, so nothing can land on it otherwise.
    openFoldOf(id) {
      const section = document.getElementById('row-' + id)?.closest('.fold');
      if (section) {
        const f = this.foldIdOf(section);
        if (f && this.closed(f)) this.setFold(f, false);
      }
    },

    /// What the cursor is on, kept as the month changes (`gotoPeriod`), so the
    /// next month's page can put it back on the same row. Each month's row is
    /// a different transaction, so the identity kept is one that spans months:
    /// the rule — which survives a rename, Nubank to Nubank&Co — and the exact
    /// title, the spec's "same line" for a one-off; and the position, for when
    /// neither is there. In `sessionStorage`, not the URL: a title is
    /// household content, and per window is right for a cursor.
    keepFollow(period) {
      const el = this.rows()[this.row];
      const row = el && el.closest('div[id^="row-"]')?.querySelector('.row');
      const follow = {
        period,
        rule: row?.dataset.ruleId || null,
        title: row?.querySelector('.name > b')?.textContent || null,
        at: this.row,
      };
      try { sessionStorage.setItem('ffm.follow', JSON.stringify(follow)); } catch (_) {}
    },

    /// Read once on load, and dropped, so a refresh does not replay it. Only
    /// for the month it was kept for — any other load ignores it.
    takeFollow() {
      let saved = null;
      try {
        saved = JSON.parse(sessionStorage.getItem('ffm.follow') || 'null');
        sessionStorage.removeItem('ffm.follow');
      } catch (_) {}
      return saved && saved.period === this.shell().dataset.period ? saved : null;
    },

    /// The same row as last month's cursor: its rule, else its exact title
    /// (first hit), else the same position, clamped to the last row. No blink
    /// — this is ordinary navigation, not a search hit.
    follow({ rule, title, at }) {
      const wraps = Array.from(document.querySelectorAll('.pane')[this.pane]
        ?.querySelectorAll('div[id^="row-"]') || []);
      const hit = (rule && wraps.find(w => w.querySelector('.row')?.dataset.ruleId === rule))
        || (title && wraps.find(w => w.querySelector('.row .name > b')?.textContent === title));
      if (hit) {
        const id = hit.id.slice(4);
        this.openFoldOf(id);
        this.cursorTo(id);
        return;
      }
      this.row = Math.max(0, Math.min(at || 0, this.rows().length - 1));
      this.paint();
    },

    /// A surface about ONE ROW — the change log. It keeps the rule editor's
    /// shape: a column beside the member pane it belongs to, closed when the
    /// cursor leaves the row.
    showRowPane(url) {
      this.closeSplit();
      this.closeLog();
      this.closeRule();
      this.ajax('GET', url, { target: this.mount(), swap: 'innerHTML' }, this.focusedRow())
           .then(() => { this.aimArrow(); this.revealPaneTop(); this.revealPaneSide(); });
    },

    /// Bring a row-anchored pane's own top back into view.
    ///
    /// The stage scrolls vertically and these panes are columns INSIDE it, so
    /// a stage scrolled down to the focused row carries the pane's header and
    /// first entries off the top edge — a log opened on a row partway down
    /// opened showing its middle, with its header hidden under the app bar.
    ///
    /// Only ever scrolls UP, and only when the pane's top is actually above
    /// the stage's: a pane sitting lower has been put there by `aimArrow` to
    /// meet its row, which is deliberate and must not be undone.
    revealPaneTop() {
      const pane = document.querySelector('#rulepane, #logpane');
      const stage = document.querySelector('.stage');
      if (!pane || !stage) return;
      const above = pane.getBoundingClientRect().top - stage.getBoundingClientRect().top;
      if (above < 0) stage.scrollTop += above;
    },

    /// Bring a row-anchored pane into view sideways.
    ///
    /// The pane opens as a column right after the focused member's pane, so
    /// beside the LAST member it lands past the stage's right edge — the
    /// stage scrolls sideways rather than squeezing anybody — and opened
    /// there, it was simply off screen until scrolled to by hand.
    ///
    /// Scrolls only as far as the pane's right edge (plus the stage's own
    /// padding), and never so far that the pane's left edge — where its arrow
    /// points back at the row — goes past the stage's left.
    revealPaneSide() {
      const pane = document.querySelector('#rulepane, #logpane');
      const stage = document.querySelector('.stage');
      if (!pane || !stage) return;
      const p = pane.getBoundingClientRect();
      const s = stage.getBoundingClientRect();
      const pad = parseFloat(getComputedStyle(stage).paddingRight) || 0;
      const past = p.right + pad - (s.left + stage.clientWidth);
      if (past <= 0) return;
      const room = Math.max(0, p.left - pad - s.left);
      stage.scrollLeft += Math.min(past, room);
    },

    closeSplit() {
      this.splitZoom = false;
      this.clearMount('#hsplitmount > *');
      this.trackSplit(null);
    },

    /// `Ctrl-j` — into the split. Its first field if it has one, otherwise
    /// the pane itself, so the keyboard always lands somewhere.
    focusSplit() {
      const pane = document.querySelector('#hsplitmount > *');
      if (!pane) return;
      // A real, VISIBLE control. `input` alone picked the pane's first
      // hidden field, which cannot take focus, so Ctrl-j quietly landed on
      // nothing. The close button is deliberately last: it is first in the
      // markup, and arriving on "esc" is not arriving anywhere.
      const visible = (el) => el && el.offsetParent !== null;
      const pick = (sel) => [...pane.querySelectorAll(sel)].find(visible);
      const field = pick('input:not([type=hidden]), select, textarea')
        || pick('a, button:not(.rp-close):not(.rp-zoom)')
        || pick('button');
      if (field) field.focus();
      else {
        // Nothing focusable: give the pane itself the cursor so the keyboard
        // has somewhere to be, and Ctrl-k something to leave from.
        pane.setAttribute('tabindex', '-1');
        pane.focus();
      }
    },

    /// The log owns `logFor`, so closing it clears that too — otherwise
    /// `paint` keeps trying to close a pane that is already gone.
    closeLog() {
      this.logFor = null;
      this.logOpen = [];
      this.logCursor = 0;
      this.clearMount('#logpane');
    },

    // --- reading the log with the keyboard --------------------------------
    //
    // While the log is open it takes `j`/`k`, the fold keys and `Enter`.
    // Nothing is stolen by doing so: moving the LEDGER cursor closes the log,
    // so ledger navigation is already meaningless while it is up. That is
    // also why there is no key to focus the pane — there is nothing to focus
    // away from.

    /// Every binding whose first key armed a prefix. Returns whether the
    /// pair meant anything, so the caller knows to swallow the keystroke.
    prefixed(buffer, k, n, counted) {
      // `dd` — the only destructive pair, and the only one that is not a
      // `g`/`z` family member.
      if (buffer === 'd' && k === 'd') { this.askDelete(); return true; }

      if (buffer === 'g') {
        if (k === 'g') {
          if (this.logIsOpen()) { this.logMove(-this.logEntries().length); return true; }
          this.row = 0; this.paint(); return true;
        }
        // g1 / g2 / g3 — a genuine extension of the g-prefix "goto"
        // vocabulary already in use (gg, gt), with numbering fixed by KIND so
        // g1 means the bill on every row in the ledger.
        if (['1', '2', '3'].includes(k)) { this.openFile(Number(k)); return true; }
        // `gu` — "goto upload". A second step of the g-prefix rather than a
        // new pattern, reusing g1/g2/g3's numbering so one set of numbers
        // means one thing whether you are opening a file or adding one.
        if (k === 'u') { this.pending = 'gu'; return true; }
        // `gx` — select the whole pane, or clear it: the trash's `gx`.
        if (k === 'x') { this.toggleAllRows(); return true; }
        // `gl` — "go to log", the same extension of the g vocabulary.
        if (k === 'l') { this.openLog(); return true; }
        if (k === 'd') { this.showDetails(); return true; }
        // `gh` — "go hide": every amount pixelated on this device, or back.
        if (k === 'h') { this.toggleValues(); return true; }
        // `gp` — "go preview": the g-prefix spelling of `P`, beside gl / gd.
        if (k === 'p') { this.openCarousel(); return true; }
        // `gc` — "go charts": the `:charts` viewer, on the month on screen.
        if (k === 'c') { this.openCharts(); return true; }
        // `gs` — this month's manual order, copied to other months: the
        // `:order` drawer. Beside `s`, which picks the order to read by.
        if (k === 's') { this.openOrder(); return true; }
        // `ga` — "give to": move the row to another member, picked by name.
        if (k === 'a') { this.askGive(); return true; }
        // `gt` / `gT` cycle the member tabs, wrapping — and a count jumps
        // straight to that tab (`2gt`), which is why the count is read here
        // rather than treated as a repeat. H/L keep working unchanged, so
        // nothing is relearned when the layout flips underneath you.
        if (k === 't' || k === 'T') { this.gotoTab(k === 't' ? 1 : -1, n, counted); return true; }
        return false;
      }

      if (buffer === 'gu' && ['1', '2', '3'].includes(k)) {
        this.attachKey(Number(k));
        return true;
      }

      if (buffer === 'z') {
        // The fold keys mean the same thing in both places; which folds they
        // act on is whichever is in front of you.
        if (['a', 'o', 'c'].includes(k)) {
          if (this.logIsOpen()) this.logFold(k); else this.fold(k);
          return true;
        }
        if (k === 'R') { if (this.logIsOpen()) this.logAll(true); else this.allFolds(false); return true; }
        if (k === 'M') { if (this.logIsOpen()) this.logAll(false); else this.allFolds(true); return true; }
        // `zz` zooms the bottom split between half the ledger and nearly all
        // of it — worth it for `:grep` with forty hits. It borrows the `z`
        // prefix without muddying the fold family: `zz` is not a fold command
        // in Vim either.
        if (k === 'z') { this.splitZoom = !this.splitZoom; return true; }
        return false;
      }

      return false;
    },

    logIsOpen() { return !!document.querySelector('#logpane'); },

    logEntries() { return [...document.querySelectorAll('#logpane .logentry')]; },

    logMove(n) {
      const entries = this.logEntries();
      if (!entries.length) return;
      this.logCursor = Math.max(0, Math.min(this.logCursor + n, entries.length - 1));

      // At either end, scroll the body all the way rather than just far
      // enough. `nearest` stops the moment the entry is in view, which is
      // right in the middle of a list and wrong at its edges: the first
      // entry has its day heading above it, so `gg` left "TODAY" hidden, and
      // the last has the body's padding below it, so `G` stopped short of
      // the end. Both read as "it did not quite go there".
      const body = document.querySelector('#logpane .logbody');
      if (!body) return;
      if (this.logCursor === 0) body.scrollTop = 0;
      else if (this.logCursor === entries.length - 1) body.scrollTop = body.scrollHeight;
      else entries[this.logCursor].scrollIntoView({ block: 'nearest' });
    },

    /// The id of the entry under the cursor, or null when it has nothing to
    /// open — an inline single field, or a delete with no fields at all.
    logCursorId() {
      const el = this.logEntries()[this.logCursor];
      const x = el && el.querySelector('.lx');
      return x ? Number(el.dataset.entry) : null;
    },

    logToggle(id, index) {
      if (index !== undefined) this.logCursor = index;
      if (id === null || id === undefined) return;
      const at = this.logOpen.indexOf(id);
      if (at === -1) this.logOpen.push(id);
      else this.logOpen.splice(at, 1);
    },

    /// `za` / `zo` / `zc` on the entry under the cursor. The log's entries
    /// ARE folds — a header with collapsible content — so they answer to the
    /// vocabulary `:group bucket` already taught, rather than a key invented
    /// for this one pane.
    logFold(k) {
      const id = this.logCursorId();
      if (id === null) return;
      const open = this.logOpen.includes(id);
      if (k === 'a' || (k === 'o' && !open) || (k === 'c' && open)) this.logToggle(id);
    },

    /// `zR` / `zM` — every entry at once.
    logAll(open) {
      this.logOpen = open
        ? this.logEntries()
            .filter((el) => el.querySelector('.lx'))
            .map((el) => Number(el.dataset.entry))
        : [];
    },

    runCommand() {
      const raw = this.command.trim();
      this.commanding = false;
      this.command = '';
      // Inside the Enter that ran it, so a phone keeps its keyboard.
      if (this.keyboard) this.backToKeys();
      if (!raw) return;
      this.keepHist(this.cmdHistKey, raw);
      this.echoCommand(raw);
      const [name, ...rest] = raw.split(/\s+/);
      const arg = rest.join(' ');

      switch (name) {
        // The settings surfaces. `:config` survives as an alias so muscle
        // memory and the keymap entry do not break; `:household` is the real
        // name, because nothing is called after a file any more.
        case 'profile': case 'me':        return this.showSplit('/profile');
        case 'household': case 'config':  return this.showSplit('/household');
        case 'members':                   return this.showSplit('/members');
        case 'trash':                     return this.openTrash();
        case 'charts':                    return this.openCharts();
        // The bell's list — Vim's own word for the messages it has shown.
        case 'messages':
          if (arg === 'clear') return this.clearNotes();
          if (!arg) return this.openBell();
          break;
        // This month's manual order, to other months — `gs` too.
        case 'order':                     return this.openOrder();
        // Duplicate files, admin only — the server refuses anybody else.
        case 'prune':                     return this.openPrune();
        // `:help` opens the keymap — the same modal `?` does. It is not a
        // nicety: `not-a-command` literally tells people to "try help", so
        // without this the app answers "not a command: help (try help)".
        // With a term it opens searched, as Vim's `:help gt` does.
        case 'help': case 'h':
          return this.openKeymap(arg);
        // Every month, as opposed to `/`, which filters what is on screen.
        case 'grep':
          // A bare `:grep` is a command MISSING ITS ARGUMENT, not an unknown
          // one. Saying "not a command: grep" was the one thing that is not
          // true about it, and it is why `:grep` read as unimplemented.
          //
          // And bare, with a search already run in this tab, it is that
          // search again — re-run, so the hits are fresh, with the keyboard
          // on the hit picked last time.
          if (!arg) {
            const last = this.lastGrep();
            if (!last) return this.say(this.grepNeedsTerm, true);
            return this.openGrep(last.q, last.at);
          }
          this.keepGrep(arg, 0);
          return this.openGrep(arg, 0);
        case 'invite':
          this.showSplit('/members');
          return;
        case 'logout':                    return this.post('/logout');
        case 'preview':                   return this.post('/preview');

        // A command and a form control write the same row: `:theme nord` is
        // the swatch in :profile, reached with the keyboard instead.
        // A bare `:theme` is a command MISSING ITS ARGUMENT, not a broken one
        // — the same reading `:grep` already gets. It used to post an empty
        // value and answer `✗ theme is not a bundled palette`, which reads as
        // a rejection of the whole command. It opens the picker instead,
        // which is exactly what the menu item of the same name does.
        case 'theme':
          if (!arg) return this.openColors();
          return this.setting('/settings/member', 'theme_name', arg);
        case 'lang':     return this.setting('/settings/member', 'language', arg);
        case 'sort':     return this.setting('/settings/member', 'ledger_sort', arg);
        case 'group':    return this.setting('/settings/member', 'ledger_group', arg);
        case 'currency': return this.setting('/settings/household', 'locale_currency', arg);
        case 'set': {
          // `:set rnu` / `nu` / `nonu`, the Vim spellings.
          const nu = { rnu: 'relative', relativenumber: 'relative', nu: 'absolute',
                       number: 'absolute', nonu: 'none', nonumber: 'none' }[arg];
          if (nu) return this.setting('/settings/member', 'line_numbers', nu);
          break;
        }
      }
      this.say(this.notACommand.replace('{cmd}', name), true);
    },

    /// The command a name or alias means, if any.
    cmdNamed(name) {
      return this.cmds.find(c => c.name === name || c.aliases.includes(name)) || null;
    },

    /// Keys inside the `:` bar that are not Enter or Esc. Tab completes,
    /// Ctrl-k / Ctrl-j walk the history (`histDir`); anything else is
    /// typing, and ends both. A modifier alone is not a key — see the Shift
    /// gotcha in CLAUDE.md — or Shift-Tab would end the cycle it steps back
    /// through.
    cmdKey(e) {
      if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) return;
      if (e.key === 'Tab' && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        this.completeCmd(e.shiftKey ? -1 : 1);
        return;
      }
      const dir = this.histDir(e);
      if (dir) {
        e.preventDefault();
        this.wild = null;
        this.recall(dir, this.cmdHistKey, 'command');
        return;
      }
      this.wild = null;
      this.recallAt = null;
    },

    /// Keys inside the `/` bar that are not Enter or Esc: the same history
    /// walk as `:`, over past searches. No Tab — a search term is anybody's
    /// guess, like `:grep`'s. The highlights follow the recalled term, which
    /// assigning the model alone would not do: `@input` never fires.
    searchKey(e) {
      if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph'].includes(e.key)) return;
      const dir = this.histDir(e);
      if (dir) {
        e.preventDefault();
        this.recall(dir, this.searchHistKey, 'search');
        this.runSearch();
        return;
      }
      this.recallAt = null;
    },

    /// Which way a key walks a bar's history: 1 older, -1 newer, 0 neither.
    /// Ctrl-k / Ctrl-j first, because Ctrl-n cannot work everywhere: Chrome
    /// and Firefox on Linux and Windows keep Ctrl-N for a new window, and
    /// the page never even sees the key. Ctrl-p / Ctrl-n (Vim's) and the
    /// arrows still work wherever the browser lets them. preventDefault on
    /// all of them: Ctrl-p is Print, Ctrl-j Downloads and Ctrl-k the search
    /// box on Linux, and the arrows would move the caret to an end.
    histDir(e) {
      const ctrl = e.ctrlKey && !e.metaKey && !e.altKey;
      if ((ctrl && (e.key === 'k' || e.key === 'p')) || e.key === 'ArrowUp') return 1;
      if ((ctrl && (e.key === 'j' || e.key === 'n')) || e.key === 'ArrowDown') return -1;
      return 0;
    },

    /// Tab: the command's name, then its argument — Vim's wildmenu. One
    /// candidate is filled in; several are cycled through, Shift-Tab going
    /// back, with the candidates on show in the hint. A name that takes an
    /// argument gets its space, so the next Tab is already completing that.
    /// A value already complete cycles through the others from it.
    /// Only a CLOSED set is completed: `:grep`'s term is anybody's guess.
    completeCmd(dir) {
      const w = this.wild;
      if (w) {
        w.i = (w.i + dir + w.list.length) % w.list.length;
        this.command = w.head + w.list[w.i] + w.tail;
        return;
      }
      const text = this.command.replace(/^\s+/, '');
      const sp = text.indexOf(' ');
      let head, list, tail = '';
      if (sp < 0) {
        head = '';
        list = this.cmds.map(c => c.name).filter(n => n.startsWith(text));
        if (list.length === 1 && this.cmdNamed(list[0]).args.length) tail = ' ';
      } else {
        const c = this.cmdNamed(text.slice(0, sp));
        if (!c || !c.args.length) return;
        head = text.slice(0, sp) + ' ';
        const word = text.slice(sp + 1).trim().toLowerCase();
        // A value already complete — typed, or recalled with Ctrl-p — is not
        // a prefix of itself only: Tab steps on to the next value from it.
        const k = c.args.findIndex(a => a.toLowerCase() === word);
        if (k >= 0) {
          const n = c.args.length;
          if (n < 2) return;
          this.wild = { list: c.args, i: (k + dir + n) % n, head, tail: '' };
          this.command = head + this.wild.list[this.wild.i];
          return;
        }
        list = c.args.filter(a => a.toLowerCase().startsWith(word));
      }
      if (!list.length) return;
      if (list.length === 1) { this.command = head + list[0] + tail; return; }
      this.wild = { list, i: dir > 0 ? 0 : list.length - 1, head, tail: '' };
      this.command = head + this.wild.list[this.wild.i];
    },

    /// The wildmenu's candidates, scrolled so the one on show is never cut
    /// off by the hint's ellipsis — `:theme` has eleven.
    wildWindow() {
      const w = this.wild;
      if (!w) return [];
      const start = Math.max(0, w.i - 3);
      return w.list.slice(start).map((text, k) => ({
        n: start + k,
        text: (k === 0 && start > 0 ? '‹ ' : '') + text,
        on: start + k === w.i,
      }));
    },

    /// Ctrl-k / Ctrl-j: back and forward through a bar's history, newest
    /// first, into `this[field]` — `command` for `:`, `search` for `/`.
    /// Like Vim, whatever was typed before the walk began is a PREFIX —
    /// `:th` then Ctrl-k steps through past `:theme …` lines only — and
    /// walking forward past the newest puts back what was typed.
    recall(dir, key, field) {
      const hist = this.history(key);
      if (!this.recallAt) this.recallAt = { i: -1, prefix: this[field], draft: this[field] };
      const r = this.recallAt;
      let i = r.i;
      do { i += dir; } while (i >= 0 && i < hist.length && !hist[i].startsWith(r.prefix));
      if (i < 0) { r.i = -1; this[field] = r.draft; return; }
      if (i >= hist.length) return;
      r.i = i;
      this[field] = hist[i];
    },

    /// A bar's last 30 lines, newest first. Browser storage can be refused (a
    /// private window), and then there is simply no history.
    history(key) {
      try {
        const v = JSON.parse(localStorage.getItem(key) || '[]');
        return Array.isArray(v) ? v.filter(x => typeof x === 'string') : [];
      } catch (_) { return []; }
    },

    /// Remember a line as typed — a refused command too, like Vim, since the
    /// usual reason to recall one is to fix it. A repeat moves to the top
    /// rather than filling the history with copies.
    keepHist(key, raw) {
      try {
        const h = this.history(key).filter(x => x !== raw);
        h.unshift(raw);
        localStorage.setItem(key, JSON.stringify(h.slice(0, 30)));
      } catch (_) {}
    },

    /// The hint beside the bar. At rest, the vocabulary. With a command
    /// typed, what it does and the values it takes; with part of one, the
    /// commands that start that way.
    get cmdHelp() {
      const text = this.command.replace(/^\s+/, '');
      if (!text) return this.cmdHintDefault;
      const sp = text.indexOf(' ');
      const name = sp < 0 ? text : text.slice(0, sp);
      const c = this.cmdNamed(name);
      if (c) {
        return ':' + c.name + ' — ' + c.about + (c.args.length ? ' · ' + c.args.join(' | ') : '');
      }
      if (sp >= 0) return '';
      const some = this.cmds.map(c => c.name).filter(n => n.startsWith(name));
      return some.length ? some.join(' · ') : '';
    },

    post(url) {
      const f = document.createElement('form');
      f.method = 'post';
      f.action = url;
      document.body.appendChild(f);
      f.submit();
    },

    /// `gh` and the statusline eye. Hiding is visual state, so the page flips
    /// at once and asks nobody; the POST only sets the device's `ffm.hide`
    /// cookie, so the next render is already hidden. Quiet — no loader for a
    /// write nothing on screen waits for. The statusline's words are the
    /// server's, on the answer.
    toggleValues() {
      const on = !document.documentElement.hasAttribute('data-hide');
      document.documentElement.toggleAttribute('data-hide', on);
      this.ajax('POST', '/hide', { target: 'body', swap: 'none', values: { on: on ? '1' : '0' } }, 'none');
    },

    setting(url, key, value) {
      htmx.ajax('POST', url, { target: 'body', swap: 'none', values: { key, value } });
    },

    // --- files -----------------------------------------------------------

    attach(id, e) {
      if (this.readOnly(e.currentTarget)) return;
      // Anchored to the control that opened it, because the ledger scrolls
      // sideways and a centered menu would point at nothing.
      const r = e.currentTarget.getBoundingClientRect();
      this.attaching = id;
      this.menuStyle = 'top:' + (r.bottom + 6) + 'px; left:' + Math.max(8, r.left - 80) + 'px';
    },

    pick(kind) {
      this.kind_ = kind;
      // From the menu: its row becomes the picker's, and the menu closes
      // BEFORE the picker's own click could close it with the row in it.
      if (this.attaching) this.pickFor = this.attaching;
      this.attaching = null;
      this.$refs.picker.click();
    },

    /// A `+` slot on the rail (`density = default`) — straight to that slot's
    /// picker, as `gu1`/`gu2`/`gu3` are, never through the `+` menu.
    attachSlot(id, kind, e) {
      if (this.readOnly(e.currentTarget)) return;
      this.pickFor = id;
      this.pick(kind);
    },

    /// `gu1` / `gu2` / `gu3` — straight to that slot, no menu. The numbering
    /// is `g1`/`g2`/`g3`'s, unchanged: 1 states the amount, 2 is the promise,
    /// 3 is proof the money moved. One set of numbers for opening a file and
    /// for attaching one.
    attachKey(n) {
      const id = this.rowUnderCursor();
      if (!id || this.readOnly()) return;
      const row = document.querySelector('#row-' + id + ' .row');
      // Only a tracked recurring occurrence has a taxonomy to file under, and
      // the rail's `+` is exactly the marker for that.
      if (!row || !row.querySelector('.fadd')) {
        this.say(this.fileNotTracked, true);
        return;
      }
      if (this.onFoldHead()) { this.say(this.cursorOnFold, true); return; }
      const kind = { 1: 'barcode', 2: 'scheduled', 3: 'receipt' }[n];
      if (kind === 'scheduled' && !row.querySelector('.fdrop[data-kind="scheduled"]')) {
        // An income row has no slot 2 — you do not schedule money arriving.
        this.say(this.noFile.scheduled, true);
        return;
      }
      // Straight to the picker: `gu1` never opens the `+` menu, which is
      // what setting `attaching` here used to do, at a stale position.
      this.pickFor = id;
      this.pick(kind);
    },

    /// True when what is being dragged is a file rather than, say, selected
    /// text from another part of the page.
    hasFile(e) {
      const dt = e.dataTransfer;
      return !!dt && Array.from(dt.types || []).includes('Files');
    },

    /// A file dropped anywhere else on the page. The server finds the row of
    /// the rule whose binding recognizes the bill (`/bills/find`) — never by
    /// its name, and never a rule nothing has bound — and the file then goes
    /// the way a drop on that row's target `1` goes. A drop a row target or
    /// the rule pane's dropzone already took arrives here prevented, and is
    /// left alone.
    dropAnywhere(e) {
      this.dragging = false;
      if (e.defaultPrevented) return;
      // Never the browser's own drop, which opens the file in place of the
      // ledger — and with it every parked draft.
      e.preventDefault();
      const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
      if (!file) return;
      if (this.carousel || this.prep || this.keymap || this.confirming || this.giving
          || this.attachAsk || this.billPick || this.removingFile || this.undoAsk) return;
      const body = new FormData();
      body.append('file', file);
      body.append('period', document.querySelector('.ledger-shell')?.dataset.period || '');
      const job = this.fidgetStart();
      this.busyFetch('status', fetch('/bills/find', { method: 'POST', body, headers: { 'HX-Request': 'true', 'X-FFM-Job': job } })
        .then(r => r.text().then(html => ({ html, trigger: r.headers.get('HX-Trigger') })))
        .then(({ html, trigger }) => {
          const events = trigger ? JSON.parse(trigger) : {};
          if (events['ffm:say']) this.say(events['ffm:say'].message, events['ffm:say'].bad);
          if (!html.trim()) { this.fidgetEnd(false); return; }
          const list = document.getElementById('billpick');
          list.innerHTML = html;
          const rows = list.querySelectorAll('.give');
          if (rows.length === 1) { list.innerHTML = ''; this.fileFound(rows[0], file); return; }
          // A question takes over from the box, as the attach's own do.
          this.fidgetEnd(false);
          this.billPick = { file, sel: 0 };
        })
        .catch(() => { this.fidgetEnd(false); this.say(this.errorNotWritten, true); }));
    },

    /// Inside the dropped bill's picker: the `ga` picker's keys.
    billPickKey(k) {
      const p = this.billPick;
      const n = document.querySelectorAll('#billpick .give').length;
      if (k === 'Escape') { this.billPick = null; return; }
      if (k === 'Enter') { this.pickBill(p.sel); return; }
      if (/^[1-9]$/.test(k)) { if (Number(k) <= n) this.pickBill(Number(k) - 1); return; }
      const step = { j: 1, ArrowDown: 1, k: -1, ArrowUp: -1 }[k];
      if (step) p.sel = Math.max(0, Math.min(p.sel + step, n - 1));
    },

    pickBill(i) {
      const p = this.billPick;
      const btn = document.querySelectorAll('#billpick .give')[i];
      this.billPick = null;
      if (!p || !btn) return;
      this.fileFound(btn, p.file);
    },

    /// Sends a dropped bill to the row it was recognized for — the upload a
    /// drop on that row's target `1` makes, trim view and every check
    /// included. A row in another month is not on screen to show it, so the
    /// statusline says where it went.
    fileFound(btn, file) {
      const id = Number(btn.dataset.tx);
      const filed = btn.dataset.filed;
      // A PDF or an image stops at the trim view, whose Save starts its own
      // box; this one's steps are done. Anything else goes on under the same box.
      if (this.prepares(file)) this.fidgetEnd(true);
      return this.prepare(file, { tx: id }, (f, extra) => this.sendUpload(id, 'barcode', f, extra).then(r => {
        if (r === true && !document.getElementById('row-' + id)) this.say(filed, false);
        return r;
      }));
    },

    /// A drop on one of a row's three targets. The row came from where you
    /// let go and the kind from which target you hit, so nothing here has to
    /// be guessed from the file itself — which is the whole reason a panel
    /// drop was downgraded to a notifier and a row drop was not.
    drop(e, id, kind) {
      this.dragging = false;
      document.querySelectorAll('.fdrop.is-over')
        .forEach(el => el.classList.remove('is-over'));
      const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
      if (!file || this.readOnly(e.currentTarget)) return;
      this.upload(id, kind, file);
    },

    send() {
      const input = this.$refs.picker;
      const id = this.pickFor;
      this.pickFor = null;
      if (!input.files.length) return;
      // A file with nowhere to go is said, never swallowed — silence is what
      // this path did for every pick from the `+` menu.
      if (!id) { input.value = ''; this.say(this.errorNotWritten, true); return; }
      this.upload(id, this.kind_, input.files[0]);
      input.value = '';
    },

    /// The one upload path, whichever gesture asked for it.
    /// Every PDF or image on its way to a row stops at the trim-and-shrink
    /// view first; a text file goes straight in.
    upload(id, kind, file) {
      return this.prepare(file, { tx: id }, (f, extra) => this.sendUpload(id, kind, f, extra));
    },

    /// The server's "attach anyway?" — resolves to whether the person said
    /// `y`. One question at a time; a second one answers the first with no.
    askAttach(payload) {
      if (this.attachAsk) this.answerAttach(false);
      return new Promise(resolve => {
        this.attachAsk = { ...payload, mismatch: !!payload.mismatch, resolve };
      });
    },

    answerAttach(yes) {
      const ask = this.attachAsk;
      if (!ask) return;
      this.attachAsk = null;
      // A question whose "no" still attaches (a proposal declined) says
      // nothing here: the answer to the re-send does.
      if (!yes && !ask.no) this.say(this.askDeclined, false);
      ask.resolve(yes);
    },

    /// The fields a question asks to send again: `yes`'s on `y`, `no`'s on
    /// anything else, or `null` — the file is left out.
    answerFields(ask, yes) {
      return (yes ? ask.yes : ask.no) || null;
    },

    /// The upload itself. Resolves to whether the file was taken: a refusal
    /// answers no body, only the statusline. A question (`ffm:ask`) is put to
    /// the person outside the loader, and `y` sends the same file again with
    /// `attach_anyway`.
    /// A Save that may take a while starts a job: its id rides on every
    /// request the Save makes (`X-FFM-Job`), and the server names its steps
    /// against it. `crypto.getRandomValues`, not `randomUUID`: the household
    /// reaches this over plain http, where only the former exists.
    fidgetStart() {
      if (this.fidget && !this.fidget.leaving) return this.fidget.job;
      clearTimeout(this.fidgetTimer);
      const bytes = crypto.getRandomValues(new Uint8Array(16));
      const job = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('');
      this.fidget = { job, steps: [], leaving: false };
      return job;
    },

    /// The running job's id, if a Save is under way.
    fidgetJob() {
      return this.fidget && !this.fidget.leaving ? this.fidget.job : null;
    },

    /// A step started: the one before it is done.
    fidgetStep(job, text) {
      const f = this.fidget;
      if (!f || f.job !== job || f.leaving) return;
      f.steps.forEach(s => { s.done = true; });
      f.steps.push({ text, done: false });
    },

    /// The Save is over. A success ticks everything and fades the box out;
    /// a refusal or a question removes it at once — the statusline or the
    /// dialog says what happened.
    fidgetEnd(ok) {
      const f = this.fidget;
      if (!f) return;
      clearTimeout(this.fidgetTimer);
      if (!ok || !f.steps.length) { this.fidget = null; return; }
      f.steps.forEach(s => { s.done = true; });
      f.leaving = true;
      this.fidgetTimer = setTimeout(() => { if (this.fidget === f) this.fidget = null; }, 1200);
    },

    sendUpload(id, kind, file, extra = {}) {
      return this.postUpload(id, kind, file, extra).then(r => {
        if (!r || typeof r !== 'object') { this.fidgetEnd(r === true); return r; }
        // A question takes over from the box; a re-send starts a new one.
        this.fidgetEnd(false);
        return this.askAttach(r).then(yes => {
          // Every answer so far rides along: a proposal kept, then "attach
          // anyway?" — the second re-send still carries the first.
          const more = this.answerFields(r, yes);
          return more ? this.sendUpload(id, kind, file, { ...extra, ...more }) : false;
        });
      });
    },

    postUpload(id, kind, file, extra) {
      const body = new FormData();
      body.append('file', file);
      for (const [k, v] of Object.entries(extra)) body.append(k, v);
      // htmx.ajax cannot carry a FormData body, so this one request is a plain
      // fetch and the response is swapped by hand. It still speaks the same
      // contract: a row fragment back, and the statusline in HX-Trigger.
      const row = document.getElementById('row-' + id) || 'status';
      // The Save's job when the trim view started one; else this upload's own.
      const job = this.fidgetJob() || this.fidgetStart();
      return this.busyFetch(row, fetch('/tx/' + id + '/file/' + kind, { method: 'POST', body, headers: { 'HX-Request': 'true', 'X-FFM-Job': job } })
        .then(r => r.text().then(html => ({ html, trigger: r.headers.get('HX-Trigger') })))
        .then(({ html, trigger }) => {
          // Through htmx's own swap, not `outerHTML`: the answer is the row
          // PLUS the totals it moved, marked `hx-swap-oob`, and a bare
          // `outerHTML` pasted those into the pane under the row — the
          // header kept its stale figures and the new ones sat inline as
          // loose text. `htmx.swap` routes each out-of-band piece to its own
          // id and processes the new row, exactly as an htmx request would.
          if (html.trim()) {
            const target = document.querySelector('#row-' + id);
            if (target) htmx.swap(target, html, { swapStyle: 'outerHTML' });
          }
          const events = trigger ? JSON.parse(trigger) : {};
          if (events['ffm:say']) this.say(events['ffm:say'].message, events['ffm:say'].bad);
          this.paint();
          // A question is answered by the caller, outside the loader.
          if (!html.trim() && events['ffm:ask']) return events['ffm:ask'];
          return !!html.trim();
        }));
    },

    // --- trim and shrink (partials/prepare.html) -------------------------

    isPdf(file) {
      return !!file && (file.type === 'application/pdf' || /\.pdf$/i.test(file.name || ''));
    },

    /// Whether a file stops at the trim view: a PDF, or a PNG or JPEG — the
    /// images a slot takes. Which one it really is, the server decides.
    prepares(file) {
      return this.isPdf(file) || /\.(png|jpe?g)$/i.test((file && file.name) || '');
    },

    /// Stages a PDF or an image and opens the view; anything else is handed straight to
    /// `onSave`, which is the upload the file was headed for. `fields` say
    /// where it is going (`tx`, `rule`, `pdf_password`, `spans`) so the server
    /// asks the same permission and opens it with the same password.
    prepare(file, fields, onSave) {
      if (!this.prepares(file)) return onSave(file);
      const body = new FormData();
      body.append('file', file);
      for (const [k, v] of Object.entries(fields)) body.append(k, v == null ? '' : String(v));
      return this.prepOpen(fetch('/prepare', { method: 'POST', body, headers: { 'HX-Request': 'true' } }), onSave);
    },

    /// `t` in `P`: the same view on the file on show. Saving replaces it — the
    /// upload path supersedes the old one into the trash, and `u` brings it back.
    trimCurrent() {
      const c = this.carousel;
      if (!c || !c.current) return;
      if (!/\.(pdf|png|jpg)$/i.test(c.current.path || '')) { this.say(this.prepNotPdf, true); return; }
      this.prepStored(fetch('/tx/' + c.id + '/file/' + c.current.kind + '/prepare',
                            { method: 'POST', headers: { 'HX-Request': 'true' } }),
                      c.id, c.current.kind);
    },

    /// A stored file, reworked on the server, into the trim view: Save there
    /// sends it through the slot's own upload, which replaces the stored copy
    /// (the old one to the trash, `u` brings it back), and the carousel then
    /// shows it as it now is. `t`, `b` and `a` all end here.
    prepStored(request, id, kind) {
      this.prepOpen(request,
        (f, extra) => this.sendUpload(id, kind, f, extra).then(ok => {
          // Only that file's stage reloads, and it starts over at 100%: it
          // is a different file now. The row's others keep their place.
          const c = this.carousel;
          const f = c && c.id === id && c.files.find(x => x.kind === kind);
          const el = f && document.querySelector('.carousel .cview[data-kind="' + kind + '"] .cviewin');
          if (ok && el) {
            f.zoom = 1;
            el.dataset.src = '';
            this.showFile(el, f, true, true);
          }
          return ok;
        }));
    },

    /// The PDF on show, or a statusline refusal saying only a PDF can take
    /// more pages.
    pdfSlot() {
      const c = this.carousel;
      if (!c || !c.current) return null;
      if (!/\.pdf$/i.test(c.current.path || '')) { this.say(this.onlyPdf, true); return null; }
      return { id: c.id, kind: c.current.kind };
    },

    /// `b`: the box for a payment code.
    askBarcode() {
      const slot = this.pdfSlot();
      if (!slot) return;
      this.code = '';
      this.codeFor = slot;
      this.focusSoon(this.$refs.codeinput);
    },

    /// The box's Add: the server checks every digit, draws the page and opens
    /// the trim view on the longer file — or says why the code is wrong, and
    /// nothing is staged.
    addBarcode() {
      const f = this.codeFor;
      if (!f) return;
      this.codeFor = null;
      this.prepStored(fetch('/tx/' + f.id + '/file/' + f.kind + '/prepare/barcode', {
        method: 'POST',
        headers: { 'HX-Request': 'true', 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({ code: this.code }),
      }), f.id, f.kind);
    },

    /// `a`: a PDF to put after the one on show.
    pickAppend() {
      const slot = this.pdfSlot();
      if (!slot) return;
      this.appendFor = slot;
      this.$refs.appendpick.click();
    },

    appendPdf(e) {
      const file = e.target.files && e.target.files[0];
      e.target.value = '';
      const f = this.appendFor;
      this.appendFor = null;
      if (!file || !f) return;
      const body = new FormData();
      body.append('file', file);
      this.prepStored(fetch('/tx/' + f.id + '/file/' + f.kind + '/prepare/append', {
        method: 'POST', headers: { 'HX-Request': 'true' }, body,
      }), f.id, f.kind);
    },

    prepOpen(request, onSave) {
      return this.busyFetch('status', request
        .then(r => r.text().then(html => ({ html, trigger: r.headers.get('HX-Trigger') })))
        .then(({ html, trigger }) => {
          if (!html.trim()) {
            const payload = trigger && JSON.parse(trigger)['ffm:say'];
            if (payload) this.say(payload.message, payload.bad);
            return false;
          }
          const mount = document.getElementById('prepmount');
          mount.innerHTML = html;
          const root = mount.querySelector('[data-prep]');
          const n = parseInt(root.dataset.pages, 10) || 1;
          this.prep = {
            token: root.dataset.token,
            name: root.dataset.name,
            pages: n,
            keep: Array(n).fill(true),
            focus: 0,
            gs: !!root.dataset.gs,
            spans: !!root.dataset.spans,
            // An image's shrink is the server's own, with no `gs` to ask.
            canShrink: !!root.dataset.gs || !!root.dataset.image,
            shrink: !!root.dataset.gs || !!root.dataset.image,
            // The large page, as the carousel shows one: fitted to height
            // first, `f` for width, `+`/`-`/`0` on top of either.
            fit: 'height',
            zoom: 1,
            buf: '',
            onSave,
          };
          this.prepSize();
          return true;
        }));
    },

    prepQuery() {
      const p = this.prep;
      const keep = p.keep.map((k, i) => k ? i + 1 : 0).filter(Boolean).join(',');
      return 'keep=' + keep + '&shrink=' + (p.shrink ? 1 : 0);
    },

    /// The size line, from the server — debounced, so a run of `x` presses
    /// asks once.
    prepSize() {
      clearTimeout(this.prepTimer);
      this.prepTimer = setTimeout(() => {
        if (!this.prep) return;
        // A shrink can take Ghostscript several seconds, so the houses show
        // here, beside the figure they will replace.
        this.ajax('GET', '/prepare/' + this.prep.token + '/size?' + this.prepQuery(),
                  { target: '#prepsize', swap: 'innerHTML' }, document.querySelector('.pz-size'));
      }, 150);
    },

    /// The trim view's fit and zoom, keeping the page where it was — the same
    /// `keepPage` the carousel uses.
    prepFit() {
      const p = this.prep, stage = document.querySelector('.pz-stage');
      if (!p) return;
      this.keepPage(stage, () => {
        p.fit = p.fit === 'height' ? 'width' : 'height';
        p.zoom = 1;
        if (stage) { stage.dataset.fit = p.fit; stage.style.setProperty('--z', '1'); }
      }, false);
    },

    prepZoom(z) {
      const p = this.prep, stage = document.querySelector('.pz-stage');
      if (!p) return;
      this.keepPage(stage, () => {
        p.zoom = z;
        if (stage) stage.style.setProperty('--z', String(z));
      }, true);
    },

    prepFocus(i) {
      if (!this.prep) return;
      const at = Math.max(0, Math.min(this.prep.pages - 1, i));
      if (at !== this.prep.focus) {
        // Another page starts at its top, at 100% of the fit — as each file
        // does in the carousel.
        this.prep.zoom = 1;
        const stage = document.querySelector('.pz-stage');
        if (stage) stage.scrollTop = 0;
      }
      this.prep.focus = at;
      document.querySelectorAll('.pz-thumb')[this.prep.focus]?.scrollIntoView({ block: 'nearest' });
    },

    prepToggle(i) {
      const p = this.prep;
      if (!p || p.spans) return;
      const idx = i == null ? p.focus : i;
      // Never down to nothing: a file with no pages is not a file.
      if (p.keep[idx] && p.keep.filter(Boolean).length === 1) { this.say(this.prepKeepOne, true); return; }
      p.keep[idx] = !p.keep[idx];
      this.prepSize();
    },

    prepShrink(on) {
      if (!this.prep || !this.prep.canShrink) return;
      this.prep.shrink = on;
      this.prepSize();
    },

    prepKey(k) {
      const p = this.prep;
      // A pending `g`, for `gg` — any other key drops it.
      const armed = p.buf;
      p.buf = '';
      if (k === 'Escape') return this.prepCancel();
      if (k === 'Enter') return this.prepSave();
      // Pages sideways, the page itself up and down — `j`/`k` scroll, as they
      // do in the carousel.
      if (k === 'h' || k === 'ArrowLeft') return this.prepFocus(p.focus - 1);
      if (k === 'l' || k === 'ArrowRight') return this.prepFocus(p.focus + 1);
      // One page is on show here, so the next page IS the next page.
      if (k === 'J') return this.prepFocus(p.focus + 1);
      if (k === 'K') return this.prepFocus(p.focus - 1);
      // `gg` / `G`: the first / last page — one page is on show here, as
      // with `J` / `K`.
      if (k === 'g') {
        if (armed !== 'g') { p.buf = 'g'; return; }
        this.prepFocus(0);
        const stage = document.querySelector('.pz-stage');
        if (stage) stage.scrollTop = 0;
        return;
      }
      if (k === 'G') return this.prepFocus(p.pages - 1);
      const stage = document.querySelector('.pz-stage');
      if (k === 'j' || k === 'ArrowDown') return this.pageScroll(stage, 1);
      if (k === 'k' || k === 'ArrowUp') return this.pageScroll(stage, -1);
      if (k === 'H' || k === 'M' || k === 'L') return this.pageJump(stage, { H: 'top', M: 'middle', L: 'bottom' }[k]);
      if (k === 'f') return this.prepFit();
      if (k === '+' || k === '=') return this.prepZoom(Math.min(4, p.zoom + 0.25));
      if (k === '-') return this.prepZoom(Math.max(0.5, p.zoom - 0.25));
      if (k === '0') return this.prepZoom(1);
      if (k === 'x' || k === ' ') return this.prepToggle();
      if (k === 's') return this.prepShrink(!p.shrink);
    },

    /// Builds the file on the server, then sends it through the upload it was
    /// headed for. The view closes only once that upload took it: a refusal
    /// (a bill whose barcode page was removed, say) leaves it open to fix.
    ///
    /// The file sent is trimmed but **not shrunk**: everything that reads the
    /// bill — barcode, recipe, binding, amount, fingerprint — reads it as it
    /// was, and the upload shrinks it as the last step before the vault
    /// (`import::filed_bytes`), from the copy the size line already built.
    /// Ghostscript can re-encode a PDF's text, and a bill read from that is
    /// not the same bill.
    prepSave() {
      const p = this.prep;
      if (!p || p.saving) return;
      p.saving = true;
      const keep = p.keep.map((k, i) => k ? i + 1 : 0).filter(Boolean).join(',');
      const url = '/prepare/' + p.token + '/file?keep=' + keep + '&shrink=0';
      const extra = p.shrink ? { shrink: '1', stage: p.token, keep } : {};
      // In the Save button: the view covers the statusline, so the loader's
      // usual fallback would be out of sight.
      // One job for the whole Save — the file build, then the upload — so one
      // box names every step of it.
      const job = this.fidgetStart();
      this.busyFetch(document.querySelector('.pz-actions .btn.pri') || 'status', fetch(url, { headers: { 'X-FFM-Job': job } })
        .then(r => r.ok ? r.blob() : Promise.reject(r.status))
        .then(blob => p.onSave(new File([blob], p.name, { type: blob.type || 'application/pdf' }), extra))
        .then(ok => {
          p.saving = false;
          this.fidgetEnd(ok);
          if (ok) this.prepClose();
        })
        .catch(() => { p.saving = false; this.fidgetEnd(false); this.say(this.prepFailed, true); }));
    },

    prepCancel() {
      if (!this.prep) return;
      this.prepClose();
    },

    prepClose() {
      const token = this.prep && this.prep.token;
      this.prep = null;
      document.getElementById('prepmount').innerHTML = '';
      if (token) fetch('/prepare/' + token + '/cancel', { method: 'POST', headers: { 'HX-Request': 'true' } });
    },

    openLog() {
      const id = this.rowUnderCursor();
      if (!id) return;
      // AFTER the swap, not before: `showRowPane` closes whatever was open
      // first, and closing the log is what clears `logFor`. Setting it first
      // meant it was wiped a line later, and the log then never closed when
      // the cursor left its row.
      this.logOpen = [];
      this.logCursor = 0;
      this.showRowPane('/tx/' + id + '/log');
      this.logFor = id;
    },

    openFile(n) {
      const rows = this.rows();
      const el = rows[this.row];
      if (this.onFoldHead()) { this.say(this.cursorOnFold, true); return; }
      const kind = { 1: 'barcode', 2: 'scheduled', 3: 'receipt' }[n];
      const badge = el && el.querySelector('.fbadge[href$="/' + kind + '"]');
      // Pressing a number for a file that is not there is a no-op with a
      // message — the same family a missing file already belongs to, not a
      // different error path to learn.
      if (!badge) { this.say(this.noFile[kind], true); return; }
      window.open(badge.getAttribute('href'), '_blank', 'noopener');
    },

    askDelete() {
      const id = this.rowUnderCursor();
      if (!id || this.readOnly()) return;
      this.confirming = id;
    },

    doDelete() {
      const id = this.confirming;
      this.confirming = null;
      htmx.ajax('POST', '/tx/' + id + '/delete', { target: '#row-' + id, swap: 'outerHTML' });
    },

    /// `u` / `U`. Scoped to the pane and the month on screen, which the
    /// server needs told: the focused pane is the page's, not the session's.
    /// The first request only asks — the answer is `ffm:undo-ask`, which
    /// raises the popup.
    undoStep(dir) {
      const member = this.paneMemberId();
      if (!member || this.readOnly()) return;
      const period = document.querySelector('.ledger-shell')?.dataset.period || '';
      htmx.ajax('POST', '/' + dir, { target: 'body', swap: 'none', values: { member, period } });
    },

    /// The popup's answer. Yes sends the same request again, naming the
    /// change it was about; the server acts only if that is still the one,
    /// and asks again if the stack moved in between.
    answerUndo(yes) {
      const ask = this.undoAsk;
      this.undoAsk = null;
      if (!yes || !ask) return;
      htmx.ajax('POST', '/' + ask.dir, {
        target: 'body', swap: 'none',
        values: { member: ask.member, period: ask.period, confirm: ask.id },
      });
    },

    init() {
      // Before anything reads `closed()`: the folds this window left closed.
      this.loadFolds();
      this.loadNotes();
      // A pane the live channel left alone because a row in it was open in
      // `e` is re-rendered the moment that form closes — saved, canceled or
      // parked. Not a retry timer: a background tab throttles those to one
      // a minute, and the form closing is the event being waited for.
      this.$watch('editing', (v) => { if (!v) this.refreshPanes(); });
      // `?row=` is a `:grep` hit landing from another month. Read once and
      // dropped from the URL, so a refresh does not blink the row again;
      // `?pane=` has already put the right member in focus server-side.
      const land = new URLSearchParams(location.search).get('row');
      if (land) {
        const u = new URL(location.href);
        u.searchParams.delete('row');
        history.replaceState(null, '', u);
      }
      // A month change carries the cursor's row across (`keepFollow`); a
      // `:grep` hit's `?row=` is more specific, so it wins.
      const follow = this.takeFollow();
      this.$nextTick(() => {
        this.paint();
        if (/^\d+$/.test(land || '')) this.landOn(land);
        else if (follow) this.follow(follow);
      });
      // Reopen the settings pane a refresh just closed. `showSplit` rewrites
      // the same parameter it reads, so this is idempotent.
      const split = new URLSearchParams(location.search).get('split');
      if (['profile', 'household', 'members'].includes(split)) {
        this.$nextTick(() => this.showSplit('/' + split).then(() => this.restoreSplitScroll(split)));
      }
      // Any control changed in a settings pane may be about to reload the page
      // (`HX-Refresh`), so where the pane was scrolled to is kept first.
      // Capture, so it is read before the change's own request goes out.
      document.getElementById('hsplitmount')
        ?.addEventListener('change', () => this.keepSplitScroll(), true);
      // A phone's keyboard covers the bottom of the screen without shrinking
      // the page, so while it is up the shell follows what is visible.
      window.visualViewport?.addEventListener('resize', () => this.fitViewport());
      window.visualViewport?.addEventListener('scroll', () => this.fitViewport());
      this.$watch('keyboard', () => this.fitViewport());
      // The statusline is fed by a response header, so a swap never has to
      // include chrome in order to say something.
      document.body.addEventListener('ffm:say', e => this.say(e.detail.message, e.detail.bad));
      // A change that re-rendered a derived pane (grouped, or sorted by date or
      // amount) names the row it moved; the cursor follows it by id, the same
      // way it follows a row that `J`/`K` moved.
      document.body.addEventListener('ffm:cursor', e => {
        this.cursorTo(String(e.detail.id));
        if (e.detail.flash) this.flashRow(String(e.detail.id));
      });
      document.body.addEventListener('ffm:undo-ask', e => { this.undoAsk = e.detail; });
      // A drawer whose job is done — `:order` after it copied — closes the
      // way Esc closes it, zoom and `?split=` included.
      document.body.addEventListener('ffm:split-close', () => this.closeSplit());
      this.listenLive();
      // A mutation that can move a month in or out of "overdue" says so in
      // its `HX-Trigger` (`live::mark_dot` on the server); and a tab coming
      // back into view may have crossed midnight, which makes bills overdue
      // with nothing else happening. Both re-fetch only the picker's trigger,
      // so nothing being typed elsewhere on the page is touched.
      document.body.addEventListener('ffm:dot', () => this.refreshDot());
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') this.refreshDot();
      });
      // Every htmx request, declarative or `htmx.ajax`, shows its loader from
      // here. It ends on the XHR's own `loadend` rather than on
      // `htmx:afterRequest`: that event is dispatched on the element that
      // made the request, and a row swapped out by its own answer is no
      // longer in the document for it to bubble from. `loadend` fires after
      // the swap, on success, error, abort and timeout alike.
      document.body.addEventListener('htmx:beforeRequest', (e) => {
        // `htmx.ajax` without a `source` reports `body` as the element that
        // asked; what it is about is then its target.
        const d = e.detail;
        const anchor = this.busyAnchor(d.elt === document.body ? d.target : d.elt);
        const xhr = d.xhr;
        this.busyStart(anchor);
        xhr.addEventListener('loadend', () => this.busyEnd(anchor), { once: true });
        // A dropped connection answers nothing, and the loader vanishing with
        // nothing changed reads exactly like the key that was not pressed.
        xhr.addEventListener('error', () => this.say(this.busyOffline, true), { once: true });

        // A write from inside an open edit form — removing a file — changes
        // the row on the server, so the copy `restoreRow` would put back is no
        // longer what the row says.
        const inEdit = d.elt.closest?.('div[id^="row-"]');
        if (inEdit && (d.requestConfig?.verb || 'get') !== 'get') ffmRowHtml.delete(inEdit.id.slice(4));

        // Save: the edit form is locked while its post is in flight — typing
        // into a field whose value has already been sent is typing into
        // nothing, and a second Enter would post twice. `inert`, not
        // `disabled`: htmx gathered the values before this event, and a
        // disabled field is one it would leave out. Lifted when the answer
        // lands, with focus back where it was, in case the form is still
        // there (a dropped connection). A refused save answers with a fresh
        // form, which places its own cursor.
        if (d.elt.matches?.('form.editrow')) {
          const form = d.elt;
          const was = form.contains(document.activeElement) ? document.activeElement : null;
          form.inert = true;
          was?.blur();
          xhr.addEventListener('loadend', () => {
            form.inert = false;
            if (was && form.isConnected && !form.contains(document.activeElement)) was.focus();
          }, { once: true });
        }
      });
      document.body.addEventListener('htmx:afterSwap', (e) => this.afterSwap(e.target));
    },
  };
}
