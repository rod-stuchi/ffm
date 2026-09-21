-- ---------------------------------------------------------------------------
-- The `:grep` index.
--
-- `/` is client-side and scoped to the month already on screen: it filters
-- rows that are ALREADY rendered, which is what makes incremental search feel
-- instant. Searching EVERY month is a different verb, and this is what it
-- hits — deliberately not an overload of `/`, because one of them can answer
-- without a round trip and the other cannot.
--
-- An EXTERNAL-CONTENT index (`content = 'transactions'`): FTS5 keeps only what
-- it needs to search and reads the text back from the real table. That matters
-- for a private row — there is no second, full copy of its title sitting in a
-- table the privacy filter knows nothing about.
-- ---------------------------------------------------------------------------
CREATE VIRTUAL TABLE transactions_fts USING fts5(
    title,
    bucket,
    bill_name,
    content = 'transactions',
    content_rowid = 'id'
);

-- Kept in step by triggers rather than by remembering to write to it. A
-- soft-deleted row stays indexed on purpose: `u` puts it back, and a search
-- that could not find what you just deleted would be the wrong answer — the
-- privacy and deleted filters live in the QUERY, where every other read has
-- them too.
CREATE TRIGGER transactions_fts_insert AFTER INSERT ON transactions BEGIN
    INSERT INTO transactions_fts (rowid, title, bucket, bill_name)
    VALUES (new.id, new.title, new.bucket, new.bill_name);
END;

-- An external-content index is told to forget a row with its OLD values, not
-- with a DELETE statement: FTS5 has to un-index exactly the terms it indexed,
-- and it cannot read them back from a row that has already changed.
CREATE TRIGGER transactions_fts_update AFTER UPDATE ON transactions BEGIN
    INSERT INTO transactions_fts (transactions_fts, rowid, title, bucket, bill_name)
    VALUES ('delete', old.id, old.title, old.bucket, old.bill_name);
    INSERT INTO transactions_fts (rowid, title, bucket, bill_name)
    VALUES (new.id, new.title, new.bucket, new.bill_name);
END;

CREATE TRIGGER transactions_fts_delete AFTER DELETE ON transactions BEGIN
    INSERT INTO transactions_fts (transactions_fts, rowid, title, bucket, bill_name)
    VALUES ('delete', old.id, old.title, old.bucket, old.bill_name);
END;

-- Backfill whatever is already here, so an existing household can search its
-- own history the moment it upgrades.
INSERT INTO transactions_fts (rowid, title, bucket, bill_name)
SELECT id, title, bucket, bill_name FROM transactions;
