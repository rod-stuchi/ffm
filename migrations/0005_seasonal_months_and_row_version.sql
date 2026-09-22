-- ---------------------------------------------------------------------------
-- Two unrelated columns, one migration: the seasonal months allow-list, and
-- the row version that makes a lost update impossible.
-- ---------------------------------------------------------------------------


-- --- recurring_rules.months ------------------------------------------------
--
-- Some bills only run part of the year, every year: São Paulo's IPTU bills in
-- ten instalments (Jan-Oct) and then starts again the next January; a school
-- fee runs Feb-Dec and skips the holidays. The rule still recurs forever, it
-- just skips certain months.
--
-- This is NOT total_installments, which is a finite run that ends permanently
-- (a car loan), and it is NOT the multi-period barcode window, which asks
-- "which page of the shared file is this month's" rather than "does this month
-- exist for this rule at all". IPTU has all three, which is why it is the demo
-- row for each.
--
-- Stored as a sorted comma list of month numbers, NULL meaning every month.
-- Not a child table: it is at most twelve small integers that are always read
-- and written as a whole, and a join would buy nothing. The GLOB is a shape
-- check only — it accepts '1,2,13' — because a real one would need a regex
-- SQLite does not have. Parsing and validation live in domain::months, which
-- is also where the expression grammar the form accepts is defined; this
-- CHECK exists to stop a hand-written INSERT putting a sentence in the column.
ALTER TABLE recurring_rules ADD COLUMN months TEXT;

-- Applies to income rules too. Income runs on the same rule machinery pointed
-- the other way, and seasonal income is real — a 13th salary in December,
-- teaching income only during term. One column, both directions.
CREATE TRIGGER recurring_rules_months_shape_insert
BEFORE INSERT ON recurring_rules
WHEN NEW.months IS NOT NULL
     AND NEW.months NOT GLOB '[0-9]*'
BEGIN
    SELECT RAISE(ABORT, 'months must be a comma list of month numbers');
END;

CREATE TRIGGER recurring_rules_months_shape_update
BEFORE UPDATE ON recurring_rules
WHEN NEW.months IS NOT NULL
     AND NEW.months NOT GLOB '[0-9]*'
BEGIN
    SELECT RAISE(ABORT, 'months must be a comma list of month numbers');
END;


-- --- transactions.version --------------------------------------------------
--
-- The concurrency answer, and deliberately the smallest one that works.
--
-- The failure being prevented is a LOST UPDATE: Rodrigo opens a row in the
-- inline edit form, Ana saves a change to it, Rodrigo saves and silently
-- overwrites her. Two people editing one row in one minute is rare in a
-- household; one of them quietly losing the other's work is what makes the
-- ledger untrustworthy when it happens.
--
-- Every write to a row bumps this. The edit form renders it as a hidden field
-- and saves with `WHERE id = ?1 AND version = ?2`, so zero rows affected means
-- somebody got there first — at which point the save is refused, nothing is
-- written, and the draft stays parked.
--
-- ONLY the edit form's save checks it. `p`, `dd`, `J`/`K` and `Ctrl-L` are
-- instantaneous and fully undoable, so a stale-version refusal there would
-- fire because the materializer touched the row while your finger was on the
-- key. They still bump it, because an edit form held open across one of them
-- is exactly the case being caught.
ALTER TABLE transactions ADD COLUMN version INTEGER NOT NULL DEFAULT 0;
