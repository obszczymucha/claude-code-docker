#!/usr/bin/env bash
set -euo pipefail

# The project TUI in a borderless floating tmux pane, 60% of the window,
# centred; the TUI draws its own rounded border. Opens on $PWD's project.
# Installed as `ccm` (see README); runs in place outside tmux.
main() {
  [[ -n "${TMUX:-}" ]] || exec claude-docker-config "$@"
  local w="#{e|/|:#{e|*|:#{window_width},60},100}"
  local h="#{e|/|:#{e|*|:#{window_height},60},100}"
  # tmux expands formats in -x/-y/-X/-Y, so the pane opens centred at its final size.
  tmux new-pane -B none -x "$w" -y "$h" \
    -X "#{e|/|:#{e|-|:#{window_width},$w},2}" -Y "#{e|/|:#{e|-|:#{window_height},$h},2}" \
    -c "$PWD" "claude-docker-config --border"
}

main "$@"
