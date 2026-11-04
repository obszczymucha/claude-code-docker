#!/usr/bin/env bash
set -euo pipefail

main() {
  tmux new-pane -B none -x 1 -y 1 -X 100% -Y 0 -c "$PWD" 'printf "\033[?25l"; cargo run -q -- --test > /tmp/pick'
}

main "$@"
