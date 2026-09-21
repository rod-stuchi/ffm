-- A household to look at while there is no /setup page yet.
--
-- The content is Brazilian-household Portuguese on purpose — "Água · Sabesp",
-- "Mesada", "Consumo". That is what a household typed into its own ledger, not
-- app copy: it never passes through the translation catalog, and it stays
-- Portuguese when the UI language is English.
--
-- NOBODY CAN SIGN IN AS THESE MEMBERS. password_hash is NOT NULL, so it has to
-- hold something; what it holds is deliberately not a valid PHC string, so
-- argon2 verification cannot succeed against it by any password. Real accounts
-- arrive with /setup and /invite, where the person sets their own.

DELETE FROM change_field;
DELETE FROM change;
DELETE FROM receipts;
DELETE FROM transactions;
DELETE FROM recurring_rules;
DELETE FROM member_settings;
DELETE FROM sessions;
DELETE FROM member_invites;
DELETE FROM members;
DELETE FROM household_settings;

INSERT INTO household_settings
    (id, name, locale_currency, default_language, default_visibility,
     ledger_show_recurring, created_at, updated_at)
VALUES (1, 'Casa Stuchi', 'BRL', 'pt-BR', 'household', 1,
        '2026-01-04T10:00:00Z', '2026-01-04T10:00:00Z');

INSERT INTO members (id, uuid, name, slug, password_hash, role, created_at) VALUES
    (1, '7c3f1a20-0f1c-4a55-9a1b-2f0c9d5e1a01', 'Rodrigo', 'rodrigo', '!seed-cannot-sign-in', 0, '2026-01-04T10:00:00Z'),
    (2, '4b8e2d31-6a77-4b12-8c90-1d7e3f4a2b02', 'Natália', 'natália', '!seed-cannot-sign-in', 1, '2026-01-04T10:05:00Z'),
    (3, '9f1c7e42-2b53-4c88-b7a3-5e6d8c9f3c03', 'Miguel',  'miguel',  '!seed-cannot-sign-in', 1, '2026-02-11T19:20:00Z');

INSERT INTO member_settings (member_id, language, theme_name) VALUES
    (1, 'pt-BR', 'ffm'),
    (2, 'pt-BR', 'rosepine'),
    (3, 'en',    'tokyonight');

-- --- recurring rules -------------------------------------------------------
-- Tracked rules exist here, but nothing is on disk yet: the vault arrives with
-- the receipts phase. Until then a tracked occurrence honestly reads `pending`
-- rather than claiming a file that is not there.

INSERT INTO recurring_rules
    (id, member_id, title, kind, amount_cents, cadence_months, due_day,
     start_date, end_date, total_installments, carry_forward_amount, tracked,
     bucket, bill_name, instance, no_barcode, varies, visibility, created_at)
VALUES
    (1, 1, 'Energia · Enel SP',      'out',  34362, 1, 11, '2026-01-01', NULL, NULL, 1, 1, 'Consumo', 'Energia', NULL, 0, 0, 'household', '2026-01-04T10:10:00Z'),
    (2, 1, 'Água · Sabesp',          'out',  10840, 1, 18, '2026-01-01', NULL, NULL, 1, 1, 'Consumo', 'Água',    NULL, 0, 0, 'household', '2026-01-04T10:11:00Z'),
    (3, 1, 'Internet · Vivo Fibra',  'out',  12990, 1, 20, '2026-01-01', NULL, NULL, 1, 0, 'Casa',    'Internet',NULL, 1, 0, 'household', '2026-01-04T10:12:00Z'),
    -- A finite run: the car, twelve installments, ending on its own.
    (4, 1, 'Financiamento · Carro',  'out',  89700, 1,  5, '2026-03-01', '2027-02-05', 12, 1, 0, 'Dívidas', 'Carro', NULL, 1, 0, 'household', '2026-03-01T09:00:00Z'),
    (5, 1, 'Salário',                'in',  980000, 1,  5, '2026-01-01', NULL, NULL, 1, 0, 'Renda',   'Salário', NULL, 0, 0, 'household', '2026-01-04T10:15:00Z'),
    -- Income that varies: the projection is computed on read, never stored.
    (6, 2, 'Consultoria',            'in',  420000, 1, 15, '2026-01-01', NULL, NULL, 0, 0, 'Renda',   'Consultoria', NULL, 0, 1, 'household', '2026-01-04T10:20:00Z'),
    (7, 2, 'Academia · Smart Fit',   'out',   9990, 1,  8, '2026-01-01', NULL, NULL, 1, 0, 'Saúde',   'Academia', NULL, 1, 0, 'household', '2026-01-04T10:21:00Z'),
    (8, 3, 'Mesada',                 'in',   15000, 1,  1, '2026-02-01', NULL, NULL, 1, 0, 'Renda',   'Mesada',  NULL, 0, 0, 'household', '2026-02-11T19:25:00Z');

-- --- transactions ----------------------------------------------------------
-- Two months, so the month-over-month delta has something real to compare
-- against: a row's delta is derived from "the same line" — same member, same
-- title — in the previous period, never stored.
--
-- sort_key is a fractional index, not a position: moving a row between two
-- neighbours writes ONE row (the midpoint), so two people reordering at once
-- do not renumber each other's ledger.

INSERT INTO transactions
    (member_id, rule_id, period, kind, title, amount_cents, date, due_date,
     manual_paid, visibility, bucket, bill_name, sort_key, created_at, created_by)
VALUES
    -- August, Rodrigo
    (1, 5, '2026-08', 'in',  'Salário',              980000, '2026-08-05', '2026-08-05', 1, 'household', 'Renda',   'Salário',  'a0', '2026-08-01T03:00:00Z', NULL),
    (1, 1, '2026-08', 'out', 'Energia · Enel SP',     28140, '2026-08-11', '2026-08-11', 0, 'household', 'Consumo', 'Energia',  'b0', '2026-08-01T03:00:00Z', NULL),
    (1, 2, '2026-08', 'out', 'Água · Sabesp',         11520, '2026-08-18', '2026-08-18', 0, 'household', 'Consumo', 'Água',     'c0', '2026-08-01T03:00:00Z', NULL),
    (1, 3, '2026-08', 'out', 'Internet · Vivo Fibra', 12990, '2026-08-20', '2026-08-20', 1, 'household', 'Casa',    'Internet', 'd0', '2026-08-01T03:00:00Z', NULL),
    (1, 4, '2026-08', 'out', 'Financiamento · Carro', 89700, '2026-08-05', '2026-08-05', 1, 'household', 'Dívidas', 'Carro',    'e0', '2026-08-01T03:00:00Z', NULL),
    (1, NULL, '2026-08', 'out', 'Mercado · Pão de Açúcar', 70090, '2026-08-13', NULL, 0, 'household', 'Mercado', NULL, 'f0', '2026-08-13T22:10:00Z', 1),

    -- September, Rodrigo. Energia is up on August, Mercado is down — both
    -- deltas fall out of the previous period rather than being stored.
    (1, 5, '2026-09', 'in',  'Salário',              980000, '2026-09-05', '2026-09-05', 1, 'household', 'Renda',   'Salário',  'a0', '2026-09-01T03:00:00Z', NULL),
    (1, 1, '2026-09', 'out', 'Energia · Enel SP',     34362, NULL,         '2026-09-11', 0, 'household', 'Consumo', 'Energia',  'b0', '2026-09-01T03:00:00Z', NULL),
    (1, 2, '2026-09', 'out', 'Água · Sabesp',         10840, NULL,         '2026-09-18', 0, 'household', 'Consumo', 'Água',     'c0', '2026-09-01T03:00:00Z', NULL),
    (1, 3, '2026-09', 'out', 'Internet · Vivo Fibra', 12990, '2026-09-20', '2026-09-20', 1, 'household', 'Casa',    'Internet', 'd0', '2026-09-01T03:00:00Z', NULL),
    (1, 4, '2026-09', 'out', 'Financiamento · Carro', 89700, '2026-09-05', '2026-09-05', 1, 'household', 'Dívidas', 'Carro',    'e0', '2026-09-01T03:00:00Z', NULL),
    (1, NULL, '2026-09', 'out', 'Mercado · Pão de Açúcar', 65890, '2026-09-13', NULL, 0, 'household', 'Mercado', NULL, 'f0', '2026-09-13T21:40:00Z', 1),
    -- Opt-in per transaction, and hidden from everyone but its owner and the
    -- admin — who in this household happen to be the same person.
    (1, NULL, '2026-09', 'out', 'Presente · aniversário', 24000, '2026-09-16', NULL, 0, 'private', 'Presentes', NULL, 'g0', '2026-09-16T12:05:00Z', 1),

    -- August, Natália
    (2, 6, '2026-08', 'in',  'Consultoria',          380000, '2026-08-15', '2026-08-15', 1, 'household', 'Renda', 'Consultoria', 'a0', '2026-08-01T03:00:00Z', NULL),
    (2, 7, '2026-08', 'out', 'Academia · Smart Fit',   9990, '2026-08-08', '2026-08-08', 1, 'household', 'Saúde', 'Academia',    'b0', '2026-08-01T03:00:00Z', NULL),

    -- September, Natália. The consultoria has not landed yet: `expected`, and
    -- once phase 8 lands it will show a projection rather than its baseline.
    (2, 6, '2026-09', 'in',  'Consultoria',          420000, NULL,         '2026-09-15', 0, 'household', 'Renda', 'Consultoria', 'a0', '2026-09-01T03:00:00Z', NULL),
    (2, 7, '2026-09', 'out', 'Academia · Smart Fit',   9990, '2026-09-08', '2026-09-08', 1, 'household', 'Saúde', 'Academia',    'b0', '2026-09-01T03:00:00Z', NULL),
    (2, NULL, '2026-09', 'out', 'Farmácia · Drogasil', 8740, '2026-09-19', NULL, 0, 'household', 'Saúde', NULL, 'c0', '2026-09-19T18:00:00Z', 2),

    -- Miguel
    (3, 8, '2026-08', 'in',  'Mesada', 15000, '2026-08-01', '2026-08-01', 1, 'household', 'Renda', 'Mesada', 'a0', '2026-08-01T03:00:00Z', NULL),
    (3, 8, '2026-09', 'in',  'Mesada', 15000, '2026-09-01', '2026-09-01', 1, 'household', 'Renda', 'Mesada', 'a0', '2026-09-01T03:00:00Z', NULL),
    (3, NULL, '2026-09', 'out', 'Cantina da escola', 4200, '2026-09-10', NULL, 0, 'household', 'Escola', NULL, 'b0', '2026-09-10T13:30:00Z', 3);
