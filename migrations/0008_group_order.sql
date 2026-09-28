-- ---------------------------------------------------------------------------
-- member_settings.group_order — how `:group bucket` orders its groups.
-- ---------------------------------------------------------------------------
--
-- Groups used to come in the order each bucket first appeared in the pane's
-- own sort, which is fragile in exactly the way that matters: one unfiled
-- income at the top pulled `Uncategorized` above every real bucket, and any
-- row moving could reshuffle the groups. Three orders now, cycled with `S`:
--
--   appearance  first seen, as before — the default, so nothing moves for a
--               member who never asks
--   name        A–Z by bucket, accent-insensitive
--   subtotal    by what the group costs, the biggest spend first
--
-- `Uncategorized` is last in all three: it is a catch-all, not a category, so
-- the order only ever decides among buckets somebody actually chose.
--
-- A member setting, not a household one — how somebody likes to read their
-- ledger, like `ledger_sort` and `group_fold_default` beside it. The values
-- are identifiers and never pass through the translation catalog.
ALTER TABLE member_settings
    ADD COLUMN group_order TEXT NOT NULL DEFAULT 'appearance'
    CHECK (group_order IN ('appearance', 'name', 'subtotal'));
