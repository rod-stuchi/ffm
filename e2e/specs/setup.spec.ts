import type { Page } from '@playwright/test';
import { expect, test } from '../fixtures';
import { autofocused, command } from '../lib/keys';

const PASSWORD = 'correct horse battery';

/// `/setup` is one form and one POST, so a test fills what it cares about.
async function fill(page: Page, fields: { household?: string; name?: string; password?: string; confirm?: string }) {
  await autofocused(page);
  if (fields.household !== undefined) await page.locator('input[name="household"]').fill(fields.household);
  if (fields.name !== undefined) await page.locator('input[name="name"]').fill(fields.name);
  if (fields.password !== undefined) await page.locator('input[name="password"]').fill(fields.password);
  if (fields.confirm !== undefined) await page.locator('input[name="confirm"]').fill(fields.confirm);
}

const submit = (page: Page) => page.locator('form[action="/setup"] button[type="submit"]').click();

test('with no household, every way in leads to /setup', async ({ blank, page }) => {
  await page.goto(`${blank.url}/`);
  await expect(page).toHaveURL(/\/setup$/);
  await page.goto(`${blank.url}/login`);
  await expect(page).toHaveURL(/\/setup$/);
});

test('setup creates the household and its admin, and lands in the ledger signed in', async ({ blank, page }) => {
  await page.goto(`${blank.url}/setup`);
  await fill(page, { household: 'Lar dos Testes', name: 'Ana Clara', password: PASSWORD, confirm: PASSWORD });
  await page.locator('select[name="currency"]').selectOption('USD');
  await page.locator('input[name="language"][value="pt-BR"]').check();
  await page.locator('.swatch', { has: page.locator('input[value="nord"]') }).click();
  await page.locator('input[name="nu"][value="absolute"]').check();
  await submit(page);

  // Never "now sign in with the password you just typed".
  await expect(page.locator('.pane.is-focus .pane-head b').first()).toHaveText('Ana Clara');
  expect(new URL(page.url()).pathname).toBe('/');

  // The three axes stay apart: Portuguese words, a US dollar's number format.
  await expect(page.locator('html')).toHaveAttribute('lang', 'pt-BR');
  await expect(page.locator('.pane.is-focus .ptotals')).toContainText('$0.00');

  expect(blank.sql(`SELECT name, locale_currency, default_language FROM household_settings`)).toEqual([
    { name: 'Lar dos Testes', locale_currency: 'USD', default_language: 'pt-BR' },
  ]);
  // Stored lowercase, shown capitalized: the name is the login identifier.
  const [member] = blank.sql<{ name: string; role: number; hash: string; id: number }>(
    `SELECT id, name, role, substr(password_hash, 1, 7) AS hash FROM members`,
  );
  expect(member).toMatchObject({ name: 'ana clara', role: 0, hash: '$argon2' });
  expect(blank.sql(`SELECT language, theme_name, line_numbers FROM member_settings WHERE member_id = ${member.id}`)).toEqual([
    { language: 'pt-BR', theme_name: 'nord', line_numbers: 'absolute' },
  ]);
});

test('the password chosen at setup signs in afterwards, in any case', async ({ blank, page }) => {
  await page.goto(`${blank.url}/setup`);
  await fill(page, { household: 'Lar dos Testes', name: 'Ana Clara', password: PASSWORD, confirm: PASSWORD });
  await submit(page);
  await expect(page.locator('.pane.is-focus .pane-head b').first()).toHaveText('Ana Clara');

  await command(page, 'logout');
  await expect(page).toHaveURL(/\/login/);
  await autofocused(page);
  await page.locator('input[name="name"]').fill('ANA CLARA');
  await page.locator('input[name="password"]').fill(PASSWORD);
  await page.keyboard.press('Enter');
  await expect(page.locator('.pane.is-focus .pane-head b').first()).toHaveText('Ana Clara');
});

test('an empty form says everything wrong at once, and creates nothing', async ({ blank, page }) => {
  await page.goto(`${blank.url}/setup`);
  await submit(page);

  await expect(page.locator('.warnbox')).toBeVisible();
  await expect(page.locator('.ferr')).toHaveCount(3);
  await expect(page.locator('label.is-bad input[name="household"]')).toBeVisible();
  await expect(page.locator('label.is-bad input[name="name"]')).toBeVisible();
  await expect(page.locator('label.is-bad input[name="password"]')).toBeVisible();
  // The cursor lands on the first thing wrong.
  await expect(page.locator('input[name="household"]')).toBeFocused();
  expect(blank.sql(`SELECT id FROM members`)).toEqual([]);
  expect(blank.sql(`SELECT id FROM household_settings`)).toEqual([]);
});

test('a refused form keeps what was typed, except the passwords, and can be corrected', async ({ blank, page }) => {
  await page.goto(`${blank.url}/setup`);
  await fill(page, { household: 'Lar dos Testes', name: 'Ana Clara', password: PASSWORD, confirm: 'something else' });
  await submit(page);

  // A mismatch is reported on Confirm: the two are a pair, and that is the box to retype.
  await expect(page.locator('label.is-bad input[name="confirm"]')).toBeVisible();
  await expect(page.locator('.ferr')).toHaveCount(1);
  await expect(page.locator('input[name="household"]')).toHaveValue('Lar dos Testes');
  await expect(page.locator('input[name="name"]')).toHaveValue('Ana Clara');
  await expect(page.locator('input[name="password"]')).toHaveValue('');
  await expect(page.locator('input[name="confirm"]')).toHaveValue('');
  expect(blank.sql(`SELECT id FROM members`)).toEqual([]);

  await fill(page, { password: 'short', confirm: 'short' });
  await submit(page);
  await expect(page.locator('label.is-bad input[name="password"]')).toBeVisible();
  await expect(page.locator('.ferr')).toHaveCount(1);
  expect(blank.sql(`SELECT id FROM members`)).toEqual([]);

  await fill(page, { password: PASSWORD, confirm: PASSWORD });
  await submit(page);
  await expect(page.locator('.pane.is-focus .pane-head b').first()).toHaveText('Ana Clara');
});

test('the password meter and the match line answer as you type', async ({ blank, page }) => {
  await page.goto(`${blank.url}/setup`);
  const meter = page.locator('.pwmeter');
  const match = page.locator('.pwmatch');
  await expect(meter).toBeHidden();

  await fill(page, { password: 'abc' });
  await expect(meter).toHaveAttribute('data-level', 'short');
  await fill(page, { password: 'aaaaaaaa' });
  await expect(meter).toHaveAttribute('data-level', 'weak');
  await fill(page, { password: 'qwmtzkrpvbng' });
  await expect(meter).toHaveAttribute('data-level', 'medium');
  await fill(page, { password: 'Tr0ub4dor&3-xtra-long!' });
  await expect(meter).toHaveAttribute('data-level', 'strong');

  // Silent while Confirm is still on its way to matching.
  await fill(page, { confirm: 'Tr0ub' });
  await expect(match).toBeHidden();
  await fill(page, { confirm: 'Tr0ub-wrong' });
  await expect(match).toHaveAttribute('data-ok', 'no');
  await fill(page, { confirm: 'Tr0ub4dor&3-xtra-long!' });
  await expect(match).toHaveAttribute('data-ok', 'yes');

  await fill(page, { password: '' });
  await expect(meter).toBeHidden();
});

test('setup runs once: the page has no form afterwards, and a forged POST changes nothing', async ({ blank, page, stranger }) => {
  await page.goto(`${blank.url}/setup`);
  await fill(page, { household: 'Lar dos Testes', name: 'Ana Clara', password: PASSWORD, confirm: PASSWORD });
  await submit(page);
  await expect(page.locator('.pane.is-focus .pane-head b').first()).toHaveText('Ana Clara');

  await stranger.goto(`${blank.url}/setup`);
  await expect(stranger.locator('.notebox')).toBeVisible();
  await expect(stranger.locator('input[name="password"]')).toHaveCount(0);

  await stranger.request.post(`${blank.url}/setup`, {
    form: {
      household: 'Tomado',
      currency: 'EUR',
      language: 'en',
      name: 'Intruso',
      password: PASSWORD,
      confirm: PASSWORD,
      theme: 'ffm',
      nu: 'relative',
    },
  });
  expect(blank.sql(`SELECT name FROM members`)).toEqual([{ name: 'ana clara' }]);
  expect(blank.sql(`SELECT name, locale_currency FROM household_settings`)).toEqual([
    { name: 'Lar dos Testes', locale_currency: 'BRL' },
  ]);
});
