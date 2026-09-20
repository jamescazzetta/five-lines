#!/bin/sh
# Install the latest five-lines release for this machine (macOS and Linux).
#   curl -fsSL https://raw.githubusercontent.com/jamescazzetta/five-lines/main/install.sh | sh
# Set FIVE_LINES_REPO=owner/repo to install from a fork, FIVE_LINES_DIR to choose the directory.
set -eu

repo="${FIVE_LINES_REPO:-jamescazzetta/five-lines}"
dir="${FIVE_LINES_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)               target=aarch64-apple-darwin ;;
  Darwin-x86_64)              target=x86_64-apple-darwin ;;
  Linux-x86_64)               target=x86_64-unknown-linux-musl ;;
  Linux-aarch64|Linux-arm64)  target=aarch64-unknown-linux-gnu ;;
  *) echo "no prebuilt binary for $(uname -s) $(uname -m); use: cargo install --git https://github.com/$repo" >&2; exit 1 ;;
esac

tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
[ -n "$tag" ] || { echo "could not find a release of $repo" >&2; exit 1; }

name="five-lines-$tag-$target"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "https://github.com/$repo/releases/download/$tag/$name.tar.gz" | tar -xz -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/$name/five-lines" "$dir/five-lines"
echo "installed five-lines $tag to $dir/five-lines"
case ":$PATH:" in *":$dir:"*) ;; *) echo "note: $dir is not on your PATH" ;; esac
