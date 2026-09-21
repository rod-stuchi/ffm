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
