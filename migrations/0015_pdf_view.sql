-- ---------------------------------------------------------------------------
-- member_settings.pdf_view — how `P` first shows a PDF.
-- ---------------------------------------------------------------------------
--
--   png   the pages as images, drawn by the server (`pdftoppm`) — the
--         carousel's own zoom and keys work alike in every browser
--   pdf   the browser's own PDF viewer, in an <embed> — sharp at any size,
--         text selectable, zooming itself
--
-- `v` still flips between the two while the carousel is open; this is only
-- which side it opens on. Taste, like the palette, so it is the member's own
-- and nothing seeds it: everybody starts on `png`, which is what `P` always
-- opened on.
ALTER TABLE member_settings
    ADD COLUMN pdf_view TEXT NOT NULL DEFAULT 'png'
    CHECK (pdf_view IN ('png', 'pdf'));
