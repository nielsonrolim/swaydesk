# Swaydesk

[Português (Brasil)](README.pt-BR.md)

Swaydesk is a desktop session built on [Sway](https://swaywm.org): the tiling Wayland compositor, plus the pieces that make it feel like a complete desktop. You get a bar with popups, a dock, an app grid, notifications, a lock screen and a login screen. Each piece is a small native program written in Rust and GTK 4, and they share one theme.

Full documentation, with screenshots and the keyboard shortcuts: **https://swaydesk.nielsonrolim.com** ([English](https://swaydesk.nielsonrolim.com/en-us/) · [Português](https://swaydesk.nielsonrolim.com/pt-br/)).

**Current release: 0.1.7.** What changes in each release is in [CHANGELOG.md](CHANGELOG.md) and on the [releases page](https://github.com/nielsonrolim/swaydesk/releases). Once installed, updates arrive with your usual `apt upgrade` or `pacman -Syu`.

Swaydesk is an independent project built on top of [Sway](https://swaywm.org). It is not affiliated with, or endorsed by, the Sway project or its developers.

> **This repository is a read-only mirror.** Development happens in a private repository, and every change merged there is published here as one commit. Issues and pull requests are not open yet.

## What's in it

| Component | Package | What it does |
| --- | --- | --- |
| Bar items | `swaydesk-calendar`, `swaydesk-weather`, `swaydesk-audio`, `swaydesk-network`, `swaydesk-bluetooth`, `swaydesk-battery`, `swaydesk-brightness`, `swaydesk-caffeine`, `swaydesk-system`, `swaydesk-media`, `swaydesk-language` | One package per Waybar module and its popup |
| Volume and brightness indicator | `swaydesk-osd` | Icon, bar and percentage on screen for a second; `sway-osd` changes the level from the volume and brightness keys |
| Window switcher | `swaydesk-switcher` | Alt+Tab: windows in the order you used them, from every workspace |
| Dock | `swaydesk-dock` | Pinned and running apps, auto-hide, right-click menu |
| App grid | `swaydesk-drawer` | Full-screen app grid with groups, search, files and a calculator (qalc for units and conversions) |
| Launcher | `swaydesk-desktop` | rofi (Alt+Space) with apps, open windows, clipboard history and web search |
| Notifications | `swaydesk-notifd` | Notification daemon with toasts, history and do-not-disturb |
| Clipboard | `swaydesk-clipboard` | History (cliphist) with a popup |
| Screenshot and recording | `swaydesk-capture` | Panels for screenshots (area, window, screen) and screen recording |
| Theme | `swaydesk-theme-app`, `swaydesk-theme-tool`, `swaydesk-portal` | The Theme app: accent color, light/dark and icons, applied to GTK, Qt, Sway, Waybar and rofi; the command-line tools it uses, and the portal that gives the accent color to libadwaita apps |
| Displays | `swaydesk-displays` | The Displays app: arrange the screens on a map, with resolution, refresh rate, scale and rotation, and a profile per set of monitors, applied when they're plugged in or the lid closes |
| Help | `swaydesk-help` | A window with the Sway shortcuts in use and a card per desktop app |
| Lock screen | `swaydesk-lock` | Time, date and the password on every monitor, over the blurred wallpaper |
| Session | `swaydesk-session-menu`, `swaydesk-keyring`, `swaydesk-sessionmgr` | Session menu (log out, suspend, restart, shut down), keyring unlock and the session manager for software updates |
| Login screen | `swaydesk-greeter` | Greeter for greetd, in the look of the lock screen: optional with another display manager, needed for a graphical login without one |

Each component installs on its own. A few packages install groups:
- `swaydesk-bar`: every bar item, with the volume and brightness indicator;
- `swaydesk-apps`: the Theme, Displays and Help apps, and the screenshot panels;
- `swaydesk-desktop`: the full session, with everything.

`swaydesk-widgets`, `swaydesk-theme` and `swaydesk-session`, the package names before 0.1.7, now install the same components as before. `swaydesk-scripts` is a transitional package that goes away in 0.2.0. `swaydesk-common` holds the shared data, fonts and helper scripts. On Debian and Ubuntu, `swaydesk-desktop` also brings `swaydesk-efi`, which keeps the firmware's fallback boot path (`EFI/BOOT`) working under Secure Boot on systems installed from a Swaydesk image. On any other system, including one that only added this repository, it does nothing: it never touches the ESP, the boot entries or another system's files. To turn it off: `sudo mkdir -p /etc/swaydesk && echo off | sudo tee /etc/swaydesk/efi-fallback`. To see why it did or didn't act, run `sudo SWAYDESK_EFI_VERBOSE=1 /usr/libexec/swaydesk/swaydesk-efi-fallback`: it says which check stopped it, or what it changed.

## How it fits together

- `swaydesk-desktop` installs the **Sway Desktop** session, which you pick in your display manager (or in `swaydesk-greeter`), and depends on all of the above.
- At login, the session starts Sway with Swaydesk's config. That config applies the theme and starts Waybar, the dock, the notification daemon, the on-screen display, the Alt+Tab switcher, the clipboard history and the session services (idle, lock, keyring). Popups, the app grid and the capture panels open on demand.
- The theme tool writes one set of colors for every component, so changing the accent recolors the whole desktop.
- Your own settings live in `~/.config/swaydesk/` and are never overwritten by package updates.

## Install

The packages come from a repository on the [Open Build Service](https://build.opensuse.org/), signed with the project's key. The desktop you already have stays on the login screen, and updates come with your usual `apt upgrade` or `pacman -Syu`.

You need Debian 13 or testing, Ubuntu 26.04, Linux Mint 27 or Arch Linux, 64-bit (x86_64), and an Intel, AMD or NVIDIA graphics card with a Wayland driver. Pick your distribution and run one step at a time:

The same steps, with a copy button on each command, are on the [install page](https://swaydesk.nielsonrolim.com/en-us/install/).

### Debian 13

1. Install `curl`, which downloads the repository package.

   ```bash
   sudo apt install curl
   ```

2. Download the `swaydesk-archive-keyring` package: it adds the repository and the project's key.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/swaydesk-archive-keyring_2026.11.20_all.deb -o ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

3. Check the downloaded file. The command must print `OK`; if it doesn't, stop here, delete the file and don't go on.

   ```bash
   echo "2527bebb378c926e631fb367f2ccb18120cea26617c3a90b859c633cb2845700  $HOME/swaydesk-archive-keyring_2026.11.20_all.deb" | sha256sum -c
   ```

4. Install it. It puts the key in `/usr/share/keyrings/` and the repository in `/etc/apt/sources.list.d/swaydesk.sources`, and from then on keeps both up to date.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.20_all.deb && rm ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

Optional, before step 5: see the installed key's fingerprint (needs `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Exactly one key must show up (one pub line, no sub line), with this fingerprint right under pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. The project's key on OBS: RSA, 4096 bits, valid until 2029-01-23. If it's different, stop here and remove the repository and its key: `sudo apt purge swaydesk-archive-keyring`.

5. Update the package list and install Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

#### All in one command

After step 1 (curl), the other steps chained: if the downloaded file isn't the expected one, nothing gets installed.

```bash
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_13/all/$deb -o "$dir/$deb" &&
echo "2527bebb378c926e631fb367f2ccb18120cea26617c3a90b859c633cb2845700  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

### Debian testing

Debian testing is best effort: during a library transition in testing the packages can break for a few days, until a new revision.

1. Install `curl`, which downloads the repository package.

   ```bash
   sudo apt install curl
   ```

2. Download the `swaydesk-archive-keyring` package: it adds the repository and the project's key.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_Testing/all/swaydesk-archive-keyring_2026.11.20_all.deb -o ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

3. Check the downloaded file. The command must print `OK`; if it doesn't, stop here, delete the file and don't go on.

   ```bash
   echo "0fa0971606384baf62a634376006c5b01e52a095795f88c730dc8e07104cb664  $HOME/swaydesk-archive-keyring_2026.11.20_all.deb" | sha256sum -c
   ```

4. Install it. It puts the key in `/usr/share/keyrings/` and the repository in `/etc/apt/sources.list.d/swaydesk.sources`, and from then on keeps both up to date.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.20_all.deb && rm ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

Optional, before step 5: see the installed key's fingerprint (needs `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Exactly one key must show up (one pub line, no sub line), with this fingerprint right under pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. The project's key on OBS: RSA, 4096 bits, valid until 2029-01-23. If it's different, stop here and remove the repository and its key: `sudo apt purge swaydesk-archive-keyring`.

5. Update the package list and install Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

#### All in one command

After step 1 (curl), the other steps chained: if the downloaded file isn't the expected one, nothing gets installed.

```bash
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/Debian_Testing/all/$deb -o "$dir/$deb" &&
echo "0fa0971606384baf62a634376006c5b01e52a095795f88c730dc8e07104cb664  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

### Ubuntu 26.04 / Mint 27

1. Install `curl`, which downloads the repository package.

   ```bash
   sudo apt install curl
   ```

2. Download the `swaydesk-archive-keyring` package: it adds the repository and the project's key.

   ```bash
   curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/all/swaydesk-archive-keyring_2026.11.20_all.deb -o ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

3. Check the downloaded file. The command must print `OK`; if it doesn't, stop here, delete the file and don't go on.

   ```bash
   echo "a34f455e495c69a2c5dd94ff8ce6dd0618ec569d6b010010c2360432f6029020  $HOME/swaydesk-archive-keyring_2026.11.20_all.deb" | sha256sum -c
   ```

4. Install it. It puts the key in `/usr/share/keyrings/` and the repository in `/etc/apt/sources.list.d/swaydesk.sources`, and from then on keeps both up to date.

   ```bash
   sudo dpkg -i ~/swaydesk-archive-keyring_2026.11.20_all.deb && rm ~/swaydesk-archive-keyring_2026.11.20_all.deb
   ```

Optional, before step 5: see the installed key's fingerprint (needs `gpg`: `sudo apt install gpg`).

```bash
gpg --show-keys /usr/share/keyrings/swaydesk-archive-keyring.asc
```

Exactly one key must show up (one pub line, no sub line), with this fingerprint right under pub: `71A5E6CB39B9844E42CE42D35815F0B65BE1CFB9`. The project's key on OBS: RSA, 4096 bits, valid until 2029-01-23. If it's different, stop here and remove the repository and its key: `sudo apt purge swaydesk-archive-keyring`.

5. Update the package list and install Swaydesk.

   ```bash
   sudo apt update && sudo apt install swaydesk-desktop
   ```

#### All in one command

After step 1 (curl), the other steps chained: if the downloaded file isn't the expected one, nothing gets installed.

```bash
deb=swaydesk-archive-keyring_2026.11.20_all.deb
dir=$(mktemp -d) && curl -fsSL https://download.opensuse.org/repositories/home:/nielsonrolim/xUbuntu_26.04/all/$deb -o "$dir/$deb" &&
echo "a34f455e495c69a2c5dd94ff8ce6dd0618ec569d6b010010c2360432f6029020  $dir/$deb" | sha256sum -c && sudo dpkg -i "$dir/$deb" && rm -r "$dir" &&
sudo apt update && sudo apt install swaydesk-desktop
```

On a minimized Ubuntu, such as the server and cloud images, dpkg leaves out every package's translations and manuals, the language packs' included. Swaydesk keeps its own translations; for the rest, run `sudo unminimize` before installing a language pack.

If your system isn't in English, also install the GTK translations for your language. Without them, some windows, such as the file chooser, stay in English. For Portuguese, for example (es for Spanish, de for German, and so on):

```bash
sudo apt install language-pack-gnome-pt
```

The Swaydesk session reminds you once, with the command for your language, when they're missing. Debian doesn't need this: it ships these translations with GTK.

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
- Swaydesk turns the effects on by itself at the next login, from the `[effects]` section of `~/.config/swaydesk/config.toml`.
- The blur behind the bar, the dock and the popups (`layer_blur`) is off by default, since it showed visual glitches in a VM; turn it on with `layer_blur = true`.
- Window animations are off by default; turn them on with `animation_duration_ms = 250` (in milliseconds, up to 5000) in the same section. They need SwayFX 0.6 or newer: on 0.5, a value above 0 is a config error (swaynag shows it, and the rest loads).

### Requirements

- A GPU with a Wayland-capable driver: Intel, AMD, or NVIDIA (the proprietary driver works; Sway runs with `--unsupported-gpu`). In a VM, virtio-gpu works.
- Screen recording uses VA-API on Intel and AMD and falls back to the CPU elsewhere.

### Logging in

If you use GDM, SDDM or LightDM, there's nothing to set up: pick the **Sway Desktop** session on the login screen you already have.

Without a display manager, as on a minimal or server install, the system starts in a text console: install `swaydesk-greeter`, below, for a graphical login.

### The Swaydesk login screen

`swaydesk-greeter` is a login screen for [greetd](https://sr.ht/~kennylevinsen/greetd/), in the look of the Swaydesk lock screen. It's optional if you already have GDM, SDDM or LightDM, and needed for a graphical login on a system without a display manager. Install it with `sudo apt install swaydesk-greeter` (Debian, Ubuntu, Mint) or `sudo pacman -S swaydesk-greeter` (Arch). On Debian and Ubuntu it turns itself on only when no other display manager is set; on Arch, turn it on with `sudo systemctl enable greetd`. To switch from GDM, SDDM or LightDM, disable that one and run `sudo systemctl enable greetd`; to go back, `sudo systemctl disable greetd` and enable the old one again (for example `sudo systemctl enable gdm`).

## Using one component on plain Sway

You don't need the whole session: install only the package you want (`sudo apt install swaydesk-dock`, or `sudo pacman -Syu swaydesk-dock`), and it brings only what it needs. Nothing changes in your Sway until you opt in, and the Swaydesk session, if you install it later, starts its components itself.

**On plain Sway, include the component's snippet.** Each package that runs something on Sway ships one in `/usr/share/swaydesk/sway/config.d/`, and nothing reads it until you add its `include` line to `~/.config/sway/config`, after the `set $mod` line:

```
include /usr/share/swaydesk/sway/config.d/50-swaydesk-dock.conf
```

| Component | Package | Snippet | What it adds |
| --- | --- | --- | --- |
| Dock | `swaydesk-dock` | `50-swaydesk-dock.conf` | The dock |
| App grid | `swaydesk-drawer` | `50-swaydesk-drawer.conf` | The grid, on Super+Shift+D |
| Notifications | `swaydesk-notifd` | `50-swaydesk-notifd.conf` | The notification service, and its center on Super+N |
| Volume and brightness indicator | `swaydesk-osd` | `50-swaydesk-osd.conf` | The indicator, and the volume and brightness keys that show it |
| Window switcher | `swaydesk-switcher` | `50-swaydesk-switcher.conf` | Alt+Tab |
| Displays | `swaydesk-displays` | `50-swaydesk-displays.conf` | The saved layout, applied when screens change and when the lid opens or closes |
| Clipboard history | `swaydesk-clipboard` | `50-swaydesk-clipboard.conf` | The history, on Super+Shift+V |
| Lock screen | `swaydesk-lock` | `50-swaydesk-lock.conf` | Ctrl+Alt+L, and the lock after 10 minutes idle and before sleep, through swayidle |

- **The running components** (all but the lock) start as systemd user services, restarted if they crash, and stop when that Sway exits. Each snippet explains itself in its comments, and ends with the plain `exec` line to use instead on a system without a systemd user session.
- **If your config already starts the component** with its own `exec` line, remove that line: the dock, the grid, the switcher, the displays and the indicator exit at once beside a copy already running, the notifications service waits until the other one (mako, say) exits, and two clipboard watchers would store every copy twice.
- **In a session that starts `graphical-session.target`** (niri, or Hyprland with uwsm, for example), turn the service on once instead: `systemctl --user enable --now swaydesk-dock` (the service names are the package names). No package turns them on, and they never start inside the Swaydesk session.
- **Bar items for your own Waybar:** each one ships its module in `/usr/share/swaydesk/waybar/`, for example `swaydesk-calendar.jsonc`, with the lines to paste into your Waybar config.

**The lock screen.** `swaydesk-lock` on its own locks the screen and returns once it's locked, so it works from a key and from swayidle. If your config already starts swayidle, keep a single one: instead of including the lock's snippet, add its timeout and before-sleep to your own line, for example:

```
bindsym Ctrl+Alt+l exec swaydesk-lock
exec swayidle -w timeout 600 swaydesk-lock before-sleep swaydesk-lock
```

**Caffeine.** Outside the Swaydesk session (a plain Sway with your own swayidle), caffeine holds a Wayland idle inhibitor (`swaydesk-caffeinate`) instead of replacing swayidle, so your swayidle and its timeouts are left as they are. With every monitor disabled or unplugged (or, on Sway 1.12, powered off), the idle timeouts can still fire.

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
