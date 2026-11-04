import { expect, test, type App } from '../fixtures';

/// A tracked bill with a bill on file and no receipt, due on the 11th.
function dueOnThe11th(app: App): number {
  const [row] = app.sql<{ id: number }>(
    `SELECT t.id FROM transactions t JOIN recurring_rules r ON r.id = t.rule_id
     WHERE t.period = '2026-09' AND r.tracked = 1 AND r.due_day = 11`,
  );
  return row.id;
}

test.describe('before anything is due', () => {
  test.use({ today: '2026-09-03' });

  test('the ring is not red', async ({ app, page }) => {
    await app.open(page, app.admin);
    await expect(page.locator(`#row-${dueOnThe11th(app)} circle[stroke="var(--neg)"]`)).toHaveCount(0);
  });
});

test.describe('past the due date', () => {
  test.use({ today: '2026-09-20' });

  // The dot leaves out the current month: it is where the person already is.
  test('the ring goes red, and the month picker has no dot yet', async ({ app, page }) => {
    await app.open(page, app.admin);
    await expect(page.locator(`#row-${dueOnThe11th(app)} circle[stroke="var(--neg)"]`)).toHaveCount(1);
    await expect(page.locator('.mlabel .dot')).toHaveCount(0);
  });
});

test.describe('into a month the data has not reached', () => {
  test.use({ today: '2026-10-15' });

  test('September, left unpaid, puts the dot on the month picker', async ({ app, page }) => {
    await app.open(page, app.admin);
    await expect(page.locator('.mlabel .dot')).toBeVisible();
  });

  test('the materializer brings the month, and the multi-month bill stamps it', async ({ app, page }) => {
    await app.open(page, app.admin);
    await expect(page.locator('.ledger-shell')).toHaveAttribute('data-period', '2026-10');

    // The one rule with a code waiting for this month in rule_bill_code.
    const [row] = app.sql<{ id: number; due_date: string; barcode_raw: string; code: string }>(
      `SELECT t.id, t.due_date, t.barcode_raw, c.barcode_raw AS code
       FROM transactions t
       JOIN recurring_rules r ON r.id = t.rule_id AND r.spans_period = 1
       JOIN rule_bill_code c ON c.rule_id = r.id AND c.period = t.period AND c.retired_at IS NULL
       WHERE t.period = '2026-10'`,
    );
    expect(row.barcode_raw).toBe(row.code);
    await expect(page.locator(`#row-${row.id}`)).toBeVisible();
  });
});
