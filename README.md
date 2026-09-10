# claude-code-docker

Runs the Claude Code CLI in a container so it can only touch the directories
that are explicitly bind-mounted. `docker-compose.yaml` defines two services:
`claude-code` (the CLI itself) and `netsync-mcp` (a host bridge for the
netsync MCP server and the Neovim IDE websocket — see
[`host/README.md`](host/README.md)).

The image is `node:20-slim` plus the tooling Claude Code needs on `PATH`:
git, ripgrep, curl, jq, python3, lua5.4 + luarocks, and
[lua-language-server](https://github.com/LuaLS/lua-language-server).

## Rebuild

`run.sh` is a thin wrapper around `docker compose` (it generates a throwaway
override file for extra bind mounts, then passes every argument straight
through), so any compose subcommand works:

```sh
./run.sh build claude-code
```

Then start a session as usual:

```sh
./run.sh run --rm claude-code
```

Only the `claude-code` service is built from the `Dockerfile`; `netsync-mcp`
uses a stock `python:3.11-slim` image and never needs rebuilding.

### When to rebuild

- **You changed the `Dockerfile`** — `./run.sh build claude-code`. Docker
  reuses cached layers above the edit, so adding a tool to the last `RUN`
  costs seconds.
- **You want a newer Claude Code CLI** — the CLI is installed at build time
  by the last `RUN` layer, which stays cached forever otherwise. Force it:

  ```sh
  ./run.sh build --no-cache claude-code
  ```

  This also re-runs `apt-get update`, so system packages come along.
- **You changed `docker-compose.yaml` or `run.sh`** — no rebuild needed, the
  next `./run.sh run` picks it up.

Nothing in the image holds state worth keeping: `~/.claude`,
`~/.claude.json`, and the working directory are all bind mounts from the
host, so rebuilding is always safe.

### Bumping lua-language-server

It ships as a prebuilt release tarball rather than an apt or npm package —
it is a compiled binary, absent from Debian bookworm and from the npm
registry entirely, and building it from source would pull in ninja and a C++
toolchain. The version is pinned by an `ARG` at the top of the `Dockerfile`:

```dockerfile
ARG LUA_LS_VERSION=3.19.1
```

Edit that line and rebuild, or override it for a one-off build:

```sh
./run.sh build --build-arg LUA_LS_VERSION=3.19.2 claude-code
```

The `RUN` layer ends in `lua-language-server --version`, so a bad version or
a changed release layout fails the build instead of producing a broken
image. Check available tags on the
[releases page](https://github.com/LuaLS/lua-language-server/releases).

## Verify a build

```sh
./run.sh run --rm --entrypoint sh claude-code -c \
  'lua-language-server --version; lua -v; rg --version | head -1; ~/.local/bin/claude --version'
```

The CLI needs its full path here: it is installed at
`/home/node/.local/bin/claude` and only reaches `PATH` through the shell
profile, which `sh -c` does not read.

## Notes

- `lua-language-server` on `PATH` is a small wrapper script. The real binary
  lives in `/opt/lua-language-server`, which is root-owned; the wrapper
  redirects the server's `log/` and `meta/` writes to
  `$HOME/.cache/lua-language-server` so it works as the non-root `node` user.
- The build downloads from `claude.ai` and `github.com`, so it needs network
  access — a `docker build` behind a proxy needs the usual `HTTP_PROXY` build
  args.
- Both `linux/amd64` and `linux/arm64` builds work; the `Dockerfile` picks
  the matching lua-language-server tarball from `uname -m` and fails loudly
  on anything else.
