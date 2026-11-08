FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config clang libclang-dev lld cmake \
    libssl-dev libdbus-1-dev libfontconfig1-dev libfreetype6-dev \
    libx11-dev libx11-xcb-dev libxcb1-dev libxcb-composite0-dev \
    libxcb-randr0-dev libxcb-render0-dev libxcb-shape0-dev \
    libxcb-xfixes0-dev libxcb-xkb-dev libxkbcommon-dev \
    libxkbcommon-x11-dev libwayland-dev libvulkan-dev libasound2-dev \
    libgtk-3-dev gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
    nsis zip python3 file binutils \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-gnu
RUN apt-get update && apt-get install -y --no-install-recommends wixl msitools \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
WORKDIR /workspace
