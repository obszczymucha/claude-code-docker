#!/usr/bin/env bash
for f in "$XDG_RUNTIME_DIR"/claudecode-control/*.json; do
  [ -e "$f" ] || continue
  port=$(sed -n 's/.*"controlPort":\s*\([0-9]\+\).*/\1/p' "$f")
  [ -n "$port" ] && exec 3<>"/dev/tcp/127.0.0.1/$port" && exec 3<&-
done
