#!/usr/bin/env bash
# The glasses package of a release: rynx with its voice controller, set up to start in glasses
# mode, for trying rynx on glasses (or a laptop standing in for them) without building it.
#
#   scripts/package-glasses.sh FOLDER RYNX     FOLDER made from RYNX (a built rynx), then
#                                              FOLDER.tar.gz
#
# Unix only: the control socket the voice controller uses isn't on Windows yet.
set -euo pipefail

folder=${1:?the package folder to make}
program=${2:?the rynx program to put in it}
source=$(cd "$(dirname "$0")/.." && pwd)

rm -rf "$folder"
mkdir -p "$folder/layouts"
cp "$program" "$folder/rynx"
cp "$source/scripts/rynx-glasses" "$folder/rynx-glasses"
chmod +x "$folder/rynx" "$folder/rynx-glasses"
cp "$source/glasses/voice.py" "$folder/"
cp "$source"/glasses/layouts/*.conf "$folder/layouts/"
cp "$source/glasses/PACKAGE.md" "$folder/README.md"
cp "$source/glasses/README.md" "$folder/GUIDE.md"
cp "$source/LICENSE.txt" "$folder/"
# The archive holds the folder by its own name, wherever it was made
tar -C "$(dirname "$folder")" -czf "$folder.tar.gz" "$(basename "$folder")"
