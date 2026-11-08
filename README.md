# claude-code-docker

Runs the Claude Code CLI in a container that can only touch explicitly mounted
directories: the current directory, `~/.claude`, `~/.claude.json`, and any extra
mounts defined in the config file.

Edit the `Dockerfile` to add or remove tools as needed.

## Usage

```sh
claude-docker.sh [claude args...]
```

Set `DOCKER_COMPOSE_FILE` in `claude-docker.sh` to this repo's `docker-compose.yaml`.

Equivalent to running `claude` locally in the current directory (it adds
`--permission-mode auto`).

Extra mounts and `/etc/hosts` entries go in
`~/.config/claude-code-docker/config.json` (needs `jq`), keyed by project:

```json
{
  "/home/alien/.projects/myproj/.git": {
    "mounts": ["$HOME/.projects/shared-lib:$HOME/.projects/shared-lib", "$HOME/notes:/notes:ro"],
    "hosts": ["pi2:192.168.1.50"]
  }
}
```

The key is the bare repo dir in a bare repo or any of its worktrees (so all
worktrees share it), the repo root in a regular git repo, otherwise `$PWD`.
`mounts` are `host:container[:ro]`, `hosts` are `hostname:ip` (like
`docker run --add-host`). `$VARS` work, and missing host paths are skipped. In a
bare-repo worktree, the bare repo is mounted automatically so git works.

The config lives outside the projects on purpose: the project is mounted
read-write, so a session could otherwise add mounts to the next one.

To move old `.claude-code-extra-mounts` / `.claude-code-extra-hosts` files into
the config (run on the host). It only lists the old files until you pass
`--delete`:

```sh
./migrate-extra-files.sh ~            # write config, list old files
./migrate-extra-files.sh ~ --delete   # same, and delete them
```

### Editing the config

`tui/` is a small TUI for the config (run on the host):

```sh
cargo install --path tui --root ~/.local   # lands in ~/.local/bin
install -m755 tui/run.sh ~/.local/bin/ccm  # ccm: the project TUI in a floating tmux pane
claude-docker-config                 # TUI for the current project: a add mount (fuzzy dir finder), h add host, e edit, d delete
claude-docker-config --force         # same, for $PWD even inside a repo
claude-docker-config --path <dir>    # use <dir> instead of $PWD (also works with add, and through ccm)
claude-docker-config manage          # TUI for all projects: type to filter, alt+a/e/d add/edit/delete, tab pane, esc quit
claude-docker-config add .           # add the current project (same key rule as above)
claude-docker-config add . --force   # add $PWD even inside a repo
claude-docker-config add <path>      # add <path> as given
```

Mounts whose host path doesn't exist are shown in red, since they'd be skipped.

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
