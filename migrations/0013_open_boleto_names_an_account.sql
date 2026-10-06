-- ---------------------------------------------------------------------------
-- idx_tx_barcode — a card's payment slip is not one bill.
-- ---------------------------------------------------------------------------
--
-- "The same boleto can only ever be one live row" (0001) assumed a barcode
-- names one bill. A credit-card fatura breaks that: its cobrança carries no
-- due factor and no amount (`… 3 00000000000000`) — the payer chooses how much
-- to pay — and an issuer like Inter prints the very same digits on every
-- month's fatura. January's then read as "already on 10/2026" and was refused,
-- which is not an error at all.
--
-- So a cobrança whose factor and value are both zero — positions 6–19 of the
-- 44 digits — is left out of the index: it names the account, not a bill, and
-- any number of rows may carry it. Every other barcode keeps the rule. The
-- same test lives in `boleto::names_an_account` and `trash::bill_held_by`;
-- keep the three in step.
DROP INDEX idx_tx_barcode;

CREATE UNIQUE INDEX idx_tx_barcode
    ON transactions(barcode_raw)
    WHERE barcode_raw IS NOT NULL AND deleted_at IS NULL
      AND NOT (barcode_kind = 'cobranca'
               AND substr(barcode_raw, 6, 14) = '00000000000000');
