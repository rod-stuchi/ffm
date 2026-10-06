-- ---------------------------------------------------------------------------
-- What a rule's bill is recognized by, beyond its barcode — and a fingerprint
-- for the bills no barcode can tell apart.
-- ---------------------------------------------------------------------------
--
-- Nubank sometimes sends a fatura with no barcode at all (to push payment in
-- its app), and a rule bound only by `barcode_kind` had nothing to check such
-- a file against: it was refused as unreadable. A bill's TEXT does say who it
-- is from — the issuer's CNPJ — and, on some statements, which card. So the
-- binding grows:
--
--   issuer_id    (0007, display-only until now) — the profile a bill's CNPJ
--                detected. Now compared: a Nubank fatura on a rule bound to
--                Inter is another card's bill, barcode or not.
--   card_last4   the card's last four digits, read by the issuer's profile.
--                A statement can list several cards (the holder's and an
--                extra one), so a later bill AGREES when it lists the bound
--                card anywhere. Not read for an issuer whose printed digits
--                rotate (Nubank's per-purchase virtual cards).
--
-- Bound by the first bill that carries them, like everything else bound —
-- only what is empty is filled, never replaced.
--
-- And a bill that cannot be confirmed, or that contradicts the binding, is no
-- longer refused: it is a QUESTION, and only `y` attaches it anyway — noted in
-- the change log, and teaching the rule nothing after a contradiction.
--
-- `receipts.text_hash` — a bill file's text, reduced to its letters and
-- digits and hashed (`import::fingerprint`). Set on bill files a barcode
-- cannot identify: a text-only bill, or a card's payment slip that is the
-- same every month (0013). The same fingerprint on another live row is the
-- same bill twice, and refused. For comparison only; never shown.
ALTER TABLE recurring_rules ADD COLUMN card_last4 TEXT
    CHECK (card_last4 IS NULL OR (length(card_last4) = 4 AND card_last4 NOT GLOB '*[^0-9]*'));

-- The same rule 0007's trigger states for the rest of the binding: it belongs
-- to a spend rule with a barcode. 0007 is applied and pinned, so the new
-- column gets its own trigger rather than an edit there.
CREATE TRIGGER recurring_rules_card_spend_only
BEFORE UPDATE OF card_last4 ON recurring_rules
WHEN (NEW.kind <> 'out' OR NEW.no_barcode = 1) AND NEW.card_last4 IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a card binding belongs to a spend rule with a barcode');
END;

ALTER TABLE receipts ADD COLUMN text_hash TEXT;

CREATE INDEX idx_receipts_text_hash
    ON receipts(text_hash)
    WHERE text_hash IS NOT NULL AND deleted_at IS NULL;
