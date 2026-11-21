#!/usr/bin/env bash
# Builds "VYRO Client.app" and installs it into /Applications (or ~/Applications if that is not writable),
# so it can be opened like any other app (Spotlight, Dock, double-click). Run: npm run package:mac
set -euo pipefail
cd "$(dirname "$0")/.."

# 1) Refresh the embedded in-game mod. That needs Java 25 on PATH; without it we keep whatever is embedded already.
JAVA_MAJOR=$(java -version 2>&1 | head -1 | sed -E 's/.*"([0-9]+).*/\1/' || echo 0)
if [ "${JAVA_MAJOR:-0}" -ge 25 ] 2>/dev/null; then
  bash scripts/sync-mod.sh
else
  echo "Java 25 is not active in this terminal, so the in-game mod is NOT refreshed in this build."
  echo "(Run the two 'export JAVA_HOME=...' lines for Java 25 first if you want the latest mod bundled.)"
fi

# 2) Build the app bundle (the first release build takes a while)
npm install
npm run tauri build -- --bundles app

APP="src-tauri/target/release/bundle/macos/VYRO Client.app"
if [ ! -d "$APP" ]; then echo "Build finished but $APP was not found."; exit 1; fi

# 3) Install
DEST="/Applications"
if [ ! -w "$DEST" ]; then DEST="$HOME/Applications"; mkdir -p "$DEST"; fi
rm -rf "$DEST/VYRO Client.app"
cp -R "$APP" "$DEST/"
echo
echo "Installed: $DEST/VYRO Client.app"
echo "Open it from Spotlight (Cmd+Space, type VYRO) or from the Applications folder."
