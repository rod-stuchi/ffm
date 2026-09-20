# FFM — Bill Import & Barcode Parsing

Spec for the file-upload feature: the user drops a bill PDF (barcode/boleto first,
payment receipt later) and the system fills in **issuer, value and due date**
without manual typing. A dialog appears only when the system genuinely cannot
decide — and "cannot decide" has two distinct meanings that must not be conflated.

> Companion to `CLAUDE.md`. Everything about no-hard-deletes, member attribution,
> the recurring-rule model and the `_` receipt convention applies here unchanged.

---

## 1. The core insight

**For value and due date, do not parse text. Do not OCR. Do not infer.**

Brazilian bills carry a FEBRABAN barcode that *encodes* the amount and (often) the
due date as digits, protected by check digits. This is a deterministic parse with
built-in error detection, not a heuristic extraction.

Verified in this repo's reference implementation: round-trip encode/decode passes,
and **423/423 single-digit corruptions (100%) are detected** rather than silently
producing a wrong number. For a system that writes money into a ledger, that
property is the entire reason to prefer the barcode over OCR.

OCR is a **last-resort fallback for scanned bills with no text layer** — never the
primary path.

### What the barcode never tells you

The **issuer name**. That always comes from the text layer, matched against an
issuer profile (§6).

---

## 2. Two barcode kinds

Detect by first digit. `8` → arrecadação. Anything else → cobrança.

### 2a. Boleto de cobrança — 44-digit barcode

Banks, credit-card faturas, condomínio, school fees, financing.

| Pos | Len | Field |
|---|---|---|
| 1–3 | 3 | Banco |
| 4 | 1 | Moeda (`9` = BRL) |
| 5 | 1 | DV geral (**módulo 11**) |
| 6–9 | 4 | **Fator de vencimento** → due date |
| 10–19 | 10 | **Valor** in centavos |
| 20–44 | 25 | Campo livre (bank-specific) |

**Linha digitável — 47 digits**, a rearrangement with extra check digits:

| Block | Digits | Contents | DV |
|---|---|---|---|
| Campo 1 | 10 | banco(3) + moeda(1) + livre[20–24] | mod 10 |
| Campo 2 | 11 | livre[25–34] | mod 10 |
| Campo 3 | 11 | livre[35–44] | mod 10 |
| Campo 4 | 1 | DV geral of the barcode | — |
| Campo 5 | 14 | fator(4) + valor(10) | — |

Reassembling barcode from linha: `campo1[0..4] + dvGeral + campo5 + campo1[4..9] + campo2[..-1] + campo3[..-1]`.

### 2b. Arrecadação / conta de consumo — 44-digit barcode

Energia, água, gás, telecom, tributos, multas. **This is most of your bills.**

| Pos | Len | Field |
|---|---|---|
| 1 | 1 | Produto — always `8` |
| 2 | 1 | Segmento (see below) |
| 3 | 1 | **Identificação de valor** — see §3, this is the trap |
| 4 | 1 | DV geral (mod 10 or mod 11, decided by pos 3) |
| 5–15 | 11 | **Valor** — meaning depends on pos 3 |
| 16–19 | 4 | Identificação da empresa/órgão |
| 20–44 | 25 | Campo livre |

**Segmento** (pos 2): `1` prefeitura · `2` saneamento · `3` energia/gás · `4` telecom ·
`5` órgãos governamentais · `6` empresas/multas · `7` uso exclusivo do banco ·
`9` uso exclusivo

**Linha digitável — 48 digits**: 4 blocks of 11 digits, each followed by its own
**mod 10** check digit.

**Due date is NOT a dedicated field.** FEBRABAN: *"In case due date is used
(AAAAMMDD), include it in the first 8 positions of the free field."* So when
present it sits at **positions 20–27**, and when absent you must fall back to the
text layer (§6). Validate it parses as a plausible date before trusting it — those
positions hold arbitrary issuer data when the convention isn't followed.

---

## 3. The value trap

**Position 3 of an arrecadação barcode declares what the 11 value digits mean.**

| Pos 3 | Value means | DV algorithm |
|---|---|---|
| `6` | Valor efetivo em reais | módulo 10 |
| `7` | **Quantidade de moeda** — *not* currency | módulo 10 |
| `8` | Valor efetivo em reais | módulo 11 |
| `9` | **Quantidade de moeda** — *not* currency | módulo 11 |

A `7` or `9` bill read as reais posts a plausible-looking **wrong number** to the
ledger. This is worse than failing to parse.

**Zeros are legal.** FEBRABAN: *"Zeros — somente na impossibilidade de utilizar o
valor."* On a **credit-card fatura this is the normal case**: the barcode carries
zeros because the payer chooses between the full total, the minimum payment, or
any amount in between.

### Therefore the parser returns a sum type, never a number

```rust
pub enum BarcodeAmount {
    /// tipo 6/8 with a non-zero value, or cobrança with valor != 0
    Fixed(Cents),
    /// all zeros — the payer decides the amount (cartão de crédito, some faturas)
    Open,
    /// tipo 7/9 — quantidade de moeda / indexed value. NOT currency.
    /// Never coerce this into Cents.
    Reference(u64),
}

pub enum DueDate {
    /// cobrança, from fator de vencimento
    Encoded(NaiveDate),
    /// arrecadação, campo livre positions 20–27 as AAAAMMDD
    FreeField(NaiveDate),
    /// not carried by the barcode — fall back to the issuer profile
    Absent,
}
```

---

## 4. The due date trap — fator de vencimento rollover

The 4-digit factor is a day count. It **overflowed and was reset**:

| Period | Base date | Anchor |
|---|---|---|
| until 21/02/2025 | **07/10/1997** | factor reached 9999 on 21/02/2025 |
| from 22/02/2025 | **29/05/2022** | factor **1000 = 22/02/2025** |

Get this wrong and every current bill lands ~27 years off.

`fator = 0000` means **no due date encoded** → `DueDate::Absent`.

**Disambiguation rule.** A bare factor is ambiguous for archived bills. Resolve by
proximity to the document's own issue date when you have one; otherwise prefer the
new base when the resulting date is ≥ 22/02/2025, else the legacy base. Rodrigo's
`~/Documents/contas` archive contains pre-2025 bills, so the legacy path is not
dead code.

```
9999 = 21/02/2025 (legacy base)   1000 = 22/02/2025 (new base)   9999 = 13/10/2049
```

---

## 5. Check digits

| Where | Algorithm |
|---|---|
| Cobrança — DV geral (pos 5) | módulo 11, weights 2→9 cycling right-to-left; `dv = 11 - (sum % 11)`; if result ∈ {0,10,11} → **1** |
| Cobrança — linha digitável blocks | módulo 10, weights 2,1 alternating; digits > 9 → subtract 9; `dv = (10 - sum % 10) % 10` |
| Arrecadação — pos 3 ∈ {6,7} | módulo 10 (same as above) over the other 43 digits |
| Arrecadação — pos 3 ∈ {8,9} | módulo 11 — **⚠ variants exist in the wild for remainder 0/1; validate against real bills before trusting** |
| Arrecadação — linha digitável blocks | módulo 10 per 11-digit block |

**A failed check digit is a hard stop.** Do not fall back to text extraction for a
value that failed its own checksum — report the misread and ask.

---

## 6. Issuer profiles

Per-issuer config is real and necessary, but it should be **small**, because the
barcode structure is universal. A profile declares rules **only for the holes the
barcode leaves open**:

1. Who is this? (never in the barcode)
2. Due date — *only* when `DueDate::Absent`
3. Amount — *only* when `Open` or `Reference`
4. Reference month / installment, if wanted
5. **Client/account code** — the subscriber's own account number (Sabesp's
   "matrícula", Enel's "número do cliente/UC", a bank's "número do contrato").
   FEBRABAN never encodes this — the barcode's job is value and due date, not
   who's paying — so like due date's fallback, it's always a text-layer regex.
   See "Client code lives on the rule" under §9 for where this is stored and
   shown.

If the barcode answered it, the profile must not restate it.

### Detect on CNPJ, not the trade name

Layouts get redesigned and brands get renamed; the CNPJ in the footer does not
move. Use the name only as a secondary signal.

### `config/issuers.toml`

```toml
[[issuer]]
id      = "enel-sp"
name    = "Energia · Enel SP"
detect  = ['\b\d{2}\.?\d{3}\.?\d{3}/?0001-?\d{2}\b']  # ← replace with the real CNPJ from a bill
kind    = "consumo"            # expected barcode kind; mismatch = warn, don't fail
recurring = "energia"          # prefill hint for r's Bucket/Bill name when creating
                                # a rule from a recognized issuer — no longer used to
                                # bind a drop to an existing rule, see §9

[issuer.due_date]
when  = "absent"               # consulted only if campo livre carries no AAAAMMDD
regex = 'Vencimento\s*(\d{2}/\d{2}/\d{4})'

# no [issuer.amount] — tipo 6/8 gives it to us

[issuer.client_code]
regex = 'N[uú]mero do Cliente\s*[:\-]?\s*([\d.\-]+)'

[[issuer]]
id     = "nubank-cartao"
name   = "Cartão · Nubank"
detect = ['\bNU PAGAMENTOS\b', '\b\d{2}\.?\d{3}\.?\d{3}/?0001-?\d{2}\b']
kind   = "cobranca"

[issuer.amount]
when    = "open"               # barcode is zeros — two payable numbers exist in the text
choose  = true                 # genuinely ambiguous → always ask, never guess
total   = 'Valor total\s*R\$\s*([\d.,]+)'
minimum = 'Pagamento m[ií]nimo\s*R\$\s*([\d.,]+)'

[issuer.due_date]
when  = "absent"
regex = 'Vencimento\s*(\d{2}/\d{2}/\d{4})'
```

**Amount `when` values:** `open` · `reference` · `always` (override the barcode —
requires an explicit reason in a comment).

Parse `R$ 1.234,56` with a single shared pt-BR money parser. Never `f64` — cents as
integers throughout.

---

## 7. Extraction pipeline

Run in order; stop as soon as every field is resolved.

| Tier | Step | Produces | Confidence |
|---|---|---|---|
| 0 | `pdftotext -layout` | text layer | — |
| 1 | regex for 47/48-digit runs, strip separators | candidate barcodes | — |
| 2 | classify kind, validate DV, parse | value, due date | **certain** if DV passes |
| 3 | `detect` regexes against text | issuer, recurring binding | high |
| 4 | profile regexes for remaining holes | due date and/or amount | medium |
| 5 | `ocrmypdf` / `tesseract -l por`, then retry 1–4 | — | low |

**Tier 5 runs only when tier 0 yields no usable text.** Most utility bills are
digital PDFs with a real text layer; OCR is for scans and photos.

---

## 8. When to show the dialog — two different reasons

These must look and read differently. Conflating them makes the good case feel
like a failure.

**(a) Genuine ambiguity — the system read the bill correctly.**
`BarcodeAmount::Open` with `choose = true`. Both amounts are known; a human must
pick. Prefill both.

> **Cartão · Nubank — vence 15/10/2026**
> Pay the total **R$ 2.184,90** or the minimum **R$ 436,98**?

**(b) Extraction failure — the system could not read something.**
No barcode found, DV failed, unknown issuer, or a hole with no profile rule.

> **Couldn't read the due date.** The barcode doesn't carry one and there's no
> rule for this issuer yet. *(field prefilled with whatever was found)*

A bill left without an amount by either case is also what leaves a tracked
rule's receipt filename without its amount suffix, where the rule asks for
one (`{month}_{amount}.{ext}`, see `design/spec.md`'s Edit fields): the
file keeps the plain `{month}.{ext}` name until a real value is known,
rather than being named after a zero nobody owes.

**Ask only for the fields that are actually missing.** A valid check digit means
value and date are certain — if only the issuer is unknown, ask for the name
alone, not an empty four-field form.

**A third variant this section used to describe — extraction failure asking
"which recurring rule is this? (or create one)" — no longer applies.** That
was for a panel-drop that couldn't infer which rule a file belonged to; see
"Drop target" under §9 for why that inference step was removed rather than
fixed. A file is now only ever parsed once it's already been dropped into a
*specific* rule's own upload control, so which rule it's for is never in
question — only (a) and (b) above, about the bill's own contents, remain.

---

## 9. Integration with the existing model

**Upload and the receipt tracker are the same feature from opposite ends.** Do not
build a second path.

| Upload | Effect |
|---|---|
| Barcode / boleto PDF (arrives first) | creates **or updates** the pending occurrence for the period |
| Payment receipt (arrives later) | flips that occurrence to **paid** |

This is the existing `bill_.pdf` → `bill.pdf` convention expressed as uploads
instead of filesystem watching. Both must converge on the same row.

**Bind by attaching the file inside the rule's own editor** — `A`'s upload
field at creation, or `E`'s afterward — rather than inferring which rule a
dropped file belongs to. This is a deliberate reversal of an earlier design;
see "Drop target" below for why.

**Everything in this document is spend-only.** A recurring *income* rule
(`I`) has no barcode, no boleto and no issuer to parse — money arriving
doesn't come with a payable document — so the whole import path is absent
from its form. Its slot-1 file is the invoice the household issued, which
is filed like any other attachment and never parsed. See CLAUDE.md's Income
section.

### Drop target: a notifier, not a binding path

An earlier design made a household member's **panel** the primary drop
target — parse whatever landed there, then infer which rule it belonged to
by `issuer.recurring` + period + value proximity, or (failing that) by
matching the dropped file's *name* against a row's title. Both inference
paths were rejected: a real bill's filename, or even its parsed content,
shouldn't have to line up with something already in the ledger for the
household to trust the result, and a wrong guess would post a stranger's
bill onto the wrong rule with nothing forcing a second look.

The replacement removes the inference step entirely rather than trying to
make it more confident:

- **A barcode file is only ever parsed after it's been dropped into a
  specific rule's own upload control** (`A` when creating, `E` when
  editing). Which rule it's for is never in question — the household is
  looking straight at the one they mean, the same way typing an amount
  into a specific occurrence's edit form was never ambiguous either.
- **Dropping a file onto a panel now binds nothing.** It's a passive
  nudge: scan that panel for recurring, tracked rules that expect a
  barcode (`no_barcode` unset) but don't have a `client_code` yet, and
  name them in the statusline if any exist — *"2 recurring bills still
  need a barcode file (Água, Energia) — focus one and press R to set it
  up."* If nothing is pending, the dropped file is discarded with no
  message at all. The panel still decides member attribution for a
  **new** rule created via `A` (same "which panel, not who's logged in"
  rule `CLAUDE.md` states for `a`/`i`/`:add`) — it just never resolves a
  dropped file to a rule, because that resolution doesn't happen at the
  panel anymore.
- **§2–§7's extraction pipeline is unchanged** — FEBRABAN decoding, issuer
  profiles, the value/due-date/client-code holes all still run on whatever
  file lands in the upload control. Only the *targeting* step (which rule
  does this belong to) moved from "the system infers it" to "the user
  already told you by which rule's form they opened."

### Client code lives on the rule, not the occurrence

Value, due date and `barcode_raw` are per-period facts — a new bill, a new
`linha digitável`, every month. The client/account code (§6, item 5) is
different: it's the same number every period, because it identifies the
*account*, not the bill. It belongs on the **recurring rule**, alongside
`bucket`/`bill_name`/`instance` — not duplicated onto every occurrence.

- **Populated by attaching a file in the rule's own editor, not typed by
  hand.** There's no form field for it — it's set (and later re-set) only
  by dropping a barcode file into `A`'s or `E`'s upload control, the same
  "no separate upload flow, re-attaching is the update" posture `CLAUDE.md`
  already takes for the multi-page-barcode window.
- **Shown read-only in the rule editor**, once set — not a manual field, so
  not one that opens for editing. Absent for a rule with no file attached
  yet, same as `instance` is absent when a rule doesn't need one.
- **A newly-attached file that decodes to a *different* client code than
  the one already on file is flagged, not silently overwritten** — same
  principle as a failed check digit (§5) or an out-of-window barcode page
  (`CLAUDE.md`'s multi-page barcode section): a mismatch here means either
  the household got sent a different account's bill, or the wrong file was
  attached, and both are worth a human glance before the number on file
  quietly changes.
- **Unlinkable independently of the rule itself.** A dedicated control
  clears just the code — not `tracked`, `bucket`, `bill_name` or `pattern`
  — for correcting "the wrong file got attached the first time" without
  undoing anything else about the rule. It doesn't close the editor, since
  the obvious next step is dropping the corrected file into the same,
  still-open upload control.

### Store the linha digitável on the transaction

It is the natural **idempotency key**: re-uploading the same bill must not create a
second row. It also lets value and due date be re-derived later without keeping the
PDF parsed, and gives the fixtures something stable to assert against.

### Suggested columns

On the **occurrence** — a new value, source and `linha digitável` every period:

```
barcode_raw        TEXT    -- digits only, no separators; UNIQUE per period
barcode_kind       TEXT    -- 'cobranca' | 'consumo'
amount_source      TEXT    -- 'barcode' | 'text' | 'user'
due_date_source    TEXT    -- 'fator' | 'free_field' | 'text' | 'user'
issuer_id          TEXT    -- FK into issuers.toml, nullable
```

On the **recurring rule** — the two columns here that aren't per-occurrence,
per "Client code lives on the rule" above:

```
no_barcode         BOOLEAN -- true = this bill never has one (PIX/TED/wire
                            -- transfer); decided once, at rule creation
client_code        TEXT    -- subscriber/account number; nullable until a
                            -- file is attached; null again if unlinked;
                            -- always null when no_barcode is true
```

Recording *where each value came from* is what lets you audit a wrong number later
and what drives the "should I ask?" decision.

---

## 10. Verified test vectors

Generated by the reference implementation; encode/decode round-trips cleanly.
Port these as Rust unit tests.

| # | kind | expected | linha digitável |
|---|---|---|---|
| 1 | cobrança | `Fixed(34362)` / 2026-09-15 | `34191790010104351004779102015009815700000034362` |
| 2 | cobrança | `Open` / 2026-10-10 — **credit-card case** | `26091234560000123450900012345005115950000000000` |
| 3 | cobrança | `Fixed(114800)` / 2024-03-08 — **legacy fator base** | `03390000102345678901723456789017996490000114800` |
| 4 | cobrança | `Fixed(5990)` / `DueDate::Absent` — fator `0000` | `23799999868887777666165555444432100000000005990` |
| 5 | consumo t6 | `Fixed(34362)` / free-field 2026-07-15 | `836600000035436212342022607150000000000000000000` |
| 6 | consumo t7 | `Reference(34362)` — **not currency** | `837200000037436212340000000000000000000000000000` |
| 7 | consumo t6 | `Open` | `846600000000000056780000000000000000000000000000` |

**Also port the corruption test.** Take vector 1, substitute every digit position
with each other digit (47 × 9 = 423 mutations), and assert **every one is
rejected**. This is the property that justifies the whole approach; if a refactor
breaks it, the ledger starts accepting wrong amounts silently.

---

## 11. Fixtures — the real answer to "every company is different"

Layouts change without warning. No schema survives that on its own; you can only
**detect** the change.

```
tests/fixtures/enel-sp/2026-07.txt            # extracted text, account no. redacted
tests/fixtures/enel-sp/2026-07.expected.toml  # issuer, amount, due date, sources
```

- Assert over **extracted text, not the PDF** — nothing sensitive enters the repo
  and tests run without poppler installed.
- **Redact** account numbers, CPF/CNPJ of the payer, and addresses. Keep the
  issuer's CNPJ (it is public and it is the detection key).
- One fixture per issuer per layout change. `~/Documents/contas` is the corpus.
- When a fixture starts failing, the issuer redesigned their bill — that is the
  signal to update the profile, and it should arrive from CI, not from a wrong
  number in the ledger.

---

## 12. Tooling decisions

| Concern | Choice | Why |
|---|---|---|
| Barcode parse | **Rust, in-app** | ~150 lines of integer math, zero deps. `CLAUDE.md`: *Rust owns anything related to money* — a sidecar puts the money path outside compile-time checking |
| PDF → text | **`pdftotext -layout`** subprocess (poppler) | Not a language, a binary. Handles the common case |
| OCR fallback | **`ocrmypdf` / `tesseract -l por`** subprocess | Only when tier 0 yields nothing |
| Issuer rules | **TOML** (`config/issuers.toml`) | Data, not code. New biller = config edit, not recompile. Same pattern as `ffm.toml` |
| Barcode *image* decode | `pyzbar` (Python), only if needed | The one job the CLI tools don't cover — when digits aren't in the text layer |

**Not awk.** **Not a Python parsing layer** for anything above.

---

## 13. Open questions

1. **Minimum payment creates revolving debt.** Paying the minimum on a card fatura
   doesn't settle it — the remainder returns next month with interest. The ledger
   has no concept of this. Log one OUT row for now; decide later whether a
   revolving balance is a first-class thing.
2. **Arrecadação mod 11 DV variants** (pos 3 ∈ {8,9}) — remainder 0/1 handling
   differs between implementations. Validate against real bills before shipping.
3. **PIX QR / Copia e Cola** (EMV BR Code) is increasingly present on bills and
   also encodes value + merchant deterministically. A second parser worth adding
   once the barcode path works.
4. **Multi-page / combined statements** — a card fatura lists dozens of purchases.
   Is that one row, or does FFM ever want the line items?

~~5. Which member owns an uploaded bill?~~ Decided, though the question
changed shape — see "Drop target: a notifier, not a binding path" under §9.
There's no more "uploaded bill" that needs a member guessed for it: a
barcode file is only ever attached inside a specific rule's own editor, and
that rule already has an owner. Panel attribution now only matters for a
**new** rule created via `A` — whichever panel is focused, exactly like
`a`/`i`/`:add`; `Ctrl-H`/`Ctrl-L` re-attribution still covers correction if
that's wrong.

---

## 14. References

- [FEBRABAN — "Layout" Padrão de Arrecadação/Recebimento com Utilização do Código de Barras (v7, 01/03/2023)](https://cmsarquivos.febraban.org.br/Arquivos/documentos/PDF/Layout%20-%20C%C3%B3digo%20de%20Barras%20-%20Vers%C3%A3o%207%20-%2001_03_2023_mn.pdf) — authoritative for the arrecadação layout, segmento table, position-3 semantics, and the "zeros" rule
- [FEBRABAN — Codbar (28/05/2004)](https://www.febraban.org.br/7Rof7SWg6qmyvwJcFwF7I0aSDf9jyV/sitefebraban/Codbar4-v28052004.pdf) — earlier revision, useful for cross-checking
- [Santander — Layout de Código de Barras (v34)](https://cms.santander.com.br/sites/WPS/documentos/arq-cobranca-barras-port-jan22/22-01-13_181337_cobran%C3%A7a+-+c%C3%B3digo+de+barras+portugu%C3%AAs.pdf) — a bank's own cobrança spec, concrete on campo livre
- [Data de vencimento e valor — fator de vencimento](https://boletobancario-codigodebarras.com/vencimento-valor/) — rollover, base dates
- [Nova regra de Fator de Vencimento FEBRABAN (2025)](https://ajuda.sankhya.com.br/hc/pt-br/articles/29458524627223-Nova-regra-de-Fator-de-Vencimento-FEBRABAN) — the 22/02/2025 reset
- [Campos do boleto, código de barras e linha digitável](https://sejaefi.com.br/blog/campos-dos-boletos-linha-digitavel) — readable overview of the 47-digit layout
- [Boleto com valor R$ 0,00](https://www.toolspace.com.br/blog/boleto-valor-zerado-o-que-significa) — the open-value case in practice
- [Tesouro Nacional — Código de barras GRU](https://cdn.tesouro.gov.br/sistemas-internos/apex/producao/sistemas/thot/arquivos/publicacoes/31629_1066641/Codigo_Barras_GRU_Simples_e_Judicial.pdf) — a worked arrecadação example with real field breakdowns

---

## 15. Implementation order

1. `boleto.rs` — both kinds, the sum types, DV validation, fator rollover.
   Port the 7 vectors + the 423-mutation corruption test. **No PDF involved yet.**
2. `issuers.rs` — TOML loader, `detect` matching, hole-filling rules.
3. Text extraction subprocess + the tier pipeline.
4. Fixture harness; seed it from `~/Documents/contas`.
5. Upload endpoint, binding to recurring rules, the two dialog variants.
6. OCR fallback — last, and only if real bills demand it.

Step 1 is fully testable with no I/O and no sample bills. Start there.
