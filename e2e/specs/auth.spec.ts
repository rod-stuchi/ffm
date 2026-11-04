import { expect, test } from '../fixtures';
import { command } from '../lib/keys';

test('a member signs in, in any case, and lands on their own pane', async ({ app, page }) => {
  await page.goto(`${app.url}/login`);
  await page.locator('input[name="name"]').fill(app.adult.name.toUpperCase());
  await page.locator('input[name="password"]').fill(app.password);
  await page.keyboard.press('Enter');

  await expect(page.locator('.pane.is-focus .pane-head b').first()).toHaveText(new RegExp(`^${app.adult.name}$`, 'i'));
  expect(new URL(page.url()).pathname).toBe('/');
});

test('a wrong password keeps the name, clears the password and says so', async ({ app, page }) => {
  await page.goto(`${app.url}/login`);
  await page.locator('input[name="name"]').fill(app.adult.name);
  await page.locator('input[name="password"]').fill('not-the-password');
  await page.keyboard.press('Enter');

  await expect(page.locator('.warnbox')).toBeVisible();
  await expect(page.locator('input[name="name"]')).toHaveValue(app.adult.name);
  await expect(page.locator('input[name="password"]')).toHaveValue('');
  expect(app.sql(`SELECT id FROM sessions WHERE member_id = ${app.adult.id} AND id <> '${app.adult.sid}'`)).toEqual([]);
});

test('nobody signed in is sent to the sign-in page', async ({ app, page }) => {
  await page.goto(`${app.url}/`);
  await expect(page).toHaveURL(/\/login/);
});

test(':logout ends the session, and the ledger is gone with it', async ({ app, page }) => {
  await app.open(page, app.admin);
  await command(page, 'logout');
  await expect(page).toHaveURL(/\/login/);

  await page.goto(`${app.url}/`);
  await expect(page).toHaveURL(/\/login/);
  expect(app.sql(`SELECT id FROM sessions WHERE id = '${app.admin.sid}'`)).toEqual([]);
});
