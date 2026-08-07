#!/bin/bash
# Quick verification script to check if snap package is ready to build

set -e

echo "🔍 Verifying snap package setup..."
echo ""

ERRORS=0

# Check required files
echo "Checking required files..."
FILES=(
    "snap/snapcraft.yaml"
    "snap/hooks/install"
    "snap/hooks/configure"
    "snap/local/start-daemon.sh"
    "snap/local/calendar-notifications-modal-wrapper.sh"
    "snap/local/config-template.toml"
)

for file in "${FILES[@]}"; do
    if [ -f "$file" ]; then
        echo "  ✅ $file"
    else
        echo "  ❌ $file - MISSING"
        ERRORS=$((ERRORS + 1))
    fi
done

echo ""

# Check if scripts are executable
echo "Checking executable permissions..."
SCRIPTS=(
    "snap/hooks/install"
    "snap/hooks/configure"
    "snap/local/start-daemon.sh"
    "snap/local/calendar-notifications-modal-wrapper.sh"
)

for script in "${SCRIPTS[@]}"; do
    if [ -x "$script" ]; then
        echo "  ✅ $script is executable"
    else
        echo "  ❌ $script is NOT executable"
        ERRORS=$((ERRORS + 1))
    fi
done

echo ""

# Check if snapcraft is installed
echo "Checking snapcraft installation..."
if command -v snapcraft >/dev/null 2>&1; then
    VERSION=$(snapcraft --version 2>/dev/null | head -1)
    echo "  ✅ snapcraft is installed: $VERSION"
else
    echo "  ⚠️  snapcraft is NOT installed"
    echo "     Install with: sudo snap install snapcraft --classic"
    ERRORS=$((ERRORS + 1))
fi

echo ""

# Check Rust installation
echo "Checking Rust installation..."
if command -v cargo >/dev/null 2>&1; then
    VERSION=$(cargo --version)
    echo "  ✅ $VERSION"
else
    echo "  ⚠️  Rust is NOT installed"
    echo "     Install from: https://rustup.rs"
    ERRORS=$((ERRORS + 1))
fi

echo ""

# Check GTK4 dependencies
echo "Checking GTK4 development libraries..."
if pkg-config --exists gtk4 2>/dev/null; then
    VERSION=$(pkg-config --modversion gtk4)
    echo "  ✅ GTK4 is installed: $VERSION"
else
    echo "  ⚠️  GTK4 dev libraries NOT found"
    echo "     Install with: sudo apt install libgtk-4-dev"
    ERRORS=$((ERRORS + 1))
fi

echo ""
echo "════════════════════════════════════════════════════════════════"

if [ $ERRORS -eq 0 ]; then
    echo "✅ All checks passed! Ready to build snap package."
    echo ""
    echo "Build with:"
    echo "  make snap-all       # Build and install"
    echo "  make build-snap     # Just build"
    echo "  snapcraft           # Direct snapcraft command"
    exit 0
else
    echo "❌ Found $ERRORS error(s). Please fix them before building."
    exit 1
fi
