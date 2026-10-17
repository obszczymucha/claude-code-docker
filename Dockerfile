FROM archlinux:latest
ARG LUA_LS_VERSION=3.19.1

# Add whatever tools you want to give claude.
RUN pacman -Syu --noconfirm --needed \
    git \
    ripgrep \
    curl \
    lua \
    luarocks \
    python \
    python-pip \
    jq \
    rsync \
    tmux \
    gcc \
    nodejs \
    npm \
    clang \
    odin \
    libx11 libxrandr libxinerama libxcursor libxi \
    mesa mesa-utils \
    xorg-server-xvfb xorg-xdpyinfo \
    imagemagick \
    binutils \
    unzip \
 && pacman -Scc --noconfirm \
 && useradd -m -u 1000 node \
 && odin version

# lua-language-server: prebuilt release tarball extracts flat, so give it its
# own dir. The wrapper redirects log/meta writes into $HOME/.cache, keeping
# /opt read-only for the node user.
RUN case "$(uname -m)" in \
      x86_64) arch=x64 ;; \
      aarch64) arch=arm64 ;; \
      *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;; \
    esac \
 && mkdir -p /opt/lua-language-server \
 && curl -fsSL "https://github.com/LuaLS/lua-language-server/releases/download/${LUA_LS_VERSION}/lua-language-server-${LUA_LS_VERSION}-linux-${arch}.tar.gz" \
    | tar -xz -C /opt/lua-language-server \
 && printf '%s\n' \
      '#!/bin/sh' \
      'cache="${XDG_CACHE_HOME:-$HOME/.cache}/lua-language-server"' \
      'exec /opt/lua-language-server/bin/lua-language-server --logpath "$cache/log" --metapath "$cache/meta" "$@"' \
    > /usr/local/bin/lua-language-server \
 && chmod +x /usr/local/bin/lua-language-server \
 && lua-language-server --version

# Software GL via Mesa llvmpipe; DISPLAY stays unset (agent runs its own Xvfb).
ENV LIBGL_ALWAYS_SOFTWARE=1 \
    MESA_GL_VERSION_OVERRIDE=4.6

USER node
RUN curl -fsSL https://claude.ai/install.sh | bash \
 && /home/node/.local/bin/claude update \
 && /home/node/.local/bin/claude --version

# Rust via rustup into ~/.cargo; links with the pacman gcc above. registry/ is
# pre-created so the cargo-registry volume is seeded node-owned, not root.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal \
 && /home/node/.cargo/bin/cargo --version \
 && mkdir /home/node/.cargo/registry
ENV PATH=/home/node/.cargo/bin:$PATH

WORKDIR /workspace
ENTRYPOINT ["/home/node/.local/bin/claude"]
