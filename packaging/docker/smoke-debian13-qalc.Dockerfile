# swaydesk-smoke:debian13-qalc: the smoke image plus qalc and wl-clipboard, for the app grid's calculator
# (rust/scripts/headless-drawer-qalc.sh). BASE is the smoke image's tag. See README.md.
ARG BASE=swaydesk-smoke:debian13
FROM ${BASE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends qalc wl-clipboard && rm -rf /var/lib/apt/lists/*
