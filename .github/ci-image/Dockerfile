# Ubuntu 22.04 matches the oldest glibc baseline for the Linux AppImage release and also runs CI.
FROM ubuntu:22.04

ARG RUST_TOOLCHAIN=1.99.0
ARG ZIGLANG_VERSION=0.15.2
ARG CARGO_ZIGBUILD_VERSION=0.23.4
ARG CARGO_ZIGBUILD_SHA256=9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b
ARG CARGO_AUDIT_VERSION=0.23.0
ARG CARGO_AUDIT_SHA256=4f01c74ed84b2dffd0b1d37012a62cdf2a92c96e25bdf8eec7fcc0d4d3eb7811
ARG MOLD_VERSION=3.0.0
ARG MOLD_SHA256=6c90d4a474c7c0409dfb575be03a5345878ac14fdba18de8b40fa58c60121189
ARG APPIMAGETOOL_VERSION=1.9.1
ARG APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      clang \
      curl \
      desktop-file-utils \
      file \
      git \
      libdbus-1-dev \
      libfontconfig-dev \
      libfreetype-dev \
      libwayland-dev \
      libxcb1-dev \
      libxkbcommon-dev \
      libxkbcommon-x11-dev \
      pkg-config \
      python3-pip \
      xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && pip3 install --no-cache-dir "ziglang==${ZIGLANG_VERSION}"

RUN curl -fsSL https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}" \
        -c rustfmt,clippy \
        -t x86_64-unknown-linux-musl,aarch64-unknown-linux-musl,x86_64-pc-windows-gnu \
    && chmod -R a+rX "${RUSTUP_HOME}" "${CARGO_HOME}"

RUN set -eux; \
    curl -fsSL -o /tmp/mold.tar.gz \
      "https://github.com/rui314/mold/releases/download/v${MOLD_VERSION}/mold-${MOLD_VERSION}-x86_64-linux.tar.gz"; \
    echo "${MOLD_SHA256}  /tmp/mold.tar.gz" | sha256sum --check; \
    tar -xzf /tmp/mold.tar.gz -C /tmp; \
    install -m 0755 "/tmp/mold-${MOLD_VERSION}-x86_64-linux/bin/mold" /usr/local/bin/mold; \
    ln -sf /usr/local/bin/mold /usr/local/bin/ld.mold; \
    curl -fsSL -o /tmp/cargo-zigbuild.tar.xz \
      "https://github.com/rust-cross/cargo-zigbuild/releases/download/v${CARGO_ZIGBUILD_VERSION}/cargo-zigbuild-x86_64-unknown-linux-musl.tar.xz"; \
    echo "${CARGO_ZIGBUILD_SHA256}  /tmp/cargo-zigbuild.tar.xz" | sha256sum --check; \
    tar -xJf /tmp/cargo-zigbuild.tar.xz -C /tmp; \
    install -m 0755 /tmp/cargo-zigbuild-x86_64-unknown-linux-musl/cargo-zigbuild /usr/local/bin/cargo-zigbuild; \
    curl -fsSL -o /tmp/cargo-audit.tgz \
      "https://github.com/rustsec/rustsec/releases/download/cargo-audit/v${CARGO_AUDIT_VERSION}/cargo-audit-x86_64-unknown-linux-musl-v${CARGO_AUDIT_VERSION}.tgz"; \
    echo "${CARGO_AUDIT_SHA256}  /tmp/cargo-audit.tgz" | sha256sum --check; \
    tar -xzf /tmp/cargo-audit.tgz -C /tmp; \
    install -m 0755 "/tmp/cargo-audit-x86_64-unknown-linux-musl-v${CARGO_AUDIT_VERSION}/cargo-audit" /usr/local/bin/cargo-audit; \
    curl -fsSL -o /usr/local/bin/appimagetool \
      "https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-x86_64.AppImage"; \
    echo "${APPIMAGETOOL_SHA256}  /usr/local/bin/appimagetool" | sha256sum --check; \
    chmod 0755 /usr/local/bin/appimagetool; \
    rm -rf /tmp/mold* /tmp/cargo-zigbuild* /tmp/cargo-audit*
