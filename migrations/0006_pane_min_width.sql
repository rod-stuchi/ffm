-- ---------------------------------------------------------------------------
-- member_settings.layout_min_width — the floor a pane will not be squeezed
-- below while it shares the stage with another.
-- ---------------------------------------------------------------------------
--
-- It arrived first as a constant in the stylesheet, which was the wrong shape
-- for it: how narrow a pane may get is a fact about the screen somebody is
-- sitting at, exactly like `layout_max_width` beside it and every other
-- `layout_*` value here. A number in a stylesheet is one the household cannot
-- reach.
--
-- Below the floor a row's file rail and amount start colliding with the title,
-- which is what the ledger is read for, so the stage scrolls sideways instead
-- of compressing further. Three unreadable panes are worse than two readable
-- ones and a scroll.
--
-- 0 = no floor, mirroring `layout_max_width`'s 0 = unlimited, so the pair
-- reads the same way and a household that wants panes to squeeze can say so.
-- The default is the constant it replaces.
--
-- Rendered into the page's own `<style>` block alongside the cap, and lifted
-- in tabs mode: one pane has nothing to share the stage with, so a floor there
-- would only overflow a narrow viewport.
ALTER TABLE member_settings
    ADD COLUMN layout_min_width INTEGER NOT NULL DEFAULT 420
    CHECK (layout_min_width BETWEEN 0 AND 4000);
