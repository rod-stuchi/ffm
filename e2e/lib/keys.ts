import { expect, type Page } from '@playwright/test';

/// Types a Vim sequence the way a keyboard does: `gT` is `g`, then Shift going
/// down on its own, then `T`. A synthetic KeyboardEvent skips the Shift, which
/// is how `gT` stayed broken for months with every check passing (CLAUDE.md's
/// Gotchas).
export async function vim(page: Page, sequence: string): Promise<void> {
  for (const key of sequence) {
    await page.keyboard.press(/[A-Z]/.test(key) ? `Shift+${key}` : key);
  }
}

/// Runs a `:` command, typed straight after the `:` as a person types it.
export async function command(page: Page, text: string): Promise<void> {
  await page.keyboard.press(':');
  await page.keyboard.type(text);
  await page.keyboard.press('Enter');
}

/// Waits for a pre-auth page's `autofocus` to land before anything is typed.
/// WebKit can apply it while Playwright is already filling another field, and
/// the text then goes to the field that took focus: a password typed into the
/// name field, ahead of the name (`ffmmarina`). A person sees the cursor first.
export async function autofocused(page: Page): Promise<void> {
  await page.waitForFunction(() => document.activeElement !== document.body, null, { timeout: 7_000 });
}

/// The focused pane's rows, top to bottom, by the id on each row's wrapper.
export async function rowIds(page: Page): Promise<number[]> {
  return page.locator('.pane.is-focus [id^="row-"]').evaluateAll((els) =>
    els.map((el) => Number(el.id.slice('row-'.length))),
  );
}

/// Walks the cursor to a row with keys, and checks it landed.
export async function cursorTo(page: Page, id: number): Promise<void> {
  const index = (await rowIds(page)).indexOf(id);
  expect(index, `row ${id} is in the focused pane`).toBeGreaterThanOrEqual(0);
  await vim(page, 'gg');
  if (index > 0) await vim(page, `${index}j`);
  await expect(page.locator(`#row-${id} .row.is-focus`)).toBeVisible();
}
