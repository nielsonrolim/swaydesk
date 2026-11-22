# Changelog

Each release is published as packages and, shortly after, as install images. Updates arrive through `apt upgrade` or `pacman -Syu`.

## 0.1.3.1 — 2026-11-22

A bug-fix release for the keyboard and the install images: `ç` on US International, accents in GTK 4 apps, the system keyboard reaching existing accounts, and the Arch installer from a USB stick.

### Fixed

- With the US international keyboard, `'` + `c` now gives `ç` (and `'` + `C` gives `Ç`) in GTK, Qt and terminal apps and at the login screen: the session creates `~/.XCompose` with `include "%L"`, so the other accents keep working. A `~/.XCompose` of your own is never touched.
- A system keyboard change (`localectl`, `dpkg-reconfigure keyboard-configuration`) now reaches existing accounts at their next login, as long as the keyboard block in `~/.config/sway/config.d/50-local.conf` hasn't been edited. The login screen and new accounts follow it too, including `localectl` on Arch and Ubuntu 26.04. On Debian and Ubuntu, `sudo dpkg-reconfigure keyboard-configuration` is the change that survives a reboot.
- On Sway without an input method, the accents (`'` + `e`, `~` + `a`, `^` + `o`…) work again in GTK 4 apps such as Text Editor and GNOME Software: the session uses `GTK_IM_MODULE=simple` when no input method (ibus, fcitx) is set up. Anyone using an on-screen keyboard through the input method can put `export GTK_IM_MODULE=wayland` in `~/.profile`.
- `~/.local/bin` is in the session's PATH on every distro (Arch has no `~/.profile`, and Debian's only adds it if the folder already exists), so scripts placed there run from rofi, the dock and the terminal. The folder is created at login. The PATH is not passed to systemd or D-Bus, so the next GDM login still reads `~/.profile`.

### Install images

- In the installer of every image, with the **English (US, intl., with dead keys)** layout, `'` + `c` now gives **ç** (and `'` + `C` gives **Ç**), as on Windows. Before, it gave ć, from the keyboard page's test field onwards. Other dead keys are unchanged.
- Arch image: the installer no longer stops at once with "The source filesystem /run/archiso/bootmnt/arch/x86_64/airootfs.sfs does not exist" when the image was copied to RAM at boot, as on a USB stick in a PC with 4 GB or more free.
- Arch image: the installer no longer writes Debian's `/etc/default/keyboard`, so a keyboard changed later with `localectl` now reaches the login screen. Systems already installed keep that file, and the package update above handles them.
- Swap is zram, half the RAM with zstd, on all four images. The Arch image installed a system with no swap at all; on Debian and Ubuntu, zram was capped at 4 GiB with lz4 or lzo-rle. Ubuntu keeps its small swapfile as a fallback.
- The Debian 13 and Debian testing live sessions boot about a minute faster: they no longer rebuild the library cache on every boot (the desktop came up in 141 s instead of 196 s from a USB stick).

## 0.1.3 — 2026-11-21

The accent color now follows the wallpaper, the icon theme can be chosen, Swaydesk speaks Spanish, Debian testing gets packages and Arch gets its own update notice. The install images bring an app store.

### Added

- **Accent color from the wallpaper**, on by default: Swaydesk picks the Papirus color closest to the wallpaper's main color, and changes it with every new wallpaper, the daily one included. Desktop Theme has a **Use the wallpaper's color** switch, and picking a color yourself turns it off. A wallpaper without color gives the default accent. In `~/.config/sway-desktop/config.toml` it's `accent_auto`. On upgrade it turns on only if you still have the default accent, so a color you picked stays.
- **Choose the icon theme**: Desktop Theme has an **Icons** list, with a strip of each theme's icons. On the command line, `swaydesk-theme icons` lists them and `swaydesk-theme icons NAME` changes only the icons; `set` and `pick` take `--icons NAME`. With icons other than Papirus, the ones a theme lacks (Help, the dock, the popups) come from Papirus, Adwaita or Breeze, and the folders don't take the accent color. The preview card shows the chosen theme's folders.
- **Spanish**: the desktop, the app menus and the optional login screen, for any Spanish locale (es_ES, es_MX, es_AR…). The long date reads "viernes, 20 de noviembre".
- **Debian testing** (forky): packages in the `Debian_Testing` repository. Like Arch, it's a rolling release: Swaydesk is tested on it at every release, but updates to the distribution itself may need fixes afterwards. See the [install page](https://swaydesk.nielsonrolim.com/en-us/install/).
- **Update notice on Arch**: Swaydesk checks for updates in the background and shows their count in the bar, with a notification when there is something new. It covers the official repositories, the AUR (through paru or yay, if you already have one installed), Flatpak apps and firmware (fwupd). A click, or the notification's **Update** button, opens a terminal with the update; nothing is updated on its own. On Arch, GNOME Software's own update checks come turned off, since the notice covers them. To turn the notice off, set `update_notifier = false` in `[enabled]`; to hide only the icon, `updates = false` in `[enabled.bar]`. See [Updates on Arch](https://swaydesk.nielsonrolim.com/en-us/guide/config/#updates-on-arch).

### Changed

- `swaydesk-archive-keyring` 2026.11.20 adds the Debian testing repository. The keyring `.deb`'s sha256 changed on Debian 13 and Ubuntu 26.04 too; the install page has the new values. If you installed through the keyring `.deb`, apt updates it and there's nothing to do.
- When an app asks to restart or shut down, such as GNOME Software's **Restart & Update**, Swaydesk now asks first, with a 60-second countdown, **Restart now** and **Cancel**, as GNOME does. Before, it restarted at once. Cancelling installs nothing and the update stays downloaded.
- If you customized `/etc/pam.d/greetd` (a fingerprint reader, a security key), make that change in `/etc/pam.d/swaydesk-greeter`: the login screen's sessions now use that file.

### Fixed

- After logging out and back in quickly, Waybar could take up to 2 minutes to appear, and the file chooser and screenshots through the portal hung meanwhile. The previous session left the portal's start pending. Now the new login restarts the stuck portals, and the previous session's cleanup no longer erases the new session's environment.
- Shutting down with an update pending (GNOME Software 50 or later: Ubuntu 26.04, Debian testing) now restarts, installs the update and then powers off, as on GNOME. Before, it powered off directly and the update was installed only at the next boot. On Debian 13 (GNOME Software 48) it shuts down as before, and the update is installed at the next boot.
- Every Help card now has an icon with any icon theme; some were blank under Adwaita, Yaru, Papirus or Breeze. Desktop Theme has its own icon (`org.swaydesktop.Theme`), which shows with any icon theme.
- A theme or color change keeps the icon theme you chose, such as Yaru. Before, it went back to Papirus, and with the accent color following the wallpaper, that would have happened with every new wallpaper.
- In Desktop Theme, the default accent color (#199ca8) is no longer taken for the nearest swatch: changing only the style keeps it, and picking that swatch can be applied.
- If the wallpaper file is missing, a theme change no longer opens Sway's error bar ("unable to access background file"): the plain background comes back.
- After logging out of another desktop and into Swaydesk, apps no longer take that desktop's theme or light/dark style through the portal.
- `swaydesk-theme list`, `current`, `families`, `colors` and `icons` piped into a command that stops early, such as `head`, no longer print a Rust panic (broken pipe).
- With the packages, the theme's error messages (`swaydesk-theme` and the Desktop Theme app) point to `swaydesk-theme colors`, `families`, `list` and `icons`, and so does its usage line; they pointed to `sway-theme`, which with the packages isn't on the PATH.
- On Debian testing, the desktop opened from the login screen came up in English and without unlocking the keyring: testing's greetd changed its PAM file. `swaydesk-greeter` now brings its own PAM service, `/etc/pam.d/swaydesk-greeter`, on Debian and Ubuntu as already on Arch. Nothing changes on Debian 13 and Ubuntu 26.04.
- Arch: the gtk portal starts through a drop-in on its systemd unit, without dbus-broker's "Ignoring duplicate name" warning at every login. Debian and Ubuntu are unchanged.

### Install images

- **App store**: GNOME Software with Flathub, on the three images. On Debian and Ubuntu it also installs and updates the distro's packages and the firmware; on Arch it handles Flatpak apps, and the update notice handles the rest.
- **Spanish** in the installer, on its codecs page and in the notices after the install, and offline in the live session.
- **NVIDIA under Secure Boot on Debian**: with Secure Boot on, the codecs and drivers box now installs the NVIDIA driver too, signed with a key of the computer. After the first login a notice asks you to run `sudo swaydesk-extras`, which shows a one-time password of 8 digits, and at the next restart you enroll the key on MokManager's blue screen with that password. Before, the driver was left out until you turned Secure Boot off.
- On Debian and Ubuntu, the installer's keyboard test field now follows the layout you pick. In 0.1.2 that worked only on Arch.
- On Ubuntu, the screen shown while updates install at restart is in the installed system's language (Portuguese or Spanish; English otherwise).

### Known limitations

- Debian testing and Arch are rolling releases: Swaydesk is tested on them at every release, but updates to the distributions themselves may need fixes afterwards.
- In the installer, a Latin American Spanish locale (es_MX, es_AR…) preselects the Spain keyboard layout; pick **Spanish (Latin American)** by hand. The date and number formats follow the location detected from your network, not the language you pick.
- In Spanish, the keyring's password prompt on the first login is partly in English; that text comes from gnome-keyring.
- On Ubuntu, the progress line of the screen shown while updates install at restart ("55% complete") stays in English; it comes from PackageKit.

## 0.1.2 — 2026-11-19

A bug-fix release.

### Fixed

- On a minimal or server install of Ubuntu 26.04, `apt install swaydesk-desktop` no longer pulls in the whole GNOME desktop and gdm3. `swaydesk-desktop` now depends on a polkit agent (mate-polkit or lxpolkit), which the session needs anyway. It's installed even if you use another agent or turn it off with `polkit_agent = false` in `[enabled]`.
- On such a minimal or server install, if an earlier `apt install` already brought in GNOME and gdm3, upgrading doesn't remove them. To remove them and log in through the Swaydesk login screen (`swaydesk-greeter`), run this as one line from a text console (Ctrl+Alt+F3, then log in) or over SSH, not from a terminal in the graphical session, since removing gdm3 ends that session: `sudo apt purge gdm3 gnome-shell && sudo apt autoremove && sudo systemctl enable greetd`. Review the list apt shows before confirming each step, then reboot. greetd can only be enabled once gdm3 is gone, and until then no login screen starts.
- With a single keyboard layout, the layout indicator now stays hidden after a Sway reload, a theme change or a virtual keyboard. Before, it came back showing the layout, or as an empty gap.
- Record Screen and Screenshot have their own icons, which show with any icon theme. Record Screen was a plain dot in Papirus.
- The dock no longer shows a pinned app that isn't installed, since clicking it opened nothing. The pin stays in your list and comes back when the app is installed.
- If your dock still pins Nemo and Nemo isn't installed, the next login pins your file manager in its place.

### Install images

- The live session's dock pins the file manager (it pinned Nemo, which the images don't have), Desktop Theme, Displays and Help, then the installer.
- On the installer's keyboard page, the test field now types with the layout you pick.
- After `sudo swaydesk-extras` installs everything, the codecs notice closes by itself.

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
- No Debian testing packages yet. (Packages since 0.1.3.)
- Debian 13: on each user's first login, the keyring asks for your password once, after a notification that explains it. From the next login on, it unlocks by itself. gnome-keyring 48 creates the login keyring but doesn't publish it on D-Bus ([gnome-keyring#137](https://gitlab.gnome.org/GNOME/gnome-keyring/-/issues/137), fixed upstream in 51.0; Debian 13 ships 48), so Swaydesk restarts the daemon, and the unlock from the login is lost. Ubuntu 26.04 and Arch are not affected.
