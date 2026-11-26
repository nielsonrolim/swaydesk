# swaydesk-smoke:debian13-gsettings: the smoke image plus the desktop's GSettings schemas and the gsettings tool, for
# the windows that follow a session theme change (rust/scripts/headless-dark-windows.sh). Apart from the smoke image,
# so the other headless tests keep GTK without org.gnome.desktop.interface (its font, theme and cursor defaults would
# change their screenshots). BASE is the smoke image's tag. See README.md.
ARG BASE=swaydesk-smoke:debian13
FROM ${BASE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends gsettings-desktop-schemas libglib2.0-bin && rm -rf /var/lib/apt/lists/*
