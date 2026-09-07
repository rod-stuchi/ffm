## The row component

`templates/partials/row.html`, rendered from a `LedgerRow` struct. The status mark,
delta arrow and meta glyphs are **pure functions of fields** — put them as methods
on the struct, not as logic in the template.

Each visual channel encodes exactly one fact, so a row never has to be read to be
understood:

| State | Mark | Meaning |
|---|---|---|
| `paid` | filled green disc + checkered-flag date | tracked, receipt found on disk |
| `pending` | ring, filling green → amber → red | tracked, receipt not found yet |
| `plain` | hollow dot | untracked — **no paid/unpaid state exists at all** |
| income | filled dot, `+ R$`, positive colour | sums into the pane header |
| focused | accent left rail, gutter `0`, tinted ground | cursor position |
| selected | `--info` inset bar | `x` — a marker for a batch op, never a delete |
| private | 🔒 glyph | hidden from other members, never from admin |
| deleted | dimmed, strikethrough, deletion date | soft delete, row retained |
| restored | `↩ #id` | new row superseding the deleted one |
| skeleton | shimmer bars, **identical geometry** | in flight |
| error | red ground, message + **Try Again** | server said no, nothing was written |

### Specifics that carry meaning

- **The due ring is elapsed time**, not a countdown — from period start to due
  date. Three-quarters amber reads as "still room, but not much" with no legend.
- **The MoM delta is derived, not stored.** Compare each row to the same line in
  the previous month (a window function). Red ▲ up, green ▼ down, grey dash for
  unchanged.
- **Skeleton geometry must match the real row exactly**, so nothing shifts when
  the server's fragment lands.
- **Error copy says what happened and that nothing was written.** No apologies.
  "Try Again" resubmits the original action.

---

## Keymap

Lowercase acts on the **occurrence**, uppercase on the **rule or the window**.
Nothing destructive fires from a single keystroke.

### Move
| Key | Action |
|---|---|
| `j` / `k` | row down / up |
| `5j` `7k` | counted motion |
| `gg` / `G` | first / last row |
| `H` / `L` | focus member pane, left / right |
| `gt` / `gT` | next / previous member tab (tabs mode) |
| `2gt` | jump straight to tab 2 |
| `Ctrl-P` / `Ctrl-N` | previous / next month |

### Act
| Key | Action |
|---|---|
| `J` / `K` | move the row — or every selected row — down / up |
| `3J` | move it three places |
| `Ctrl-H` / `Ctrl-L` | **re-attribute** the row to the member pane left / right |
| `a` / `i` | add after / before the current row |
| `x` | toggle selection for a batch operation |
| `e` | edit this occurrence |
| `R` | edit the recurring rule behind it |
| `p` | toggle private *(proposal, not in the original spec)* |

### Find
| Key | Action |
|---|---|
| `/` | search the ledger — incremental, regex |
| `n` / `N` | next / previous match, wrapping |
| `Esc` | clear the search highlight |

### Undo
| Key | Action |
|---|---|
| `dd` | delete — opens a confirmation |
| `y` | confirm in the delete modal |
| `u` | undo the last change |
| `Esc` | clear count, buffer, selection and search |

### Type
| Key | Action |
|---|---|
| `:` | open the command bar |
| `:add 50 mercado publix` | a spend, in one line |
| `:in 2400 freelance` | an incoming |
| `:move ana` | re-attribute to another member |
| `:sort date` | row order: manual · date · amount |
| `:set rnu` | gutter: relative · absolute · off |
| `:theme gruvbox` | colourscheme (writes `ffm.toml`) |
| `:config` | open `ffm.toml` beside the ledger |
| `?` | keymap |

### Deliberate deviations from Vim

- **`?` opens the keymap, not reverse search.** `N` already walks a `/` search
  backwards, and `?`-as-help is the one key a non-Vim user will try. Half this
  app's userbase does not use Vim. One-line swap in the keydown switch if this
  is ever reversed.
- **`J` moves the row, not join-lines.** There is nothing to join in a ledger,
  and `j` moves me / `J` moves the thing is a clean extension of the
  lowercase/uppercase rule.
- **`Ctrl-H` / `Ctrl-L` are separate from `J` / `K` on purpose.** `J` reorders —
  cosmetic, affects nobody's totals. `Ctrl-L` re-attributes — it moves money
  between two people's INs/OUTs and changes who owns the row. Same-shaped
  gesture, very different blast radius; they must not share a keystroke you can
  overshoot into.

### Confirmation rule

**Confirm when the consequence is not obvious from the action — not merely when
the action writes something.**

- `dd` confirms: deletion reads as destructive even though it is a soft delete.
- Re-attributing an ordinary row does **not** confirm: it is fully undoable and
  the statusline says where it went.
- Re-attributing a **private** row **does** confirm: handing a hidden transaction
  to another member makes *them* the owner, so the person who hid it stops seeing
  it. That is invisible in the gesture.

---

## Configuration — `ffm.toml`

Lives in the repo, versioned with the code, watched at runtime (`notify` crate).
Parsed with `toml` + `serde` and a `#[derive(Deserialize)]` struct with
`#[serde(default)]`.

### The single-source-of-truth rule

**There is no settings dialog.** `:set`, `:sort`, `:theme` and the clickable
statusline cells **write back into the file**. If a setting could be changed in two
places that disagree, the file stops being the source of truth — which is the
entire point of the TOML approach (à la Yazi).

### Surface

The config opens as a **vertical split** beside the ledger — not a modal, not a
full-screen buffer. Reasons, so this is not relitigated:

- A full-screen buffer hides the ledger, and `layout.panes` is precisely the
  setting you need to *watch* take effect.
- A modal needs a backdrop, scroll-lock and a focus trap, and it implies a
  settings dialog with toggles — a second way to change one setting.
- A vsplit is just a second pane. `H`/`L` already means "move between panes". In
  HTMX it is one swap changing the grid from `1fr` to `360px 1fr`.

### Invalid config

**Keep the last valid config and say which line is wrong.** Do not fall back to
defaults mid-session — that silently changes behaviour the user did not ask for.
Error format: `✗ line 8 · layout.panes must be "auto", "split", "tabs" — got "tab"`.
Serde already reports the failing field; wire it to the line.

### Schema

```toml
[theme]
name = "ffm"            # any of the 11 bundled palettes
# accent = "#c4a7e7"    # override any of the 11 tokens

[layout]
panes = "auto"          # "auto" | "split" | "tabs"
min_split_width = 900   # px; below this, auto → tabs
max_width = 900         # px cap per pane; 0 = unlimited
gap = 16                # px between panes

[ledger]
sort = "manual"         # "manual" | "date" | "amount"
search_scope = "pane"   # "pane" | "household"
line_numbers = "relative"   # "relative" | "absolute" | "none"
show_delta = true
show_recurring = true

[privacy]
default_visibility = "household"   # "household" | "private"
```

`[theme]` is the one **open-ended** section: `name` plus any of the 11 tokens.
Model it as `#[serde(flatten)] HashMap<ThemeToken, Color>` with a strongly-typed
key enum, so an unknown key produces a real error naming the valid tokens.

---

