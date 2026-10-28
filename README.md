# claude-code-docker

Runs the Claude Code CLI in a container that can only touch explicitly mounted
directories: the current directory, `~/.claude`, `~/.claude.json`, and any extra
mounts defined in the `.claude-code-extra-mounts` file.

Edit the `Dockerfile` to add or remove tools as needed.

## Usage

```sh
claude-docker.sh [claude args...]
```

Set `DOCKER_COMPOSE_FILE` in `claude-docker.sh` to this repo's `docker-compose.yaml`.

Equivalent to running `claude` locally in the current directory (it adds
`--permission-mode auto`).

Extra mounts go in `.claude-code-extra-mounts` in the current directory or git
worktree root, one `host:container` per line. `#` comments and `$VARS` work, and
missing host paths are skipped. In a bare-repo worktree, the bare repo is mounted
automatically so git works.

Example:
```sh
# .claude-code-extra-mounts
$HOME/.projects/shared-lib:$HOME/.projects/shared-lib
$HOME/notes:/notes:ro
```

This mounts `~/.projects/shared-lib` at the same path inside the container, and
`~/notes` read-only at `/notes`. To check, run `claude-docker.sh` and ask Claude
whether it can see `shared-lib`.

## claude-code-docker-img-bridge

Claude Code can't display images, so Claude shows them in a floating feh window
on the host instead. `/data/claude-code/.images` is mounted at the same path in the
container. Claude copies an image there and writes its file name into the
`.show` FIFO. The host's `claude-code-docker-img-bridge` user service only
opens regular files inside that directory, so the container can't run anything
on the host.

The host side is managed by `arch-bootstrap`:

```sh
# install, before starting the container
./run.sh --tags claude-code-docker-img-bridge
# uninstall
./run.sh --tags claude-code-docker-img-bridge-uninstall
```

Install first: if the directory doesn't exist when the container starts, Docker
creates it owned by root and the bridge can't use it. For the same reason, when
uninstalling, also remove the `/data/claude-code/.images` volume from
`docker-compose.yaml`.

## Rebuild

```sh
docker compose build claude-code               # after editing the Dockerfile
docker compose build --no-cache claude-code    # newer system packages
```

## CLI updates

The Claude CLI lives in a Docker volume, so its auto-updates persist across
sessions. To reset it to the image's copy:

```sh
docker volume rm claude-code-docker_claude-cli-bin \
                 claude-code-docker_claude-cli-versions
```
