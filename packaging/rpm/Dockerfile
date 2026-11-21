# Fedora build environment for the .rpm (binaries built against Fedora's glibc and Qt).
ARG FEDORA=41
FROM fedora:${FEDORA}
RUN dnf -y install gcc gcc-c++ make cmake pkgconf-pkg-config rpm-build systemd-rpm-macros \
        qt6-qtbase-devel qt6-qtdeclarative-devel layer-shell-qt-devel libxkbcommon-devel gzip findutils curl \
    && dnf clean all
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.88.0
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /build
