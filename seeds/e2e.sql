-- What the browser tests shuffle: a made-up household holding every state a row
-- can be in. `ffm-anon --from` this, with a new --seed each run, gives the
-- random data the tests run on, so no real database has to go near a container.
--
-- Everyone and everything here is fictional. The content is Brazilian-household
-- Portuguese on purpose, like seeds/household.sql: it is what a household typed,
-- never app copy. Applied to an empty, migrated database; tests/e2e_seed.rs
-- fails when a migration stops it applying.
--
-- The dates matter and the tests lean on them. "Today" is 20/09/2026 unless a
-- test says otherwise (FFM_TODAY, set by e2e/fixtures.ts), so:
--   - on 03/09/2026 nothing tracked is overdue, and no month has a dot.
--   - by 20/09/2026 three are: Luz (due 11, nothing filed), Condomínio (due 05,
--     only scheduled) and IPTU (due 15, nothing filed).
--   - every rule starts in the first month it has a row here, because the
--     materializer fills any gap between a rule's start and today.
--
-- The barcodes are real in form: anon re-reads each with boleto::parse before
-- rebuilding it, so a code with a bad check digit stops the whole run.

INSERT INTO household_settings
    (id, name, locale_currency, default_language, default_visibility,
     ledger_show_recurring, assistant, created_at, updated_at)
VALUES (1, 'Lar Exemplo', 'BRL', 'en', 'household', 1, 'none',
        '2026-06-20T10:00:00Z', '2026-06-20T10:00:00Z');

-- A password_hash here is never used: anon sets every member's to its --password.
INSERT INTO members (id, uuid, name, slug, password_hash, role, created_at) VALUES
    (1, '11111111-1111-4111-8111-111111111111', 'helena', 'helena', '!e2e', 0, '2026-06-20T10:00:00Z'),
    (2, '22222222-2222-4222-8222-222222222222', 'otavio', 'otavio', '!e2e', 1, '2026-06-20T10:05:00Z'),
    (3, '33333333-3333-4333-8333-333333333333', 'lia',    'lia',    '!e2e', 1, '2026-06-20T10:10:00Z');

INSERT INTO member_settings (member_id, language, theme_name) VALUES
    (1, 'en',    'ffm'),
    (2, 'en',    'rosepine'),
    (3, 'pt-BR', 'tokyonight');

-- --- rules ------------------------------------------------------------------

INSERT INTO recurring_rules
    (id, member_id, title, kind, amount_cents, cadence_months, due_day, start_date, end_date,
     total_installments, carry_forward_amount, tracked, bucket, bill_name, no_barcode,
     client_code, issuer_id, barcode_kind, barcode_company, spans_period, window_size,
     window_start, varies, months, visibility, created_at, deleted_at)
VALUES
    -- Tracked, with a bill: paid, paid, then overdue with nothing filed.
    (1, 1, 'Luz · Aurora Energia', 'out', 21870, 1, 11, '2026-07-01', NULL, NULL, 1, 1, 'Contas', 'Luz', 0,
     '00123456', '11222333000181', 'cobranca', NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:20:00Z', NULL),
    -- Tracked, a utility's code: paid, then not due yet with its bill on file.
    (2, 1, 'Saneamento · Fonte Clara', 'out', 9840, 1, 25, '2026-08-01', NULL, NULL, 1, 1, 'Contas', 'Saneamento', 0,
     NULL, NULL, 'consumo', '0147', 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:21:00Z', NULL),
    -- Tracked, paid by wire: paid, then scheduled and overdue.
    (3, 1, 'Condomínio · Vila Serena', 'out', 64000, 1, 5, '2026-08-01', NULL, NULL, 1, 1, 'Moradia', 'Condominio', 1,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:22:00Z', NULL),
    -- Untracked: the manual tick.
    (4, 1, 'Internet · Nuvem Fibra', 'out', 11990, 1, 20, '2026-07-01', NULL, NULL, 1, 0, 'Contas', 'Internet', 1,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:23:00Z', NULL),
    (5, 1, 'Salário', 'in', 780000, 1, 5, '2026-07-01', NULL, NULL, 1, 0, 'Renda', 'Salario', 0,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:24:00Z', NULL),
    -- A finite run.
    (6, 1, 'Financiamento · Sedã', 'out', 75000, 1, 8, '2026-07-01', '2027-06-08', 12, 1, 0, 'Dívidas', 'Veiculo', 1,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:25:00Z', NULL),
    -- Seasonal, and one file covering several months.
    (7, 1, 'IPTU · Prefeitura', 'out', 18230, 1, 15, '2026-07-01', NULL, NULL, 1, 1, 'Impostos', 'IPTU', 0,
     NULL, NULL, 'cobranca', NULL, 1, 4, '2026-07', 0, '1,2,3,4,5,6,7,8,9,10', 'household', '2026-06-20T10:26:00Z', NULL),
    -- Income that varies: the projection is computed on read.
    (8, 2, 'Consultoria', 'in', 350000, 1, 15, '2026-07-01', NULL, NULL, 0, 0, 'Renda', 'Consultoria', 0,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 1, NULL, 'household', '2026-06-20T10:27:00Z', NULL),
    -- Tracked on another member, with files of every extension and one in the trash.
    (9, 2, 'Academia · Corpo Leve', 'out', 12900, 1, 6, '2026-07-01', NULL, NULL, 1, 1, 'Bem-estar', 'Academia', 1,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:28:00Z', NULL),
    (10, 3, 'Mesada', 'in', 20000, 1, 1, '2026-08-01', NULL, NULL, 1, 0, 'Renda', 'Mesada', 0,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:29:00Z', NULL),
    -- A deleted rule, whose rows stay as orphans.
    (11, 1, 'Streaming · Cinemax Plus', 'out', 5590, 1, 12, '2026-07-01', NULL, NULL, 1, 0, 'Lazer', 'Streaming', 1,
     NULL, NULL, NULL, NULL, 0, NULL, NULL, 0, NULL, 'household', '2026-06-20T10:30:00Z', '2026-08-30T12:00:00Z');

-- --- transactions -----------------------------------------------------------

INSERT INTO transactions
    (id, member_id, rule_id, period, kind, title, amount_cents, date, due_date, manual_paid,
     visibility, bucket, bill_name, sort_key, barcode_raw, barcode_kind, amount_source,
     due_date_source, issuer_id, created_at, created_by, deleted_at, deleted_by)
VALUES
    -- Helena, July
    (1,  1, 5,    '2026-07', 'in',  'Salário',                  780000, '2026-07-05', '2026-07-05', 1, 'household', 'Renda',    'Salario',    'a', NULL, NULL, NULL, NULL, NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (2,  1, 1,    '2026-07', 'out', 'Luz · Aurora Energia',      21870, '2026-07-10', '2026-07-11', 0, 'household', 'Contas',   'Luz',        'b', '34191150400000218700700100000000000000000001', 'cobranca', 'barcode', 'fator', '11222333000181', '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (3,  1, 4,    '2026-07', 'out', 'Internet · Nuvem Fibra',    11990, '2026-07-20', '2026-07-20', 1, 'household', 'Contas',   'Internet',   'c', NULL, NULL, NULL, NULL, NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (4,  1, 6,    '2026-07', 'out', 'Financiamento · Sedã',      75000, '2026-07-08', '2026-07-08', 1, 'household', 'Dívidas',  'Veiculo',    'd', NULL, NULL, NULL, NULL, NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (5,  1, 7,    '2026-07', 'out', 'IPTU · Prefeitura',         18230, '2026-07-14', '2026-07-15', 0, 'household', 'Impostos', 'IPTU',       'e', '10491150800000182300800200000000000000000007', 'cobranca', 'barcode', 'fator', NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (6,  1, 11,   '2026-07', 'out', 'Streaming · Cinemax Plus',   5590, '2026-07-12', '2026-07-12', 1, 'household', 'Lazer',    'Streaming',  'f', NULL, NULL, NULL, NULL, NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (7,  1, NULL, '2026-07', 'out', 'Mercado · Bom Preço',       61250, '2026-07-18', NULL,         0, 'household', 'Mercado',  NULL,         'g', NULL, NULL, NULL, NULL, NULL, '2026-07-18T21:00:00Z', 1, NULL, NULL),

    -- Helena, August
    (10, 1, 5,    '2026-08', 'in',  'Salário',                  780000, '2026-08-05', '2026-08-05', 1, 'household', 'Renda',    'Salario',    'a', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (11, 1, 1,    '2026-08', 'out', 'Luz · Aurora Energia',      24315, '2026-08-10', '2026-08-11', 0, 'household', 'Contas',   'Luz',        'b', '34193153500000243150700100000000000000000002', 'cobranca', 'barcode', 'fator', '11222333000181', '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (12, 1, 2,    '2026-08', 'out', 'Saneamento · Fonte Clara',   9840, '2026-08-24', '2026-08-25', 0, 'household', 'Contas',   'Saneamento', 'c', '83610000000984001472026082500000000000010001', 'consumo', 'barcode', 'free_field', NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (13, 1, 3,    '2026-08', 'out', 'Condomínio · Vila Serena',  64000, '2026-08-05', '2026-08-05', 0, 'household', 'Moradia',  'Condominio', 'd', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (14, 1, 4,    '2026-08', 'out', 'Internet · Nuvem Fibra',    11990, '2026-08-20', '2026-08-20', 1, 'household', 'Contas',   'Internet',   'e', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (15, 1, 6,    '2026-08', 'out', 'Financiamento · Sedã',      75000, '2026-08-08', '2026-08-08', 1, 'household', 'Dívidas',  'Veiculo',    'f', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (16, 1, 7,    '2026-08', 'out', 'IPTU · Prefeitura',         18230, '2026-08-14', '2026-08-15', 0, 'household', 'Impostos', 'IPTU',       'g', '10499153900000182300800200000000000000000008', 'cobranca', 'barcode', 'fator', NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (17, 1, 11,   '2026-08', 'out', 'Streaming · Cinemax Plus',   5590, '2026-08-12', '2026-08-12', 1, 'household', 'Lazer',    'Streaming',  'h', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (18, 1, NULL, '2026-08', 'out', 'Mercado · Bom Preço',       58400, '2026-08-16', NULL,         0, 'household', 'Mercado',  NULL,         'i', NULL, NULL, NULL, NULL, NULL, '2026-08-16T21:00:00Z', 1, NULL, NULL),

    -- Helena, September: a good spread of marks, on the month the tests open on.
    (20, 1, 5,    '2026-09', 'in',  'Salário',                  780000, '2026-09-05', '2026-09-05', 1, 'household', 'Renda',    'Salario',    'a', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (21, 1, 1,    '2026-09', 'out', 'Luz · Aurora Energia',      26790, NULL,         '2026-09-11', 0, 'household', 'Contas',   'Luz',        'b', '34196156600000267900700100000000000000000003', 'cobranca', 'barcode', 'fator', '11222333000181', '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (22, 1, 2,    '2026-09', 'out', 'Saneamento · Fonte Clara',  10215, NULL,         '2026-09-25', 0, 'household', 'Contas',   'Saneamento', 'c', '83630000001021501472026092500000000000010002', 'consumo', 'barcode', 'free_field', NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (23, 1, 3,    '2026-09', 'out', 'Condomínio · Vila Serena',  64000, NULL,         '2026-09-05', 0, 'household', 'Moradia',  'Condominio', 'd', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (24, 1, 4,    '2026-09', 'out', 'Internet · Nuvem Fibra',    11990, NULL,         '2026-09-20', 0, 'household', 'Contas',   'Internet',   'e', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (25, 1, 6,    '2026-09', 'out', 'Financiamento · Sedã',      75000, '2026-09-08', '2026-09-08', 1, 'household', 'Dívidas',  'Veiculo',    'f', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (26, 1, 7,    '2026-09', 'out', 'IPTU · Prefeitura',         18230, NULL,         '2026-09-15', 0, 'household', 'Impostos', 'IPTU',       'g', '10496157000000182300800200000000000000000009', 'cobranca', 'barcode', 'fator', NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (27, 1, NULL, '2026-09', 'out', 'Mercado · Bom Preço',       54320, '2026-09-13', NULL,         0, 'household', 'Mercado',  NULL,         'h', NULL, NULL, NULL, NULL, NULL, '2026-09-13T21:00:00Z', 1, NULL, NULL),
    (28, 1, NULL, '2026-09', 'out', 'Presente · surpresa',       23000, '2026-09-16', NULL,         0, 'private',   'Presentes', NULL,        'i', NULL, NULL, NULL, NULL, NULL, '2026-09-16T12:00:00Z', 1, NULL, NULL),
    -- Deleted with `dd`, and undoable from the trash.
    (29, 1, NULL, '2026-09', 'out', 'Padaria · Trigo Dourado',    3150, '2026-09-14', NULL,         0, 'household', 'Mercado',  NULL,         'j', NULL, NULL, NULL, NULL, NULL, '2026-09-14T08:00:00Z', 1, '2026-09-14T09:00:00Z', 1),

    -- Otávio
    (30, 2, 8,    '2026-07', 'in',  'Consultoria',              310000, '2026-07-15', '2026-07-15', 1, 'household', 'Renda',     'Consultoria', 'a', NULL, NULL, NULL, NULL, NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (31, 2, 9,    '2026-07', 'out', 'Academia · Corpo Leve',     12900, '2026-07-06', '2026-07-06', 0, 'household', 'Bem-estar', 'Academia',    'b', NULL, NULL, NULL, NULL, NULL, '2026-07-01T03:00:00Z', NULL, NULL, NULL),
    (32, 2, 8,    '2026-08', 'in',  'Consultoria',              365000, '2026-08-15', '2026-08-15', 1, 'household', 'Renda',     'Consultoria', 'a', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (33, 2, 9,    '2026-08', 'out', 'Academia · Corpo Leve',     12900, '2026-08-06', '2026-08-06', 0, 'household', 'Bem-estar', 'Academia',    'b', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    -- Has not landed: an estimate.
    (34, 2, 8,    '2026-09', 'in',  'Consultoria',              350000, NULL,         '2026-09-15', 0, 'household', 'Renda',     'Consultoria', 'a', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (35, 2, 9,    '2026-09', 'out', 'Academia · Corpo Leve',     12900, '2026-09-06', '2026-09-06', 0, 'household', 'Bem-estar', 'Academia',    'b', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (36, 2, NULL, '2026-09', 'out', 'Farmácia · Vida Plena',      8740, '2026-09-10', NULL,         0, 'household', 'Bem-estar', NULL,          'c', NULL, NULL, NULL, NULL, NULL, '2026-09-10T18:00:00Z', 2, NULL, NULL),
    -- Hidden from Lia and from Helena's non-admin preview, never from Helena.
    (37, 2, NULL, '2026-09', 'out', 'Clínica · Bem Cuidar',      32000, '2026-09-12', NULL,         0, 'private',   'Bem-estar', NULL,          'd', NULL, NULL, NULL, NULL, NULL, '2026-09-12T15:00:00Z', 2, NULL, NULL),

    -- Lia
    (40, 3, 10,   '2026-08', 'in',  'Mesada',                    20000, '2026-08-01', '2026-08-01', 1, 'household', 'Renda',  'Mesada', 'a', NULL, NULL, NULL, NULL, NULL, '2026-08-01T03:00:00Z', NULL, NULL, NULL),
    (41, 3, 10,   '2026-09', 'in',  'Mesada',                    20000, '2026-09-01', '2026-09-01', 1, 'household', 'Renda',  'Mesada', 'a', NULL, NULL, NULL, NULL, NULL, '2026-09-01T03:00:00Z', NULL, NULL, NULL),
    (42, 3, NULL, '2026-09', 'out', 'Cantina · Escola Nova',      4200, '2026-09-10', NULL,         0, 'household', 'Escola', NULL,     'b', NULL, NULL, NULL, NULL, NULL, '2026-09-10T13:30:00Z', 3, NULL, NULL);

-- --- files ------------------------------------------------------------------
-- anon writes a stand-in for each row here. Each path is what receipts::slot_of
-- computes; one that disagreed would be moved by the refile pass at startup.

INSERT INTO receipts
    (id, transaction_id, kind, path, ext, original_filename, uploaded_by, uploaded_at,
     deleted_at, deleted_by, delete_reason, deleted_from_path)
VALUES
    (1,  2,  'barcode',   'helena/contas/luz/2026/07_barcode.pdf',          'pdf', 'aurora-julho.pdf',     1, '2026-07-02T10:00:00Z', NULL, NULL, NULL, NULL),
    (2,  2,  'receipt',   'helena/contas/luz/2026/07.pdf',                  'pdf', 'comprovante-1.pdf',    1, '2026-07-10T10:00:00Z', NULL, NULL, NULL, NULL),
    (3,  11, 'barcode',   'helena/contas/luz/2026/08_barcode.pdf',          'pdf', 'aurora-agosto.pdf',    1, '2026-08-02T10:00:00Z', NULL, NULL, NULL, NULL),
    (4,  11, 'receipt',   'helena/contas/luz/2026/08.pdf',                  'pdf', 'comprovante-2.pdf',    1, '2026-08-10T10:00:00Z', NULL, NULL, NULL, NULL),
    (5,  21, 'barcode',   'helena/contas/luz/2026/09_barcode.pdf',          'pdf', 'aurora-setembro.pdf',  1, '2026-09-02T10:00:00Z', NULL, NULL, NULL, NULL),
    (6,  12, 'barcode',   'helena/contas/saneamento/2026/08_barcode.pdf',   'pdf', 'fonte-agosto.pdf',     1, '2026-08-02T10:00:00Z', NULL, NULL, NULL, NULL),
    (7,  12, 'receipt',   'helena/contas/saneamento/2026/08.pdf',           'pdf', 'comprovante-3.pdf',    1, '2026-08-24T10:00:00Z', NULL, NULL, NULL, NULL),
    (8,  22, 'barcode',   'helena/contas/saneamento/2026/09_barcode.pdf',   'pdf', 'fonte-setembro.pdf',   1, '2026-09-02T10:00:00Z', NULL, NULL, NULL, NULL),
    (9,  13, 'receipt',   'helena/moradia/condominio/2026/08.pdf',          'pdf', 'pix-agosto.pdf',       1, '2026-08-05T10:00:00Z', NULL, NULL, NULL, NULL),
    (10, 23, 'scheduled', 'helena/moradia/condominio/2026/09_scheduled.pdf', 'pdf', 'agendado.pdf',        1, '2026-09-01T10:00:00Z', NULL, NULL, NULL, NULL),
    -- One file for four months: kept on July's row, read through rule_bill_code.
    (11, 5,  'barcode',   'helena/impostos/iptu/2026/07_barcode.pdf',       'pdf', 'carne-iptu.pdf',       1, '2026-07-02T10:00:00Z', NULL, NULL, NULL, NULL),
    (12, 5,  'receipt',   'helena/impostos/iptu/2026/07.pdf',               'pdf', 'comprovante-4.pdf',    1, '2026-07-14T10:00:00Z', NULL, NULL, NULL, NULL),
    (13, 16, 'receipt',   'helena/impostos/iptu/2026/08.pdf',               'pdf', 'comprovante-5.pdf',    1, '2026-08-14T10:00:00Z', NULL, NULL, NULL, NULL),
    -- Otávio: one of each extension.
    (14, 31, 'receipt',   'otavio/bem-estar/academia/2026/07.txt',          'txt', 'recibo.txt',           2, '2026-07-06T10:00:00Z', NULL, NULL, NULL, NULL),
    (15, 33, 'receipt',   'otavio/bem-estar/academia/2026/08.png',          'png', 'recibo.png',           2, '2026-08-06T10:00:00Z', NULL, NULL, NULL, NULL),
    (16, 35, 'receipt',   'otavio/bem-estar/academia/2026/09.pdf',          'pdf', 'recibo.pdf',           2, '2026-09-06T10:00:00Z', NULL, NULL, NULL, NULL),
    -- In the trash: replaced by the one above it, and restorable.
    (17, 31, 'receipt',   'otavio/bem-estar/academia/2026/07.txt',          'txt', 'recibo-errado.txt',    2, '2026-07-05T10:00:00Z',
     '2026-07-06T10:00:00Z', 2, 'superseded', 'otavio/bem-estar/academia/2026/07.txt');

INSERT INTO rule_bill_code
    (id, rule_id, receipt_id, period, barcode_raw, kind, amount_cents, due_date, page, created_at)
VALUES
    (1, 7, 11, '2026-07', '10491150800000182300800200000000000000000007', 'cobranca', 18230, '2026-07-15', 1, '2026-07-02T10:00:00Z'),
    (2, 7, 11, '2026-08', '10499153900000182300800200000000000000000008', 'cobranca', 18230, '2026-08-15', 2, '2026-07-02T10:00:00Z'),
    (3, 7, 11, '2026-09', '10496157000000182300800200000000000000000009', 'cobranca', 18230, '2026-09-15', 3, '2026-07-02T10:00:00Z'),
    (4, 7, 11, '2026-10', '10491160000000182300800200000000000000000010', 'cobranca', 18230, '2026-10-15', 4, '2026-07-02T10:00:00Z');

-- --- change log -------------------------------------------------------------
-- A little of what `gl` shows. All of it older than anything a test does, so `u`
-- in a test reverses the test's own change first.

INSERT INTO change (id, at, member_id, source, action, entity, entity_id, rule_id, period, note) VALUES
    (1, '2026-06-20T10:20:00Z', 1, 'user', 'rule_create', 'rule', 1, 1, NULL, NULL),
    (2, '2026-09-13T21:00:00Z', 1, 'user', 'create', 'transaction', 27, NULL, '2026-09', NULL),
    (3, '2026-09-13T21:05:00Z', 1, 'user', 'edit', 'transaction', 27, NULL, '2026-09', NULL),
    (4, '2026-09-14T08:00:00Z', 1, 'user', 'create', 'transaction', 29, NULL, '2026-09', NULL),
    (5, '2026-09-14T09:00:00Z', 1, 'user', 'delete', 'transaction', 29, NULL, '2026-09', NULL),
    (6, '2026-08-10T10:00:00Z', 1, 'user', 'file_upload', 'file', 4, 1, '2026-08', NULL),
    (7, '2026-07-06T10:00:00Z', 2, 'user', 'file_delete', 'file', 17, 9, '2026-07', 'superseded');

INSERT INTO change_field (id, change_id, field, old_value, new_value, kind) VALUES
    (1, 3, 'amount', '50000', '54320', 'money');
