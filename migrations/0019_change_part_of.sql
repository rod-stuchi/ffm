-- ---------------------------------------------------------------------------
-- change.part_of — an entry that is a PART of another, never a step of its own.
-- ---------------------------------------------------------------------------
--
-- `:order` copies a pane's manual order to other months of the year: one
-- gesture that moves rows in several months at once. Each row moved is its
-- own `reorder` entry, in its own month, so that row's `gl` says what happened
-- to it. But `u` must take the whole copy back in one press, from the pane and
-- month it was made in — and those entries live in other months, where `u`
-- in this one cannot see them.
--
-- So the copy also writes ONE anchor, and every row's entry points at it:
--
--   anchor  entity = 'member' (the pane's owner), action = 'edit',
--           period = the month copied FROM, note = 'order copied to other
--           months', one `months` field listing where it went
--   parts   entity = 'transaction', action = 'reorder', period = the row's
--           own month, part_of = the anchor
--
-- `u` picks the anchor and reverses every part with it; a part is never
-- picked on its own. Undoing writes a new anchor (undo_of = the old one) and
-- new parts pointing at it, so redo is that same reversal run again — the
-- undo_of chain reads exactly as it does for every other verb.
--
-- **This amends two things 0001 says.** `entity = 'member'` was "account
-- actions, not ledger ones"; a member's entry with a `period` is now also
-- that member's ledger for that month — a pane. And 0001 warns against an
-- "ad-hoc reuse of 'edit'": this is one, chosen over rebuilding `change` for a
-- new action, which cannot be done here — `change_field` and `undo_of` both
-- reference the table, and dropping it under foreign keys fails inside the
-- transaction every migration runs in. The note tells the anchor apart from
-- a member being turned off (also an `edit`, which has no period).
--
-- Adding a column rewrites only the schema, so the append-only triggers never
-- fire; NULL is the default, which a REFERENCES column needs under foreign
-- keys, and every existing entry is a step of its own.
-- ---------------------------------------------------------------------------

ALTER TABLE change
    ADD COLUMN part_of INTEGER REFERENCES change(id)
    CHECK (part_of IS NULL OR (part_of <> id AND entity = 'transaction'));

CREATE INDEX idx_change_part ON change(part_of) WHERE part_of IS NOT NULL;
