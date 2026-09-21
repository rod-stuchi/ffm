-- ---------------------------------------------------------------------------
-- rule_barcode_window — closes the KNOWN HOLE recorded in CLAUDE.md.
--
-- A multi-period barcode is one uploaded document covering several months, and
-- an occurrence's page was derived from the rule's CURRENT window_start. A
-- cyclic bill renews by re-entering that field — so the moment the 2027 window
-- was set, January 2026's row stopped reading "page 1" and started reading
-- "page -11, outside this file's 10 pages". The file rail and the P carousel
-- would both begin flagging history that was never wrong.
--
-- The fix is to store the WINDOW rather than the page: an occurrence resolves
-- against the window that covered its own period. "Derived, never stored"
-- still holds — the page number is computed, it is the window that persists.
--
-- recurring_rules.spans_period / window_size / window_start stay exactly where
-- they are. They are the rule's CURRENT window, which is what the editor
-- writes and what a new upload sets; this table is the history of the ones
-- before it. Retiring a window is setting `retired_at`, never deleting a row.
-- ---------------------------------------------------------------------------
CREATE TABLE rule_barcode_window (
    id            INTEGER PRIMARY KEY,
    rule_id       INTEGER NOT NULL REFERENCES recurring_rules(id),
    window_start  TEXT    NOT NULL,   -- 'YYYY-MM'; this period is page 1
    window_size   INTEGER NOT NULL,   -- pages in the uploaded file
    file_path     TEXT,               -- the one document every page points at
    uploaded_at   TEXT    NOT NULL,
    retired_at    TEXT,               -- set when a later window replaces it

    CHECK (window_size BETWEEN 1 AND 120),
    CHECK (window_start GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'
           AND substr(window_start, 6, 2) BETWEEN '01' AND '12')
);

-- The lookup is always "which window covered this rule at this period", so the
-- index leads with the rule and orders by start.
CREATE INDEX idx_window_rule ON rule_barcode_window(rule_id, window_start);

-- At most one live window per rule: a second upload retires the first rather
-- than leaving two documents both claiming to be current.
CREATE UNIQUE INDEX idx_window_live ON rule_barcode_window(rule_id)
    WHERE retired_at IS NULL;
