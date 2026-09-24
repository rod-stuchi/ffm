-- ---------------------------------------------------------------------------
-- recurring_rules — what a rule's bill is BOUND to.
-- ---------------------------------------------------------------------------
--
-- A rule already carried `client_code`, the subscriber's account number read
-- off the bill's text. That alone cannot bind a rule whose issuer has no
-- profile — the text layer names nobody the app recognises — while the
-- barcode itself always says who is being paid: an arrecadação barcode carries
-- a four-digit company code in positions 16–19. So the binding is now:
--
--   client_code      the account, when a profile could read it   (wins)
--   barcode_company  the company, straight from the barcode       (fallback)
--   issuer_id        which profile recognised the bill, for display
--   barcode_kind     'consumo' | 'cobranca', for display
--
-- A later upload that disagrees with what is bound is refused before anything
-- is written — "wrong row?" — rather than stored and noted afterwards.
--
-- A cobrança barcode's bank code is deliberately NOT a binding: it names the
-- bank that issued the slip, and one bank issues slips for a hundred billers.
--
-- The whole apparatus is spend-only and absent for a "no barcode" rule, the
-- same CHECKs `client_code` already lives under in 0001.
--
-- (Also recorded here, since 0001's checksum is pinned: the vault folder is
-- now `{slug}`, not `{uuid}-{slug}`. Names are UNIQUE and cannot be changed,
-- so the uuid bought nothing but an unreadable path. `members.uuid` stays; the
-- startup step in services::vault moves existing folders.)
ALTER TABLE recurring_rules ADD COLUMN issuer_id TEXT;
ALTER TABLE recurring_rules ADD COLUMN barcode_company TEXT;
ALTER TABLE recurring_rules ADD COLUMN barcode_kind TEXT
    CHECK (barcode_kind IS NULL OR barcode_kind IN ('consumo', 'cobranca'));

CREATE TRIGGER recurring_rules_binding_spend_only
BEFORE UPDATE OF issuer_id, barcode_company, barcode_kind ON recurring_rules
WHEN (NEW.kind <> 'out' OR NEW.no_barcode = 1)
     AND (NEW.issuer_id IS NOT NULL OR NEW.barcode_company IS NOT NULL
          OR NEW.barcode_kind IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'a barcode binding belongs to a spend rule with a barcode');
END;
