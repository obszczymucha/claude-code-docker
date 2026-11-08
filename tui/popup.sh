#!/usr/bin/env bash
set -euo pipefail

main() {
  local w=30 h=5
  # tmux expands formats in -X/-Y, so the pane opens centred at its final size.
  tmux new-pane -B none -x "$w" -y "$h" \
    -X "#{e|/|:#{e|-|:#{window_width},$w},2}" -Y "#{e|/|:#{e|-|:#{window_height},$h},2}" \
    -c "$PWD" 'printf "\033[?25l"; cargo run -q -- --test > /tmp/pick'
}

main "$@"
