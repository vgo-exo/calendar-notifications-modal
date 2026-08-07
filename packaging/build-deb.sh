#!/usr/bin/env bash
# Build Debian package for Calendar Notifications Modal

set -euo pipefail

BIN_NAME="calendar-notifications-modal"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "════════════════════════════════════════════════════════════════"
echo "  Building Debian Package for $BIN_NAME"
echo "════════════════════════════════════════════════════════════════"
echo ""

cd "$REPO_ROOT"

# Check if cargo-deb is installed
if ! command -v cargo-deb >/dev/null 2>&1; then
    echo "⚠️  cargo-deb is not installed."
    echo "Installing cargo-deb..."
    cargo install cargo-deb
fi

echo "✓ cargo-deb is available"
echo ""

# Check if LICENSE file exists, create if needed
if [ ! -f LICENSE ]; then
    echo "Creating LICENSE file..."
    cat > LICENSE << 'EOFLIC'
MIT License

Copyright (c) 2024-2026 Calendar Notifications Modal Team

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
EOFLIC
fi

echo ">> Building release binary..."
cargo build --release --bin "$BIN_NAME"
echo ""

echo ">> Building Debian package..."
cargo deb --no-build -p cnm-app
echo ""

# Find the generated .deb file
DEB_FILE=$(find target/debian -name "*.deb" -type f | head -1)

if [ -z "$DEB_FILE" ]; then
    echo "❌ Error: No .deb file found!"
    exit 1
fi

# Copy to root for easy access
cp "$DEB_FILE" .
DEB_NAME=$(basename "$DEB_FILE")

echo "════════════════════════════════════════════════════════════════"
echo "✅ Debian package built successfully!"
echo "════════════════════════════════════════════════════════════════"
echo ""
echo "Package: $DEB_NAME"
echo "Location: $REPO_ROOT/$DEB_NAME"
echo ""
echo "Package info:"
dpkg -I "$DEB_NAME"
echo ""
echo "Package contents:"
dpkg -c "$DEB_NAME"
echo ""
echo "To install locally:"
echo "  sudo dpkg -i $DEB_NAME"
echo "  sudo apt --fix-broken install  # If dependencies are missing"
echo ""
echo "To share with coworkers:"
echo "  Just send them the $DEB_NAME file!"
echo "  They can install with: sudo dpkg -i $DEB_NAME"
