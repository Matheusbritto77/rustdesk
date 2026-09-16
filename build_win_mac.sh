#!/usr/bin/env bash
set -e

echo "=================================================="
echo "  Building Remora Windows (.exe) on macOS Host    "
echo "=================================================="

# 1. Ensure target is installed
if ! rustup target list | grep -q "x86_64-pc-windows-gnu (installed)"; then
    echo "[+] Installing rustup target x86_64-pc-windows-gnu..."
    rustup target add x86_64-pc-windows-gnu
fi

# 2. Check for MinGW cross-linker
if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
    echo "[!] Warning: x86_64-w64-mingw32-gcc not found in PATH."
    echo "    To install via Homebrew: brew install mingw-w64"
fi

# 3. Set environment and static C/C++ runtime linking flags
export VCPKG_ROOT="${VCPKG_ROOT:-/Users/matheusbrito/vcpkg}"
export RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-static -C link-arg=-static-libgcc -C link-arg=-static-libstdc++ -C link-arg=-Wl,-Bstatic"

# Fix bindgen header search path when cross-compiling on macOS with MinGW
if [ -d "/opt/homebrew/opt/mingw-w64/toolchain-x86_64/x86_64-w64-mingw32/include" ]; then
    export BINDGEN_EXTRA_CLANG_ARGS="--target=x86_64-w64-mingw32 -I/opt/homebrew/opt/mingw-w64/toolchain-x86_64/x86_64-w64-mingw32/include"
fi

# 4. Build release binary
echo "[+] Compiling Remora Windows release executable..."
cargo build --target x86_64-pc-windows-gnu --release

# 5. Prepare output bundle
BUNDLE_DIR="target/release-win-bundle"
mkdir -p "$BUNDLE_DIR"

if [ -f "target/x86_64-pc-windows-gnu/release/remora.exe" ]; then
    cp "target/x86_64-pc-windows-gnu/release/remora.exe" "$BUNDLE_DIR/remora.exe"
    echo "[+] Copied remora.exe to $BUNDLE_DIR/"
fi

# 6. Check for sciter.dll (required for Sciter GUI)
if [ -f "res/sciter.dll" ]; then
    cp "res/sciter.dll" "$BUNDLE_DIR/sciter.dll"
    echo "[+] Bundled res/sciter.dll into $BUNDLE_DIR/"
elif [ -f "sciter.dll" ]; then
    cp "sciter.dll" "$BUNDLE_DIR/sciter.dll"
    echo "[+] Bundled sciter.dll into $BUNDLE_DIR/"
else
    echo "[!] Notice: If running Sciter mode on Windows, place 64-bit sciter.dll alongside remora.exe in $BUNDLE_DIR/"
fi

echo "=================================================="
echo "  Build Completed Successfully!                  "
echo "  Output Directory: $BUNDLE_DIR/                 "
echo "=================================================="
