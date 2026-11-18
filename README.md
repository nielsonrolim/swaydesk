# Swaydesk

[Português (Brasil)](README.pt-BR.md)

Swaydesk is a desktop session built on [Sway](https://swaywm.org): the tiling Wayland compositor, plus the pieces that make it feel like a complete desktop. You get a bar with popups, a dock, an app grid, notifications, a lock screen and a login screen. Each piece is a small native program written in Rust and GTK 4, and they share one theme.

Full documentation, with screenshots and the keyboard shortcuts: **https://swaydesk.nielsonrolim.com** ([English](https://swaydesk.nielsonrolim.com/en-us/) · [Português](https://swaydesk.nielsonrolim.com/pt-br/)).

**Current release: 0.1.0.** What changes in each release is in [CHANGELOG.md](CHANGELOG.md) and on the [releases page](https://github.com/nielsonrolim/swaydesk/releases). Once installed, updates arrive with your usual `apt upgrade` or `pacman -Syu`.

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
| Help | `swaydesk-widgets` | A window with the Sway shortcuts in use and a card per desktop app |
| Displays | `swaydesk-theme` | The Displays app: arrange the screens on a map, with resolution, refresh rate, scale and rotation, and a profile per set of monitors, applied when they're plugged in or the lid closes |

`swaydesk-common` holds the shared data and fonts, and `swaydesk-scripts` the helper scripts.

## How it fits together

- `swaydesk-desktop` installs the **Sway Desktop** session, which you pick in your display manager (or in `swaydesk-greeter`), and depends on all of the above.
- At login, the session starts Sway with Swaydesk's config. That config applies the theme and starts Waybar, the dock, the notification daemon, the on-screen display, the Alt+Tab switcher, the clipboard history and the session services (idle, lock, keyring). Popups, the app grid and the capture panels open on demand.
- The theme tool writes one set of colors for every component, so changing the accent recolors the whole desktop.
- Your own settings live in `~/.config/sway-desktop/` and are never overwritten by package updates.

## Install

The packages come from a repository on the [Open Build Service](https://build.opensuse.org/), signed with the project's key. The desktop you already have stays on the login screen, and updates come with your usual `apt upgrade` or `pacman -Syu`.

You need Debian 13, Ubuntu 26.04, Linux Mint 27 or Arch Linux, 64-bit (x86_64), and an Intel, AMD or NVIDIA graphics card with a Wayland driver. Pick your distribution and run one step at a time:

The same steps, with a copy button on each command, are on the [install page](https://swaydesk.nielsonrolim.com/en-us/install/).

### Debian 13

1. Install `curl` and `gpg`, which download and check the key.

   ```bash
   sudo apt install curl gpg
   ```

2. Download the repository key.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/Release.key -o ~/swaydesk.key
   ```

3. Show the downloaded key's fingerprint.

   ```bash
   gpg --show-keys ~/swaydesk.key
   ```

   Exactly one key must show up (one pub line, no sub line), with this fingerprint right under pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. If it's different, stop here and don't go on. The project's key on OBS: RSA, 4096 bits, valid until 2029-01-23.

4. Install the key where apt looks for it. The command checks the key again and refuses any other: exactly one key, with that fingerprint right under `pub`, and no subkeys.

   ```bash
   [ "$(gpg --show-keys --with-colons ~/swaydesk.key | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
   sudo install -m644 ~/swaydesk.key /usr/share/keyrings/swaydesk-archive-keyring.asc &&
   rm ~/swaydesk.key
   ```

5. Add the repository.

   ```bash
   sudo tee /etc/apt/sources.list.d/swaydesk.sources <<'EOF'
   Types: deb
   URIs: https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/
   Suites: ./
   Signed-By: /usr/share/keyrings/swaydesk-archive-keyring.asc
   EOF
   ```

6. Update the package list.

   ```bash
   sudo apt update
   ```

7. Install Swaydesk. From then on, the `swaydesk-archive-keyring` package takes care of the key and the repository.

   ```bash
   sudo apt install swaydesk-archive-keyring swaydesk-desktop
   ```

#### All in one command

After step 1 (curl and gpg), the other steps chained: if the downloaded key doesn't have the right fingerprint, nothing gets installed.

```bash
repo=https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13
key=$(mktemp) && curl -fsSL "$repo/Release.key" -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo install -m644 "$key" /usr/share/keyrings/swaydesk-archive-keyring.asc && rm -f "$key" &&
printf 'Types: deb\nURIs: %s/\nSuites: ./\nSigned-By: /usr/share/keyrings/swaydesk-archive-keyring.asc\n' "$repo" |
  sudo tee /etc/apt/sources.list.d/swaydesk.sources >/dev/null &&
sudo apt update && sudo apt install swaydesk-archive-keyring swaydesk-desktop
```

### Ubuntu 26.04 / Mint 27

1. Install `curl` and `gpg`, which download and check the key.

   ```bash
   sudo apt install curl gpg
   ```

2. Download the repository key.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/Release.key -o ~/swaydesk.key
   ```

3. Show the downloaded key's fingerprint.

   ```bash
   gpg --show-keys ~/swaydesk.key
   ```

   Exactly one key must show up (one pub line, no sub line), with this fingerprint right under pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. If it's different, stop here and don't go on. The project's key on OBS: RSA, 4096 bits, valid until 2029-01-23.

4. Install the key where apt looks for it. The command checks the key again and refuses any other: exactly one key, with that fingerprint right under `pub`, and no subkeys.

   ```bash
   [ "$(gpg --show-keys --with-colons ~/swaydesk.key | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
   sudo install -m644 ~/swaydesk.key /usr/share/keyrings/swaydesk-archive-keyring.asc &&
   rm ~/swaydesk.key
   ```

5. Add the repository.

   ```bash
   sudo tee /etc/apt/sources.list.d/swaydesk.sources <<'EOF'
   Types: deb
   URIs: https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/
   Suites: ./
   Signed-By: /usr/share/keyrings/swaydesk-archive-keyring.asc
   EOF
   ```

6. Update the package list.

   ```bash
   sudo apt update
   ```

7. Install Swaydesk. From then on, the `swaydesk-archive-keyring` package takes care of the key and the repository.

   ```bash
   sudo apt install swaydesk-archive-keyring swaydesk-desktop
   ```

#### All in one command

After step 1 (curl and gpg), the other steps chained: if the downloaded key doesn't have the right fingerprint, nothing gets installed.

```bash
repo=https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04
key=$(mktemp) && curl -fsSL "$repo/Release.key" -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo install -m644 "$key" /usr/share/keyrings/swaydesk-archive-keyring.asc && rm -f "$key" &&
printf 'Types: deb\nURIs: %s/\nSuites: ./\nSigned-By: /usr/share/keyrings/swaydesk-archive-keyring.asc\n' "$repo" |
  sudo tee /etc/apt/sources.list.d/swaydesk.sources >/dev/null &&
sudo apt update && sudo apt install swaydesk-archive-keyring swaydesk-desktop
```

### Arch Linux

1. Add the repository to the end of `/etc/pacman.conf` (only once).

   ```bash
   sudo tee -a /etc/pacman.conf <<'EOF'

   [home_nielsonrolim_Arch]
   Server = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch
   EOF
   ```

2. Download the repository key.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o ~/swaydesk.key
   ```

3. Show the downloaded key's fingerprint.

   ```bash
   gpg --show-keys ~/swaydesk.key
   ```

   Exactly one key must show up (one pub line, no sub line), with this fingerprint right under pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. If it's different, stop here and don't go on. The project's key on OBS: RSA, 4096 bits, valid until 2029-01-23.

4. Add the key to pacman's keyring and sign it locally, so pacman trusts it. The command checks the key again first and refuses any other, as in step 3.

   ```bash
   [ "$(gpg --show-keys --with-colons ~/swaydesk.key | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
   sudo pacman-key --add ~/swaydesk.key &&
   sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
   rm ~/swaydesk.key
   ```

5. Install Swaydesk. If you already have `jack2`, pacman asks whether to replace it with `pipewire-jack`, the JACK of PipeWire, which Swaydesk uses: answer y.

   ```bash
   sudo pacman -Syu swaydesk-desktop
   ```

#### All in one command

The same steps, chained: if the downloaded key doesn't have the right fingerprint, nothing gets added or installed.

```bash
key=$(mktemp) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/x86_64/home_nielsonrolim_Arch.key -o "$key" &&
[ "$(gpg --show-keys --with-colons "$key" | grep -E '^(pub|sub|fpr):' | cut -d: -f1,10 | tr '\n' ' ')" = 'pub: fpr:71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 ' ] &&
sudo pacman-key --add "$key" && rm -f "$key" &&
sudo pacman-key --lsign-key 71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9 &&
printf '\n[home_nielsonrolim_Arch]\nServer = https://download.opensuse.org/repositories/home:/nielsonrolim/Arch/$arch\n' | sudo tee -a /etc/pacman.conf >/dev/null &&
sudo pacman -Syu swaydesk-desktop
```


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

`swaydesk-greeter` is an optional login screen for [greetd](https://sr.ht/~kennylevinsen/greetd/), in the look of the Swaydesk lock screen. Install it with `sudo apt install swaydesk-greeter` (Debian, Ubuntu, Mint) or `sudo pacman -S swaydesk-greeter` (Arch). On Debian and Ubuntu it turns itself on only when no other display manager is set; on Arch, turn it on with `sudo systemctl enable greetd`. To switch from GDM, SDDM or LightDM, disable that one and run `sudo systemctl enable greetd`; to go back, `sudo systemctl disable greetd` and enable the old one again (for example `sudo systemctl enable gdm`).

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
