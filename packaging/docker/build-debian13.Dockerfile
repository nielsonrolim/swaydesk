# swaydesk-build:debian13: compiles the Rust components (cargo build/test) for Debian 13, and the base the
# headless tests' binaries come from (rust/target/debian13). See README.md.
FROM debian:13
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    cargo rustc libgtk-4-dev libgtk4-layer-shell-dev pkg-config ca-certificates && rm -rf /var/lib/apt/lists/*
CMD ["bash"]
