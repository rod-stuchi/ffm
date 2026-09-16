-- FFM — migration 1: initial schema
--
-- Conventions used throughout, decided in design/spec.md + CLAUDE.md:
--   * Dates/timestamps are ISO-8601 TEXT. Date-only columns are 'YYYY-MM-DD';
--     timestamps are UTC 'YYYY-MM-DDTHH:MM:SSZ'. Zero-padded and fixed-width,
--     so lexicographic comparison and index range scans are correct.
--   * `period` is 'YYYY-MM' — which month's *view* a row belongs to. Explicit
--     rather than derived from a date, because July's bill paid on 03/08 still
--     belongs to July.
--   * Money is INTEGER cents. Never REAL — binary floats cannot represent
--     decimal currency exactly. pt-BR formatting (R$ 1.234,56) is server-side.
--   * Booleans are INTEGER 0/1 (SQLite has no BOOLEAN).
--   * Nothing is hard-deleted: `deleted_at` marks a row instead.
--
-- NOTE: two settings deliberately do NOT live in this file, because they cannot:
--   * `PRAGMA foreign_keys = ON` is per-connection and OFF by default, so none
--     of the REFERENCES below are enforced unless the connection sets it.
--   * `PRAGMA journal_mode = WAL` cannot be set inside a transaction, and SQLx
--     runs each migration in one ("cannot change into wal mode from within a
--     transaction").
-- Both belong in the SQLx connect options:
--   SqliteConnectOptions::new().foreign_keys(true).journal_mode(SqliteJournalMode::Wal)

-- ---------------------------------------------------------------------------
-- members — real accounts, not just attribution labels
-- ---------------------------------------------------------------------------
CREATE TABLE members (
    id             INTEGER PRIMARY KEY,
    uuid           TEXT    NOT NULL UNIQUE,           -- receipt vault folder: {uuid}-{slug}
    name           TEXT    NOT NULL UNIQUE,           -- display name + login identifier
    slug           TEXT    NOT NULL UNIQUE,           -- path-safe, derived from name
    email          TEXT    UNIQUE,                    -- optional
    password_hash  TEXT    NOT NULL,                  -- argon2
    role           INTEGER NOT NULL DEFAULT 1,        -- 0 = admin, sees private rows
    created_at     TEXT    NOT NULL,
    deleted_at     TEXT,

    CHECK (role IN (0, 1)),
    CHECK (slug NOT LIKE '.%' AND lower(slug) <> 'delete')
);

-- ---------------------------------------------------------------------------
-- sessions — server-side, so one lost device can be revoked on its own
-- ---------------------------------------------------------------------------
CREATE TABLE sessions (
    id            TEXT    PRIMARY KEY,                -- opaque; this is the cookie value
    member_id     INTEGER NOT NULL REFERENCES members(id),
    created_at    TEXT    NOT NULL,
    last_seen_at  TEXT    NOT NULL                    -- sliding expiry
);

CREATE INDEX idx_sessions_member ON sessions(member_id);

-- ---------------------------------------------------------------------------
-- member_settings — personal display preferences, 1:1 with member.
-- Deliberately NOT in ffm.toml: taste, not process config.
-- ---------------------------------------------------------------------------
CREATE TABLE member_settings (
    member_id           INTEGER PRIMARY KEY REFERENCES members(id),
    theme_name          TEXT    NOT NULL DEFAULT 'ffm',
    theme_overrides     TEXT    NOT NULL DEFAULT '{}',      -- JSON: token -> colour
    line_numbers        TEXT    NOT NULL DEFAULT 'relative',
    ledger_group        TEXT    NOT NULL DEFAULT 'none',
    group_fold_default  TEXT    NOT NULL DEFAULT 'expanded',

    CHECK (line_numbers       IN ('relative', 'absolute', 'none')),
    CHECK (ledger_group       IN ('none', 'bucket')),
    CHECK (group_fold_default IN ('expanded', 'collapsed')),
    CHECK (json_valid(theme_overrides))
);

-- ---------------------------------------------------------------------------
-- recurring_rules — defined once, materialized into transactions each period
-- ---------------------------------------------------------------------------
CREATE TABLE recurring_rules (
    id                    INTEGER PRIMARY KEY,
    member_id             INTEGER NOT NULL REFERENCES members(id),
    title                 TEXT    NOT NULL,
    kind                  TEXT    NOT NULL,                 -- salary recurs too
    amount_cents          INTEGER,                          -- seed for the first occurrence; NULL = always entered by hand
    cadence               TEXT    NOT NULL DEFAULT 'monthly',
    due_day               INTEGER,                          -- day of month; NULL = no due date
    start_date            TEXT    NOT NULL,
    end_date              TEXT,                             -- NULL = ongoing
    total_installments    INTEGER,                          -- NULL = ongoing; set = finite, non-cyclic run (car loan)
    carry_forward_amount  INTEGER NOT NULL DEFAULT 1,       -- seed next period from last period's actual amount
    tracked               INTEGER NOT NULL DEFAULT 0,       -- receipt tracking opted in

    -- Categorisation. General-purpose on any transaction; a tracked rule also
    -- uses these to compute where its files live.
    bucket                TEXT,
    bill_name             TEXT,
    instance              TEXT,                             -- tracked rules only; disambiguates two properties

    visibility            TEXT    NOT NULL DEFAULT 'household',
    created_at            TEXT    NOT NULL,
    deleted_at            TEXT,

    CHECK (kind       IN ('in', 'out')),
    CHECK (cadence    IN ('monthly', 'bimonthly', 'quarterly', 'yearly')),
    CHECK (visibility IN ('household', 'private')),
    CHECK (due_day IS NULL OR (due_day BETWEEN 1 AND 31)),
    CHECK (total_installments IS NULL OR total_installments > 0),
    CHECK (end_date IS NULL OR end_date >= start_date),
    -- a tracked rule must have the two path segments it needs
    CHECK (tracked = 0 OR (bucket IS NOT NULL AND bill_name IS NOT NULL)),
    -- '.delete/' is reserved in the vault; full slug validation lives in Rust
    CHECK (bucket    IS NULL OR (bucket    NOT LIKE '.%' AND lower(bucket)    <> 'delete')),
    CHECK (bill_name IS NULL OR (bill_name NOT LIKE '.%' AND lower(bill_name) <> 'delete')),
    CHECK (instance  IS NULL OR (instance  NOT LIKE '.%' AND lower(instance)  <> 'delete'))
);

CREATE INDEX idx_rules_member ON recurring_rules(member_id) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- transactions — one continuous table. "Months" are views over it, never copies.
-- A materialized recurring occurrence is just a transaction with a rule_id.
-- ---------------------------------------------------------------------------
CREATE TABLE transactions (
    id             INTEGER PRIMARY KEY,
    member_id      INTEGER NOT NULL REFERENCES members(id),   -- attribution; re-attribution rewrites this
    rule_id        INTEGER REFERENCES recurring_rules(id),    -- NULL for a one-off
    period         TEXT    NOT NULL,                          -- 'YYYY-MM', which month's view this belongs to
    kind           TEXT    NOT NULL,
    title          TEXT    NOT NULL,                          -- copied from the rule at materialization, then independent
    amount_cents   INTEGER,                                   -- NULL = not known yet, renders as "R$ __,__"
    date           TEXT,                                      -- occurred/paid-on; NULL until known
    due_date       TEXT,

    -- The one manual tick in the whole model, and only for untracked rules —
    -- a tracked occurrence's paid state is read from the filesystem, never here.
    manual_paid    INTEGER NOT NULL DEFAULT 0,

    visibility     TEXT    NOT NULL DEFAULT 'household',
    bucket         TEXT,
    bill_name      TEXT,

    sort_key       TEXT    NOT NULL,                          -- LexoRank-style fractional index, global not per-month
    supersedes_id  INTEGER REFERENCES transactions(id),       -- restore chain: new row points at the one it replaces

    created_at     TEXT    NOT NULL,
    created_by     INTEGER REFERENCES members(id),
    deleted_at     TEXT,
    deleted_by     INTEGER REFERENCES members(id),

    CHECK (kind       IN ('in', 'out')),
    CHECK (visibility IN ('household', 'private')),
    CHECK (manual_paid IN (0, 1)),
    CHECK (period LIKE '____-__'),
    CHECK (amount_cents IS NULL OR amount_cents >= 0)         -- direction lives in `kind`, not in the sign
);

-- One *live* occurrence per rule per period. Partial on purpose:
--   * a soft-deleted occurrence must not block a restore (which inserts a new row);
--   * the materializer's own gap query is what must treat a soft-deleted
--     occurrence as present, so the hourly job never resurrects a deleted row.
CREATE UNIQUE INDEX idx_tx_rule_period_live
    ON transactions(rule_id, period)
    WHERE rule_id IS NOT NULL AND deleted_at IS NULL;

-- The hot path: one member's pane for one month, in manual order.
CREATE INDEX idx_tx_pane      ON transactions(member_id, period, sort_key) WHERE deleted_at IS NULL;
CREATE INDEX idx_tx_period    ON transactions(period)                      WHERE deleted_at IS NULL;
CREATE INDEX idx_tx_rule      ON transactions(rule_id)                     WHERE rule_id IS NOT NULL;
CREATE INDEX idx_tx_supersede ON transactions(supersedes_id)               WHERE supersedes_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- receipts — audit/undo metadata only. The DISK decides paid/not-paid; this
-- table records who put a file there and how to put it back.
-- ---------------------------------------------------------------------------
CREATE TABLE receipts (
    id                 INTEGER PRIMARY KEY,
    transaction_id     INTEGER NOT NULL REFERENCES transactions(id),
    kind               TEXT    NOT NULL,                      -- barcode | scheduled | receipt
    path               TEXT    NOT NULL,                      -- computed, never authored
    ext                TEXT    NOT NULL,
    original_filename  TEXT    NOT NULL,                      -- what the user's file was called, kept for reference
    uploaded_by        INTEGER NOT NULL REFERENCES members(id),
    uploaded_at        TEXT    NOT NULL,

    -- soft delete: the file moves to a sibling .delete/ folder, never off disk
    deleted_at         TEXT,
    deleted_by         INTEGER REFERENCES members(id),
    delete_reason      TEXT,                                  -- 'manual' | 'superseded' (receipt replaced a scheduled file)
    deleted_from_path  TEXT,                                  -- where it lived, so `u` can put it back

    CHECK (kind IN ('barcode', 'scheduled', 'receipt')),
    CHECK (ext  IN ('pdf', 'jpg', 'png', 'txt')),
    CHECK (delete_reason IS NULL OR delete_reason IN ('manual', 'superseded'))
);

-- One live file of each kind per occurrence; deleted ones stay for the undo trail.
CREATE UNIQUE INDEX idx_receipts_live
    ON receipts(transaction_id, kind)
    WHERE deleted_at IS NULL;

CREATE INDEX idx_receipts_tx ON receipts(transaction_id);
