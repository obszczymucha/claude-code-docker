#!/usr/bin/env bash
# Moves every .claude-code-extra-mounts / .claude-code-extra-hosts under $1
# into claude-docker.sh's CONFIG_FILE. The old files are only listed unless
# --delete is given. Re-running is safe: new entries are appended, existing ones skipped.
# Usage: migrate-extra-files.sh <base-dir> [--delete]
set -euo pipefail

# shellcheck source=claude-docker.sh
source "$(dirname "$(readlink -f "$0")")/claude-docker.sh"

base="${1:?usage: $0 <base-dir> [--delete]}"
delete=$([[ "${2:-}" == "--delete" ]] && echo 1 || echo "")

mkdir -p "$(dirname "$CONFIG_FILE")"
config=$(cat "$CONFIG_FILE" 2>/dev/null || echo '{}')
files=()

while IFS= read -r -d '' file; do
  case "$(basename "$file")" in
    .claude-code-extra-mounts) list=mounts ;;
    *) list=hosts ;;
  esac
  key=$(cd "$(dirname "$file")" && project_key)
  # Same rules as the old reader: skip blank lines and # comments, keep $VARS.
  entries=$({ grep -v -e '^$' -e '^#' "$file" || true; } | jq -R . | jq -s .)

  echo "$file -> $key [$list]: $(jq -c . <<<"$entries")"
  config=$(jq --arg k "$key" --arg l "$list" --argjson e "$entries" \
    '.[$k] //= {mounts: [], hosts: []}
     | .[$k][$l] = reduce (.[$k][$l] + $e)[] as $x ([]; if index([$x]) then . else . + [$x] end)' \
    <<<"$config")
  files+=("$file")
done < <(find "$base" -type f \( -name .claude-code-extra-mounts -o -name .claude-code-extra-hosts \) -print0)

if [[ ${#files[@]} -eq 0 ]]; then
  echo "Nothing to migrate under $base."
  exit 0
fi

# Write the config before deleting anything, so a failure loses no entries.
tmp=$(mktemp "$CONFIG_FILE.XXXXXX")
jq . <<<"$config" > "$tmp"
mv "$tmp" "$CONFIG_FILE"
echo "Wrote $CONFIG_FILE."

if [[ -n "$delete" ]]; then
  rm -- "${files[@]}"
  echo "Deleted ${#files[@]} file(s)."
else
  echo "Would delete (re-run with --delete):"
  printf '  %s\n' "${files[@]}"
fi
