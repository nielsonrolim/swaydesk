# Build and test images

The container images the project's builds and headless tests run in. Until 11/2026 they only existed in one
machine's Docker; these files recreate them. The ISOs need no image of their own: `iso/*/build.sh` start from
the base distribution image (`debian:13`, `ubuntu:26.04`, `archlinux`).

| Image | File | Used for |
| --- | --- | --- |
| `swaydesk-build:debian13` | `build-debian13.Dockerfile` | `cargo build`/`cargo test` of `rust/` for Debian 13 |
| `swaydesk-build:arch` | `build-arch.Dockerfile` | `makepkg` of `packaging/arch/PKGBUILD`, and the Rust tests on Arch |
| `swaydesk-smoke:debian13` | `smoke-debian13.Dockerfile` | the headless Sway tests, `rust/scripts/headless-*.sh` |
| `swaydesk-smoke:debian13-qalc` | `smoke-debian13-qalc.Dockerfile` | `rust/scripts/headless-drawer-qalc.sh` (the smoke image plus qalc and wl-clipboard) |

## Making them

From the repository root; the qalc image is built on top of the smoke one, so that one comes first:

```sh
docker build -t swaydesk-build:debian13 -f packaging/docker/build-debian13.Dockerfile packaging/docker
docker build -t swaydesk-build:arch -f packaging/docker/build-arch.Dockerfile packaging/docker
docker build -t swaydesk-smoke:debian13 -f packaging/docker/smoke-debian13.Dockerfile packaging/docker
docker build -t swaydesk-smoke:debian13-qalc -f packaging/docker/smoke-debian13-qalc.Dockerfile packaging/docker
```

`--build-arg BASE=<tag>` gives the qalc image another smoke image to start from. The bases are the moving
tags `debian:13` and `archlinux:latest`, so a rebuild picks up the distributions' updates; the Arch one, a
rolling release, also its newer Rust and GTK.

## Using them

The builds take several GB of memory. Every build or test container runs with `--memory 10g --memory-swap 10g`
and `CARGO_BUILD_JOBS=8`, or 4 for an Arch package build, whose LTO links went over 10 GB at 8 jobs. It also waits
for a shared slot (for example a `flock` file), and at most 2 Rust builds run at once. Without the cap, cargo
uses every core: an Arch `makepkg` at 16 jobs reached 19.6 GB. Large outputs (the cargo target, packages) are
better kept outside `/tmp`.

Rust on Debian 13, as your own user (so the target isn't root's), with the cargo cache outside the
repository:

```sh
docker run --rm --memory 10g --memory-swap 10g -e CARGO_BUILD_JOBS=8 -u "$(id -u):$(id -g)" -e HOME=/tmp \
  -v "$PWD":/repo -v CARGO_HOME_DIR:/cargo -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/repo/rust/target/debian13 \
  -w /repo/rust swaydesk-build:debian13 \
  cargo build --release
```

The Arch packages, from a `git archive` of the commit and the PKGBUILD, as your own user. The tarball is made
locally, which is why `--skipchecksums` is fine here; never with a downloaded source:

```sh
git archive --prefix=sway-desktop-0.1.0/ -o BUILD_DIR/swaydesk-0.1.0.tar.gz HEAD
cp packaging/arch/PKGBUILD packaging/arch/*.install BUILD_DIR/
docker run --rm --memory 10g --memory-swap 10g -e CARGO_BUILD_JOBS=4 -u "$(id -u):$(id -g)" \
  -v /etc/passwd:/etc/passwd:ro -e HOME=/tmp -v BUILD_DIR:/build -v CARGO_HOME_DIR:/cargo -e CARGO_HOME=/cargo -w /build \
  swaydesk-build:arch makepkg -fd --skipchecksums --noconfirm
```

A headless test, with the binaries built above: the header of each `rust/scripts/headless-*.sh` has its
`docker run` line. Keep each run short and let the container go (`--rm`): the host sees its processes, under
your own UID, and a drawer, dock or lock left running there confuses your real session.
