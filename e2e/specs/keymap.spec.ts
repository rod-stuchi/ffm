import { expect, test } from '../fixtures';
import { cursorTo, rowIds, vim } from '../lib/keys';

const focusedName = '.pane.is-focus .pane-head b';

test('the ledger opens on your own pane, in the current month', async ({ app, page }) => {
  await app.open(page, app.adult);
  await expect(page.locator(focusedName).first()).toHaveText(new RegExp(`^${app.adult.name}$`, 'i'));
  await expect(page.locator('.ledger-shell')).toHaveAttribute('data-period', app.period);
});

test('j and k walk the rows, and a count walks several', async ({ app, page }) => {
  await app.open(page, app.admin);
  const ids = await rowIds(page);
  expect(ids.length).toBeGreaterThan(3);

  await vim(page, 'gg');
  await expect(page.locator(`#row-${ids[0]} .row.is-focus`)).toBeVisible();
  await vim(page, 'j');
  await expect(page.locator(`#row-${ids[1]} .row.is-focus`)).toBeVisible();
  await vim(page, '2j');
  await expect(page.locator(`#row-${ids[3]} .row.is-focus`)).toBeVisible();
  await vim(page, 'k');
  await expect(page.locator(`#row-${ids[2]} .row.is-focus`)).toBeVisible();
});

test('[ and ] change the month, and T comes back', async ({ app, page }) => {
  await app.open(page, app.admin);
  const shell = page.locator('.ledger-shell');

  await vim(page, '[');
  await expect(shell).toHaveAttribute('data-period', '2026-08');
  await vim(page, ']');
  await expect(shell).toHaveAttribute('data-period', '2026-09');
  await vim(page, '[');
  await expect(shell).toHaveAttribute('data-period', '2026-08');
  await vim(page, 'T');
  await expect(shell).toHaveAttribute('data-period', app.period);
});

// The first of the bugs this suite exists for: holding Shift sent its own
// keydown, which cleared the pending `g`, so `gT` arrived as a bare `T`.
test('gt and gT cycle the panes, with Shift pressed as a keyboard presses it', async ({ app, page }) => {
  await app.open(page, app.admin);
  const name = page.locator(focusedName).first();
  await expect(name).toHaveText(new RegExp(`^${app.admin.name}$`, 'i'));

  await vim(page, 'gt');
  await expect(name).toHaveText(new RegExp(`^${app.adult.name}$`, 'i'));
  await expect(page).toHaveURL(new RegExp(`[?&]pane=${app.adult.id}`));

  await vim(page, 'gT');
  await expect(name).toHaveText(new RegExp(`^${app.admin.name}$`, 'i'));
  await expect(page.locator('.ledger-shell')).toHaveAttribute('data-period', app.period);
});

// The field used to take focus a tick after `:` (`focusSoon`), and keys typed in
// that gap ran as normal-mode commands: Linux WebKit always lost the race,
// Chromium sometimes (`help` read as `elp`), and real Safari in 2 of 3 tries on a
// reopened bar, where `s` wrote `ledger_sort`. `focusBar` now focuses inside the
// keydown. Playwright types with no delay, so this is the race at its tightest.
test('keys typed straight after : land in the command field, opened again too', async ({ app, page }) => {
  await app.open(page, app.admin);
  const field = page.locator('.cmdbar:not(.searchbar) input');

  await page.keyboard.press(':');
  await page.keyboard.type('help');
  await expect(field).toHaveValue('help');
  await page.keyboard.press('Escape');
  await expect(field).toBeHidden();

  await page.keyboard.press(':');
  await page.keyboard.type('sort');
  await expect(field).toHaveValue('sort');
});

test('keys typed straight after / land in the search field', async ({ app, page }) => {
  await app.open(page, app.admin);
  await page.keyboard.press('/');
  await page.keyboard.type('xyz');
  await expect(page.locator('.searchbar input')).toHaveValue('xyz');
});

// `2J` once lost its count to the same Shift, and a second `J` once moved the
// row back, because the cursor stayed on the index rather than on the row.
test('2J moves a row two places, and J again keeps walking the same row', async ({ app, page }) => {
  await app.open(page, app.admin);
  const before = await rowIds(page);
  const moved = before[0];
  await cursorTo(page, moved);

  await vim(page, '2J');
  await expect.poll(() => rowIds(page)).toEqual([before[1], before[2], moved, ...before.slice(3)]);
  await expect(page.locator(`#row-${moved} .row.is-focus`)).toBeVisible();

  await vim(page, 'J');
  await expect.poll(() => rowIds(page)).toEqual([before[1], before[2], before[3], moved, ...before.slice(4)]);
  await expect(page.locator(`#row-${moved} .row.is-focus`)).toBeVisible();
});
