import { existsSync } from 'node:fs';
import { expect, test } from '../fixtures';
import { cursorTo, vim } from '../lib/keys';

const PROVEN = '.mark .disc.tone-pos:not(.is-outline)';

type Row = { id: number; path: string };

test('a row whose receipt is on disk reads paid, and P shows the file', async ({ app, page }) => {
  // July's: an old month, with a receipt and a bill both in the vault.
  const [row] = app.sql<Row>(
    `SELECT t.id, r.path FROM transactions t
     JOIN receipts r ON r.transaction_id = t.id AND r.kind = 'receipt' AND r.deleted_at IS NULL
     WHERE t.member_id = ${app.admin.id} AND t.period = '2026-07' AND r.ext = 'pdf' ORDER BY t.id LIMIT 1`,
  );
  expect(existsSync(`${app.vault}/${row.path}`), 'anon wrote the file').toBe(true);

  await app.open(page, app.admin, '?period=2026-07');
  await expect(page.locator(`#row-${row.id} ${PROVEN}`)).toBeVisible();
  await cursorTo(page, row.id);

  await vim(page, 'P');
  const carousel = page.locator('.carousel');
  await expect(carousel).toBeVisible();
  // The page is really drawn: pdftoppm ran, and the browser decoded the image.
  const image = carousel.locator('img:visible').first();
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate((el: HTMLImageElement) => el.naturalWidth)).toBeGreaterThan(0);

  await page.keyboard.press('Escape');
  await expect(carousel).toBeHidden();
});

test('a receipt filed over a scheduled payment makes it paid and retires the promise', async ({ app, page }) => {
  const [row] = app.sql<Row>(
    `SELECT t.id, r.path FROM transactions t
     JOIN receipts r ON r.transaction_id = t.id AND r.kind = 'scheduled' AND r.deleted_at IS NULL
     WHERE t.member_id = ${app.admin.id} AND t.period = '${app.period}' LIMIT 1`,
  );
  await app.open(page, app.admin);
  // Scheduled, and past its due date: the warning state, not a generic pending.
  await expect(page.locator(`#row-${row.id} .mark .disc.tone-neg`)).toBeVisible();
  await cursorTo(page, row.id);

  // A text file goes straight in; a PDF or an image stops at the trim view.
  const chooser = page.waitForEvent('filechooser');
  await vim(page, 'gu3');
  await (await chooser).setFiles({
    name: 'comprovante.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('comprovante de teste'),
  });

  await expect(page.locator(`#row-${row.id} ${PROVEN}`)).toBeVisible();

  const files = app.sql<{ kind: string; path: string; deleted_at: string | null; delete_reason: string | null }>(
    `SELECT kind, path, deleted_at, delete_reason FROM receipts WHERE transaction_id = ${row.id} ORDER BY id`,
  );
  const receipt = files.find((f) => f.kind === 'receipt')!;
  expect(receipt.deleted_at).toBeNull();
  expect(existsSync(`${app.vault}/${receipt.path}`)).toBe(true);

  const scheduled = files.find((f) => f.kind === 'scheduled')!;
  expect(scheduled.deleted_at).not.toBeNull();
  expect(scheduled.delete_reason).toBe('superseded');
  expect(existsSync(`${app.vault}/${row.path}`), 'the scheduled file left its place').toBe(false);
});
