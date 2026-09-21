-- ---------------------------------------------------------------------------
-- member_invites.created_by becomes nullable — NULL = issued from the console.
--
-- The one account with nobody above it is the sole admin's. Locked out of it,
-- there was no recovery but hand-editing the database, and password_hash is
-- Argon2, so there is no hash a person can type into sqlite3: you had to write
-- a program to get back into your own ledger.
--
-- `ffm reset-link <name>` closes that, and it deliberately issues the SAME
-- link :members issues rather than setting a password directly — so there
-- stays exactly one code path that writes password_hash, and the reset keeps
-- the accountability the web path has (every session revoked, two entries in
-- the append-only change log).
--
-- Which leaves created_by with nobody to name. The console operator is not a
-- member and inventing a row for them would be worse than the gap: NULL is the
-- truth, and it is the same truth the change log records as source = 'system'.
-- Whoever ran it had the database file in their hands, which is the authority
-- this grants and the only one it could.
--
-- SQLite cannot drop a NOT NULL in place, so the table is rebuilt. Nothing
-- references member_invites, so the drop orphans nothing; its two partial
-- indexes go with the old table and are recreated below, unchanged.
-- ---------------------------------------------------------------------------

CREATE TABLE member_invites_rebuilt (
    id                  INTEGER PRIMARY KEY,
    token_hash          TEXT    NOT NULL UNIQUE,      -- sha256 of 32 random bytes; the token itself is never stored
    member_id           INTEGER REFERENCES members(id),  -- NULL = invite; set = password reset for this member
    label               TEXT,                         -- admin-only note: which link is for whom (invites only)
    role                INTEGER NOT NULL DEFAULT 1,   -- the role the accepted member will get; ignored on a reset
    created_by          INTEGER REFERENCES members(id),  -- NULL = issued from the console, which has no member behind it
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
           OR accepted_member_id = member_id),
    -- only a reset can come from the console: an invite creates a member, and
    -- deciding who joins the household is the admin's, not the operator's.
    CHECK (created_by IS NOT NULL OR member_id IS NOT NULL)
);

INSERT INTO member_invites_rebuilt
    (id, token_hash, member_id, label, role, created_by, created_at,
     expires_at, accepted_at, accepted_member_id, revoked_at)
SELECT
     id, token_hash, member_id, label, role, created_by, created_at,
     expires_at, accepted_at, accepted_member_id, revoked_at
FROM member_invites;

DROP TABLE member_invites;
ALTER TABLE member_invites_rebuilt RENAME TO member_invites;

CREATE INDEX idx_invites_open ON member_invites(expires_at)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

CREATE UNIQUE INDEX idx_invites_open_reset ON member_invites(member_id)
    WHERE member_id IS NOT NULL AND accepted_at IS NULL AND revoked_at IS NULL;
