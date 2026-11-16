# swaydesk-smoke:debian13: a headless Sway for the tests in rust/scripts/headless-*.sh, with GTK 3 and 4,
# layer-shell for both, Python's GObject bindings (the Python components), Mesa (software GL), grim for the
# screenshots and wtype for the keyboard. See README.md.
FROM debian:13
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    sway python3 python3-gi python3-gi-cairo gir1.2-gtk-3.0 gir1.2-gtk-4.0 gir1.2-gtklayershell-0.1 gir1.2-gtk4layershell-1.0 \
    libgtk4-layer-shell0 gir1.2-pango-1.0 gir1.2-gdkpixbuf-2.0 adwaita-icon-theme fonts-noto-core \
    procps psmisc ca-certificates libgl1-mesa-dri libegl1 mesa-vulkan-drivers mesa-utils-bin grim wtype librsvg2-common \
    && rm -rf /var/lib/apt/lists/*
CMD ["bash"]
