FROM rust:1.89.0-trixie@sha256:57407b378b2b6e07b48a6135a20c87cc22ea6e249c0acf6cb1833ead3cf116e9
# Windows SDK/CRT stay in this private build image, never in release source archives.
RUN apt-get update && apt-get install -y --no-install-recommends clang lld llvm cmake ninja-build wine64 wine python3 ca-certificates git && rm -rf /var/lib/apt/lists/*
RUN cargo install --locked cargo-xwin --version 0.20.2 && rustup target add x86_64-pc-windows-msvc
ENV XWIN_CACHE_DIR=/opt/xwin XWIN_ARCH=x86_64 XWIN_SDK_VERSION=10.0.26100 XWIN_CRT_VERSION=14.44.17.14 WINEDEBUG=-all WINEPREFIX=/tmp/arkline-wine
RUN test "$(dpkg-query -W -f='${Version}' wine64)" = "10.0~repack-6" && wine --version && cargo xwin --version && \
    cargo new --bin /tmp/sdk-probe && cd /tmp/sdk-probe && \
    cargo xwin build --target x86_64-pc-windows-msvc && rm -rf /tmp/sdk-probe
WORKDIR /workspace
