# FFM — Family Finances Manager

A shared household ledger that replaces a spreadsheet. It can be used by one
person, a couple or the whole family. It is keyboard-first and Vim-inspired, and
fully usable with a mouse.

- **Why it exists and what it does:** `design/overview.md`
- **The working agreement** (stack, decisions, conventions): `CLAUDE.md`
- **Keymap, row states, config schema:** `design/spec.md`

## Getting started

The dev toolchain is pinned in `mise.toml`: the standalone `tailwindcss` binary,
`just`, `cargo-watch`, `sqlx-cli` and `python`. Rust is pinned separately in
`rust-toolchain.toml`, which rustup reads (see below).

```sh
# 1. mise itself, once per machine
brew install mise                  # macOS
sudo pacman -S mise                # Arch Linux
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc   # then open a new shell

# 2. Rust, once per machine. rustup installs the toolchain and components
#    rust-toolchain.toml asks for on the first cargo call in this repo.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # macOS
sudo pacman -S rustup && rustup default stable                   # Arch Linux

# 3. Tools mise does not manage
brew install poppler qpdf ghostscript           # macOS — sqlite3 ships with the OS
sudo pacman -S poppler qpdf ghostscript sqlite  # Arch Linux
                                                # poppler: pdftoppm / pdftotext, for the P
                                                # carousel and bill import
                                                # qpdf: unlocks password-protected PDFs,
                                                # and drops pages before a PDF is filed
                                                # ghostscript (optional): shrinks them

# 4. The pinned tools
mise trust                 # allow this repo's mise.toml
mise install

# 5. Config, a vault, and a household to look at
cp ffm.example.toml ffm.toml
mkdir -p receipts          # receipts_root must exist, or ffm refuses to start
just seed                  # creates ffm.db, applies migrations, loads sample data

# 6. Run it
just dev                   # http://127.0.0.1:3000, rebuilds and reloads on save
```

`just` with no arguments lists every recipe. `just check` runs everything CI
would: fmt, clippy, tests, the offline sqlx build, and the css/template/js/
command checks.

If you'd rather not activate mise in your shell, prefix commands with
`mise exec --`, for example `mise exec -- just dev`.

## Updating dependencies

### mise tools

```sh
mise outdated --bump       # what has a newer version than the pin (plain
                           # `mise outdated` says nothing: the pins are exact)
mise upgrade --bump        # install the new versions AND rewrite the pins in mise.toml
mise upgrade --bump just   # or just one tool
```

`--bump` is what changes `mise.toml`. Without it, `mise upgrade` only installs
newer versions that still match the current pins, and the pins here are exact.
Commit the updated `mise.toml`.

Two pins are tied to something else, so don't bump them on their own:

- **`sqlx-cli`** must match the `sqlx` version in `Cargo.lock`. Bump both
  together, then run `just prepare` and commit `.sqlx/`.
- **`tailwindcss`**: after bumping, run `just css-build` and check the app in
  Safari and Chrome. v4 minors occasionally change the output.

mise itself is updated the way it was installed: `brew upgrade mise` on macOS,
`sudo pacman -Syu` on Arch Linux.

### Rust — rustup, not mise

mise does not manage Rust here. `rust-toolchain.toml` pins `channel = "stable"`
with `rustfmt`, `clippy`, `rust-analyzer` and `rust-src`, and rustup owns it:

```sh
rustup update              # moves stable forward; the components come with it
cargo update               # crate versions in Cargo.lock
```

Pinning Rust in `mise.toml` too would just call rustup underneath, and it would
be a second pin that could disagree with the first.

### Everything else

`brew upgrade poppler qpdf ghostscript` on macOS, where `sqlite3` and `safaridriver` come
with the OS. On Arch Linux, `sudo pacman -Syu` covers `poppler`, `qpdf`,
`ghostscript` and `sqlite`. There is no
`safaridriver` on Linux, so the Safari half of UI testing needs a Mac.
