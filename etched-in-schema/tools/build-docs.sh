#!/bin/sh
# Build the docs site: mdBook guide + rustdoc API, combined into site/.
# Same steps the Pages workflow runs. No network required after setup.
set -eu
cd "$(dirname "$0")/.."
rm -rf site
mdbook build book -d "$PWD/site"
for crate in approvals; do
  (cd "$crate" && cargo doc --no-deps --quiet)
  mkdir -p "site/api"
  cp -r "$crate/target/doc/${crate}/" "site/api/${crate}/"
done
echo "wrote site/ ($(find site -type f | wc -l) files)"
