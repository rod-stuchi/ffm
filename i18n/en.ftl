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
menu-previewing = Previewing without admin rights — :preview in this menu turns them back on
# The row menu: a long press on a phone, a right-click with a mouse. The key
# each item runs is shown beside it, outside the message.
row-menu-edit = Edit
row-menu-edit-rule = Edit rule
row-menu-paid = Mark paid
row-menu-unpaid = Unmark paid
row-menu-received = Mark received
row-menu-unreceived = Unmark received
row-menu-give = Move to…
row-menu-files = Files
row-menu-log = History

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
group-select-needs-group = ledger.group = "none" — X selects a group; :group bucket first
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
# A selection's J/K is one step for u: the row named, and how many moved with it.
undo-moved-with = { $n ->
        [one] + 1 more row with it
       *[other] + { $n } more rows with it
    }
undo-skipped = { $who } changed "{ $title }" since — left as they made it
undo-skipped-copy = a copied order moved "{ $title }" since — left where it is
# The question before every u / U. `when` arrives formatted: "today 14:32",
# "12/09/2026 09:05".
undo-ask-title = Undo this?
redo-ask-title = Redo this?
undo-ask-row = { $verb } "{ $title }" · { $when }
undo-ask-order = order copied to { $months } · { $when }
undo-ask-yes = Undo
redo-ask-yes = Redo
undo-ask-hint = u again or y undoes it — any other key leaves it as it is.
redo-ask-hint = U again or y redoes it — any other key leaves it as it is.
# :order's copy, undone or redone whole. `month` arrives formatted (10/2026),
# `months` as short names (Jan, Feb).
order-ask-month = { $month } · { $n ->
        [one] 1 row
       *[other] { $n } rows
    }
order-left = { $n ->
        [one] 1 row moved since — left where it is
       *[other] { $n } rows moved since — left where they are
    }
order-undo-did = undone: order copied to { $months } · { $n ->
        [one] 1 row put back
       *[other] { $n } rows put back
    }
order-redo-did = redone: order copied to { $months } · { $n ->
        [one] 1 row moved
       *[other] { $n } rows moved
    }
error-not-written = That didn't go through — nothing was written.
tx-row-gone = that row is no longer there — deleted, or moved out of your sight
tx-cannot-read-household = the household could not be read — try again
tx-cannot-read-month = that month could not be read — try again
tx-cannot-read-row = that row could not be read — try again

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
rule-past-note = Earlier months keep what they recorded; this month's row follows a new due day while it is unpaid.
# The rule pane's two sections that fold, and what a closed one says it holds.
rule-sect-schedule = Schedule
rule-sect-files = Files
rule-sum-ongoing = ongoing
rule-sum-due = due
rule-sum-expected = expected
rule-sum-tracked = receipts on disk
rule-sum-untracked = no files
rule-sum-bill = bill on file
rule-bill-card = Bill
rule-pdf-password-placeholder = only for locked statements
rule-created = rule created · { $n } period(s) materialized
rule-saved = rule saved
rule-moved-due = this month's row now due { $date }
rule-renamed-row = this month's row renamed too
files-refiled =
    { $n ->
        [one] 1 file moved to its new folder
       *[other] { $n } files moved to their new folder
    }
files-refile-blocked = not moved — a file is already at { $path }
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
file-type-refused = that file type is not one we keep — PDF, JPG, PNG or TXT
file-store-failed = the file could not be saved — { $detail }
file-pdf-wrong-password = the rule's PDF password does not open this file — check it in the rule — nothing was saved
file-pdf-no-tool = this PDF opens only with a password, and qpdf is not installed on the server to open it — nothing was saved
file-pdf-unreadable = could not open this PDF — it may be damaged — nothing was saved
file-preview = Files
file-no-files = no files on this row
file-page-of = One file, { $total } pages — this month is page { $page }
rule-spans-period = Barcode covers multiple months
rule-spans-hint = each month takes the code due in it — attach the file and it is read
estimate-no-history = no confirmed months yet — the rule's own amount is used

## Settings panes — :profile, :household, :members

settings-profile = Your settings
settings-household = Household
settings-members = Members
settings-saved = saved — { $name }
settings-admin-only = that is admin only
settings-invalid-one-of = ✗ { $key } must be one of { $allowed }
settings-invalid-between = ✗ { $key } must be a number between { $lo } and { $hi }
settings-invalid-palette = ✗ { $key } is not a bundled palette
settings-invalid-empty = ✗ { $key } cannot be empty
settings-invalid-currency = ✗ { $key } is not one we format
settings-invalid-assistant = ✗ { $key } is not one this app runs
settings-invalid-storage = ✗ { $key } could not be saved — { $detail }
# Only a command sets it (`:group`); every other setting is named by its control's label.
settings-label-group = Grouping
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
status-keyboard = Show the keyboard
status-hide = Hide the amounts — gh
status-show = Show the amounts — gh
values-hidden = amounts hidden on this device · gh shows them
values-shown = amounts shown
profile-panes = Panes
profile-min-split = Split below this width, use tabs
profile-min-width = Minimum pane width
profile-max-width = Maximum pane width
profile-gap = Gap between panes
profile-sort = Row order
profile-sort-manual = Manual
profile-sort-name = Name
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
profile-pdf-view = PDFs in P open as
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
device-on = { $browser } on { $os }
device-unknown = unknown device

household-name = Household name
household-currency = Currency
household-language = Default language
household-visibility = Default for new rows
household-show-recurring = Use recurring rules
household-storage = Set by the operator
household-assistant = Bill assistant
household-assistant-label = Teaches each rule to read its bills
household-assistant-none = None — type what the barcode does not say
household-assistant-note = On a rule's first bill, the assistant reads its first page and writes how to read that issuer's bills; you approve it, and every month after is read here, with no assistant. Only page 1 is sent, with CPF and card numbers masked. It runs on this server, on the subscription its login belongs to.
household-assistant-off = nothing is sent anywhere
household-assistant-ready = { $name } · ready
household-assistant-installed = { $name } · installed
household-assistant-checking = checking { $name }…
household-assistant-not-found = { $name } is not installed on this server
household-assistant-logged-out = { $name } is installed but not logged in — run `{ $login }` on the server
household-trash = Trash
household-trash-keep = Days a removed file is kept
household-trash-keep-hint = after that, :prune offers it for removal for good — it still asks first · 0 keeps every file forever
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

command-placeholder = command · Tab completes · Ctrl-k recalls
search-placeholder = search · regex · Ctrl-k recalls
command-about-help = the keymap — the same as ? · :help ga opens it searched
command-about-profile = your own settings
command-about-sort = the order of the rows
command-about-group = group the rows by bucket, or flatten them
command-about-set = line numbers: relative · absolute · off
command-about-theme = your color palette — bare, it opens the picker
command-about-lang = your language — words only, never dates or money
command-about-grep = search every month — bare, the last search again
command-about-trash = deleted rows and removed files, to bring back
command-about-order = copy this month's order to other months
command-about-prune = duplicate files, and files in the trash longer than the household keeps them — remove them for good
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
log-field-issuer = issuer
log-field-card = card
log-field-pdf-password = PDF password
log-field-cadence = cadence
log-field-start-date = start date
log-field-end-date = end date
log-field-tracked = track receipt
log-field-path = path
log-field-kind = kind
log-field-original-filename = original filename
log-field-rows = rows renamed with it
log-field-position = position
log-field-sort-key = place in the order
log-field-visibility = visibility
log-field-months = months
log-field-company-code = company code
log-field-client-code = client code
log-field-barcode = barcode
log-field-status = status
log-field-recipe = bill recipe
log-field-member = member
log-field-kept = kept
log-value-household = shared
log-value-private = private
log-value-active = active
log-value-inactive = inactive
# Notes are stored in English (the trash matches on them) and worded here.
log-note-manual = removed on purpose
log-note-superseded = replaced by a newer file
log-note-swapped = swapped aside
log-note-with-row = moved with its row
log-note-moved = its bill is on another row
log-note-found-by-hand = found on disk — filed by hand
log-note-removed-by-hand = removed from disk by hand
log-note-refiled = re-filed where its row now points
log-note-pruned = removed for good — a duplicate kept elsewhere
log-note-expired = removed for good — kept in the trash past its time
log-note-order-copied = order copied to other months
log-note-order-from = order copied from { $period }
log-note-order-from-undone = order copied from { $period } — undone
log-note-attached-unconfirmed = attached anyway — not confirmed as this rule's bill
log-note-attached-mismatch = attached anyway — it names something else: { $detail }
log-note-recipe = bill recipe taught by { $source }
log-note-console-reset = reset link issued from the console
log-note-signed-out-others = signed out of every other device
log-on = on
log-off = off
moved-down = down
moved-up = up
group-uncategorized = Uncategorized
group-rows = { $n ->
        [one] 1 row
       *[other] { $n } rows
    }
# A fold header's ring, in words: its rows counted by the color of their marks
fold-done = { $n } done
fold-scheduled = { $n } scheduled
fold-late = { $n } late
fold-open = { $n } open
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

## :order / gs — this pane's manual order, copied to other months.
## `who` is a member's name, `month` / `from` a formatted month (10/2026),
## `months` short month names (Jan, Feb), `year` a year.
order-title = Copy this order
order-hint = Puts { $who }'s rows, in the months you pick, in the order they have in { $month } — each matched to its row here by its rule, then by its name. Rows with no match here go after them.
order-months = Months of { $year }
order-apply = Copy order
order-note = A month still to come arrives in the order of the month before it, so later months follow without being picked. u in this month puts the whole copy back, every month at once.
order-pick = type the months to copy this order to — 1-9, or ^10 for all the others
order-empty-source = this month has no rows — there is no order here to copy
order-mark-source = this order
order-mark-moves = { $n ->
        [one] 1 row moves
       *[other] { $n } rows move
    }
order-mark-same = already in this order
order-mark-follows = follows when it arrives
order-mark-not = arrives in { $from }'s order instead
order-mark-empty = no rows
order-nothing-to-move = nothing to move
order-line-moves = { $n ->
        [one] 1 row moves
       *[other] { $n } rows move
    } in { $months }
order-line-same = { $months } already in this order
order-line-follows = { $months } follow when they arrive
order-line-not = { $months } will arrive in { $from }'s order, not this one
order-line-empty = no rows in { $months }
order-copied = order copied to { $months } · { $n ->
        [one] 1 row moved
       *[other] { $n } rows moved
    }
order-nothing-moved = nothing moved — the months picked already read this way

## Bill import

import-dropzone = Drop the bill here, or choose a file
import-choose-file = Choose file
import-drop-replace = drop a new bill here to replace this one
import-client-code = Client code
import-unlink = Unlink
import-read = read { $what } from the bill
import-installments =
    { $n ->
        [one] 1 month · { $first }
       *[other] { $n } months · { $first }–{ $last }
    }
import-installments-each =
    { $n ->
        [one] 1 month · { $first } · { $amount }
       *[other] { $n } installments · { $first }–{ $last } · { $amount } each
    }
import-filled = read { $amount } due { $date } from the barcode
import-read-amount = read { $amount } from the barcode
import-amount-differs = the barcode says { $barcode }, this row says { $row } — left as it was
import-date-differs = the barcode says { $barcode }, this row says { $row } — left as it was
import-replaced = replaced { $was }
import-unresolved = the barcode did not carry { $what }
import-missing-amount = an amount
import-missing-due = a due date
import-missing-issuer = who issued it
import-missing-kind = the kind this rule's bills have
import-no-occurrence = there is no row for this month to attach it to
import-unlinked = bill unlinked from this rule — drop the right one
import-already-attached = this bill is already on { $period } ({ $title } — { $amount }) — nothing was saved
import-already-attached-hidden = this bill is already on { $period }, on a row you cannot see — nothing was saved
import-bill-moved = moved from { $title } { $period }
rule-path-missing-bucket = ‹bucket›
import-ask-mismatch = this bill is from { $found }, but { $rule } is bound to { $known } — attach it anyway?
import-ask-unconfirmed = nothing on this bill confirms it is { $rule }'s — no barcode, and its issuer is not recognized. Attach it anyway?
import-ask-unreadable = { $why } — nothing confirms it is the bill for { $rule }. Attach it anyway?
import-ask-title = Attach this bill?
import-ask-title-mismatch = Wrong row?
import-ask-hint = Only y attaches it. Anything else leaves it out, and nothing is saved.
import-ask-yes = Attach anyway
import-ask-declined = not attached — nothing was saved
import-attached-anyway = attached to { $rule } anyway, as you asked
import-why-no-text = this file has no text to read — a scan or a photo
import-why-no-barcode = there is no barcode in it, and no issuer profile recognizes it
import-why-no-barcode-known = there is no barcode in it, and { $issuer } bills always carry one
import-why-misread = its barcode's check digits do not add up — misprinted or misread
import-why-unreadable = this file could not be read
import-filled-text = read { $amount } due { $date } from the bill's text
import-read-amount-text = read { $amount } from the bill's text
import-id-account = account { $code }
import-id-company = company { $code }
import-id-bank = a bank slip from bank { $code }
import-id-card = card ending { $digits }
import-id-cnpj = CNPJ { $cnpj }
import-teach-title = Teach { $rule } to read its bills?
import-teach-message = { $assistant } read this bill. Keep it, and every month after is read here, with no assistant:
import-teach-yes = Keep and attach
import-teach-hint = Only y keeps it. Anything else attaches the bill without it.
import-teach-kept = { $rule } learned to read its bills
import-teach-failed = the assistant could not read this bill — attached with what it said on its own
import-teach-amount = Amount
import-teach-due = Due date
progress-trim = Keeping the pages you chose
progress-shrink = Shrinking the file
progress-unlock = Opening the locked PDF
progress-read = Reading the bill
progress-ask = Asking { $assistant } to read it
progress-check = Checking what it read
progress-file = Filing it
progress-find = Finding its row
drop-no-rule = no rule knows this bill yet — drop it on its row, or attach it with E
drop-no-row = { $rule } has no row in { $period }
drop-locked = this PDF is locked and no rule's password opens it — drop it on its row, or set the password with E
drop-pick-title = Which row is this bill for?
drop-pick-note = More than one rule recognizes this bill. Pick its row — nothing is filed until you do.
drop-filed-elsewhere = filed on { $row }
import-meta-taught = read by { $assistant } — kept when you create the rule
import-meta-card = Card
import-meta-card-value = ending { $digits }
import-meta-text-only = no barcode on this bill — read from its text
import-meta-hint = what this rule is bound to — a bill for anything else is asked about first
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
# `ga` — the member picker that re-attribution opens.
give-title = Move to which member?
give-private = This row is hidden from the others. Whoever gets it becomes its owner, so you stop seeing it.
give-nobody = nobody else to move it to
give-moved-key = rows move to another member with ga now
drafts-parked = { $n ->
        [one] 1 unsaved draft
       *[other] { $n } unsaved drafts
    }

month-leave-key = changing month loses { $drafts } — { $key } again to go anyway
month-leave-click = changing month loses { $drafts } — click again to go anyway
month-already-current = already on the current month
month-prev = previous month
month-next = next month
status-admin = (admin)
ledger-empty-title = No household yet.
ledger-empty-hint = Put one in the database with:

## `gd` — the details popup, and the `P` carousel

details-due = Due
details-cadence = Cadence
details-receipt = Receipt
details-status = Status
details-rule = Rule
details-rule-deleted = deleted on { $date }
details-progress = Progress
details-progress-paid = { $paid } of { $total } · { $done } paid
details-progress-received = { $paid } of { $total } · { $done } received
details-progress-to-go = { $left } to go
details-progress-to-come = { $left } to come
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
carousel-size-title = File size
carousel-add-barcode = Add barcode
carousel-append-pdf = Append PDF
carousel-code-title = Add a barcode page
carousel-code-label = Payment code
carousel-code-hint = the digits under a bill's barcode, or the code from an email — 44, 47 or 48 digits
carousel-code-add = Add
carousel-only-pdf = Only a PDF can take another page
carousel-append-not-pdf = Only a PDF can be appended
barcode-code-length = a payment code is 44, 47 or 48 digits — this one has { $n }
barcode-code-digit = the check digit for { $at } does not match — check the code
barcode-code-not = that is not a payment code
barcode-page-heading-consumo = Barcode · utility payment
barcode-page-heading-cobranca = Barcode · bank slip
barcode-page-value = Amount
barcode-page-open = open — the payer decides
barcode-page-reference = { $value } (an index, not money)
barcode-page-company = Company
barcode-page-segment = Segment
barcode-page-bank = Bank
barcode-page-due = Due date
barcode-page-footer = { $row } — made by ffm on { $date } from a typed code
barcode-segment-1 = city hall
barcode-segment-2 = water and sewage
barcode-segment-3 = energy and gas
barcode-segment-4 = telecom
barcode-segment-5 = government body
barcode-segment-6 = company or fine
barcode-segment-7 = bank use
barcode-segment-other = other
carousel-view-pages = Image
carousel-view-pdf = PDF
carousel-view-title = Image: the pages drawn as pictures · PDF: the browser's own viewer
page-fit-height = Height
page-fit-width = Width
page-fit-title = Fit the page to the view's height or width · j k scroll · J K page · H M L top, middle, bottom
carousel-page-alt = page { $n }
keymap-title = Keymap
keymap-close = Close
keymap-search = search keys and what they do
keymap-none = Nothing in the keymap matches that — Esc clears the search.
details-months = month(s)
keymap-move = Move
keymap-act = Act
keymap-find = Find
keymap-fold = Fold
keymap-files = Files
keymap-undo = Undo
keymap-type = Type

# The `?` keymap's rows. The keys beside them are identifiers and stay in
# Rust; these say what each does.
keymap-row-down-up = row down / up
keymap-row-counted = counted motion
keymap-row-first-last = first / last row
keymap-row-pane = focus pane, left / right
keymap-row-tab = next / previous member tab
keymap-row-tab-n = jump straight to tab 2
keymap-row-month = previous / next month
keymap-row-month-n = three months on — 3[ three back
keymap-row-today = this month — today's
keymap-row-add = add a one-off spend / income
keymap-row-create = create a recurring bill / income
keymap-row-edit = edit this row / its rule
keymap-row-move = move the row down / up
keymap-row-sort = row order: manual · name · amount · date
keymap-row-copy-order = copy this month's order to other months — :order
keymap-row-give = give the row to another member — picked by name, admin only
keymap-row-select = select, for a batch operation — on a group's header, every row in it
keymap-row-select-group = select the group the cursor is in — or clear it
keymap-row-select-all = select every row in the pane — or clear them
keymap-row-private = toggle private
keymap-row-paid = mark paid by hand — untracked recurring bills
keymap-row-details = due, cadence and receipt details
keymap-row-history = this row's history
keymap-row-hide = hide or show every amount — pixelated, on this device
keymap-row-search = search this month — incremental, regex
keymap-row-bar-history = inside the : and / bars — earlier commands and searches
keymap-row-match = next / previous match
keymap-row-grep = search every month
keymap-row-fold-toggle = toggle the fold under the cursor
keymap-row-fold-one = open / close it
keymap-row-fold-all = open / close every fold
keymap-row-fold-order = order the groups: first seen · A–Z · by cost
keymap-row-file-open = open the bill / scheduled / receipt
keymap-row-file-attach = attach a bill / scheduled / receipt
keymap-row-file-preview = preview this row's files
keymap-row-p-jump = inside P: jump to a file · step
keymap-row-p-zoom = inside P: zoom in, out, reset
keymap-row-p-scroll = inside P: scroll · fit height / width
keymap-row-p-hml = inside P: top, middle, bottom of the page
keymap-row-p-page = inside P: next / previous page
keymap-row-p-ends = inside P: first page, the end of the last
keymap-row-p-trim = inside P: trim or shrink the PDF on show
keymap-row-p-barcode = inside P: add a page with a barcode drawn from a code
keymap-row-p-append = inside P: append another PDF's pages
keymap-row-p-viewer = inside P: the PDF in the browser's own viewer, and back
keymap-row-trim-view = trim view: pages · scroll · keep / drop · shrink
keymap-row-delete = delete — opens a confirmation
keymap-row-confirm = confirm in a modal
keymap-row-undo = undo · redo — your changes, this pane, this month; asks first
keymap-row-trash = the trash — bring back any deleted row or file
keymap-row-clear = clear count, buffer, selection and search
keymap-row-help = this — / searches it, j k scroll it

date-echo-title = the date this field holds — the picker beside it writes it in the browser's own order

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
legend-fold-ring = A group's rows, counted by their marks: an arc each for done, scheduled and late, in those marks' colors, and the empty track for what is still open. One-offs and untracked bills are left out.
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

# --- toasts and the bell: what the app has said in this window ----------------
notes-title = Notifications
notes-empty = Nothing yet. What the app tells you shows here until this window closes.
notes-clear = Clear
notes-dismiss = Dismiss
command-about-messages = what the app has told you in this window · :messages clear empties it

## Which kind of server this is — the badge a dev build wears (`crate::run`)

run-dev-title = A development build — not the household's server. What you change here is not the real ledger's.
run-dev-lan-title = A development build, open to the local network — not the household's server.
run-demo-title = A development build running as of { $date }, on a copy of the database — nothing here reaches the real ledger.

## Live updates — a row changed while this page was open

live-from-disk = updated from disk — a file was filed or removed by hand
live-under-edit = this row's files changed on disk — Esc to see them

## Duplicate files — :prune

prune-title = Prune the trash
prune-count = { $size } can go
prune-group = { $hash } · { $n } copies · { $size }
prune-empty = No two files in the vault are the same, and nothing has been in the trash longer than the household keeps it. Nothing to prune.
prune-hint = Only files in a trash folder can be checked — what is live stays · x or click to check · X the group · gx all · Enter removes
prune-note = Each copy here has the same bytes as one that stays, so removing it loses nothing. It is removed from the disk for good: not to the trash, and u cannot bring it back.
prune-live = live · { $row }
prune-live-unknown = live · on disk, not on record
prune-trash = trash · { $row } · deleted { $day }
prune-trash-unknown = trash · not on record
prune-expired-group = { $n ->
        [one] 1 file
       *[other] { $n } files
    } in the trash over { $days } days · { $size }
prune-expired-note = Kept in the trash longer than the household keeps removed files — the number of days is set in :household. There is no other copy: removing one loses it.
prune-remove = Remove
prune-confirm = Remove the checked files for good?
prune-confirm-body = They do not go to the trash — they leave the disk, and u cannot bring them back.
prune-confirm-yes = Remove for good
prune-nothing-picked = nothing checked — x or click a file first
prune-done = removed { $n ->
        [one] 1 file
       *[other] { $n } files
    } · { $size } freed
prune-not-in-trash = { $path } is not in a trash folder — only a file there can be removed
prune-last-copy = every copy of { $path } is checked — keep one
prune-not-prunable = { $path } is no longer a copy of anything, nor older than the trash keeps files — the list was out of date, look again
prune-failed = could not remove every file — the list shows what is left

## The totals' words — lowercase; the app bar and the picker uppercase them

totals-in = in
totals-out = out
totals-net = net
totals-balance = bal

# --- the trim-and-shrink view -----------------------------------------------------
prepare-title = Before filing — keep the pages you need
prepare-title-stored = Trim or shrink this file
prepare-title-image = Before filing — make the image smaller
prepare-for = for { $row }
prepare-remove = Remove
prepare-keep = Keep
prepare-shrink = Shrink
prepare-hint = x keep / drop · h l pages · j k scroll · f fit
prepare-spans = One page per month — every page stays
prepare-image-hint = One image, nothing to trim — s shrink · j k scroll · f fit
prepare-save = Save
prepare-keep-one = Keep at least one page
prepare-failed = Could not work out the size — the file is as it was
prepare-shrink-no-gain = no smaller — kept as is
prepare-not-pdf = Only a PDF or an image can be trimmed or shrunk
prepare-trim = Trim / shrink

# --- :charts / gc -----------------------------------------------------------------
charts-title = Charts
charts-menu = charts of the money — this month, this year, every year
charts-tab-month = Month
charts-tab-year = Year
charts-tab-years = Years
charts-admin = admin
charts-household = Household
charts-from = from
charts-to = to
charts-close = Close
charts-table = table
charts-empty = Nothing recorded in these months yet.
charts-refused = The Years tab is for an admin.
charts-selected = { $n ->
        [one] 1 selected row · s to show all
       *[other] { $n } selected rows · s to show all
    }
charts-selected-off = { $n ->
        [one] 1 selected row · s to show only it
       *[other] { $n } selected rows · s to show only them
    }
charts-in = IN
charts-out = OUT
charts-net = NET
charts-vs = vs { $period }
charts-flat = flat
charts-new = new
charts-includes-estimate = { $n ->
        [one] income ~ includes 1 estimate
       *[other] income ~ includes { $n } estimates
    }
charts-kept = Kept of what came in
charts-to-pay = Still to pay
charts-to-pay-none = nothing left this month
charts-to-pay-note = { $n ->
        [one] 1 bill · next { $day }
       *[other] { $n } bills · next { $day }
    }
charts-bills = { $n ->
        [one] 1 bill
       *[other] { $n } bills
    }
charts-buckets-title = Spend by bucket
charts-buckets-sub = { $period } · biggest first
charts-this-month = this month
charts-bucket = Bucket
charts-members-title = Who moved the money
charts-members-sub = { $period } · spending left, income right
charts-fixed-title = Fixed and variable
charts-fixed-sub = what recurs every month, and what does not
charts-recurring = Recurring
charts-oneoff = One-off
charts-fixed-note = Recurring is what a rule brought in; one-off is what was typed by hand.
charts-topay-title = Still to pay this month
charts-topay-sub = bills not yet confirmed, by due date
charts-due = due { $day }
charts-late = late since { $day }
charts-scheduled = scheduled
charts-flow-title = Cash flow
charts-flow-sub = { $range } · income above the line, spending below
charts-kept-title = Kept so far
charts-kept-sub = NET added up, month by month
charts-kept-so-far = kept so far
charts-that-month = that month
charts-trend-title = Is income growing?
charts-trend-sub = 3-month average, since { $month } ·
charts-trend-short = the average needs four months
charts-month = month
charts-avg = 3-month avg
charts-moved-title = What moved
charts-moved-sub = monthly average, { $now } against { $before } · biggest change first
charts-moved-none = Nothing moved by much.
charts-moved-short = Needs six months: the last three against the three before.
charts-per-month = { $amount }/mo
charts-change = change per month
charts-each-bucket-title = Every bucket, month by month
charts-each-bucket-sub = { $range } · one shared scale, so heights compare across cards
charts-together = together
charts-together-title = { $n ->
        [one] The selected row
       *[other] The { $n } selected rows, together
    }
charts-together-sub = followed across months by their rule, or their title · { $range }
charts-each-title = Each one
charts-each-sub = followed across months by their rule, or their title · one shared scale
charts-years-flow-title = The same months, every year
charts-years-flow-sub = { $range } of each year, so this year is not a part-year against whole ones
charts-spent-so-far = Spent so far, by year
charts-earned-so-far = Earned so far, by year
charts-lines-last = vs last year
charts-lines-five = 5 years
charts-lines-key = v switches between last year and the four before it
charts-by-month-against = by { $month }, against { $year } ·
charts-years-table-title = Year by year
charts-years-table-sub = { $range } · the last column is the whole year, for years that are over
charts-col-year = Year
charts-col-kept = kept
charts-col-vs = NET vs year before
charts-col-whole = whole year NET
charts-key-tab = tab
charts-key-period = earlier · later
charts-key-member = member
charts-key-sel = selection ⇄ all
charts-key-scroll = scroll
charts-key-hide = hide amounts
charts-key-close = close
command-about-charts = charts of the money — this month, this year, every year
keymap-row-charts = charts — this month, this year, every year
