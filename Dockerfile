FROM node:20-slim
ARG LUA_LS_VERSION=3.19.1
RUN apt-get update \
 && apt-get install -y \
    git \
    ripgrep \
    curl \
    lua5.4 \
    luarocks \
    python3 \
    python3-pip \
    jq \
    rsync \
 && rm -rf /var/lib/apt/lists/* \
 && update-alternatives --set lua-interpreter /usr/bin/lua5.4 \
 && update-alternatives --set lua-compiler /usr/bin/luac5.4
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
USER node
RUN curl -fsSL https://claude.ai/install.sh | bash \
 && /home/node/.local/bin/claude update \
 && /home/node/.local/bin/claude --version
WORKDIR /workspace
ENTRYPOINT ["/home/node/.local/bin/claude"]
