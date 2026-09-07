FROM node:20-slim
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
 && rm -rf /var/lib/apt/lists/* \
 && update-alternatives --set lua-interpreter /usr/bin/lua5.4 \
 && update-alternatives --set lua-compiler /usr/bin/luac5.4
USER node
RUN curl -fsSL https://claude.ai/install.sh | bash
WORKDIR /workspace
ENTRYPOINT ["/home/node/.local/bin/claude"]
