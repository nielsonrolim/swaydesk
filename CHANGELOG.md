# Changelog

Each release is published as packages and, shortly after, as install images. Updates arrive through `apt upgrade` or `pacman -Syu`.

## 0.1.1 — 2026-11-18

A bug-fix release.

### Fixed

- After logging out and in again, apps no longer wait 120 s for the gtk portal, and libadwaita apps keep the dark style. `swaydesk-desktop` now starts the gtk portal directly. It takes effect after a full logout or a reboot following the upgrade. It applies to every session on the machine, and in other desktops it starts the same portal they already use.
- The login screen no longer starts the desktop portals for its own user, which logged "Failed to start xdg-desktop-portal-gtk.service" in the journal.
- With a single keyboard layout, the bar no longer leaves an empty gap where the layout indicator was. With two or more layouts, the indicator shows up. If you add a layout while logged in, it appears after a layout switch (on Debian and Ubuntu, sometimes only after the second) or at the next login.

## 0.1.0 — 2026-11-16

The first release. Swaydesk is a desktop session for [Sway](https://swaywm.org). Each component is a native program in Rust and GTK 4, and they all share one theme.

### Supported systems

- Debian 13, Ubuntu 26.04 and Linux Mint 27: packages from the `home:nielsonrolim` repository on the Open Build Service, signed with the project's key, plus `swaydesk-archive-keyring` to keep the key and the source up to date.
- Arch Linux: packages from the same repository. `pacman -Syu swaydesk-desktop` installs the whole session and asks no provider questions.
- x86_64, with an Intel, AMD or NVIDIA graphics card with a Wayland driver.

### What's in it

- **Bar and popups** (`swaydesk-widgets`): Waybar modules with popups for audio, network, Bluetooth, battery, brightness, media, weather, calendar, caffeine and system. Also the Alt+Tab switcher, the volume and brightness on-screen display, and the clipboard history.
- **Dock** (`swaydesk-dock`): pinned and running apps, auto-hide, right-click menu.
- **App grid** (`swaydesk-drawer`): a full-screen grid with groups, search, files and a calculator.
- **Launcher**: rofi (Alt+Space in the Swaydesk profile), with apps, open windows, clipboard history and web search. One click opens an entry, and a click outside closes it.
- **Notifications** (`swaydesk-notifd`): toasts, history and do-not-disturb.
- **Screenshot and recording** (`swaydesk-capture`): panels for area, window and screen.
- **Theme** (`swaydesk-theme`): the Desktop Theme app sets the accent color, light or dark mode and the icons, and applies them to GTK, Qt, Sway, Waybar and rofi.
- **Session** (`swaydesk-session`): lock screen, session menu, keyring unlock and session services.
- **Help** (`swaydesk-widgets`): a window with the Sway shortcuts in use and a card per desktop app. It also switches the shortcut profile and opens your shortcuts file: in a graphical text editor (the packages recommend gnome-text-editor) or, without one, in an editor in the terminal.
- **Displays** (`swaydesk-theme`): arrange the screens on a map and set resolution, refresh rate, scale, rotation and on/off; a change is undone by itself after 15 s unless you keep it. Each set of monitors gets its own profile, applied automatically when you plug or unplug a screen, and the internal screen turns off with the lid closed and an external one on. The app opens on the screen you are on, and Undo puts back the workspaces of a screen you had turned off.
- **Login screen** (`swaydesk-greeter`): an optional greeter for greetd, in the look of the lock screen. Logging in through it unlocks the keyring with your login password (on Debian 13, see Known limitations for the first login).
- **SwayFX on Arch** (optional, from the AUR): rounded corners, shadows and window blur, set in the `[effects]` section of `~/.config/sway-desktop/config.toml`. SwayFX is third-party and outside the project's support.

### Make it yours

- **Shortcut profiles**: Swaydesk, i3, Hyprland, niri and dwm (dwm uses Alt as the modifier, as dwm does), all with 9 workspaces. Pick one in the Help, where it applies at once, or with `shortcuts` in `~/.config/sway-desktop/config.toml` followed by `swaymsg reload`; either way, no need to log out.
- **Your own shortcuts**: `~/.config/sway/config.d/50-local.conf`, created at your first login and read after the profile, with commented examples to change, remove and add a shortcut. The Help shows the result. See [Customize shortcuts](https://swaydesk.nielsonrolim.com/en-us/shortcuts/#customize-shortcuts).
- **Turn components off**: the `[enabled]` section of `config.toml` turns off the dock, the app grid, the Alt+Tab card, notifications, the on-screen display, the clipboard history, the night light, the daily wallpaper, the polkit agent, the update notifier, the automatic screen profiles and single bar modules. A mistake in the file never turns anything off. Changes apply at the next login.

Your settings live in `~/.config/sway-desktop/`, and package updates never overwrite them.

### Known limitations

- Blur behind the bar, the dock and the popups (`layer_blur`) is off by default. In a VM it left visual glitches; it hasn't been tested on a real GPU yet.
- No Debian testing packages yet.
- Debian 13: on each user's first login, the keyring asks for your password once, after a notification that explains it. From the next login on, it unlocks by itself. gnome-keyring 48 creates the login keyring but doesn't publish it on D-Bus ([gnome-keyring#137](https://gitlab.gnome.org/GNOME/gnome-keyring/-/issues/137), fixed upstream in 51.0; Debian 13 ships 48), so Swaydesk restarts the daemon, and the unlock from the login is lost. Ubuntu 26.04 and Arch are not affected.
