#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STYLE="${1:-notionists}"
SIZE="${2:-192}"
OUTPUT_DIR="$ROOT_DIR/assets/avatars"

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

mkdir -p "$OUTPUT_DIR"

mapfile -t ids < <(
    awk '$1 == "id" && $2 == "=" && $3 ~ /^[0-9]+$/ { print $3 }' "$ROOT_DIR/exchange.toml" \
        | sort -n -u
)
if ((${#ids[@]} == 0)); then
    printf 'No subscriber IDs found in %s.\n' "$ROOT_DIR/exchange.toml" >&2
    exit 1
fi

for id in "${ids[@]}"; do
    output="$OUTPUT_DIR/$id.png"
    url="https://api.dicebear.com/9.x/$STYLE/png?seed=$id&size=$SIZE&backgroundColor=ffffff"
    printf 'Generating %s (%s)\n' "$output" "$STYLE"
    curl --fail --silent --show-error --location "$url" --output "$output"
done

printf 'Generated %d avatars in %s\n' "${#ids[@]}" "$OUTPUT_DIR"
