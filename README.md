# claude-code-docker

Runs the Claude Code CLI in a container that can only touch explicitly mounted
directories: the current directory, `~/.claude`, and `~/.claude.json`.

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

## Rebuild

```sh
docker compose build claude-code               # after editing the Dockerfile
docker compose build --no-cache claude-code    # newer system packages
```

## CLI updates

Run `claude update` inside a session. The CLI lives in named volumes, so updates
persist and rebuilds don't replace it. To go back to the image's copy:

```sh
docker volume rm claude-code-docker_claude-cli-bin \
                 claude-code-docker_claude-cli-versions
```
