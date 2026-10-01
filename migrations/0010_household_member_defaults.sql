-- ---------------------------------------------------------------------------
-- household_settings.default_* — what a new member's settings start as.
-- ---------------------------------------------------------------------------
--
-- A member's palette and layout are theirs (member_settings), and stay that
-- way: these columns SEED a new member's row when they join, and are never
-- read again for them. Changing one changes nobody's current screen — a
-- member adopts them later only by asking (`:profile`'s "Use the household's
-- defaults"). That is why they are separate columns rather than nullable
-- member columns meaning "follow the household".
--
-- Each mirrors the member column it seeds: the same default, the same CHECK.
-- The palette is validated in Rust, like member_settings.theme_name, so a new
-- bundled palette needs no migration. Existing households get exactly what
-- their members already received, so nothing moves until an admin says so.
ALTER TABLE household_settings
    ADD COLUMN default_theme TEXT NOT NULL DEFAULT 'ffm';

ALTER TABLE household_settings
    ADD COLUMN default_layout_panes TEXT NOT NULL DEFAULT 'auto'
    CHECK (default_layout_panes IN ('auto', 'split', 'tabs'));

ALTER TABLE household_settings
    ADD COLUMN default_layout_min_split_width INTEGER NOT NULL DEFAULT 900
    CHECK (default_layout_min_split_width BETWEEN 320 AND 4000);

ALTER TABLE household_settings
    ADD COLUMN default_layout_min_width INTEGER NOT NULL DEFAULT 420
    CHECK (default_layout_min_width BETWEEN 0 AND 4000);

ALTER TABLE household_settings
    ADD COLUMN default_layout_max_width INTEGER NOT NULL DEFAULT 900
    CHECK (default_layout_max_width BETWEEN 0 AND 4000);

ALTER TABLE household_settings
    ADD COLUMN default_layout_gap INTEGER NOT NULL DEFAULT 16
    CHECK (default_layout_gap BETWEEN 0 AND 64);
