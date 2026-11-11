-- ---------------------------------------------------------------------------
-- member_settings.dim_unfocused — whether the panes out of focus recede.
-- ---------------------------------------------------------------------------
--
--   1   a pane out of focus is faded and desaturated, lifted on hover; the
--       focused one is at full strength
--   0   every pane at full strength; focus is the accent border alone, which
--       is how the ledger looked before the fading was built
--
-- Taste, like the palette and `pdf_view`, so it is the member's own and
-- nothing seeds it: everybody starts with it on.
ALTER TABLE member_settings
    ADD COLUMN dim_unfocused INTEGER NOT NULL DEFAULT 1
    CHECK (dim_unfocused IN (0, 1));
