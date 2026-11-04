import { test as base, expect, type Page, type TestInfo } from '@playwright/test';
import { cp, mkdir, rm } from 'node:fs/promises';
import { fixture, GOLDEN, type Member } from './lib/fixture';
import { type Paths, query, Server, write } from './lib/server';

export { expect };

export interface App {
  url: string;
  db: string;
  vault: string;
  today: string;
  /// The month `today` falls in, as the URLs and the database spell it.
  period: string;
  password: string;
  admin: Member;
  /// The other two, in the seed's order: an adult and a kid.
  adult: Member;
  kid: Member;
  sql<T>(q: string): T[];
  /// Changes the data behind the running server, for a state the page cannot
  /// reach, like a link past its date.
  exec(q: string): void;
  /// Opens the ledger as `member`, ready for keys.
  open(page: Page, member: Member, params?: string): Promise<void>;
}

/// A server with no household: what `/setup` needs, and nothing else does.
export interface Blank {
  url: string;
  sql<T>(q: string): T[];
}

function paths(info: TestInfo): Paths {
  const dir = `/work/t/${info.workerIndex}-${info.testId}`;
  return { db: `${dir}/ffm.db`, vault: `${dir}/receipts`, backups: `${dir}/backups` };
}

/// Stops the server, keeps its log if the test failed, and removes the data.
async function finish(server: Server, at: Paths, info: TestInfo): Promise<void> {
  await server.stop();
  if (info.status !== info.expectedStatus) {
    await info.attach('server.log', { body: server.log, contentType: 'text/plain' });
  }
  await rm(at.db.slice(0, at.db.lastIndexOf('/')), { recursive: true, force: true });
}

/// Every test gets a server of its own on a copy of the golden data, because
/// `u` depends on who did what and the ledger is shared: on one server, tests
/// would depend on their order. A test moves the clock with
/// `test.use({ today: '2026-10-15' })`.
export const test = base.extend<{ today: string | undefined; app: App; blank: Blank; stranger: Page }>({
  today: [undefined, { option: true }],

  app: async ({ today }, use, info) => {
    const f = fixture();
    const date = today ?? f.today;
    info.annotations.push({ type: 'data seed', description: f.seed });

    const at = paths(info);
    await mkdir(at.backups, { recursive: true });
    await cp(`${GOLDEN}/ffm.db`, at.db);
    await cp(`${GOLDEN}/receipts`, at.vault, { recursive: true });

    const server = await Server.start(at, 5000 + info.parallelIndex, date);
    // Only a moved clock has anything to materialize.
    if (date !== f.today) await server.settled(at.db);

    const [admin, adult, kid] = f.members;
    await use({
      url: server.url,
      db: at.db,
      vault: at.vault,
      today: date,
      period: date.slice(0, 7),
      password: f.password,
      admin,
      adult,
      kid,
      sql: (q) => query(at.db, q),
      exec: (q) => write(at.db, q),
      open: async (page, member, params = '') => {
        await page.context().addCookies([{ name: 'ffm.sid', value: member.sid, url: server.url }]);
        await page.goto(`${server.url}/${params}`);
        // `paint()` puts the cursor down once Alpine has started the keymap.
        await expect(page.locator('.row.is-focus')).toBeVisible();
      },
    });

    await finish(server, at, info);
  },

  blank: async ({}, use, info) => {
    const f = fixture();
    const at = paths(info);
    await mkdir(at.vault, { recursive: true });
    await mkdir(at.backups, { recursive: true });
    const server = await Server.start(at, 5000 + info.parallelIndex, f.today);
    await use({ url: server.url, sql: (q) => query(at.db, q) });
    await finish(server, at, info);
  },

  /// A second person, on a device of their own: no cookies shared with `page`.
  stranger: async ({ browser }, use) => {
    const context = await browser.newContext();
    await use(await context.newPage());
    await context.close();
  },
});
