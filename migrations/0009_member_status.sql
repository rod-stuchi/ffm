-- ---------------------------------------------------------------------------
-- member_invites.token and member_inactive — `:members` can copy a pending
-- link again, and turn a member off without erasing them.
-- ---------------------------------------------------------------------------
--
-- member_invites.token: the raw token, kept ONLY while the link is pending,
-- and set to NULL the moment it is accepted, revoked or expires. This reverses
-- "the token itself is never stored", which is what made a link "shown once":
-- an admin who closed the pane before copying it could only revoke and issue
-- another. It gives nothing away — whoever can read this file can already add
-- a member directly — and `token_hash` stays what a link is looked up by.
--
-- member_inactive: one row per stretch a member was inactive, from the month
-- they were turned off to the month they were turned back on (exclusive; NULL
-- while it lasts). An inactive member cannot sign in, and in the months the
-- stretch covers their pane is hidden and their rows count in no total; the
-- months before it still show them. A stretch rather than a `disabled_at`
-- column because turning somebody back on in November must not bring back
-- September and October — nor let the materializer backfill bills into them.
-- Turned back on in the same month, the stretch is empty and hides nothing.
ALTER TABLE member_invites ADD COLUMN token TEXT;

CREATE TABLE member_inactive (
    id           INTEGER PRIMARY KEY,
    member_id    INTEGER NOT NULL REFERENCES members(id),
    from_period  TEXT NOT NULL,
    until_period TEXT,
    created_by   INTEGER REFERENCES members(id),
    created_at   TEXT NOT NULL,
    CHECK (from_period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    CHECK (until_period IS NULL OR until_period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    CHECK (until_period IS NULL OR until_period >= from_period)
);

-- At most one open stretch per member: being inactive is a state, not a count.
CREATE UNIQUE INDEX idx_inactive_open ON member_inactive(member_id)
    WHERE until_period IS NULL;
CREATE INDEX idx_inactive_member ON member_inactive(member_id, from_period);
