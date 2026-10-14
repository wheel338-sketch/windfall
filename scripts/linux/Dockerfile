# Linux build and test images for Windfall, on Ubuntu 22.04: the oldest
# distribution supported, so the .deb and AppImage built here also run on
# newer ones. See scripts/linux/README.md.
#
#   docker build -f scripts/linux/Dockerfile --target build   -t windfall-linux-build   scripts/linux
#   docker build -f scripts/linux/Dockerfile --target runtime -t windfall-linux-runtime scripts/linux
#
# The source is not baked in: scripts/linux/xplat.sh copies the checkout into
# a volume and keeps Cargo's target folder in another.

ARG UBUNTU=ubuntu:22.04

# ---------------------------------------------------------------------------
# Build: Rust, Node, pnpm and the libraries Tauri 2 links against.
FROM ${UBUNTU} AS build
ARG RUST_VERSION=1.99.0
ARG NODE_VERSION=24.11.1
ARG PNPM_VERSION=11
ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/opt/node/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl wget git rsync file xz-utils build-essential pkg-config \
      libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
      librsvg2-dev libxdo-dev libasound2-dev libdbus-1-dev libsoup-3.0-dev \
      libjavascriptcoregtk-4.1-dev patchelf desktop-file-utils fuse libfuse2 \
      xvfb xauth dbus-x11 webkit2gtk-driver xdotool \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal \
      --default-toolchain ${RUST_VERSION} --component clippy,rustfmt \
    && chmod -R a+rwX /opt/rustup /opt/cargo
RUN mkdir -p /opt/node \
    && curl -fsSL https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz \
       | tar -xJ -C /opt/node --strip-components=1 \
    && npm install -g pnpm@${PNPM_VERSION}
# The WebDriver bridge for Tauri apps, used by the window smoke.
RUN cargo install tauri-driver --locked && rm -rf /opt/cargo/registry /opt/cargo/git
WORKDIR /src

# ---------------------------------------------------------------------------
# Runtime: a fresh Ubuntu 22.04 with no compiler, no -dev packages and no
# Rust. The .deb is installed with apt so its declared dependencies are what
# pulls the libraries in. Xvfb, the WebDriver bridge, xdotool and Node are
# test tools only.
FROM ${UBUNTU} AS runtime
ARG NODE_VERSION=24.11.1
ENV DEBIAN_FRONTEND=noninteractive \
    PATH=/opt/node/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl xz-utils xvfb xauth dbus-x11 webkit2gtk-driver xdotool \
      x11-utils imagemagick alsa-utils file \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /opt/node \
    && curl -fsSL https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz \
       | tar -xJ -C /opt/node --strip-components=1
COPY --from=build /opt/cargo/bin/tauri-driver /usr/local/bin/tauri-driver
# ALSA's null device for anything that opens the default PCM.
RUN printf 'pcm.!default { type null }\nctl.!default { type hw card 0 }\n' > /etc/asound.conf
RUN useradd -m tester
WORKDIR /home/tester
