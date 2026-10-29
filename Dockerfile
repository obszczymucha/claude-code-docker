FROM archlinux:latest
ARG LUA_LS_VERSION=3.19.1

# Add whatever tools you want to give claude.
# node gets passwordless sudo for pacman only; the /usr/local/bin/pacman wrapper
# makes plain `pacman -S foo` work. ponytail: pacman can run install hooks, so
# this is effectively root for node; drop it if the container must stay unprivileged.
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
    jdk17-openjdk \
    sudo \
 && pacman -Scc --noconfirm \
 && useradd -m -u 1000 node \
 && echo 'node ALL=(root) NOPASSWD: /usr/bin/pacman' > /etc/sudoers.d/node-pacman \
 && chmod 440 /etc/sudoers.d/node-pacman \
 && printf '#!/bin/sh\nexec sudo /usr/bin/pacman "$@"\n' > /usr/local/bin/pacman \
 && chmod +x /usr/local/bin/pacman \
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

# Android: Arch's gradle is 9.x (needs JDK 21), so pin an 8.x zip. SDK is
# node-owned so AGP can fetch extra components it wants at build time.
# ponytail: Google ships SDK build-tools for x86_64 only; arm64 hosts won't build APKs.
ARG GRADLE_VERSION=8.14.3
ARG ANDROID_CMDLINE_TOOLS=13114758
# JAVA_HOME explicit: archlinux-java's /usr/lib/jvm/default link isn't set up in the image.
ENV ANDROID_HOME=/opt/android-sdk \
    JAVA_HOME=/usr/lib/jvm/java-17-openjdk
ENV PATH=$JAVA_HOME/bin:/opt/gradle/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH
RUN curl -fsSLo /tmp/gradle.zip "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" \
 && unzip -q /tmp/gradle.zip -d /opt \
 && mv "/opt/gradle-${GRADLE_VERSION}" /opt/gradle \
 && curl -fsSLo /tmp/cmdline-tools.zip "https://dl.google.com/android/repository/commandlinetools-linux-${ANDROID_CMDLINE_TOOLS}_latest.zip" \
 && mkdir -p "$ANDROID_HOME/cmdline-tools" \
 && unzip -q /tmp/cmdline-tools.zip -d "$ANDROID_HOME/cmdline-tools" \
 && mv "$ANDROID_HOME/cmdline-tools/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest" \
 && rm /tmp/gradle.zip /tmp/cmdline-tools.zip \
 && yes | sdkmanager --licenses >/dev/null \
 && sdkmanager --install "platform-tools" "platforms;android-35" "build-tools;35.0.0" \
 && chown -R node:node "$ANDROID_HOME" \
 && java -version && javac -version && gradle --version && adb version

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
