#!/bin/sh
# Build Slingshot.app and install it in ~/Applications. Then run: slingshot menubar
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
app="$here/.build/Slingshot.app"
destination="$HOME/Applications"

if ! command -v swift >/dev/null 2>&1; then
    echo "✗ Swift was not found. Install the Xcode command line tools with: xcode-select --install" >&2
    exit 1
fi

swift build -c release --package-path "$here"
binary="$(swift build -c release --package-path "$here" --show-bin-path)/SlingshotMenuBar"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Slingshot"
cp "$here/Info.plist" "$app/Contents/Info.plist"
codesign --force --sign - "$app"
echo "✓ Built $app"

osascript -e 'quit app id "dev.slingshot.menubar"' >/dev/null 2>&1 || true
for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -f "$destination/Slingshot.app/Contents/MacOS/Slingshot" >/dev/null || break
    sleep 0.5
done
mkdir -p "$destination"
rm -rf "$destination/Slingshot.app"
cp -R "$app" "$destination/"
echo "✓ Installed ~/Applications/Slingshot.app"
echo "  Open it with: slingshot menubar"
