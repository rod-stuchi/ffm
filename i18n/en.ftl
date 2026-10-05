# FFM — English. THE REFERENCE LOCALE.
#
# `en` fixes every message's parameter order, so the two files can never
# disagree about arity or ordering. build.rs enforces that, plus: no duplicate
# ids, no id in one locale and not the other, no differing placeable sets.
#
# The catalog holds WORDS, and only words. It must never contain a formatted
# amount, a date pattern or a currency glyph — those are three independent axes
# (language per member, money per household, dates fixed at dd/mm/yyyy) and
# fusing them is the mistake. Money arrives already rendered by ui::fmt::money,
# dates by ui::fmt::date, both interpolated as plain strings.
#
# This comment can say "R$ 1.234,56" freely: build.rs scans message VALUES, not
# comments. A value containing that would fail the build.

## Pre-auth — the only surface outside the .app shell

login-title = Sign in
login-name = Name
login-password = Password
login-submit = Sign in
# One message for both a wrong name and a wrong password, on purpose: saying
# which was wrong tells an attacker which half they got right.
login-error-mismatch = That name and password don't match.
login-signed-out = This device was signed out — from another device, or by an admin. Sign in again to carry on.

setup-title = Set up your household
setup-household-section = Your household
setup-you-section = You
setup-storage-section = Set by the operator
setup-household-name = Household name
setup-currency = Currency
setup-language = Default language
setup-password-confirm = Confirm password
setup-submit = Create household
setup-taken-title = This household is already set up
setup-taken-body = Someone has already completed setup. Sign in instead.

invite-title = Join { $household }
invite-name-taken = That name is already taken — choose another.
invite-dead-title = This link has expired
invite-dead-body = Ask an admin for a new one.

## Ledger chrome

ledger-ins = INs
ledger-outs = OUTs
ledger-balance = Balance
ledger-household-in = Household in
ledger-household-out = Out
ledger-household-net = Net

## Row states — the meta line's short form. The long sentence lives in `gd`.

row-overdue-since = overdue since { $date }
row-scheduled-for = scheduled · due { $date }
row-due = due { $date }
row-paid-on = paid { $date }
row-received-on = received { $date }
row-manual-paid = marked paid manually
row-deleted = deleted
row-unsaved = unsaved · e to resume
row-unsaved-rule = unsaved · E to resume
row-try-again = Try Again
# The profile menu on a phone, where the bar has no room for these.
menu-mode = Light or dark
menu-preview = See the app as a member without admin rights
# The row menu: a long press on a phone, a right-click with a mouse. The key
# each item runs is shown beside it, outside the message.
row-menu-edit = Edit
row-menu-edit-rule = Edit rule
row-menu-paid = Mark paid
row-menu-unpaid = Unmark paid
row-menu-received = Mark received
row-menu-unreceived = Unmark received

## File rail. Slot numbers never change; only the labels do, by direction.

file-bill = Bill
file-invoice = Invoice
file-scheduled = Scheduled
file-receipt = Receipt
file-payslip = Payslip
file-page = page { $page }
file-none-bill = no bill on file for this row
file-none-scheduled = no scheduled payment on file for this row
file-none-receipt = no receipt on file for this row
file-none-income-scheduled = no scheduled payment on an income row

## Statusline refusals. The interpolated values are literal config identifiers
## ("manual", "date"), not words — only the sentence around them is translated.

sort-refuses-reorder = ledger.sort = "{ $sort }" — reorder needs sort = "manual"
group-refuses-reorder = ledger.group = "{ $group }" — reorder needs group = "none"
group-order-needs-group = ledger.group = "none" — S orders groups; :group bucket first
not-a-command = not a command: { $name } (try help)
no-rule-behind-row = not generated from a recurring rule
# A non-admin reads the other ledgers and writes only their own.
read-only-ledger = read-only: this is { $who }'s ledger
reattribute-admin-only = only an admin can move a row to another member
# The tag on the header of a pane the viewer can read but not change.
pane-read-only = read-only

# The one message with a plural selector, which is the only thing that reaches
# FluentBundle at runtime. `$n` is typed i64 by convention.
rows-moved =
    { $n ->
        [one] moved 1 row { $dir }
       *[other] moved { $n } rows { $dir }
    }

## Pre-auth chrome — shared by /login, /setup and /invite

auth-brand-sub = household ledger
auth-where-login = sign in
auth-where-setup = first run
auth-where-invite = invite
auth-lang-switch = Language
auth-theme-system = System
auth-theme-light = Light
auth-theme-dark = Dark
auth-to-sign-in = Go to sign in

## /login

login-subtitle = Sign in to the household ledger.
# The second half of the one error message: the copy rule says tell people what
# to do about it, and a wrong name and a wrong password have the same answer.
login-error-body = Check the name, or ask an admin for a new invite link.

## /setup — first run only

setup-step = 1 / 1
setup-subtitle = Nobody has an account yet, so this first one is yours — and it is the admin.
setup-household-hint = Shown on the sign-in page.
# The date pattern is NOT written here: it is passed in from ui::fmt, which is
# the only thing that decides it. A catalog that spelled it out would be a
# second place for it to disagree.
setup-note-axes = Dates are always { $format }. Money follows the currency you pick. Language only changes the words.
setup-aside-signin = you will sign in with this
setup-theme = Color scheme
setup-theme-hint = Change it any time in :profile.
setup-line-numbers = Line numbers
setup-nu-relative = Relative
setup-nu-absolute = Absolute
setup-nu-none = Off
setup-storage-hint = Edit ffm.toml and restart to change these.
setup-done-title = Already set up
setup-done-body = This household already has members, so there is nothing to set up. Sign in instead, or ask an admin for an invite link.

## Currency names. The FORMAT beside each one is rendered by ui::fmt::money
## from the same table the ledger uses — never written out here.

currency-brl = Real
currency-usd = US dollar
currency-eur = Euro
currency-gbp = Pound

## /invite/:token

invite-subtitle = { $admin } invited you. Pick your own name and password — nobody else has seen this link.
invite-name-hint = This is also your folder in the receipt vault, so it is easiest to get right now.
invite-language = Language
invite-language-hint = Dates and money do not change with this.
invite-submit = Join the household
invite-expiry = This link works once and expires { $date }.
invite-used-title = This invite has already been used.
invite-used-body = If that was not you, ask an admin to revoke it.
invite-expired-title = This invite expired on { $date }.
invite-expired-body = Ask an admin for a new link.
invite-unknown-title = This link does not work.
invite-unknown-body = Check you copied all of it, or ask an admin for a new one.

## Password reset — the same link mechanism, pointed at a member who exists

reset-title = Set a new password
reset-subtitle = Your account is unchanged — only the password.
reset-submit = Set the password

## What a form refuses, and what to do about it

field-required = { $field } cannot be empty.
password-too-short = Use at least { $n } characters.
password-mismatch = The two passwords do not match.
password-strength-weak = Weak
password-strength-medium = Medium
password-strength-strong = Strong
password-match = The passwords match.
name-taken = Somebody already uses that name.
name-taken-body = Pick another — names are how everyone signs in, so they have to be unique.

## Session

sign-out = Sign out
signed-out = Signed out.

## Mutations — the one optimistic flow

row-title = Description
row-amount = Amount
row-add-spend = Add a spend
row-add-income = Add an income
row-save = Save
row-cancel = Cancel
row-added = added "{ $title }"
row-deleted-say = deleted · u to undo
row-delete-confirm = Delete this row?
row-delete-body = It stays in the database and `u` puts it back. Press y to confirm.
row-delete-yes = Delete
nothing-to-undo = nothing of yours to undo in this ledger this month
nothing-to-redo = nothing to redo
undone = undone
undo-did = undone: { $verb } "{ $title }"
redo-did = redone: { $verb } "{ $title }"
undo-redo-hint = U redoes
redo-undo-hint = u undoes
undo-more = { $n ->
        [one] + 1 more field
       *[other] + { $n } more fields
    }
undo-confirm = u again to undo: { $verb } "{ $title }", from { $date }
redo-confirm = U again to redo: { $verb } "{ $title }", from { $date }
undo-skipped = { $who } changed "{ $title }" since — left as they made it
error-not-written = That didn't go through — nothing was written.

## The rule pane — A / I create, E edits

rule-new-spend = New recurring bill
rule-new-income = New recurring income
rule-edit = Recurring rule
rule-bill-name = Bill name
rule-source = Source
rule-bucket = Bucket
rule-cadence = Cadence
rule-start = Start date
rule-due-day = Due date
rule-expected-day = Expected day
rule-end = End date
rule-installments = Installments
rule-payments = Payments
rule-tracked = Track receipt on disk
rule-no-barcode = This bill has no barcode file
rule-carry-forward = Carry the amount forward
rule-varies = Amount varies
rule-estimate-basis = Estimate from
rule-estimate-lowest = Lowest
rule-estimate-average = Average
rule-estimate-last = Last
rule-estimate-window = of the last { $n } confirmed
rule-instance = Instance
rule-pdf-password = PDF password
rule-pdf-password-hint = for statements that open only with a password — files are kept unlocked
rule-pdf-password-show = Show password
rule-pdf-password-hide = Hide password
rule-private = Hidden from other members
rule-amount-in-filename = Amount in filename
rule-create = Create
rule-save = Save
rule-delete = Delete rule
rule-path-legend = bucket / bill name / instance / year / month.ext
rule-past-note = Earlier months, and this month once it is paid, keep what they recorded. This month's row follows a new due day while it is unpaid.
rule-created = rule created · { $n } period(s) materialized
rule-saved = rule saved
rule-moved-due = this month's row now due { $date }
rule-deleted = rule deleted · its rows stay
rule-none-on-row = not generated from a recurring rule
rule-deleted-on-row = the rule behind this row was deleted · its files and status still hold
cadence-monthly = Monthly
cadence-bimonthly = Every 2 months
cadence-quarterly = Every 3 months
cadence-four = Every 4 months
cadence-half = Every 6 months
cadence-yearly = Yearly

## Files

file-add-bill = Add bill
file-add-scheduled = Add scheduled
file-add-receipt = Add receipt
file-added = file added
file-removed = file moved to .delete · u to put it back
file-open = Open in new tab
file-delete = Delete file
file-not-tracked = this row has nothing to file under
file-too-big = that file is too large to keep
file-pdf-locked = this PDF opens only with a password — fill in the rule's PDF password, then attach it again — nothing was saved
file-pdf-wrong-password = the rule's PDF password does not open this file — check it in the rule — nothing was saved
file-pdf-no-tool = this PDF opens only with a password, and qpdf is not installed on the server to open it — nothing was saved
file-pdf-unreadable = could not open this PDF — it may be damaged — nothing was saved
file-preview = Files
file-no-files = no files on this row
file-page-of = One file, { $total } pages — this month is page { $page }
file-page-outside = page { $page } — outside this file's { $total } pages, a new upload is needed for this month
rule-spans-period = Barcode covers multiple months
rule-window-size = Pages in this file
rule-window-start = First page covers
estimate-no-history = no confirmed months yet — the rule's own amount is used

## Settings panes — :profile, :household, :members

settings-profile = Your settings
settings-household = Household
settings-members = Members
settings-saved = { $name } saved
settings-admin-only = that is admin only
pane-close = esc
pane-maximize = Maximize · zz
pane-restore = Restore · zz

profile-appearance = Appearance
profile-ledger = Ledger
profile-layout = Layout
profile-privacy = New rows
profile-theme = Color scheme
profile-line-numbers = Line numbers
status-version = The version this server is running
profile-panes = Panes
profile-min-split = Split below this width, use tabs
profile-min-width = Minimum pane width
profile-max-width = Maximum pane width
profile-gap = Gap between panes
profile-sort = Row order
profile-sort-manual = Manual
profile-sort-date = Date
profile-sort-amount = Amount
profile-search-scope = Search scope
profile-scope-pane = This pane
profile-scope-household = The household
profile-show-delta = Show month-over-month change
profile-density = Density
density-default = Default
density-comfortable = Comfortable
density-compact = Compact
profile-fold-default = Groups start
profile-fold-expanded = Open
profile-fold-collapsed = Closed
profile-group-order = Groups ordered
profile-group-order-appearance = First seen
profile-group-order-name = A–Z
profile-group-order-subtotal = By cost
profile-visibility-inherit = Follow the household ({ $resolved })
profile-visibility-household = Shared with the household
profile-visibility-private = Private to me
profile-devices = Signed in on
profile-no-devices = no other devices

household-name = Household name
household-currency = Currency
household-language = Default language
household-visibility = Default for new rows
household-show-recurring = Use recurring rules
household-storage = Set by the operator
household-new-members = For new members
household-new-members-note = Copied into each new member's own settings when they join. Nobody's current settings change.
profile-use-household = Use the household's defaults
profile-use-household-note = The palette, layout and density the household gives new members.
profile-used-household = now using the household's palette, layout and density
household-storage-note = Edit ffm.toml and restart to change these.

members-role-admin = Admin
members-role-member = Member
members-invite-label = For whom (optional)
members-invite-create = Create invite link
members-invite-note = You never type anyone else's password. They set it on the link, along with their name.
members-copy = Copy
members-pending = Pending links
members-expires = expires { $date }
members-revoke = Revoke
members-reset = Reset password
members-signout = Sign out everywhere
members-reset-note = Issuing a reset signs { $name } out everywhere and is recorded in the change log.
members-signed-out = signed { $name } out of { $n } device(s)
members-signout-others = Sign out other devices
members-signed-out-others = signed out of { $n } other device(s) — this one stays
members-no-sessions = not signed in anywhere
members-tab-active = Active
members-tab-inactive = Inactive
members-tab-trash = Trash
members-disable = Turn off
members-enable = Turn back on
members-to-trash = Move to trash
members-restore = Restore
members-disable-note = Turning { $name } off signs them out everywhere, and hides their pane — and its money — from this month on. Earlier months still show them.
members-inactive-since = inactive since { $month }
members-inactive-none = Nobody is turned off.
members-trash-none = The trash is empty.
members-trash-note = Only somebody with no rows can go to the trash — it is for a mistake. Their name stays taken while they are here.
members-link-days = Links stop working { $n } days after they are created.
members-copied = copied
members-disabled = { $name } is turned off
members-enabled = { $name } is back on
members-trashed = { $name } is in the trash
members-restored = { $name } is back
members-not-yourself = That is you — not something to do to your own account.
members-last-admin = The household would be left without an admin.
members-unknown = That member is not there any more — the pane was out of date.
members-has-rows = They have rows in the ledger, so they cannot go to the trash. Turn them off instead.
login-error-inactive = This account is turned off. Ask an admin to turn it back on.

## The command bar

command-placeholder = command · Tab completes · Ctrl-p recalls
command-about-help = the keymap — the same as ?
command-about-profile = your own settings
command-about-sort = the order of the rows
command-about-group = group the rows by bucket, or flatten them
command-about-set = line numbers: relative · absolute · off
command-about-theme = your color palette — bare, it opens the picker
command-about-lang = your language — words only, never dates or money
command-about-grep = search every month — bare, the last search again
command-about-trash = deleted rows and removed files, to bring back
command-about-prune = duplicate files — remove the copies in the trash
command-about-household = the household's settings
command-about-members = members and invite links
command-about-invite = a new invite link
command-about-currency = the household's currency and number format
command-about-preview = see the app as a member without admin rights, and back
command-about-logout = end this session
members-self-note = This is you. Change a password you know in :profile; recover one you have lost with `ffm reset-link` at the console.

## The change log — `gl`. Verbs are what HAPPENED, in the past tense.

log-title = History
log-empty = Nothing has been recorded for this row yet.
log-today = today
log-yesterday = yesterday
log-filter-row = this row
log-filter-rule = rule
log-filter-all = all
log-rule-tag = rule
log-undo-of = undo of { $time }
log-fields = { $n ->
        [one] 1 field
       *[other] { $n } fields
    }
log-system = system
log-materializer = materializer
log-import = import

log-action-create = added
log-action-edit = edited
log-action-delete = deleted
log-action-restore = restored
log-action-reattribute = re-attributed
log-action-reorder = moved
log-action-privacy = changed privacy
log-action-rule-create = rule created
log-action-rule-edit = rule edited
log-action-file-attach = file attached
log-action-file-unlink = file unlinked
log-action-file-upload = file added
log-action-file-delete = file removed
log-action-import = read from a bill
log-action-materialize = materialized
log-action-password-reset = password reset
log-action-sessions-revoked = signed out everywhere

log-field-amount = amount
log-field-paid = paid by hand
log-field-title = description
log-field-date = date
log-field-due-day = due day
log-field-due-date = due date
log-field-bucket = bucket
log-field-bill-name = bill name
log-field-instance = instance
log-field-pdf-password = PDF password
log-field-cadence = cadence
log-field-start-date = start date
log-field-end-date = end date
log-field-tracked = track receipt
log-field-path = path
log-field-kind = kind
log-field-original-filename = original filename
log-on = on
log-off = off
moved-down = down
moved-up = up
group-uncategorized = Uncategorized
group-rows = { $n ->
        [one] 1 row
       *[other] { $n } rows
    }
grep-title = Found: { $term }
grep-none = nothing matches that, in any month
grep-count = { $n ->
        [one] 1 row
       *[other] { $n } rows
    }

## The trash — Ctrl-u / :trash

trash-title = Trash
trash-empty = Nothing has been deleted. Rows you delete with dd and files you remove land here.
trash-count = { $n ->
        [one] 1 item
       *[other] { $n } items
    }
trash-more = only the newest are listed
trash-deleted = deleted { $time }
trash-deleted-by = deleted { $time } · { $who }
trash-replaced = replaced { $time }
trash-with-files = { $n ->
        [one] + 1 file
       *[other] + { $n } files
    }
trash-restore = Restore
trash-hint = x or click to check · X the day · gx all · Enter restores what is checked
trash-note = A row comes back where it was, with its files. An older file swaps places with the current one, which goes to the trash in its stead.
trash-menu = Deleted rows and removed files, to bring back
trash-nothing-picked = nothing checked — x or click an item first
trash-slot-twice = Two versions of the same file are checked. Pick one — only one can be on the row.
trash-rows = { $n ->
        [one] 1 row
       *[other] { $n } rows
    }
trash-files = { $n ->
        [one] 1 file
       *[other] { $n } files
    }
trash-restored = restored { $what }
trash-elsewhere = { $n ->
        [one] 1 in another month
       *[other] { $n } in other months
    }
trash-failed = { $n ->
        [one] 1 could not be restored
       *[other] { $n } could not be restored
    }
trash-bill-elsewhere = "{ $title }" already holds the same bill, and a bill can be on one row only

## Bill import

import-dropzone = Drop the bill here, or choose a file
import-client-code = Client code
import-unlink = Unlink
import-read = read { $what } from the bill
import-filled = read { $amount } due { $date } from the barcode
import-read-amount = read { $amount } from the barcode
import-amount-differs = the barcode says { $barcode }, this row says { $row } — left as it was
import-date-differs = the barcode says { $barcode }, this row says { $row } — left as it was
import-replaced = replaced { $was }
import-unresolved = the barcode did not carry { $what }
import-no-occurrence = there is no row for this month to attach it to
import-unlinked = bill unlinked from this rule — drop the right one
import-already-attached = this bill is already on { $period } ({ $title } — { $amount }) — nothing was saved
import-already-attached-hidden = this bill is already on { $period }, on a row you cannot see — nothing was saved
import-bill-moved = moved from { $title } { $period }
rule-path-missing-bucket = ‹bucket›
import-wrong-row = wrong row? this bill is for { $found }, but { $rule } is bound to { $known } — nothing was saved
import-id-account = account { $code }
import-id-company = company { $code }
import-id-bank = a bank slip from bank { $code }
import-unreadable-bound = could not read a barcode in this file, so there is no way to confirm it is the bill for { $rule } — nothing was saved
import-meta-title = Bill on file
import-meta-hint = what this rule is bound to — a bill for anything else is refused
import-meta-issuer = Issuer
import-meta-kind = Barcode
import-meta-company = Company code
import-meta-kind-consumo = utility / tax
import-meta-kind-cobranca = bank slip
import-meta-pending = attached when you create the rule
import-no-occurrence-new-rule = the rule starts after this month, so the bill was not attached — attach it from E once its first row exists

## The month picker and its warning dot

months-short = Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec
month-picker = Month
unresolved-dot = { $n ->
        [one] 1 bill overdue with no receipt
       *[other] { $n } bills overdue with no receipt
    }
unresolved-here = overdue here
month-picker-select = select month
month-future = nothing recorded yet
year-so-far = { $year } so far · { $range }
year-full = { $year } · full year
year-avg = avg { $amount } / month
year-unresolved = { $n ->
        [one] 1 month needs a receipt
       *[other] { $n } months need a receipt
    }

## `e` — editing an occurrence in place

edit-day = Day
edit-private = Hidden from other members
edit-paid = Paid — marked by hand
edit-received = Received — marked by hand
edit-saved = saved
row-reattributed = moved to { $name }
row-now-private = hidden from the others
paid-marked = marked paid by hand · u to undo
paid-unmarked = no longer marked paid
received-marked = marked received by hand · u to undo
received-unmarked = no longer marked received
paid-refused-tracked = a tracked bill is paid by its receipt on disk, not by hand — gu3 attaches it
paid-refused-one-off = only a recurring bill is marked paid by hand
row-now-shared = shared with the household
reattribute-private-title = Move a hidden row to { $name }?
reattribute-private-body = They become its owner, so you stop seeing it. Press y to confirm.
no-pane-that-way = no pane that way
drafts-parked = { $n ->
        [one] 1 unsaved draft
       *[other] { $n } unsaved drafts
    }

month-leave-key = changing month loses { $drafts } — { $key } again to go anyway
month-leave-click = changing month loses { $drafts } — click again to go anyway
month-already-current = already on the current month

## `gd` — the details popup, and the `P` carousel

details-due = Due
details-cadence = Cadence
details-receipt = Receipt
details-status = Status
details-rule = Rule
details-rule-deleted = deleted on { $date }
details-none = nothing recurring behind this row
status-paid = receipt on file
status-scheduled = scheduled, due date not passed yet
status-scheduled-overdue = scheduled, due date passed, no receipt yet
status-pending = nothing scheduled and nothing paid yet
status-plain = untracked, so there is no filesystem proof either way
status-manual-paid = ticked by hand — untracked, so there is nothing to prove it
status-expected = not arrived yet
status-received = arrived

carousel-open-tab = Open in new tab
carousel-zoom-reset = Reset zoom
carousel-page-alt = page { $n }
keymap-title = Keymap
keymap-close = Close
details-months = month(s)
keymap-move = Move
keymap-act = Act
keymap-find = Find
keymap-fold = Fold
keymap-files = Files
keymap-undo = Undo
keymap-type = Type

## Seasonal months — which months of the year a rule actually runs.
## The expression itself is digits in every language; only these words move.
months-seasonal = Only some months
months-seasonal-hint = a bill that runs part of the year, every year
months-label = Months
months-placeholder = 1-10
months-help = 1-10 a range · 1,3,5 a list · ^11,^12 all except
months-every = every month
months-count-skipped = { $n ->
    [one] one month a year
   *[other] { $n } months a year
  } · { $skipped } skipped
months-err-empty = that is not a month yet
months-err-not-a-number = not a month number: { $token }
months-err-out-of-range = month { $n } — months are 1 to 12
months-err-backwards = { $range } runs backwards
months-err-nothing-left = that leaves no months at all
month-short = { $n ->
    [1] Jan
    [2] Feb
    [3] Mar
    [4] Apr
    [5] May
    [6] Jun
    [7] Jul
    [8] Aug
    [9] Sep
   [10] Oct
   [11] Nov
   *[12] Dec
  }

## A save refused because the row moved underneath the form. Nothing was
## written, and what was typed is still parked on the row.
edit-stale = { $who } changed this row while you were editing. Nothing was written — your edit is still here, press e to pick it up.
edit-stale-unknown = This row changed while you were editing. Nothing was written — your edit is still here, press e to pick it up.
edit-gone = That row was deleted while you were editing. Nothing was written — press u to bring it back.
edit-day-bad = A day of the month is 1 to 31.
grep-needs-a-term = grep needs something to search for
cursor-on-fold = that is a group, not a row

## `x` - the selection cell in the statusline
selection-count = { $n } of { $total } selected
selection-no-amount = { $n } without amount

# --- ? legend: what each mark means ------------------------------------------
legend-title = Marks
legend-paid = Paid — the receipt is on file. On an income: received.
legend-manual-paid = Marked paid by hand — a bill with no file to prove it
legend-pending = Waiting — the ring fills from the start of the month to the due date, changes color as it nears, and turns red once it is overdue. On an income: expected.
legend-scheduled = Scheduled — proof of a scheduled payment is on file, not due yet
legend-scheduled-overdue = Scheduled but overdue — the due date passed and no receipt came in
legend-plain = Not tracked — no file is expected, so there is nothing to prove
legend-estimated = Estimated — an income that varies, projected from recent months until it arrives
legend-recurring = Recurring — made by a rule each month. Dim: a one-off, or its rule was deleted.
legend-private = Private — hidden from the other members, never from the admin. Dim: the household sees it.
legend-files = Files on the row — 1 bill, 2 scheduled payment, 3 receipt. g1 g2 g3 open one, P previews them.

# --- e: the row's files, inside the edit form ---------------------------------
edit-files = Files
edit-file-open = Open in a new tab
edit-file-remove = Remove

# --- P: removing the file on show ----------------------------------------------
carousel-remove = Remove
carousel-remove-title = Remove this { $file }?
carousel-remove-body = It moves to your trash folder in the vault, never off the disk. Press u to put it back.
carousel-remove-yes = Remove

# --- the loader: a request in flight -------------------------------------------
busy-label = Working…
busy-offline = No answer from the server — nothing changed. Try again.

## Which kind of server this is — the badge a dev build wears (`crate::run`)

run-dev-title = A development build — not the household's server. What you change here is not the real ledger's.
run-dev-lan-title = A development build, open to the local network — not the household's server.
run-demo-title = A development build running as of { $date }, on a copy of the database — nothing here reaches the real ledger.

## Live updates — a row changed while this page was open

live-from-disk = updated from disk — a file was filed or removed by hand
live-under-edit = this row's files changed on disk — Esc to see them

## Duplicate files — :prune

prune-title = Duplicates
prune-count = { $n ->
        [one] 1 group
       *[other] { $n } groups
    } · { $size } can go
prune-group = { $hash } · { $n } copies · { $size }
prune-empty = No two files in the vault are the same. Nothing to prune.
prune-hint = Only copies in a trash folder can be checked — what is live stays · x or click to check · X the group · gx all · Enter removes
prune-note = Each copy here has the same bytes as one that stays, so removing it loses nothing. It is removed from the disk for good: not to the trash, and u cannot bring it back.
prune-live = live · { $row }
prune-live-unknown = live · on disk, not on record
prune-trash = trash · { $row } · deleted { $day }
prune-trash-unknown = trash · not on record
prune-remove = Remove
prune-confirm = Remove the checked copies for good?
prune-confirm-body = Every one has the same bytes as a file that stays. They do not go to the trash — they leave the disk, and u cannot bring them back.
prune-confirm-yes = Remove for good
prune-nothing-picked = nothing checked — x or click a copy first
prune-done = removed { $n ->
        [one] 1 copy
       *[other] { $n } copies
    } · { $size } freed
prune-not-in-trash = { $path } is not in a trash folder — only a copy there can be removed
prune-not-a-duplicate = { $path } is no longer a copy of anything — the list was out of date, look again
prune-last-copy = every copy of { $path } is checked — keep one
prune-failed = could not remove every copy — the list shows what is left

## The totals' words — lowercase; the app bar and the picker uppercase them

totals-in = in
totals-out = out
totals-net = net
totals-balance = bal
