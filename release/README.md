# FFM — Family Finances Manager

A shared household ledger. This folder is everything needed to run it.

## Requirements

- **poppler** — `pdftotext`, `pdftoppm` and `pdfinfo`, for bill import and the
  `P` file preview. Debian/Ubuntu: `apt install poppler-utils` · Fedora:
  `dnf install poppler-utils` · macOS: `brew install poppler`.
- **qpdf** — opens a bank's password-protected PDFs with the rule's PDF
  password, so the vault keeps an unlocked copy. Only a locked file needs it.
  Debian/Ubuntu: `apt install qpdf` · Fedora: `dnf install qpdf` · macOS:
  `brew install qpdf`.
- **`TZ` set to the household's time zone** (e.g. `TZ=America/Sao_Paulo`).
  "Today" — due dates, overdue bills, the month the ledger opens on — is the
  machine's local date. The boot log prints the zone and date it is using.
- Nothing else. The Linux builds are fully static: no glibc version to match.

## Start

```sh
./start.sh
```

The first run writes `ffm.toml` and creates `receipts/` and `backups/`, all in
this folder, then serves on port 3000 on every interface. Open
`http://<this machine>:3000` — the boot log prints the address — and `/setup`
creates the household and its first admin.

- `FFM_ADDR=127.0.0.1:3000 ./start.sh` keeps it to this machine.
- `ffm.toml` holds three paths — the database, the receipt vault, backups.
  Each can also be set with `FFM_DB_PATH`, `FFM_RECEIPTS_ROOT`,
  `FFM_BACKUP_DIR`; the environment wins.
- `config/issuers.toml` is yours to edit: the bill issuers the importer knows.

**Keep it on your own network** (LAN, Tailscale). It is built for a household
behind its own router, not for the open internet.

## Lost the only admin password

```sh
./start.sh reset-link <name>
```

prints a one-time link to set a new one, run on this machine.

## Upgrade

1. Stop the server.
2. Copy `ffm`, `static/` and `start.sh` from the new release over these.
   Keep `ffm.toml`, `config/`, `ffm.db`, `receipts/` and `backups/` — they are
   the household's.
3. Start it. Database changes are applied on start.

The version is on the first line of `:profile`, and in the boot log.

## Run it as a service (Linux, systemd)

```ini
# /etc/systemd/system/ffm.service
[Unit]
Description=FFM household ledger
After=network.target

[Service]
WorkingDirectory=/opt/ffm
ExecStart=/opt/ffm/start.sh
Environment=TZ=America/Sao_Paulo
User=ffm
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

`systemctl stop ffm` sends SIGTERM, which the server answers by writing the
database out whole before it exits.
