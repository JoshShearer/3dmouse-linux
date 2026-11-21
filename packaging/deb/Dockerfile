# Build environment for Ubuntu packages. Binaries must be built against the
# target release's glibc and Qt, so each release gets its own image.
ARG UBUNTU=22.04
FROM ubuntu:${UBUNTU}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake curl ca-certificates pkg-config dpkg-dev file \
        qt6-base-dev qt6-declarative-dev libqt6opengl6-dev libxkbcommon-dev libvulkan-dev libgl-dev libspnav-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.88.0
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /build
