# Swaydesk

[Português (Brasil)](README.pt-BR.md)

Swaydesk is a desktop session built on [Sway](https://swaywm.org): the tiling Wayland compositor, plus the pieces that make it feel like a complete desktop. You get a bar with popups, a dock, an app grid, notifications, a lock screen and a login screen. Each piece is a small native program written in Rust and GTK 4, and they share one theme.

Full documentation, with screenshots and the keyboard shortcuts: **https://swaydesk.nielsonrolim.com** ([English](https://swaydesk.nielsonrolim.com/en-us/) · [Português](https://swaydesk.nielsonrolim.com/pt-br/)).

> **This repository is a read-only mirror.** Development happens in a private repository, and every change merged there is published here as one commit. Issues and pull requests are not open yet.

## What's in it

| Component | Package | What it does |
| --- | --- | --- |
| Bar and popups | `swaydesk-widgets` | Waybar modules and their popups: audio, network, Bluetooth, battery, brightness, media, weather, calendar, caffeine, system; the Alt+Tab switcher and the volume/brightness on-screen display |
| Dock | `swaydesk-dock` | Pinned and running apps, auto-hide, right-click menu |
| App grid | `swaydesk-drawer` | Full-screen app grid with groups, search, files and a calculator (qalc for units and conversions) |
| Launcher | `swaydesk-desktop` | rofi (Alt+Space) with apps, open windows, clipboard history and web search |
| Notifications | `swaydesk-notifd` | Notification daemon with toasts, history and do-not-disturb |
| Clipboard | `swaydesk-widgets` | History (cliphist) with a popup |
| Screenshot and recording | `swaydesk-capture` | Panels for screenshots (area, window, screen) and screen recording |
| Theme | `swaydesk-theme` | The Theme app: accent color, light/dark and icons, applied to GTK, Qt, Sway, Waybar and rofi |
| Session | `swaydesk-session` | Lock screen, session menu (log out, suspend, restart, shut down), keyring unlock and session services |
| Login screen | `swaydesk-greeter` | Optional greeter for greetd, in the look of the lock screen |
| Displays, Help | — | Coming soon |

`swaydesk-common` holds the shared data and fonts, and `swaydesk-scripts` the helper scripts.

## How it fits together

- `swaydesk-desktop` installs the **Sway Desktop** session, which you pick in your display manager (or in `swaydesk-greeter`), and depends on all of the above.
- At login, the session starts Sway with Swaydesk's config. That config applies the theme and starts Waybar, the dock, the notification daemon, the on-screen display, the Alt+Tab switcher, the clipboard history and the session services (idle, lock, keyring). Popups, the app grid and the capture panels open on demand.
- The theme tool writes one set of colors for every component, so changing the accent recolors the whole desktop.
- Your own settings live in `~/.config/sway-desktop/` and are never overwritten by package updates.

## Install

Packages come from the [openSUSE Build Service](https://build.opensuse.org) repository `home:nielsonrolim`, for x86_64. More on the [install page](https://swaydesk.nielsonrolim.com/en-us/install/).

The repository's signing key:
- fingerprint `71A5 E6CB 39B9 844E 42CE  42D3 5815 F0B6 5BE1 CFB9`;
- RSA 4096, expires on 2029-01-23.

The commands below check it for you: if the downloaded key isn't exactly one key with that fingerprint under `pub` and no subkeys, nothing gets installed.

### Debian 13, Ubuntu 26.04 and Linux Mint 27

You need `curl` and `gpg` (on a minimal system: `sudo apt install curl gpg`). On Ubuntu and Mint, replace `Debian_13` with `xUbuntu_26.04` in the first line.

```sh
repo=https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13
key=$(mktemp) && curl -fsSL "$repo/Release.key" -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo install -m644 "$key" /usr/share/keyrings/swaydesk-archive-keyring.asc && rm -f "$key" &&
printf 'Types: deb\nURIs: %s/\nSuites: ./\nSigned-By: /usr/share/keyrings/swaydesk-archive-keyring.asc\n' "$repo" |
  sudo tee /etc/apt/sources.list.d/swaydesk.sources >/dev/null &&
sudo apt update && sudo apt install swaydesk-archive-keyring swaydesk-desktop
```

Add `swaydesk-greeter` to the last line if you want the Swaydesk login screen (see below). From then on, the `swaydesk-archive-keyring` package takes care of the key and the repository and keeps them up to date.

### Arch Linux

In `/etc/pacman.conf`:

```ini
[home_nielsonrolim_Arch]
Server = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch
```

Then check and import the key, and install. The same check: if the downloaded key isn't exactly one key with that fingerprint under `pub` and no subkeys, nothing gets imported:

```sh
key=$(mktemp) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo pacman-key --add "$key" && rm -f "$key" &&
sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
sudo pacman -Syu swaydesk-desktop noto-fonts pipewire-jack
```

`noto-fonts` and `pipewire-jack` answer, in advance, two choices pacman would ask about (a font and the JACK provider): Swaydesk already uses Noto, and its audio is PipeWire. If you already have `jack2` and pacman asks whether to remove it, answer y.

Add `swaydesk-greeter` to the last line for the Swaydesk login screen.

#### Optional: SwayFX

[SwayFX](https://github.com/WillPower3309/swayfx) is Sway with rounded corners, blur and shadows, and on Arch it can replace Sway with the same configuration. It comes from the AUR, through an AUR helper: `yay -S swayfx` (or `paru -S swayfx`); pacman replaces `sway` with it, and `swaydesk-desktop` stays installed, since `swayfx` provides `sway`. To go back, `sudo pacman -S sway`.

- It's third-party software, outside the project's support, and an update that brings a new wlroots can break it until the AUR catches up. If the session doesn't start after an update, go back to Sway from a text console (Ctrl+Alt+F2).
- Swaydesk turns the effects on by itself at the next login, from the `[effects]` section of `~/.config/sway-desktop/config.toml`.
- The blur behind the bar, the dock and the popups (`layer_blur`) is off by default, since it showed visual glitches in a VM; turn it on with `layer_blur = true`.

### Requirements

- A GPU with a Wayland-capable driver: Intel, AMD, or NVIDIA (the proprietary driver works; Sway runs with `--unsupported-gpu`). In a VM, virtio-gpu works.
- Screen recording uses VA-API on Intel and AMD and falls back to the CPU elsewhere.

### Logging in

If you use GDM, SDDM or LightDM, there's nothing to set up: pick the **Sway Desktop** session on the login screen you already have.

### Optional: the Swaydesk login screen

`swaydesk-greeter` is an optional login screen for [greetd](https://sr.ht/~kennylevinsen/greetd/), in the look of the Swaydesk lock screen. On Debian and Ubuntu it turns itself on only when no other display manager is set; on Arch, turn it on with `sudo systemctl enable greetd`. To switch from GDM, SDDM or LightDM, disable that one and run `sudo systemctl enable greetd`; to go back, `sudo systemctl disable greetd` and enable the old one again (for example `sudo systemctl enable gdm`).

## Build from source

The components are a Cargo workspace in `rust/` (Rust 1.85 or newer, GTK 4.14 or newer):

```sh
# Debian / Ubuntu
sudo apt install cargo rustc pkgconf libgtk-4-dev libgtk4-layer-shell-dev
# Arch
sudo pacman -S --needed cargo gtk4 gtk4-layer-shell pkgconf

cd rust
cargo build --release
cargo test
```

The binaries land in `rust/target/release/swaydesk-*`. The packages bundle them with the session files:
- `packaging/install-tree.sh` installs each package's files, and both distros use it;
- `packaging/make-orig.sh` makes the source tarball (with the crates vendored, for offline builds);
- `debian/` is the Debian and Ubuntu packaging, and `packaging/arch/PKGBUILD` the Arch one.

These are the same files the OBS repository builds from. Translations are in `po/`.

## License

[MIT](LICENSE)
