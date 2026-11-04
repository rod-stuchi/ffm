import type { Page } from '@playwright/test';
import { type App, expect, test } from '../fixtures';
import { command } from '../lib/keys';

const PASSWORD = 'correct horse battery';

const freshLink = (page: Page) => page.locator('.prow.is-fresh .invitelink input');

/// Issues a link through `:members`, as the admin, and returns its full URL.
async function issue(app: App, page: Page, label: string, role: 'member' | 'admin' = 'member'): Promise<string> {
  await app.open(page, app.admin);
  await command(page, 'members');
  const pane = page.locator('#settingspane');
  await pane.locator('input[name="label"]').fill(label);
  if (role === 'admin') await pane.locator('input[name="role"][value="0"]').check();
  await pane.locator('form[hx-post="/members/invite"] button[type="submit"]').click();
  await expect(freshLink(page)).toHaveValue(/\/invite\/.+/);
  return freshLink(page).inputValue();
}

/// Fills a link's form, as whoever it reached.
async function accept(page: Page, fields: { name?: string; password: string; confirm?: string }): Promise<void> {
  if (fields.name !== undefined) await page.locator('input[name="name"]').fill(fields.name);
  await page.locator('input[name="password"]').fill(fields.password);
  await page.locator('input[name="confirm"]').fill(fields.confirm ?? fields.password);
  await page.locator('form[action^="/invite/"] button[type="submit"]').click();
}

async function expectDead(page: Page): Promise<void> {
  await expect(page.locator('.warnbox')).toBeVisible();
  await expect(page.locator('input[name="password"]')).toHaveCount(0);
}

const ownPane = (page: Page) => page.locator('.pane.is-focus .pane-head b').first();

test('an invite lets a new person choose their own name, password, language and palette', async ({ app, page, stranger }) => {
  const link = await issue(app, page, 'Para a tia');
  expect(new URL(link).origin).toBe(app.url);

  await stranger.goto(link);
  await stranger.locator('input[name="language"][value="pt-BR"]').check();
  await stranger.locator('.swatch', { has: stranger.locator('input[value="nord"]') }).click();
  await accept(stranger, { name: 'Tia Rosa', password: PASSWORD });

  // In, on their own pane, with a pane of nothing and everyone else's beside it.
  await expect(ownPane(stranger)).toHaveText('Tia Rosa');
  await expect(stranger.locator('html')).toHaveAttribute('lang', 'pt-BR');

  const [member] = app.sql<{ id: number; name: string; role: number }>(
    `SELECT id, name, role FROM members WHERE name = 'tia rosa'`,
  );
  expect(member).toMatchObject({ name: 'tia rosa', role: 1 });
  expect(app.sql(`SELECT language, theme_name FROM member_settings WHERE member_id = ${member.id}`)).toEqual([
    { language: 'pt-BR', theme_name: 'nord' },
  ]);

  // The raw token is kept only while pending.
  const [invite] = app.sql<{ token: string | null; accepted_member_id: number }>(
    `SELECT token, accepted_member_id FROM member_invites WHERE label = 'Para a tia'`,
  );
  expect(invite).toEqual({ token: null, accepted_member_id: member.id });
});

test('a link issued for an admin makes an admin', async ({ app, page, stranger }) => {
  const link = await issue(app, page, 'Outra pessoa', 'admin');
  await stranger.goto(link);
  await accept(stranger, { name: 'Bia', password: PASSWORD });
  await expect(ownPane(stranger)).toHaveText('Bia');
  expect(app.sql(`SELECT role FROM members WHERE name = 'bia'`)).toEqual([{ role: 0 }]);
});

test('a link works once: afterwards it has no form, and a forged POST adds nobody', async ({ app, page, stranger, browser }) => {
  const link = await issue(app, page, 'Uma vez');
  await stranger.goto(link);
  await accept(stranger, { name: 'Primeira', password: PASSWORD });
  await expect(ownPane(stranger)).toHaveText('Primeira');

  const context = await browser.newContext();
  const late = await context.newPage();
  await late.goto(link);
  await expectDead(late);
  await late.request.post(link, { form: { name: 'Segunda', password: PASSWORD, confirm: PASSWORD } });
  await context.close();
  expect(app.sql(`SELECT id FROM members WHERE name = 'segunda'`)).toEqual([]);
});

test('a refused form keeps the name, clears the passwords, and the link still works', async ({ app, page, stranger }) => {
  const link = await issue(app, page, 'Com erros');
  const before = app.sql(`SELECT id FROM members`).length;
  await stranger.goto(link);

  await accept(stranger, { name: 'Clara', password: 'short' });
  await expect(stranger.locator('label.is-bad input[name="password"]')).toBeVisible();
  await expect(stranger.locator('input[name="name"]')).toHaveValue('Clara');
  await expect(stranger.locator('input[name="password"]')).toHaveValue('');

  await accept(stranger, { name: 'Clara', password: PASSWORD, confirm: 'something else' });
  await expect(stranger.locator('label.is-bad input[name="confirm"]')).toBeVisible();

  // Taken, in any case: names are the login identifier, and matched ignoring it.
  await accept(stranger, { name: app.adult.name.toUpperCase(), password: PASSWORD });
  await expect(stranger.locator('label.is-bad input[name="name"]')).toBeVisible();
  await expect(stranger.locator('.notebox')).toBeVisible();
  expect(app.sql(`SELECT id FROM members`).length).toBe(before);

  await accept(stranger, { name: 'Clara', password: PASSWORD });
  await expect(ownPane(stranger)).toHaveText('Clara');
  expect(app.sql(`SELECT id FROM members`).length).toBe(before + 1);
});

test('a link nobody issued shows a page with no form', async ({ app, stranger }) => {
  await stranger.goto(`${app.url}/invite/not-a-real-token`);
  await expectDead(stranger);
});

test('a link past its date shows a page with no form, and refuses a POST', async ({ app, page, stranger }) => {
  const link = await issue(app, page, 'Atrasada');
  app.exec(`UPDATE member_invites SET expires_at = '2026-01-01T00:00:00Z' WHERE label = 'Atrasada'`);

  await stranger.goto(link);
  await expectDead(stranger);
  await stranger.request.post(link, { form: { name: 'Tarde', password: PASSWORD, confirm: PASSWORD } });
  expect(app.sql(`SELECT id FROM members WHERE name = 'tarde'`)).toEqual([]);
});

test('a revoked link stops working, and leaves the list', async ({ app, page, stranger }) => {
  const link = await issue(app, page, 'Revogada');
  await page.locator('.prow.is-fresh').getByRole('button', { name: 'Revoke' }).click();
  await expect(page.locator('.prow', { hasText: 'Revogada' })).toHaveCount(0);

  await stranger.goto(link);
  await expectDead(stranger);
  expect(app.sql(`SELECT token FROM member_invites WHERE label = 'Revogada'`)).toEqual([{ token: null }]);
});

test('only an admin can issue a link: :members refuses, and so does the server', async ({ app, page }) => {
  await app.open(page, app.adult);
  await command(page, 'members');
  await expect(page.locator('.memberspane')).toHaveCount(0);

  await page.request.post(`${app.url}/members/invite`, {
    headers: { 'HX-Request': 'true' },
    form: { label: 'Forjada', role: '0' },
  });
  expect(app.sql(`SELECT id FROM member_invites`)).toEqual([]);
});

test.describe('a password reset', () => {
  /// As the admin, from the member's row in `:members`.
  async function issueReset(app: App, page: Page): Promise<string> {
    await app.open(page, app.admin);
    await command(page, 'members');
    await page.locator('.mrow', { hasText: new RegExp(app.adult.name, 'i') }).click();
    await page.getByRole('button', { name: 'Reset password' }).click();
    await expect(freshLink(page)).toHaveValue(/\/invite\/.+/);
    return freshLink(page).inputValue();
  }

  test('sets a new password on the same member, who keeps their name and rows', async ({ app, page, stranger }) => {
    const rowsBefore = app.sql(`SELECT id FROM transactions WHERE member_id = ${app.adult.id}`);
    const [before] = app.sql<{ slug: string; password_hash: string }>(
      `SELECT slug, password_hash FROM members WHERE id = ${app.adult.id}`,
    );
    const link = await issueReset(app, page);

    await stranger.goto(link);
    // No name, no language, no palette: this person is not new.
    await expect(stranger.locator('input[name="name"]')).toHaveCount(0);
    await expect(stranger.locator('input[name="theme"]')).toHaveCount(0);
    await accept(stranger, { password: PASSWORD });

    await expect(ownPane(stranger)).toHaveText(new RegExp(`^${app.adult.name}$`, 'i'));

    const [after] = app.sql<{ slug: string; password_hash: string }>(
      `SELECT slug, password_hash FROM members WHERE id = ${app.adult.id}`,
    );
    expect(after.slug).toBe(before.slug);
    expect(after.password_hash).not.toBe(before.password_hash);
    expect(app.sql(`SELECT id FROM transactions WHERE member_id = ${app.adult.id}`)).toEqual(rowsBefore);
    expect(app.sql(`SELECT id FROM members`).length).toBe(3);

    // The old password is gone, and the new one signs in.
    await command(stranger, 'logout');
    await expect(stranger).toHaveURL(/\/login/);
    await stranger.locator('input[name="name"]').fill(app.adult.name);
    await stranger.locator('input[name="password"]').fill(app.password);
    await stranger.keyboard.press('Enter');
    await expect(stranger.locator('.warnbox')).toBeVisible();
    await stranger.locator('input[name="password"]').fill(PASSWORD);
    await stranger.keyboard.press('Enter');
    await expect(ownPane(stranger)).toHaveText(new RegExp(`^${app.adult.name}$`, 'i'));
  });

  test('revokes the member\'s sessions and is written in the log', async ({ app, page }) => {
    await issueReset(app, page);

    expect(app.sql(`SELECT id FROM sessions WHERE member_id = ${app.adult.id}`)).toEqual([]);
    const log = app.sql<{ action: string; member_id: number }>(
      `SELECT action, member_id FROM change
       WHERE entity = 'member' AND entity_id = ${app.adult.id} ORDER BY id`,
    );
    expect(log.map((l) => l.action).sort()).toEqual(['password_reset', 'sessions_revoked']);
    expect(log.every((l) => l.member_id === app.admin.id)).toBe(true);
  });

  // A known bug, pinned: `settings::reset` never calls `sessions_revoked()`, while
  // `settings::invite`, which revokes nothing, does. So the page leaves only on
  // the stream's 30-second re-check (measured 27 to 30 s). Once the call moves,
  // this fails as "expected to fail, but passed": delete the `test.fail` then.
  test('sends the member\'s open page to the sign-in page at once', async ({ app, page, stranger }) => {
    test.fail(true, 'the reset does not ping the live streams');
    await app.open(stranger, app.adult);
    await issueReset(app, page);
    await expect(stranger).toHaveURL(/\/login\?out=1/, { timeout: 5_000 });
  });

  test('is not offered on your own row', async ({ app, page }) => {
    await app.open(page, app.admin);
    await command(page, 'members');
    await page.locator('.mrow', { hasText: new RegExp(app.admin.name, 'i') }).click();
    await expect(page.getByRole('button', { name: 'Reset password' })).toHaveCount(0);
  });
});
