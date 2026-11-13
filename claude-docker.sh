#!/usr/bin/env bash
set -euo pipefail

# Adjust this to where the docker-compose.yaml lives and you can
# put this script somewhere where your $PATH has access to.
source "$HOME/.dotfiles/current/zshrc/llm.sh"
DOCKER_COMPOSE_FILE="$CLAUDE_CODE_DOCKER_DIR/docker-compose.yaml"

# Per-project extra mounts/hosts. Kept outside the projects on purpose: ${PWD}
# is mounted read-write, so a file in the project could be edited by the
# container to add mounts to the next one.
CONFIG_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/claude-code-docker/config.json"

is_git_bare_repo_worktree() {
  if [[ "$(git config --get core.bare)" == "true" ]]; then
    return 0
  else
    return 1
  fi
}

is_bare_git_repository() {
  if [[ "$(git rev-parse --is-bare-repository 2>/dev/null)" == "true" ]]; then
    return 0
  else
    return 1
  fi
}

parent_has_git_dir() {
  find .. -name ".git" -type d > /dev/null
}

# Without this, Claude the retard can't run git commands.
should_mount_git_bare_repo_parent_dir() {
  ! is_bare_git_repository && is_git_bare_repo_worktree
}

git_bare_repo_dir() {
  git rev-parse --git-common-dir
}

# Config key for $PWD: the bare repo dir in a bare repo or its worktree, the
# repo root in a regular git repo, otherwise $PWD.
project_key() {
  local top
  if is_bare_git_repository || is_git_bare_repo_worktree; then
    git rev-parse --path-format=absolute --git-common-dir
  elif top=$(git rev-parse --show-toplevel 2>/dev/null); then
    echo "$top"
  else
    echo "$PWD"
  fi
}

# $1: project key, $2: list name (mounts|hosts). One entry per line.
config_entries() {
  [[ -f "$CONFIG_FILE" ]] || return 0
  jq -r --arg k "$1" --arg l "$2" '.[$k][$l][]? // empty' "$CONFIG_FILE"
}

yaml_squote() {
  local s="$1"
  s=${s//\'/\'\'}
  printf "'%s'" "$s"
}

# Reads the caller's `mounts` and `hosts` arrays (bash dynamic scoping).
create_docker_compose_override_file() {
  local override_file
  override_file=$(mktemp -t claude-code-docker-compose-override-XXXXXX.yaml)

  {
    echo "services:"
    echo "  claude-code:"

    if [[ ${#mounts[@]} -gt 0 ]]; then
      echo "    volumes:"
      for path in "${mounts[@]}"; do
        printf '      - %s\n' "$(yaml_squote "$path")"
      done
    fi

    if [[ ${#hosts[@]} -gt 0 ]]; then
      echo "    extra_hosts:"
      for host in "${hosts[@]}"; do
        printf '      - %s\n' "$(yaml_squote "$host")"
      done
    fi
  } > "$override_file"

  echo "$override_file"
}

# Appends to caller's `hosts` array (bash dynamic scoping).
# Reads one `hostname:ip` per line from stdin, like docker's --add-host.
add_extra_hosts() {
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    hosts+=("$(envsubst <<<"$line")")
  done
}

# Appends to caller's `mounts` array (bash dynamic scoping).
# Reads one `host:container` per line from stdin.
add_extra_mounts() {
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" || "$line" == \#* ]] && continue

    line=$(envsubst <<<"$line")
    local host_path="${line%%:*}"

    if [[ ! -e "$host_path" ]]; then
      continue
    fi

    mounts+=("$line")

    if (cd "$host_path" 2>/dev/null && should_mount_git_bare_repo_parent_dir); then
      local bare_repo
      bare_repo=$(cd "$host_path" && git_bare_repo_dir)
      mounts+=("$bare_repo:$bare_repo")
    fi
  done
}

main() {
  local mounts=()
  local hosts=()

  if should_mount_git_bare_repo_parent_dir; then
    local path
    path=$(git_bare_repo_dir)
    mounts+=("$path:$path")
  fi

  local key
  key=$(project_key)

  # Writable extra mounts: make the user review them in ccm first. Entries are
  # read below, after ccm, so changes made there (e.g. to :ro) take effect.
  if [[ -f "$CONFIG_FILE" ]] && jq -e --arg k "$key" \
    'any(.[$k].mounts[]?; (. == "" or startswith("#") or endswith(":ro")) | not)' \
    "$CONFIG_FILE" >/dev/null; then
    ccm --wait || { echo "ccm failed, not starting claude." >&2; exit 1; }
  fi

  add_extra_mounts < <(config_entries "$key" mounts)
  add_extra_hosts < <(config_entries "$key" hosts)

  if [[ ${#mounts[@]} -gt 0 || ${#hosts[@]} -gt 0 ]]; then
    local override_file
    override_file=$(create_docker_compose_override_file)
    docker-compose -f "$DOCKER_COMPOSE_FILE" -f "$override_file" run --rm --remove-orphans claude-code --permission-mode auto "$@"
  else
    docker-compose -f "$DOCKER_COMPOSE_FILE" run --rm --remove-orphans claude-code --permission-mode auto "$@"
  fi
}

# Guarded so migrate-extra-files.sh can source the helpers.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
