#!/usr/bin/env bash
set -euo pipefail

# The project TUI in a borderless floating tmux pane, 60% of the window,
# centred; the TUI draws its own rounded border. Opens on $PWD's project.
# Installed as `ccm` (see README); runs in place outside tmux. Args go to
# claude-docker-config, e.g. `ccm --add`.
# `--wait` blocks until the pane closes and exits with the TUI's status
# (outside tmux it always does, as it runs in place).
main() {
  local wait=false args=()
  for a in "$@"; do
    if [[ "$a" == "--wait" ]]; then wait=true; else args+=("$a"); fi
  done
  set -- "${args[@]}"

  [[ -n "${TMUX:-}" ]] || exec claude-docker-config "$@"
  local w="#{e|/|:#{e|*|:#{window_width},50},100}"
  local h="#{e|/|:#{e|*|:#{window_height},50},100}"
  local cmd="claude-docker-config --border ${*@Q}"

  local ch status
  if $wait; then
    ch="ccm-$$"
    status=$(mktemp -t ccm-status-XXXXXX)
    # EXIT trap also fires if the pane is killed, so the waiter never hangs.
    cmd="trap 'tmux wait-for -S $ch' EXIT; $cmd; echo \$? > ${status@Q}"
  fi

  # tmux expands formats in -x/-y/-X/-Y, so the pane opens centred at its final size.
  tmux new-pane -B none -x "$w" -y "$h" \
    -X "#{e|/|:#{e|-|:#{window_width},$w},2}" -Y "#{e|/|:#{e|-|:#{window_height},$h},2}" \
    -c "$PWD" bash -c "$cmd"

  if $wait; then
    tmux wait-for "$ch"
    local s
    s=$(<"$status")
    rm -f "$status"
    exit "${s:-1}" # empty = pane killed before the TUI finished
  fi
}

main "$@"
