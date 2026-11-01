#!/usr/bin/env bash
# Build the nc-undercut external plugin package into dist/.
# The dist directory is the installable plugin package: plugin.json, the
# binary, standards tables, templates and an empty data/ for imported
# custom standards.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release --locked 2>/dev/null || cargo build --release

DIST=dist
rm -rf "$DIST"
mkdir -p "$DIST/standards" "$DIST/templates" "$DIST/data"
cp target/release/nc-undercut "$DIST/nc-undercut"
cp plugin.json "$DIST/plugin.json"
cp standards/v3-grooves.json "$DIST/standards/"
cp templates/program.j2 "$DIST/templates/"

echo "package: $DIST"
