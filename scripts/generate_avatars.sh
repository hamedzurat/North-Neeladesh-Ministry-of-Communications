#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STYLE="${1:-notionists}"
SIZE="${2:-192}"
OUTPUT_DIR="$ROOT_DIR/assets/avatars"
COUNT="${3:-256}"

if [[ ! "$STYLE" =~ ^[a-z0-9-]+$ ]]; then
    printf 'Style must contain only lowercase letters, numbers, and hyphens.\n' >&2
    exit 2
fi
if [[ ! "$SIZE" =~ ^[1-9][0-9]*$ ]]; then
    printf 'Size must be a positive integer.\n' >&2
    exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
    printf 'curl is required.\n' >&2
    exit 1
fi
if [[ ! "$COUNT" =~ ^[1-9][0-9]*$ ]] || ((COUNT > 10000)); then
    printf 'Count must be between 1 and 10000.\n' >&2
    exit 2
fi

mkdir -p "$OUTPUT_DIR"

mapfile -t ids < <(seq 0 $((COUNT - 1)))

for id in "${ids[@]}"; do
    output="$OUTPUT_DIR/$(printf '%04d' "$id").png"
    url="https://api.dicebear.com/9.x/$STYLE/png?seed=$id&size=$SIZE&backgroundColor=ffffff"
    printf 'Generating %s (%s)\n' "$output" "$STYLE"
    curl --fail --silent --show-error --location "$url" --output "$output"
done

printf 'Generated %d avatars in %s\n' "${#ids[@]}" "$OUTPUT_DIR"
