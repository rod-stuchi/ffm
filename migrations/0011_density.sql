-- ---------------------------------------------------------------------------
-- member_settings.density — how much room the ledger's rows take.
-- ---------------------------------------------------------------------------
--
-- Gmail's three, in this app's terms:
--
--   default      the roomiest: 6px between rows, a little taller, and a
--                tracked row shows each missing file slot as its own `+`
--   comfortable  the ledger as it always was — 2px between rows
--   compact      no gap, one line per row: the meta line goes
--
-- `default` is a VALUE — Gmail's name for its roomiest setting — and never
-- means "follow the household". Nothing here follows anything: like the
-- palette and the layout, a member's density is their own, seeded once from
-- the household when they join.
--
-- Everybody starts on `comfortable`, which is exactly the screen they had, so
-- nothing moves until somebody picks.
ALTER TABLE member_settings
    ADD COLUMN density TEXT NOT NULL DEFAULT 'comfortable'
    CHECK (density IN ('default', 'comfortable', 'compact'));

-- What a new member's density starts as — seeded, never inherited, like the
-- other `default_*` columns beside it (0010).
ALTER TABLE household_settings
    ADD COLUMN default_density TEXT NOT NULL DEFAULT 'comfortable'
    CHECK (default_density IN ('default', 'comfortable', 'compact'));
