import { expect, test } from '../fixtures';
import { vim } from '../lib/keys';

type Row = { id: number; member_id: number };

function privateRows(app: { sql<T>(q: string): T[]; period: string }): Row[] {
  return app.sql<Row>(
    `SELECT id, member_id FROM transactions
     WHERE visibility = 'private' AND deleted_at IS NULL AND period = '${app.period}'`,
  );
}

test('a private row shows to its owner and the admin, and to nobody else', async ({ app, page }) => {
  const rows = privateRows(app);
  const adminsOwn = rows.find((r) => r.member_id === app.admin.id)!;
  const adultsOwn = rows.find((r) => r.member_id === app.adult.id)!;
  expect(adminsOwn && adultsOwn, 'the seed holds a private row for both').toBeTruthy();

  await app.open(page, app.admin);
  await expect(page.locator(`#row-${adminsOwn.id}`)).toBeVisible();
  await expect(page.locator(`#row-${adultsOwn.id}`)).toBeAttached();

  await app.open(page, app.adult);
  await expect(page.locator(`#row-${adultsOwn.id}`)).toBeAttached();
  await expect(page.locator(`#row-${adminsOwn.id}`)).toHaveCount(0);

  await app.open(page, app.kid);
  await expect(page.locator(`#row-${adultsOwn.id}`)).toHaveCount(0);
  await expect(page.locator(`#row-${adminsOwn.id}`)).toHaveCount(0);
});

test('another member\'s pane is read-only: the page refuses first', async ({ app, page }) => {
  await app.open(page, app.adult, `?pane=${app.admin.id}`);
  await expect(page.locator('.pane.is-focus .rotag')).toBeVisible();

  const [first] = await page.locator('.pane.is-focus [id^="row-"]').evaluateAll((els) => els.map((el) => el.id));
  await vim(page, 'dd');
  await vim(page, 'y');
  await expect(page.locator(`#${first}`)).toBeVisible();
  await vim(page, 'a');
  await expect(page.locator('.pane.is-focus form.addrow')).toHaveCount(0);
});

test('the server is the rule: a forged write to another ledger changes nothing', async ({ app, page }) => {
  await app.open(page, app.adult);
  const [target] = app.sql<{ id: number; title: string }>(
    `SELECT id, title FROM transactions
     WHERE member_id = ${app.admin.id} AND period = '${app.period}'
       AND deleted_at IS NULL AND visibility = 'household' LIMIT 1`,
  );
  const headers = { 'HX-Request': 'true' };

  await page.request.post(`${app.url}/tx/${target.id}/delete`, { headers });
  await page.request.post(`${app.url}/tx/${target.id}/privacy`, { headers });
  await page.request.post(`${app.url}/tx`, {
    headers,
    form: { member_id: app.admin.id, period: app.period, kind: 'out', title: 'Forjado', amount: '1,00' },
  });

  const [after] = app.sql<{ deleted_at: string | null; visibility: string }>(
    `SELECT deleted_at, visibility FROM transactions WHERE id = ${target.id}`,
  );
  expect(after).toEqual({ deleted_at: null, visibility: 'household' });
  expect(app.sql(`SELECT id FROM transactions WHERE title = 'Forjado'`)).toEqual([]);
});
