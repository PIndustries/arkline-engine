# Ubuntu 22.04 keeps the public Linux binary's glibc baseline at 2.35.
FROM ubuntu:22.04
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config python3 \
    libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxrandr-dev \
    libxcursor-dev libgl1-mesa-dev libasound2-dev \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.89.0 \
    && rustup component add clippy rustfmt \
    && rustup target add wasm32-unknown-unknown \
    && cargo install wasm-bindgen-cli --version 0.2.129 --locked
# The editor inspects attributes in the target tree before switching branches.
# Ubuntu 22.04's Git 2.34 cannot do this; check-attr --source needs Git >= 2.43.
# Build the pinned upstream release here to retain the glibc 2.35 baseline.
# SHA-256: https://www.kernel.org/pub/software/scm/git/sha256sums.asc
RUN apt-get update && apt-get install -y --no-install-recommends \
    libcurl4-openssl-dev libexpat1-dev libssl-dev zlib1g-dev xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -fsSL \
       https://www.kernel.org/pub/software/scm/git/git-2.56.0.tar.xz -o /tmp/git.tar.xz \
    && echo '26c56c296b38c0695b26fa95f475f1d01704d2d38e73465ca30b0b2f5dc789d3  /tmp/git.tar.xz' | sha256sum --check --strict \
    && tar -xJf /tmp/git.tar.xz -C /tmp \
    && make -C /tmp/git-2.56.0 -j2 prefix=/usr/local NO_TCLTK=YesPlease NO_GETTEXT=YesPlease all install \
    && test "$(/usr/local/bin/git --version)" = 'git version 2.56.0' \
    && rm -rf /tmp/git-2.56.0 /tmp/git.tar.xz
WORKDIR /src
