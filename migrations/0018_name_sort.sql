-- ---------------------------------------------------------------------------
-- member_settings.ledger_sort gains `name` — A–Z by the row's title.
-- ---------------------------------------------------------------------------
--
-- Four row orders now, and this is also the order `s` cycles them in:
--
--   manual  the order the household put the rows in — the default
--   name    A–Z by title, ignoring case and accents; ties keep manual order
--   amount  biggest first
--   date    paid on, or due on while unpaid
--
-- `name` is a derived view like the other two, so J/K refuse under it.
--
-- The allowed values are a table-level CHECK in 0001, and SQLite cannot alter
-- a CHECK in place, so the table is rebuilt — the same shape 0002 used for
-- member_invites. Nothing references member_settings and it carries no index
-- or trigger, so the drop orphans nothing. Every column is copied by name,
-- including the four added since 0001 (0006, 0008, 0011, 0015), each with its
-- own CHECK, unchanged.
-- ---------------------------------------------------------------------------

CREATE TABLE member_settings_rebuilt (
    member_id               INTEGER PRIMARY KEY REFERENCES members(id),

    -- language: words only. Dates stay dd/mm/yyyy and money follows the
    -- household currency, regardless of what this says.
    language                TEXT    NOT NULL DEFAULT 'en',

    -- appearance. The palette lives here; light/dark is per *device* and lives
    -- in the ffm.theme cookie, since the same person uses a bright laptop and a
    -- dark phone.
    theme_name              TEXT    NOT NULL DEFAULT 'ffm',
    theme_overrides         TEXT    NOT NULL DEFAULT '{}',      -- JSON: token -> color
    line_numbers            TEXT    NOT NULL DEFAULT 'relative',

    -- layout. It describes the screen you are on, not the household.
    -- `layout_min_split_width` is rendered into a single media query
    -- server-side — resolved in CSS, never in JS.
    layout_panes            TEXT    NOT NULL DEFAULT 'auto',
    layout_min_split_width  INTEGER NOT NULL DEFAULT 900,       -- px; below this, auto -> tabs
    layout_max_width        INTEGER NOT NULL DEFAULT 900,       -- px cap per pane; 0 = unlimited
    layout_gap              INTEGER NOT NULL DEFAULT 16,        -- px between panes

    -- ledger view. `sort` is safe per-member because J/K refuses to reorder a
    -- derived view.
    ledger_sort             TEXT    NOT NULL DEFAULT 'manual',
    ledger_search_scope     TEXT    NOT NULL DEFAULT 'pane',
    ledger_show_delta       INTEGER NOT NULL DEFAULT 1,
    ledger_group            TEXT    NOT NULL DEFAULT 'none',
    group_fold_default      TEXT    NOT NULL DEFAULT 'expanded',

    -- privacy. NULL = follow household_settings.default_visibility, which is
    -- the household's stated policy; a member may opt their own new rows out.
    default_visibility      TEXT,

    -- 0006: the floor a pane will not be squeezed below; 0 = no floor.
    layout_min_width        INTEGER NOT NULL DEFAULT 420
        CHECK (layout_min_width BETWEEN 0 AND 4000),
    -- 0008: how `:group bucket` orders its groups.
    group_order             TEXT    NOT NULL DEFAULT 'appearance'
        CHECK (group_order IN ('appearance', 'name', 'subtotal')),
    -- 0011: how much room the ledger's rows take.
    density                 TEXT    NOT NULL DEFAULT 'comfortable'
        CHECK (density IN ('default', 'comfortable', 'compact')),
    -- 0015: which side `P` opens a PDF on.
    pdf_view                TEXT    NOT NULL DEFAULT 'png'
        CHECK (pdf_view IN ('png', 'pdf')),

    CHECK (language               IN ('en', 'pt-BR')),
    CHECK (line_numbers           IN ('relative', 'absolute', 'none')),
    CHECK (layout_panes           IN ('auto', 'split', 'tabs')),
    CHECK (layout_min_split_width BETWEEN 320 AND 4000),
    CHECK (layout_max_width       >= 0),
    CHECK (layout_gap             BETWEEN 0 AND 64),
    CHECK (ledger_sort            IN ('manual', 'name', 'amount', 'date')),
    CHECK (ledger_search_scope    IN ('pane', 'household')),
    CHECK (ledger_show_delta      IN (0, 1)),
    CHECK (ledger_group           IN ('none', 'bucket')),
    CHECK (group_fold_default     IN ('expanded', 'collapsed')),
    CHECK (default_visibility     IS NULL OR default_visibility IN ('household', 'private')),
    CHECK (json_valid(theme_overrides))
);

INSERT INTO member_settings_rebuilt
    (member_id, language, theme_name, theme_overrides, line_numbers,
     layout_panes, layout_min_split_width, layout_max_width, layout_gap,
     ledger_sort, ledger_search_scope, ledger_show_delta, ledger_group,
     group_fold_default, default_visibility, layout_min_width, group_order,
     density, pdf_view)
SELECT
     member_id, language, theme_name, theme_overrides, line_numbers,
     layout_panes, layout_min_split_width, layout_max_width, layout_gap,
     ledger_sort, ledger_search_scope, ledger_show_delta, ledger_group,
     group_fold_default, default_visibility, layout_min_width, group_order,
     density, pdf_view
FROM member_settings;

DROP TABLE member_settings;
ALTER TABLE member_settings_rebuilt RENAME TO member_settings;
