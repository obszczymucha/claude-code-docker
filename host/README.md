# Host bridge for the claude-code container (netsync MCP + Neovim IDE proxy)

The container runs on a normal docker bridge network (not `network_mode:
host`). It reaches the host via `host.docker.internal`, mapped to the docker
bridge IP (`172.17.0.1`) by `extra_hosts: host.docker.internal:host-gateway`.

`netsync-mcp.py` is a zero-dependency Python daemon that runs in its own
`netsync-mcp` container (`network_mode: host`, see below) and does two
things:

1. **netsync MCP server** on `127.0.0.1:8737` and `172.17.0.1:8737`, exposing
   one tool:
   - `netsync_copy_directory(directory)` — runs `netsync copy-directory
     <directory>` on the host (e.g. `directory=AutoInvite` after editing
     addon files).

   Registered in Claude Code at user scope:
   `claude mcp add --transport http --scope user netsync http://host.docker.internal:8737/mcp`

2. **Neovim IDE proxy** — claudecode.nvim binds its websocket server to
   `127.0.0.1` only (not configurable), which the container cannot reach from
   a bridge network. The daemon watches `~/.claude/ide/*.lock` (filename =
   port) and forwards `172.17.0.1:<port>` → `127.0.0.1:<port>` for each live
   lockfile. The container-side CLI is pointed at the proxy via
   `CLAUDE_CODE_IDE_HOST_OVERRIDE=host.docker.internal` in
   docker-compose.yaml.

## Run it

`netsync-mcp.py` runs as its own `netsync-mcp` service in
`docker-compose.yaml`, started automatically as a dependency of
`claude-code` (`docker compose up` brings both up together). It uses
`network_mode: host` so it can bind the real `127.0.0.1` and docker bridge
IP, and mounts the host's `/usr/local/bin` (read-only) at `/host/bin` so it
can find `netsync` on `PATH` — not at `/usr/local/bin` directly, which would
shadow the image's own `python3`. If `netsync` lives somewhere else on your
host, update the volume mount and `PATH` env var for the `netsync-mcp`
service accordingly.

To run it standalone instead (e.g. for debugging):

```sh
python3 host/netsync-mcp.py
```

## Notes

- Binds only loopback and the docker bridge IP; nothing is exposed to the
  LAN. Anything running in a docker container on this host can reach the
  bridge listeners (the IDE websocket still requires the lockfile auth
  token).
- Listeners use `IP_FREEBIND`, so the daemon can start before docker brings
  up `docker0`.
- If your docker daemon uses a non-default bridge IP, set `CLAUDE_BRIDGE_IP`
  for the daemon and keep it in sync with what `host-gateway` resolves to.
- Directory names are validated (`[A-Za-z0-9._ -]+`) and executed without a
  shell, so no command injection through tool arguments.
- To change the MCP port, edit `MCP_PORT` in `netsync-mcp.py` and re-register
  the URL in Claude Code.
