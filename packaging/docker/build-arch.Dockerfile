# swaydesk-build:arch: builds the Arch packages with makepkg (packaging/arch/PKGBUILD) and runs the Rust
# tests on Arch. See README.md.
FROM archlinux:latest
RUN pacman -Syu --noconfirm --needed base-devel git rust gtk4 gtk4-layer-shell pkgconf && pacman -Scc --noconfirm
CMD ["/usr/bin/bash"]
