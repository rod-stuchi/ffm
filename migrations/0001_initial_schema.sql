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
--
-- PRE-RELEASE: this file is edited in place while no household database exists
-- anywhere. The moment one does, that privilege ends — every later change is an
-- additive migration, and SQLx's own checksums will enforce that for us.

-- ---------------------------------------------------------------------------
-- household_settings — exactly one row. Everything that is genuinely
-- household-wide rather than personal taste, and that used to live in
-- ffm.toml's [ledger]/[privacy]/[locale] tables.
--
-- ffm.toml keeps only the three paths a running process cannot change about
-- itself: db_path, receipts_root, backup_dir. Nothing else.
-- ---------------------------------------------------------------------------
CREATE TABLE household_settings (
    id                     INTEGER PRIMARY KEY CHECK (id = 1),   -- one household, enforced
    name                   TEXT    NOT NULL,                     -- /login heading, statusline
    locale_currency        TEXT    NOT NULL DEFAULT 'BRL',       -- symbol AND number format, one preset
    default_language       TEXT    NOT NULL DEFAULT 'en',        -- /login, and members who haven't chosen
    default_visibility     TEXT    NOT NULL DEFAULT 'household', -- a member may override in member_settings
    ledger_show_recurring  INTEGER NOT NULL DEFAULT 1,           -- "we don't use this concept here"
    created_at             TEXT    NOT NULL,
    updated_at             TEXT    NOT NULL,

    CHECK (locale_currency       IN ('BRL', 'USD', 'EUR', 'GBP')),
    CHECK (default_language      IN ('en', 'pt-BR')),
    CHECK (default_visibility    IN ('household', 'private')),
    CHECK (ledger_show_recurring IN (0, 1))
);

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
    last_seen_at  TEXT    NOT NULL,                   -- sliding expiry
    label         TEXT,                               -- derived from User-Agent, so :profile can name a device

    -- Preview-as-non-admin: this admin has switched their OWN admin powers off
    -- to see what a member sees. Deliberately NOT impersonation — same session,
    -- same member, same pane, own private rows still fully visible; the only
    -- thing it changes is whether admin-only visibility and actions are in
    -- effect. It lives on the session rather than in a cookie because it
    -- decides what DATA comes back, which makes it the server's business.
    preview       INTEGER NOT NULL DEFAULT 0,

    CHECK (preview IN (0, 1))
);

CREATE INDEX idx_sessions_member ON sessions(member_id);

-- ---------------------------------------------------------------------------
-- member_invites — an admin-issued, single-use, expiring link. It serves the
-- two jobs that are actually the same mechanism:
--
--   member_id IS NULL -> INVITE. Accepting CREATES the members row. There is
--                        no self-registration; household membership stays
--                        closed. The admin never types somebody else's
--                        password — the invitee sets their own name,
--                        password, language and theme when they accept.
--                        This is also why password_hash above stays NOT
--                        NULL: pre-creating a half-real, password-less
--                        member would make "NULL means cannot authenticate"
--                        a rule enforced by one WHERE clause somebody
--                        eventually forgets.
--   member_id IS SET  -> PASSWORD RESET for that existing member. Accepting
--                        sets password_hash on the row that already exists.
--
-- A reset is NOT just an invite pointed at an existing person: an invite's
-- accept path INSERTs, and members.name is UNIQUE, so the member could only
-- get back in by choosing a different name — which also rewrites `slug`, and
-- `slug` is their receipt-vault folder. Recovering a password must never
-- orphan somebody's filing.
--
-- There is no self-service reset: no mail sender exists, so there is nowhere
-- to send a token. An admin issuing one IS the recovery path, and because it
-- hands over the ability to sign in as that member, it is recorded in the
-- change log and revokes that member's sessions. See design/spec.md's
-- Auth & sessions section for why that accountability is required, not
-- optional.
--
-- `label` is an admin-only note for telling two open INVITE links apart; a
-- reset does not need one, since member_id already says who it is for.
-- ---------------------------------------------------------------------------
CREATE TABLE member_invites (
    id                  INTEGER PRIMARY KEY,
    token_hash          TEXT    NOT NULL UNIQUE,      -- sha256 of 32 random bytes; the token itself is never stored
    member_id           INTEGER REFERENCES members(id),  -- NULL = invite; set = password reset for this member
    label               TEXT,                         -- admin-only note: which link is for whom (invites only)
    role                INTEGER NOT NULL DEFAULT 1,   -- the role the accepted member will get; ignored on a reset
    created_by          INTEGER NOT NULL REFERENCES members(id),
    created_at          TEXT    NOT NULL,
    expires_at          TEXT    NOT NULL,             -- default: created_at + 7 days
    accepted_at         TEXT,
    accepted_member_id  INTEGER REFERENCES members(id),
    revoked_at          TEXT,

    CHECK (role IN (0, 1)),
    CHECK ((accepted_at IS NULL) = (accepted_member_id IS NULL)),
    -- a reset can only ever resolve to the member it was issued for
    CHECK (member_id IS NULL
           OR accepted_member_id IS NULL
           OR accepted_member_id = member_id)
);

-- Argon2 is deliberately NOT used on the token: there is nothing to
-- brute-force in 256 bits of randomness, and a slow hash on every invite-page
-- GET would be a self-inflicted DoS.
CREATE INDEX idx_invites_open ON member_invites(expires_at)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

-- At most one live reset link per member. A second "Reset password" click
-- replaces the first rather than leaving two working links for one account.
CREATE UNIQUE INDEX idx_invites_open_reset ON member_invites(member_id)
    WHERE member_id IS NOT NULL AND accepted_at IS NULL AND revoked_at IS NULL;

-- ---------------------------------------------------------------------------
-- member_settings — personal preferences, 1:1 with member.
-- Deliberately NOT in ffm.toml: taste, not process config. A shared file that
-- two people both want to edit is the file disagreeing with itself.
--
-- The row is created in the same transaction as the member (at /setup, or when
-- an invite is accepted), so no read path ever handles a missing row. The
-- defaults below exist for columns, not for the absence of the row.
-- ---------------------------------------------------------------------------
CREATE TABLE member_settings (
    member_id               INTEGER PRIMARY KEY REFERENCES members(id),

    -- language: words only. Dates stay dd/mm/yyyy and money follows the
    -- household currency, regardless of what this says.
    language                TEXT    NOT NULL DEFAULT 'en',

    -- appearance. The palette lives here; light/dark is per *device* and lives
    -- in the ffm.theme cookie, since the same person uses a bright laptop and a
    -- dark phone.
    theme_name              TEXT    NOT NULL DEFAULT 'ffm',
    theme_overrides         TEXT    NOT NULL DEFAULT '{}',      -- JSON: token -> colour
    line_numbers            TEXT    NOT NULL DEFAULT 'relative',

    -- layout. Was [layout] in ffm.toml; it describes the screen you are on,
    -- not the household. `layout_min_split_width` is still rendered into a
    -- single media query server-side — resolved in CSS, never in JS.
    layout_panes            TEXT    NOT NULL DEFAULT 'auto',
    layout_min_split_width  INTEGER NOT NULL DEFAULT 900,       -- px; below this, auto -> tabs
    layout_max_width        INTEGER NOT NULL DEFAULT 900,       -- px cap per pane; 0 = unlimited
    layout_gap              INTEGER NOT NULL DEFAULT 16,        -- px between panes

    -- ledger view. Was [ledger] in ffm.toml. All view preferences: `sort` is
    -- safe per-member because J/K already refuses to reorder a derived view.
    ledger_sort             TEXT    NOT NULL DEFAULT 'manual',
    ledger_search_scope     TEXT    NOT NULL DEFAULT 'pane',
    ledger_show_delta       INTEGER NOT NULL DEFAULT 1,
    ledger_group            TEXT    NOT NULL DEFAULT 'none',
    group_fold_default      TEXT    NOT NULL DEFAULT 'expanded',

    -- privacy. NULL = follow household_settings.default_visibility, which is
    -- the household's stated policy; a member may opt their own new rows out.
    default_visibility      TEXT,

    CHECK (language               IN ('en', 'pt-BR')),
    CHECK (line_numbers           IN ('relative', 'absolute', 'none')),
    CHECK (layout_panes           IN ('auto', 'split', 'tabs')),
    CHECK (layout_min_split_width BETWEEN 320 AND 4000),
    CHECK (layout_max_width       >= 0),
    CHECK (layout_gap             BETWEEN 0 AND 64),
    CHECK (ledger_sort            IN ('manual', 'date', 'amount')),
    CHECK (ledger_search_scope    IN ('pane', 'household')),
    CHECK (ledger_show_delta      IN (0, 1)),
    CHECK (ledger_group           IN ('none', 'bucket')),
    CHECK (group_fold_default     IN ('expanded', 'collapsed')),
    CHECK (default_visibility     IS NULL OR default_visibility IN ('household', 'private')),
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

    -- The interval, as an integer count of months rather than a word. The form
    -- offers "every 3 months" and "every 4 months" — there is no English word
    -- for the four-month case, and "bimonthly" ambiguously means both every two
    -- months and twice a month, which is not a thing to leave lying around in a
    -- column that feeds money arithmetic. Every read site wants the number
    -- anyway: end_date = start_date + cadence_months × total_installments, and
    -- the barcode window's page maths.
    cadence_months        INTEGER NOT NULL DEFAULT 1,
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

    -- --- barcode / bill import (design/spec-bill-import.md §9) --------------
    -- Decided once, at creation. A bill paid by PIX/TED/wire never has a
    -- barcode at all — but the household may still want its RECEIPT tracked,
    -- so this is independent of `tracked`.
    no_barcode            INTEGER NOT NULL DEFAULT 0,
    -- The subscriber's own account number (Sabesp's matrícula, Enel's número
    -- do cliente). FEBRABAN never encodes it, so it is a text-layer read.
    -- Populated ONLY by attaching a file; there is no form field for it.
    client_code           TEXT,

    -- --- multi-period barcode window ---------------------------------------
    -- One uploaded document covering several periods (12 or 36 barcodes, one
    -- page per month). Only the BARCODE file is shared; _scheduled and the
    -- receipt stay genuinely per-period. The page number is DERIVED from
    -- window_start and the occurrence's own period, never stored per row.
    spans_period          INTEGER NOT NULL DEFAULT 0,
    window_size           INTEGER,                          -- pages in the currently-uploaded file
    window_start          TEXT,                             -- 'YYYY-MM'; this period is page 1

    -- --- varying income ----------------------------------------------------
    -- A commission/freelance rule cannot carry its own number forward. Until a
    -- period is confirmed the row shows a projection computed ON READ — the
    -- projection itself is never stored, so it follows new history instead of
    -- freezing an old guess, and never writes a spurious amount-changed entry.
    varies                INTEGER NOT NULL DEFAULT 0,
    estimate_basis        TEXT    NOT NULL DEFAULT 'lowest',
    estimate_window       INTEGER NOT NULL DEFAULT 3,       -- last N confirmed periods

    -- --- filing ------------------------------------------------------------
    -- '09.pdf' becomes '09_brl122.94.pdf'. Off by default, per tracked rule.
    amount_in_filename    INTEGER NOT NULL DEFAULT 0,

    visibility            TEXT    NOT NULL DEFAULT 'household',
    created_at            TEXT    NOT NULL,
    deleted_at            TEXT,

    CHECK (kind       IN ('in', 'out')),
    CHECK (cadence_months IN (1, 2, 3, 4, 6, 12)),
    CHECK (visibility IN ('household', 'private')),
    CHECK (due_day IS NULL OR (due_day BETWEEN 1 AND 31)),
    CHECK (total_installments IS NULL OR total_installments > 0),
    CHECK (end_date IS NULL OR end_date >= start_date),
    -- a tracked rule must have the two path segments it needs
    CHECK (tracked = 0 OR (bucket IS NOT NULL AND bill_name IS NOT NULL)),
    -- '.delete/' is reserved in the vault; full slug validation lives in Rust
    CHECK (bucket    IS NULL OR (bucket    NOT LIKE '.%' AND lower(bucket)    <> 'delete')),
    CHECK (bill_name IS NULL OR (bill_name NOT LIKE '.%' AND lower(bill_name) <> 'delete')),
    CHECK (instance  IS NULL OR (instance  NOT LIKE '.%' AND lower(instance)  <> 'delete')),

    -- the booleans. carry_forward_amount and tracked were missing these.
    CHECK (carry_forward_amount IN (0, 1)),
    CHECK (tracked              IN (0, 1)),
    CHECK (no_barcode           IN (0, 1)),
    CHECK (spans_period         IN (0, 1)),
    CHECK (varies               IN (0, 1)),
    CHECK (amount_in_filename   IN (0, 1)),

    CHECK (estimate_basis  IN ('lowest', 'average', 'last')),
    CHECK (estimate_window BETWEEN 1 AND 12),

    -- The whole barcode apparatus is spend-only: money arriving carries no
    -- boleto, no issuer and no client code. `varies` is the mirror image — a
    -- bill carries its amount forward, it does not get projected.
    CHECK (kind = 'out' OR (no_barcode = 0 AND client_code IS NULL AND spans_period = 0)),
    CHECK (kind = 'in'  OR varies = 0),

    -- "always null when no_barcode is true" — spec-bill-import.md §9
    CHECK (no_barcode = 0 OR client_code IS NULL),

    -- a window needs both of its numbers, and only a tracked rule has files
    CHECK (spans_period = 0
           OR (tracked = 1 AND no_barcode = 0
               AND window_size IS NOT NULL AND window_start IS NOT NULL)),
    CHECK (window_size  IS NULL OR window_size BETWEEN 1 AND 120),
    CHECK (window_start IS NULL OR (window_start GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'
                                    AND substr(window_start, 6, 2) BETWEEN '01' AND '12')),

    -- the filename suffix only exists on a file, so only on a tracked rule
    CHECK (amount_in_filename = 0 OR tracked = 1)
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

    -- Bill import (design/spec-bill-import.md §9). Per-occurrence by nature:
    -- a new value, a new source and a new linha digitável every period. The
    -- client code is NOT here — it identifies the account, not the bill, so it
    -- lives once on the rule. Recording WHERE each value came from is what
    -- lets a wrong number be audited later.
    barcode_raw      TEXT,                                    -- digits only, no separators; the idempotency key
    barcode_kind     TEXT,                                    -- 'cobranca' | 'consumo'
    amount_source    TEXT,                                    -- 'barcode' | 'text' | 'user'
    due_date_source  TEXT,                                    -- 'fator' | 'free_field' | 'text' | 'user'
    issuer_id        TEXT,                                    -- key into config/issuers.toml; no FK, it is a config file

    created_at     TEXT    NOT NULL,
    created_by     INTEGER REFERENCES members(id),
    deleted_at     TEXT,
    deleted_by     INTEGER REFERENCES members(id),

    CHECK (kind       IN ('in', 'out')),
    CHECK (visibility IN ('household', 'private')),
    CHECK (manual_paid IN (0, 1)),
    -- NOT `LIKE '____-__'`: SQL LIKE's `_` matches ANY character, so that
    -- accepted 'abcd-ef' and '....-..'. GLOB is case-sensitive and supports
    -- character classes; the substr half is also required, because
    -- GLOB '[01][0-9]' on its own still lets month 19 through.
    CHECK (period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'
           AND substr(period, 6, 2) BETWEEN '01' AND '12'),
    CHECK (amount_cents IS NULL OR amount_cents >= 0),        -- direction lives in `kind`, not in the sign

    CHECK (barcode_kind    IS NULL OR barcode_kind    IN ('cobranca', 'consumo')),
    CHECK (amount_source   IS NULL OR amount_source   IN ('barcode', 'text', 'user')),
    CHECK (due_date_source IS NULL OR due_date_source IN ('fator', 'free_field', 'text', 'user')),
    CHECK (barcode_raw IS NULL OR (barcode_raw NOT GLOB '*[^0-9]*'
                                   AND length(barcode_raw) IN (44, 47, 48))),
    CHECK (barcode_raw IS NULL OR barcode_kind IS NOT NULL),
    -- a value that claims to come from a barcode must actually have one
    CHECK (amount_source <> 'barcode' OR barcode_raw IS NOT NULL),
    CHECK (due_date_source IS NULL
           OR due_date_source NOT IN ('fator', 'free_field')
           OR barcode_raw IS NOT NULL)
);

-- One occurrence per rule per period — LIVE OR SOFT-DELETED.
--
-- This index deliberately does NOT carry a `deleted_at IS NULL` clause. It used
-- to, justified by "a soft-deleted occurrence must not block a restore, which
-- inserts a new row" — but restore is in place now (it clears the mark on the
-- row that is already there) and inserts nothing. Meanwhile the materializer's
-- correctness depends on a soft-deleted occurrence counting as PRESENT, so the
-- hourly reconciler never resurrects a row the household deleted on purpose.
-- Making that a schema fact beats leaving it to a WHERE clause somebody
-- eventually forgets. Still partial on rule_id so one-offs never index.
CREATE UNIQUE INDEX idx_tx_rule_period
    ON transactions(rule_id, period)
    WHERE rule_id IS NOT NULL;

-- The hot path: one member's pane for one month, in manual order.
CREATE INDEX idx_tx_pane   ON transactions(member_id, period, sort_key) WHERE deleted_at IS NULL;
CREATE INDEX idx_tx_period ON transactions(period)                      WHERE deleted_at IS NULL;
CREATE INDEX idx_tx_rule   ON transactions(rule_id)                     WHERE rule_id IS NOT NULL;

-- The MoM delta's previous-period lookup. A recurring row matches on rule_id,
-- but a one-off has no rule — and the mockup shows deltas on one-offs. The
-- match is (member_id, title) within the previous period: it is what a
-- household means by "the same line", it needs no extra column, and it
-- degrades gracefully — rename the line and you lose the delta, which is
-- arguably the correct answer.
CREATE INDEX idx_tx_delta ON transactions(member_id, title, period) WHERE deleted_at IS NULL;

-- The same boleto can only ever be one live row: re-uploading a bill must not
-- create a second occurrence. Deleted rows keep theirs so `u` can restore
-- without a uniqueness fight.
CREATE UNIQUE INDEX idx_tx_barcode
    ON transactions(barcode_raw)
    WHERE barcode_raw IS NOT NULL AND deleted_at IS NULL;

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

-- ---------------------------------------------------------------------------
-- change / change_field — the app's ONE audit mechanism. Append-only.
--
-- Two tables, not one: a single wide table would either repeat the actor and
-- the timestamp once per changed field, or bury the diff in a JSON blob that
-- nothing can query.
--
-- `action` is consolidated HERE from three places that had drifted apart: the
-- mockup's LOG_VERBS table (design/ffm-app-mockup.html), design/spec.md's
-- Change log section, and its Auth & sessions section's password_reset /
-- sessions_revoked. THIS LIST IS THE AUTHORITATIVE ONE.
--
-- Two gaps closed on purpose rather than by accident:
--   * Deleting a RULE uses the plain 'delete'/'restore' verbs with
--     entity = 'rule'. There is deliberately no 'rule_delete'.
--   * Settings changes are deliberately NOT logged. household_settings and
--     member_settings are not ledger entities, `entity` has no value for
--     them, and the spec never asks for it. If that changes it is a new
--     entity value plus a new action — not an ad-hoc reuse of 'edit'.
-- ---------------------------------------------------------------------------
CREATE TABLE change (
    id         INTEGER PRIMARY KEY,
    at         TEXT    NOT NULL,                       -- ISO-8601 UTC
    member_id  INTEGER REFERENCES members(id),         -- the SESSION's member, never the focused pane
    source     TEXT    NOT NULL,
    action     TEXT    NOT NULL,
    entity     TEXT    NOT NULL,
    entity_id  INTEGER NOT NULL,
    rule_id    INTEGER REFERENCES recurring_rules(id), -- stamped on transaction changes, so "this row
                                                       -- plus the rule behind it" is a single query
    period     TEXT,                                   -- 'YYYY-MM'; NULL for rule-level changes
    undo_of    INTEGER REFERENCES change(id),          -- an undo is an ENTRY, never a rewrite
    note       TEXT,                                   -- one line ('superseded by receipt upload')

    CHECK (source IN ('user', 'import', 'materializer', 'system')),
    CHECK (entity IN ('transaction', 'rule', 'file', 'member')),
    CHECK (action IN (
        'create', 'edit', 'delete', 'restore', 'reattribute', 'reorder', 'privacy',
        'rule_create', 'rule_edit',
        'file_attach', 'file_unlink', 'file_upload', 'file_delete',
        'import', 'materialize',
        'password_reset', 'sessions_revoked')),

    -- action and entity are not independent. Pinning the fixed pairs makes a
    -- 'password_reset' on a transaction impossible rather than merely wrong.
    CHECK (action NOT IN ('rule_create', 'rule_edit')            OR entity = 'rule'),
    CHECK (action NOT IN ('file_attach', 'file_unlink',
                          'file_upload', 'file_delete')          OR entity = 'file'),
    CHECK (action NOT IN ('password_reset', 'sessions_revoked')  OR entity = 'member'),
    CHECK (action NOT IN ('reattribute', 'reorder', 'privacy',
                          'materialize')                         OR entity = 'transaction'),

    -- a user action has an actor; a materializer or system one does not
    CHECK (source <> 'user' OR member_id IS NOT NULL),
    CHECK (period IS NULL OR (period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'
                              AND substr(period, 6, 2) BETWEEN '01' AND '12')),
    CHECK (undo_of IS NULL OR undo_of <> id)
);

CREATE INDEX idx_change_entity ON change(entity, entity_id, at DESC);
CREATE INDEX idx_change_rule   ON change(rule_id, at DESC) WHERE rule_id IS NOT NULL;
CREATE INDEX idx_change_at     ON change(at DESC);
CREATE INDEX idx_change_undo   ON change(undo_of)          WHERE undo_of IS NOT NULL;

CREATE TABLE change_field (
    id         INTEGER PRIMARY KEY,
    change_id  INTEGER NOT NULL REFERENCES change(id),
    field      TEXT    NOT NULL,      -- 'amount' | 'title' | 'bucket' | 'due_day' | ...
    old_value  TEXT,                  -- storage-neutral: minor units for money, ISO dates.
    new_value  TEXT,                  -- NULL renders as an em dash in the pane.

    -- `kind` travels WITH the value, so the pane formats old/new the way the
    -- app does without inferring from the field name — and a stored amount
    -- never depends on the household's *current* currency setting.
    kind       TEXT    NOT NULL,

    CHECK (kind IN ('money', 'date', 'day', 'bool', 'text', 'path')),
    CHECK (old_value IS NOT NULL OR new_value IS NOT NULL)   -- a no-op is not a field change
);

CREATE INDEX idx_change_field_change ON change_field(change_id);

-- Append-only, enforced by the schema rather than by discipline — the same
-- posture as household_settings' CHECK (id = 1). "Nothing in it is ever
-- rewritten or removed" is the entire value of an audit log; a rule that lives
-- only in prose is one careless UPDATE away from being untrue.
CREATE TRIGGER change_no_update BEFORE UPDATE ON change
  BEGIN SELECT RAISE(ABORT, 'change is append-only'); END;
CREATE TRIGGER change_no_delete BEFORE DELETE ON change
  BEGIN SELECT RAISE(ABORT, 'change is append-only'); END;
CREATE TRIGGER change_field_no_update BEFORE UPDATE ON change_field
  BEGIN SELECT RAISE(ABORT, 'change_field is append-only'); END;
CREATE TRIGGER change_field_no_delete BEFORE DELETE ON change_field
  BEGIN SELECT RAISE(ABORT, 'change_field is append-only'); END;
