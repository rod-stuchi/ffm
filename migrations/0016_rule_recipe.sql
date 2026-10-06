-- ---------------------------------------------------------------------------
-- Recipes: how ONE rule's bills are read — taught by an assistant, approved by
-- a person, kept per rule. Replaces config/issuers.toml.
-- ---------------------------------------------------------------------------
--
-- The issuer profiles were a hand-written file: every card or biller whose
-- barcode leaves a hole (no amount, no barcode at all) needed somebody to find
-- its CNPJ and write regexes, and half the shipped ones were placeholders.
-- Now a rule learns how to read its own bills on its first one:
--
--   1. the household's assistant (a CLI on the server — `codex`, `claude`,
--      `gemini` — running on the household's own subscription) reads the
--      bill's first page, redacted, and writes the rule's RECIPE: who issued
--      it, the CNPJ, and patterns for the amount, the due date, the account
--      and the card;
--   2. the app checks every pattern against the very text it was written for
--      — a pattern that does not find the value it claims is dropped, never
--      offered;
--   3. a person approves it (`y`), and from then on every month is read
--      LOCALLY, with no assistant at all. Only when the recipe stops reading
--      (a redesigned layout) is the assistant asked again — and its fix
--      approved again.
--
-- History is kept: approving a fix RETIRES the live recipe rather than
-- overwriting it, so "how was March read?" stays answerable. One live recipe
-- per rule, by a partial unique index.
CREATE TABLE rule_recipe (
    id            INTEGER PRIMARY KEY,
    rule_id       INTEGER NOT NULL REFERENCES recurring_rules(id),
    created_at    TEXT    NOT NULL,
    -- Who approved it. NULL for one converted from config/issuers.toml at
    -- startup, which nobody approved in this database.
    created_by    INTEGER REFERENCES members(id),
    source        TEXT    NOT NULL,
    -- The issuer, as the bill names it: the CNPJ's 14 digits and a name for
    -- display. The CNPJ is also bound on the rule (`issuer_id`), where a
    -- later bill is checked against it.
    cnpj          TEXT,
    issuer_name   TEXT,
    -- One capture group each. NULL where the bill does not print it, or the
    -- barcode always carries it.
    amount_regex  TEXT,
    due_regex     TEXT,
    client_regex  TEXT,
    card_regex    TEXT,
    retired_at    TEXT,

    CHECK (source IN ('codex', 'claude', 'gemini', 'migrated', 'manual')),
    CHECK (cnpj IS NULL OR (length(cnpj) = 14 AND cnpj NOT GLOB '*[^0-9]*'))
);

CREATE UNIQUE INDEX idx_rule_recipe_live
    ON rule_recipe(rule_id)
    WHERE retired_at IS NULL;

-- Which assistant teaches the recipes — household-wide and admin-only, like
-- the currency: it runs on the server, as the server's user, on the
-- subscription the operator logged it into. `none` works too: what the app
-- cannot read, a person types.
ALTER TABLE household_settings
    ADD COLUMN assistant TEXT NOT NULL DEFAULT 'codex'
    CHECK (assistant IN ('none', 'codex', 'claude', 'gemini'));
