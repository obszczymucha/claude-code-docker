#!/usr/bin/env bash
# Wrapper around `docker compose` that generates a throwaway override file
# adding one bind mount per extra directory `extra_mounts` reports needed,
# then merges it in with -f. Compose merges the `volumes` list across -f
# files by target path, so this only appends to the base mounts.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Replace with the real logic: print one host path per line for each extra
# directory that should be bind-mounted into the container (mounted at the
# same path inside). Nothing printed -> no extra mounts.
extra_mounts() {
  :
}

yaml_squote() {
  local s=$1
  s=${s//\'/\'\'}
  printf "'%s'" "$s"
}

mounts=()
while IFS= read -r path; do
  [ -n "$path" ] && mounts+=("$path")
done < <(extra_mounts)

override_file="$(mktemp -t claude-code-compose-override-XXXXXX.yaml)"
trap 'rm -f "$override_file"' EXIT

if [ "${#mounts[@]}" -eq 0 ]; then
  printf 'services: {}\n' > "$override_file"
else
  {
    echo "services:"
    echo "  claude-code:"
    echo "    volumes:"
    for path in "${mounts[@]}"; do
      printf '      - %s\n' "$(yaml_squote "$path:$path")"
    done
  } > "$override_file"
fi

docker compose -f "$script_dir/docker-compose.yaml" -f "$override_file" "$@"
