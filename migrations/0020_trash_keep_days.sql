-- ---------------------------------------------------------------------------
-- household_settings.trash_keep_days — how long a removed file is kept before
-- `:prune` offers it for removal for good.
-- ---------------------------------------------------------------------------
--
-- Every delete, replace and supersede moves a file into `{slug}/.delete/`, and
-- nothing ever left it but a byte-identical duplicate `:prune` removed — no
-- bytes lost. So the trash only grew. After this many days in the trash,
-- counted from `receipts.deleted_at` (when it was removed, never the bill's
-- month), a file is offered by `:prune` too, duplicate or not, and goes only
-- at an admin's `y`. Nothing is removed on its own.
--
-- 0 keeps every file forever, which is what the app did before this. The upper
-- bound is ten years: a number past it is a typo, not a policy.

ALTER TABLE household_settings
    ADD COLUMN trash_keep_days INTEGER NOT NULL DEFAULT 90
    CHECK (trash_keep_days BETWEEN 0 AND 3650);
