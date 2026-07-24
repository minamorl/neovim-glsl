#!/bin/sh
# Assemble nvimgl.app. A bare CLI binary has no bundle identity, which is why
# macOS logs IMKCFRunLoopWakeUpReliable errors and the process has no Dock or
# menu-bar presence even though the IME itself works.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TOOLCHAIN="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin"
# Homebrew rustc aborts on this machine (libLLVM ABI mismatch), and naming
# cargo by path is not enough because cargo resolves `rustc` from PATH.
[ -d "$TOOLCHAIN" ] && PATH="$TOOLCHAIN:$PATH"
export PATH

PROFILE=${PROFILE:-release}
case "$PROFILE" in
    release) cargo build --release ;;
    debug)   cargo build ;;
    *) echo "PROFILE must be release or debug" >&2; exit 2 ;;
esac

APP="$ROOT/nvimgl.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/target/$PROFILE/nvimgl" "$APP/Contents/MacOS/nvimgl"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>              <string>nvimgl</string>
    <key>CFBundleDisplayName</key>       <string>nvimgl</string>
    <key>CFBundleExecutable</key>        <string>nvimgl</string>
    <key>CFBundleIdentifier</key>        <string>dev.minamorl.nvimgl</string>
    <key>CFBundlePackageType</key>       <string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key>           <string>0.1.0</string>
    <key>LSMinimumSystemVersion</key>    <string>11.0</string>
    <key>NSHighResolutionCapable</key>   <true/>
    <!-- Regular so the window can become key and the IME has a real client. -->
    <key>LSUIElement</key>               <false/>
</dict>
PLIST
printf '</plist>\n' >> "$APP/Contents/Info.plist"

# Ad-hoc signature: unsigned bundles get inconsistent input-method treatment.
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "warn: codesign failed (bundle still runs)" >&2

echo "built $APP"
