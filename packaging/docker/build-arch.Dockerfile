# swaydesk-build:arch: builds the Arch packages with makepkg (packaging/arch/PKGBUILD), runs the Rust
# tests on Arch, and the Arch-only Python test files (python, and jq for sway-updates). See README.md.
FROM archlinux:latest
RUN pacman -Syu --noconfirm --needed base-devel git rust gtk4 gtk4-layer-shell pkgconf python jq kbd && pacman -Scc --noconfirm
CMD ["/usr/bin/bash"]
