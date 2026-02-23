#!/bin/bash
# Build Overwatch and auto-install to /Applications

echo "Building Overwatch..."
cd "$(dirname "$0")/src-tauri"

# Build the app bundle
cargo tauri build --bundles app

if [ $? -eq 0 ]; then
    echo "Build successful. Installing to /Applications..."
    
    # Kill running instance
    killall overwatch 2>/dev/null
    
    # Copy to Applications
    cp -R target/release/bundle/macos/Overwatch.app /Applications/
    
    echo "✅ Installed to /Applications/Overwatch.app"
    echo "Launch with: open /Applications/Overwatch.app"
else
    echo "❌ Build failed"
    exit 1
fi
