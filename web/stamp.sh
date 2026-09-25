#!/usr/bin/env sh
# Stamps web/index.html with a version made from the content of the page's
# files: the import map, the script tag, the WebAssembly file and the year's
# data are then fetched as ?v=<version>, so after any change every browser
# loads all of them anew instead of mixing cached and new modules.
set -eu
cd "$(dirname "$0")"
v=$(cat pkg/web_demo.js pkg/web_demo_bg.wasm app.js i18n.js blocks.js year.js flex.js year2025.json flex_days.json | sha1sum | cut -c1-12)
sed -i -e "s/data-build=\"[^\"]*\"/data-build=\"$v\"/" -e "s/?v=[A-Za-z0-9]*\"/?v=$v\"/g" index.html
echo "stamped index.html with version $v"
