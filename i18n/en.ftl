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
