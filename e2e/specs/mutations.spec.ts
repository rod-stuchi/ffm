import { expect, test } from '../fixtures';
import { cursorTo, rowIds, vim } from '../lib/keys';

test('a adds a spend to the focused pane, moves its totals and survives a reload', async ({ app, page }) => {
  await app.open(page, app.admin);
  const totals = page.locator(`#ptot-${app.admin.id}`);
  const before = await totals.innerText();

  await vim(page, 'a');
  const form = page.locator('.pane.is-focus form.addrow');
  await expect(form).toBeVisible();
  await form.locator('input[name="title"]').pressSequentially('Sorvete de teste');
  await form.locator('input[name="amount"]').pressSequentially('1234');
  await page.keyboard.press('Enter');

  const row = page.locator('.pane.is-focus [id^="row-"]', { hasText: 'Sorvete de teste' });
  await expect(row).toBeVisible();
  await expect(totals).not.toHaveText(before);
  const moved = await totals.innerText();

  const saved = app.sql<{ amount_cents: number; member_id: number; created_by: number; kind: string }>(
    `SELECT amount_cents, member_id, created_by, kind FROM transactions WHERE title = 'Sorvete de teste'`,
  );
  expect(saved).toEqual([{ amount_cents: 1234, member_id: app.admin.id, created_by: app.admin.id, kind: 'out' }]);

  await page.reload();
  await expect(row).toBeVisible();
  await expect(totals).toHaveText(moved);
});

test('dd deletes after asking, keeps the row, and u puts it back in place', async ({ app, page }) => {
  await app.open(page, app.admin);
  const before = await rowIds(page);
  const id = before[1];
  const totals = page.locator(`#ptot-${app.admin.id}`);
  const start = await totals.innerText();
  await cursorTo(page, id);

  await vim(page, 'dd');
  await expect(page.locator(`#row-${id}`)).toBeVisible();
  await vim(page, 'y');
  await expect(page.locator(`#row-${id}`)).toHaveCount(0);
  await expect(totals).not.toHaveText(start);

  // No hard deletes: the row is still there, marked.
  const [gone] = app.sql<{ deleted_at: string | null }>(`SELECT deleted_at FROM transactions WHERE id = ${id}`);
  expect(gone.deleted_at).not.toBeNull();

  // Every `u` asks first, and nothing is written until it is answered.
  await vim(page, 'u');
  await expect(page.locator('.modal:visible')).toBeVisible();
  await expect(page.locator(`#row-${id}`)).toHaveCount(0);
  await vim(page, 'y');

  await expect.poll(() => rowIds(page)).toEqual(before);
  await expect(totals).toHaveText(start);
  const [back] = app.sql<{ deleted_at: string | null }>(`SELECT deleted_at FROM transactions WHERE id = ${id}`);
  expect(back.deleted_at).toBeNull();

  // One row back, not a clone of it, and it is still so after a reload.
  await page.reload();
  await expect.poll(() => rowIds(page)).toEqual(before);
});

test('Esc on the delete question leaves the row alone', async ({ app, page }) => {
  await app.open(page, app.admin);
  const [id] = await rowIds(page);
  await cursorTo(page, id);

  await vim(page, 'dd');
  await page.keyboard.press('Escape');
  await expect(page.locator(`#row-${id}`)).toBeVisible();
  const [row] = app.sql<{ deleted_at: string | null }>(`SELECT deleted_at FROM transactions WHERE id = ${id}`);
  expect(row.deleted_at).toBeNull();
});
