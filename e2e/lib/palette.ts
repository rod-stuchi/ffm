import type { Page } from '@playwright/test';

/// The page's own `--paper`, as the browser resolves it (`rgb(…)`): a custom
/// property reads back as written, so it is resolved through a color.
export function paper(page: Page): Promise<string> {
  return page.evaluate(() => {
    const probe = document.createElement('i');
    probe.style.color = 'var(--paper)';
    document.body.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  });
}

/// A swatch's first bar is its palette's light paper — the page's own paper
/// in that palette while the browser runs light, which Playwright's does.
export function swatchPaper(page: Page, key: string): Promise<string> {
  return page
    .locator(`.swatch:has(input[value="${key}"]) .bars i`)
    .first()
    .evaluate((bar) => getComputedStyle(bar).backgroundColor);
}

/// A swatch, clicked where a person clicks it: its label.
export function pick(page: Page, key: string): Promise<void> {
  return page.locator('.swatch', { has: page.locator(`input[value="${key}"]`) }).click();
}
