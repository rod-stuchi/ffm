import type { Page } from '@playwright/test';
import { type App, expect, test } from '../fixtures';
import type { Member } from '../lib/fixture';
import { cursorTo, rowIds, vim } from '../lib/keys';

/// What the page fetches again on the live channel's word: a pane, or a row.
function refetches(page: Page): string[] {
  const seen: string[] = [];
  page.on('request', (r) => {
    const path = new URL(r.url()).pathname;
    if (r.method() === 'GET' && /^\/(panes\/|tx\/\d+$)/.test(path)) seen.push(path);
  });
  return seen;
}

/// Opens the ledger and waits for its `/live` stream: a page not yet
/// listening hears nothing of a change made meanwhile.
async function listening(app: App, page: Page, member: Member) {
  const stream = page.waitForResponse((r) => new URL(r.url()).pathname === '/live');
  await app.open(page, member);
  await stream;
}

/// Selects or unselects rows from inside the page, one after another with no
/// wait between, so each is a question the cell sends before the last one is
/// answered — faster than any key.
async function select(page: Page, steps: [number, boolean][]) {
  await page.evaluate((steps) => {
    const c = (window as any).Alpine.$data(document.querySelector('.ledger-shell'));
    for (const [id, on] of steps) {
      if (on) c.selected[id] = true;
      else delete c.selected[id];
      c.paintSelection();
    }
  }, steps);
}

const echo = 1_500; // the feed's tick, the page's 250ms, and room to spare

test('a page is not sent its own change again, and another device still hears it', async ({ app, page, stranger }) => {
  await listening(app, page, app.admin);
  await listening(app, stranger, app.admin);
  const fetched = refetches(page);
  const pane = `/panes/${app.admin.id}/${app.period}`;

  // `J` answers with the pane, so the page that pressed it shows the move
  // already; the other device fetches it.
  const before = await rowIds(page);
  await cursorTo(page, before[0]);
  await vim(page, 'J');
  const moved = [before[1], before[0], ...before.slice(2)];
  await expect.poll(() => rowIds(page)).toEqual(moved);
  await expect.poll(() => rowIds(stranger)).toEqual(moved);
  await page.waitForTimeout(echo);
  expect(fetched).toEqual([]);

  // `p` answers with its row.
  const id = before[0];
  await vim(page, 'p');
  const lock = `#row-${id} .mflag.flag-private.is-on`;
  await expect(page.locator(lock)).toBeVisible();
  await expect(stranger.locator(lock)).toBeVisible();
  await page.waitForTimeout(echo);
  expect(fetched).toEqual([]);

  // `dd` answers with the row struck through while its pane lost a line, so
  // its pane is fetched here too, and the row leaves both pages.
  await vim(page, 'dd');
  await vim(page, 'y');
  await expect(page.locator(`#row-${id}`)).toHaveCount(0);
  await expect(stranger.locator(`#row-${id}`)).toHaveCount(0);
  expect(fetched).toContain(pane);
});

test('a change made elsewhere still reaches the page', async ({ app, page, stranger }) => {
  await listening(app, page, app.admin);
  await listening(app, stranger, app.admin);
  const before = await rowIds(page);
  await cursorTo(stranger, before[0]);
  await vim(stranger, 'J');
  await expect.poll(() => rowIds(page)).toEqual([before[1], before[0], ...before.slice(2)]);
});

test('the selection cell answers the last x however fast they come', async ({ app, page }) => {
  await app.open(page, app.admin);
  const [a, b] = await rowIds(page);
  const total = (await rowIds(page)).length;
  const cell = page.locator('#stsel');
  await cursorTo(page, a);

  await vim(page, 'xjx');
  await expect(cell).toContainText(`2 of ${total} selected`);
  await vim(page, 'xkx');
  await expect(cell).toBeEmpty();

  // The race itself, whatever the speed of the keys: a second question sent
  // before the first is answered. Its answer used to land in the cell the
  // first answer had just replaced, out of the page, and the cell said 1.
  await select(page, [[a, true], [b, true]]);
  await expect(cell).toContainText(`2 of ${total} selected`);
  await page.waitForTimeout(500);
  await expect(cell).toContainText(`2 of ${total} selected`);

  // And the other way: cleared while a count is still on its way.
  await select(page, [[a, false], [b, false]]);
  await expect(cell).toBeEmpty();
  await page.waitForTimeout(500);
  await expect(cell).toBeEmpty();
});
