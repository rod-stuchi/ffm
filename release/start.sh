#!/bin/sh
# Start ffm from this folder — `./start.sh`, from anywhere.
#
# The server finds ffm.toml, config/ and static/ relative to where it runs, so
# this changes into its own folder first. Anything after the script's name is
# passed on: `./start.sh reset-link rodrigo` is the console's reset link.
set -eu
cd "$(dirname "$0")"

# First run: the example config as it ships — the database, the receipt vault
# and the backups all beside this script. The vault must exist before the
# server starts (it refuses a receipts_root that does not), so it is made here,
# and ONLY here: after the first run, a missing vault is a mistyped path to
# report, not a folder to conjure.
if [ ! -f ffm.toml ]; then
    cp ffm.example.toml ffm.toml
    mkdir -p receipts backups
    echo "ffm · first run: wrote ffm.toml — database, receipts and backups live in this folder"
    echo "ffm · edit ffm.toml to put the receipt vault somewhere else, then start again"
fi

# poppler is run as separate programs, not linked, so a missing one is only
# found the first time a bill is imported or a PDF previewed. Say so now.
for tool in pdftotext pdftoppm pdfinfo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "ffm · warning: $tool not found — bill import and the P preview need poppler" >&2
        echo "      Debian/Ubuntu: apt install poppler-utils · Fedora: dnf install poppler-utils · macOS: brew install poppler" >&2
        break
    fi
done

# qpdf opens a bank's password-protected PDFs with the rule's password, and
# drops the pages removed before a PDF is filed. Only a locked file, or a
# trimmed one, needs it — every other upload works without — so a warning,
# and only the one.
if ! command -v qpdf >/dev/null 2>&1; then
    echo "ffm · warning: qpdf not found — a password-protected PDF will be refused, and no PDF can be trimmed" >&2
    echo "      Debian/Ubuntu: apt install qpdf · Fedora: dnf install qpdf · macOS: brew install qpdf" >&2
fi

# Ghostscript shrinks a PDF before it is filed. Optional: without it the
# Shrink switch is simply absent, so this is a note rather than a warning.
if ! command -v gs >/dev/null 2>&1; then
    echo "ffm · note: Ghostscript (gs) not found — PDFs are filed without shrinking" >&2
    echo "      Debian/Ubuntu: apt install ghostscript · Fedora: dnf install ghostscript · macOS: brew install ghostscript" >&2
fi

# Every interface, so the phones and laptops of the house can reach it. The
# app has no business on the open internet (see README.md); set
# FFM_ADDR=127.0.0.1:3000 to keep it to this machine.
: "${FFM_ADDR:=0.0.0.0:3000}"
export FFM_ADDR

exec ./ffm "$@"
