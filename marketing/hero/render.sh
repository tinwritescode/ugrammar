#!/bin/sh
# Renders hero.html to marketing/readme-hero.png at 2x with headless Chrome.
set -e
cd "$(dirname "$0")"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$chrome" --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files --force-color-profile=srgb \
  --force-device-scale-factor=2 --window-size=1600,880 --virtual-time-budget=3000 \
  --screenshot="$PWD/../readme-hero.png" "file://$PWD/hero.html" 2>/dev/null
echo "wrote marketing/readme-hero.png"
