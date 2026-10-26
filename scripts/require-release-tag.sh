#!/usr/bin/env bash
# Fail when a GitHub Release tag does not match the workspace version.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
version="$(grep -E '^version = ' "$ROOT/Cargo.toml" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
tag="${GITHUB_REF_NAME#v}"

if [[ -z "$version" ]]; then
  echo "Could not read the workspace version."
  exit 1
fi

if [[ "$GITHUB_REF_NAME" != v* || "$tag" != "$version" ]]; then
  echo "Tag ${GITHUB_REF_NAME} does not match workspace version ${version}."
  echo "Create the release from tag v${version}."
  exit 1
fi

echo "Release tag v${version} matches the workspace."
