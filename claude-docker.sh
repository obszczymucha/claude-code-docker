#!/usr/bin/env bash
set -euo pipefail

# Adjust this to where the docker-compose.yaml lives and you can
# put this script somewhere where your $PATH has access to.
DOCKER_COMPOSE_FILE="$HOME/.projects/ops/claude-code-docker/docker-compose.yaml"

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

yaml_squote() {
  local s="$1"
  s=${s//\'/\'\'}
  printf "'%s'" "$s"
}

create_docker_compose_override_file() {
  local mounts=("$@")

  local override_file
  override_file=$(mktemp -t claude-code-docker-compose-override-XXXXXX.yaml)

  {
    echo "services:"
    echo "  claude-code:"
    echo "    volumes:"

    for path in "${mounts[@]}"; do
      printf '      - %s\n' "$(yaml_squote "$path")"
    done
  } > "$override_file"

  echo "$override_file"
}

extra_mounts_file_name() {
  local worktree_dir
  worktree_dir=$(git rev-parse --show-toplevel)
  local file="$worktree_dir/.claude-code-extra-mounts"

  if [[ -f "$file" ]]; then
    echo "$file"
  else
    echo ""
  fi
}

# Appends to caller's `mounts` array (bash dynamic scoping).
add_extra_mounts() {
  local file="$1"

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
  done < "$file"
}

main() {
  local mounts=()
  local extra_mounts_file=""

  if should_mount_git_bare_repo_parent_dir; then
    local path
    path=$(git_bare_repo_dir)
    mounts+=("$path:$path")

    extra_mounts_file=$(extra_mounts_file_name)

    if [[ -n "$extra_mounts_file" ]]; then
      add_extra_mounts "$extra_mounts_file"
    fi
  fi

  local cwd_mounts_file="$PWD/.claude-code-extra-mounts"
  if [[ -f "$cwd_mounts_file" && "$cwd_mounts_file" != "$extra_mounts_file" ]]; then
    add_extra_mounts "$cwd_mounts_file"
  fi

  if [[ ${#mounts[@]} -gt 0 ]]; then
    local override_file
    override_file=$(create_docker_compose_override_file "${mounts[@]}")
    docker-compose -f "$DOCKER_COMPOSE_FILE" -f "$override_file" run --rm --remove-orphans claude-code --permission-mode auto "$@"
  else
    docker-compose -f "$DOCKER_COMPOSE_FILE" run --rm --remove-orphans claude-code --permission-mode auto "$@"
  fi
}

main "$@"
