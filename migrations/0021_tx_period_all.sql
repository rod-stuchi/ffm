-- ---------------------------------------------------------------------------
-- idx_tx_period covers deleted rows too.
-- ---------------------------------------------------------------------------
--
-- `u` and `U` find the month's steps among the entries on that month's rows
-- (`db::tx::next`), deleted rows included: undoing a `dd` is the commonest
-- step there is. Before, the stack was read from every entry the member had
-- ever made, a log that only grows; with the index stopping at live rows, the
-- month's rows would be a scan of every row the household ever had instead.
-- One index on the month serves both kinds of query, rather than a second one
-- beside it: a query that wants live rows only still filters `deleted_at`.

DROP INDEX idx_tx_period;
CREATE INDEX idx_tx_period ON transactions(period);
