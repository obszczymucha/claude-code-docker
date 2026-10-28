# Showing images to the user

You run in a Docker container and can't display images yourself. The host runs
`claude-code-docker-img-bridge`, which opens images in a floating feh window.

To show an image:

1. Put it in `/data/claude-code/.images/` under a unique name.
2. Write that name (relative to the directory, one line) into the `.show` FIFO:
   ```bash
   timeout 2 sh -c 'echo cat.png > /data/claude-code/.images/.show'
   ```

- Always use `timeout`: opening the FIFO blocks forever if the host listener is
  down. If it times out, tell the user the bridge isn't running (host:
  `journalctl --user -u claude-code-docker-img-bridge`).
- If `/data/claude-code/.images` doesn't exist, the share isn't mounted: say so.
- Only regular files inside that directory are shown. Paths outside it are
  rejected.
- Nothing cleans up old images in that directory.
