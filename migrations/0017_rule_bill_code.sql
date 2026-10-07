-- ---------------------------------------------------------------------------
-- rule_bill_code — each month's code in a bill that covers several months.
-- ---------------------------------------------------------------------------
--
-- A city's IPTU is one PDF holding eleven codes on three pages: the single
-- payment (ÚNICA) and ten installments. The window this replaces said "page N
-- is month N" and asked for a page count and a first month — neither of which
-- describes that file, and its file_path was never even written, so no month
-- but the one it was dropped on ever showed the bill.
--
-- Each code carries its own due date, so a rule that covers several months
-- (`spans_period`) reads every code in the file at attach time and keeps one
-- per month here (`services::installments`): the code, what it says, and the
-- page it is printed on. Every covered month shows the one shared file —
-- stored once, on the row it was dropped on (`receipt_id`) — opened on its own
-- page, and carries its own amount and due date.
--
-- A later file retires these rows (`retired_at`) rather than overwriting them,
-- as the window did: history keeps the code it was billed with.
--
-- `rule_barcode_window`, and the rule's `window_size` / `window_start`, stay
-- as they are — nothing reads them now. The rule's CHECK still wants the two
-- columns set while `spans_period` is on, so the form fills them itself.
-- ---------------------------------------------------------------------------
CREATE TABLE rule_bill_code (
    id            INTEGER PRIMARY KEY,
    rule_id       INTEGER NOT NULL REFERENCES recurring_rules(id),
    receipt_id    INTEGER NOT NULL REFERENCES receipts(id),
    period        TEXT    NOT NULL,   -- 'YYYY-MM', the month this code is due in
    barcode_raw   TEXT    NOT NULL,   -- the 44 digits
    kind          TEXT    NOT NULL,   -- 'cobranca' | 'consumo'
    amount_cents  INTEGER,            -- NULL: open, or not money
    due_date      TEXT,               -- 'YYYY-MM-DD'
    page          INTEGER NOT NULL,   -- 1-based, where the code is printed
    created_at    TEXT    NOT NULL,
    retired_at    TEXT,

    CHECK (period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'
           AND substr(period, 6, 2) BETWEEN '01' AND '12'),
    CHECK (kind IN ('cobranca', 'consumo')),
    CHECK (page >= 1)
);

-- One live code per rule per month: a second file retires the first.
CREATE UNIQUE INDEX idx_bill_code_live ON rule_bill_code(rule_id, period)
    WHERE retired_at IS NULL;
