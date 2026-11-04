import { execFileSync } from 'node:child_process';
import { randomBytes, randomInt } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { ANON, query, Server, SOURCE, write } from './lib/server';
import { FIXTURE, GOLDEN, type Fixture, type Member } from './lib/fixture';

const WORK = '/work';
const PASSWORD = 'ffm';

/// seed.sql → ffm-anon → one boot → the copy every test starts from. Booted
/// once here so that a test's own server has nothing to materialize or re-file.
export default async function globalSetup(): Promise<void> {
  const seed = process.env.E2E_SEED || String(randomInt(2 ** 47));
  const today = process.env.E2E_TODAY || '2026-09-20';
  console.log(`e2e · data seed ${seed} (E2E_SEED=${seed} makes this data again) · today ${today}`);

  // 1. A migrated, empty database, from the server's own migrator, which is
  //    what writes `_sqlx_migrations`. Then the seed.
  const raw = { db: `${WORK}/seed/ffm.db`, vault: `${WORK}/seed/receipts`, backups: `${WORK}/seed/backups` };
  mkdirSync(raw.vault, { recursive: true });
  mkdirSync(raw.backups, { recursive: true });
  const first = await Server.start(raw, 4999, today);
  await first.stop();
  execFileSync('sqlite3', ['-bail', raw.db, `.read ${SOURCE}/seeds/e2e.sql`]);

  // 2. Everything made up, by the tool that does that for a real database.
  const out = execFileSync(ANON, ['--from', raw.db, '--out', GOLDEN, '--seed', seed, '--password', PASSWORD], {
    encoding: 'utf8',
  });
  console.log(out.trim().split('\n').filter((l) => /members|rows/.test(l)).join('\n'));

  // 3. Brought up to date once, and stopped so the WAL is checkpointed.
  const golden = { db: `${GOLDEN}/ffm.db`, vault: `${GOLDEN}/receipts`, backups: `${GOLDEN}/backups` };
  const server = await Server.start(golden, 4999, today);
  await server.settled(golden.db);
  await server.stop();

  // A file the watcher calls added or removed by hand means anon's vault and
  // the app disagree about a path, and every paid mark after it means nothing.
  const strays = query<{ note: string }>(golden.db, `SELECT note FROM change WHERE note LIKE '%by hand%'`);
  if (strays.length) {
    throw new Error(`the generated vault disagrees with the database: ${strays.length} file(s) "by hand"\n${server.log}`);
  }

  // 4. A session per member, so only the sign-in spec pays for argon2.
  const rows = query<Omit<Member, 'sid'>>(golden.db, `SELECT id, name, role FROM members ORDER BY id`);
  const members: Member[] = rows.map((m) => ({ ...m, sid: randomBytes(24).toString('hex') }));
  const at = `${today}T12:00:00Z`;
  write(
    golden.db,
    members
      .map((m) => `INSERT INTO sessions (id, member_id, created_at, last_seen_at) VALUES ('${m.sid}', ${m.id}, '${at}', '${at}');`)
      .join('\n'),
  );

  const fixture: Fixture = { seed, today, password: PASSWORD, members };
  writeFileSync(FIXTURE, JSON.stringify(fixture));
}
