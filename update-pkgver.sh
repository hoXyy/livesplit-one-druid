#!/usr/bin/env bash

set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "${project_dir}"

# Fetch/update the VCS source and let makepkg run pkgver(), without building.
makepkg --nobuild --nodeps

updated_version="$(sed -n 's/^pkgver=//p' PKGBUILD)"
printf 'Updated PKGBUILD to pkgver=%s\n' "${updated_version}"
