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
row-try-again = Try Again

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
not-a-command = not a command: { $name } (try help)
no-rule-behind-row = not generated from a recurring rule

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
setup-theme = Colour scheme
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
nothing-to-undo = nothing to undo
undone = undone
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
rule-private = Hidden from other members
rule-amount-in-filename = Amount in filename
rule-create = Create
rule-save = Save
rule-delete = Delete rule
rule-path-legend = bucket / bill name / instance / year / month.ext
rule-past-note = Past occurrences are left as recorded. This changes future periods only.
rule-created = rule created · { $n } period(s) materialized
rule-saved = rule saved
rule-deleted = rule deleted · its rows stay
rule-none-on-row = not generated from a recurring rule
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

profile-appearance = Appearance
profile-ledger = Ledger
profile-layout = Layout
profile-privacy = New rows
profile-theme = Colour scheme
profile-line-numbers = Line numbers
profile-panes = Panes
profile-min-split = Split below this width, use tabs
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
profile-fold-default = Groups start
profile-fold-expanded = Open
profile-fold-collapsed = Closed
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
household-storage-note = Edit ffm.toml and restart to change these.

members-role-admin = Admin
members-role-member = Member
members-invite-label = For whom (optional)
members-invite-create = Create invite link
members-invite-note = You never type anyone else's password. They set it on the link, along with their name.
members-link-once = This link is shown once. Create a new one if it is lost.
members-copy = Copy
members-pending = Pending links
members-expires = expires { $date }
members-expired = expired
members-revoke = Revoke
members-reset = Reset password
members-signout = Sign out everywhere
members-reset-note = Issuing a reset signs { $name } out everywhere and is recorded in the change log.
members-signed-out = signed { $name } out of { $n } device(s)
members-no-sessions = not signed in anywhere

## The command bar

command-placeholder = command
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
log-action-import = imported
log-action-materialize = materialized
log-action-password-reset = password reset
log-action-sessions-revoked = signed out everywhere

log-field-amount = amount
log-field-title = description
log-field-date = date
log-field-due-day = due day
log-field-bucket = bucket
log-field-bill-name = bill name
log-field-instance = instance
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

## Bill import

import-dropzone = Drop the bill here, or choose a file
import-client-code = Client code
import-unlink = Unlink
import-read = read { $what } from the bill
import-filled = read { $amount } due { $date } from the barcode
import-amount-differs = the barcode says { $barcode }, this row says { $row } — left as it was
import-date-differs = the barcode says { $barcode }, this row says { $row } — left as it was
import-code-differs = this bill's client code is { $found }, but the rule has { $known } — left as it was
import-unresolved = the barcode did not carry { $what }
import-no-occurrence = there is no row for this month to attach it to
import-unlinked = client code unlinked
import-already-attached = this bill is already on { $period } — one bill, one row

## The month picker and its warning dot

months-short = Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec
month-picker = Month
unresolved-dot = { $n ->
        [one] 1 bill overdue with no receipt
       *[other] { $n } bills overdue with no receipt
    }
unresolved-here = overdue here

## `e` — editing an occurrence in place

edit-day = Day
edit-private = Hidden from other members
edit-saved = saved
row-reattributed = moved to { $name }
row-now-private = hidden from the others
row-now-shared = shared with the household
reattribute-private-title = Move a hidden row to { $name }?
reattribute-private-body = They become its owner, so you stop seeing it. Press y to confirm.
no-pane-that-way = no pane that way
drafts-parked = { $n ->
        [one] 1 unsaved draft
       *[other] { $n } unsaved drafts
    }

## `gd` — the details popup, and the `P` carousel

details-due = Due
details-cadence = Cadence
details-receipt = Receipt
details-status = Status
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
