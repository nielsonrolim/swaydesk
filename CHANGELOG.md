# Changelog

Each release is published as packages and, shortly after, as install images. Updates arrive through `apt upgrade` or `pacman -Syu`.

## 0.1.5 — 2026-11-23

ç and the accents in Chromium and Electron apps through fcitx5, on by default in the install images, and a lock screen that no longer uses swaylock, so a password with an accent unlocks it.

### Added

- **ç and accents in Chromium and Electron apps (fcitx5)**: on Wayland, Chromium, Chrome, Vivaldi, Brave and Electron apps (VS Code, Slack, Discord) ignore `~/.XCompose` and compose with their own table, so `'` + `c` gave `ć`. With `input_method = true` in `[enabled]` of `~/.config/swaydesk/config.toml` and the `fcitx5` package installed, the session starts fcitx5. Browsers opened with `--enable-wayland-ime --wayland-text-input-version=3` then give `ç`.
  - The setting is **off in the packages and on in the Debian 13 and Arch install images**, which ship `/etc/swaydesk/config.toml`, a new system-wide layer for `[enabled]` defaults. A key in your own `config.toml` always wins.
  - fcitx5 stays out of the way. Its shortcuts are off (Super+Space still switches the layout in Sway), and so are its tray icon and notifications. It uses the session's keyboard layout, and GTK apps keep composing on their own.
  - On Arch, the flag files that browser launchers read (`chromium-flags.conf`, `code-flags.conf`…) are created in `~/.config`, only for what's installed and only if missing. A browser installed in the middle of a session gets its file at the next login.
  - Nothing happens when another input method (IBus, for example) is configured or running. Set `input_method = false` to turn it off at the next login.

### Changed

- **The lock screen no longer uses swaylock**, which has no dead keys: a password with an accent typed as `´` then a letter never unlocked it.
  - The lock tries Swaydesk's own lock first; on Debian 13 that is the GTK 3 version, now part of `swaydesk-session`. Its last resort is now **gtklock**, a dependency on every distribution.
  - If a lock stops without unlocking, another one takes over, so the desktop never shows in between.
  - If no lock can start, a critical notification says the screen is not locked.
- **The lock screen has its own login rules**, `/etc/pam.d/swaydesk-lock` (the same as the system login), so it no longer depends on swaylock's file.
- **The password field never goes through an input method**, even with fcitx5 or IBus running, so typed keys go straight into the field.
- **Wrong password**: the lock screen now shows everything the system says. On Arch, after three wrong passwords, that means "The account is locked due to 3 failed logins. (10 minutes left to unlock)", and the message stays on screen. The short wait after three misses now counts down ("Too many attempts: try again in N s").

### Fixed

- **Dragging a pinned icon in the dock** now follows the pointer: the icon used to run ahead of it, and a slow drag or a drop past either end of the dock stopped short of where you let go.

### Install images

- The Debian 13 and Arch images bring `fcitx5` and turn the input method on through `/etc/swaydesk/config.toml`, in the live session and on the installed system (see **Added**).

### Upgrading from 0.1.4

- After the upgrade, no Swaydesk package needs swaylock any more. It stays installed: sway only suggests it, so `apt autoremove` and `pacman -Qdt` leave it alone. If nothing else of yours uses it, remove it with `sudo apt remove swaylock` on Debian and Ubuntu, or `sudo pacman -Rns swaylock` on Arch.

## 0.1.4 — 2026-11-23

The accent color reaches GTK apps, apps in your autostart folder start with the session, desktops without a battery get the power profiles, and systems installed from the 0.1.3.x images boot again under Secure Boot after losing their boot entry.

### Added

- **Accent color in GTK apps**: the accent color, picked by you or taken from the wallpaper, now also colors GTK 3 and GTK 4 apps, through `~/.config/gtk-3.0/gtk.css` and `~/.config/gtk-4.0/gtk.css`. Every desktop reads those files, so Swaydesk writes them only when no other desktop session (GNOME, KDE Plasma…) is installed. A `gtk.css` you edited is left alone. In both cases, `swaydesk-theme set` prints a note, and so does Desktop Theme when you apply a theme, in the message at the top of the window.
- **adw-gtk3**: `swaydesk-desktop` now brings the `adw-gtk-theme` package (the [adw-gtk3](https://github.com/lassekongo83/adw-gtk3) theme, which follows the accent color), and new accounts use `adw-gtk3-dark` when the Mint-Y theme isn't installed. The adw-gtk3 family is in Desktop Theme. On Arch it comes from the `extra` repository. On Ubuntu with snapd-desktop-integration (Ubuntu Desktop), new accounts get `Adwaita-dark`, as in 0.1.3.1, instead of `adw-gtk3-dark`: snaps don't have adw-gtk3, and snapd asked at every login to install a third-party theme snap ("Some required themes are missing") and opened a password dialog when a snap app started. adw-gtk3 is still installed and selectable in Desktop Theme; choosing it brings snapd's prompt back. To bring Adwaita back after trying adw-gtk3, pick Adwaita (Dark) in Desktop Theme.
- **Power profiles on desktops**: on a computer without a battery, the System popup has a **Power profile** row with the profiles power-profiles-daemon offers. The battery popup, which has the same buttons, doesn't show up without a battery. Set `power_profiles_in_system` in `config.toml` to `"always"` to have the row on laptops too, or `"never"` to hide it; the default, `"auto"`, shows it only without a battery.
- **Window animations on SwayFX 0.6**: `animation_duration_ms` in `[effects]` (0 to 5000, default 0, off) sets the length of SwayFX's open and close animations. It needs SwayFX 0.6: on 0.5, any value above 0 is a config error that Sway's error bar shows, and the rest of the config still loads.
- **Language packs on Ubuntu**: on Ubuntu and Linux Mint installed from the packages, a one-time notice shows the command that installs GTK's translations for your language (`sudo apt install language-pack-gnome-<lang>`), so the file chooser and GTK's dialogs aren't left in English. It doesn't show for English, or when the pack is already installed.
- **Apps in the autostart folder**: apps in `~/.config/autostart` and `/etc/xdg/autostart` (Nextcloud, Dropbox, a `.desktop` file you put there) now start in the Swaydesk session, through systemd, and close at logout. Entries for what Swaydesk already runs, or that clash with it, are skipped in the Swaydesk session only: blueman, nm-applet, the polkit agents, xapp-sn-watcher, Ubuntu's update-notifier, Mint's touchegg (Sway already handles the 3- and 4-finger gestures), im-config's `im-launch`, and pulseaudio when PipeWire is the sound server. GNOME, KDE or Cinnamon on the same account still start them. To use IBus or fcitx5 in Swaydesk, start it from `~/.config/sway/config.d/50-local.conf` (`exec ibus-daemon -drx` or `exec fcitx5 -d`); fcitx5's own autostart entry still works. To turn it off, set `autostart = false` in `[enabled]` of `~/.config/swaydesk/config.toml` (a file from an earlier version doesn't have the key, which means on: add the line). To skip a single app, the usual rule applies: a copy in `~/.config/autostart` with `Hidden=true`. On Linux Mint, this means Mint's own update manager (mintupdate) now also starts in the Swaydesk session, and `update_notifier = false` doesn't turn it off: untick it in Cinnamon's Startup Applications (that turns it off in Cinnamon too; to hide it in Swaydesk only, put a copy of its entry in `~/.config/autostart` with `NotShowIn=sway;`).
- **Arch: the update window runs `paku maintain`**: the Arch news, then the full upgrade (official repositories, the AUR and Flatpak), then cleanup: orphan packages, unused Flatpaks, the package cache, `.pacnew` files, a check for a newer kernel and the services to restart. Every step that removes or changes something defaults to No. The login screen and your session are never restarted; when a reboot is needed, a line says so. Firmware updates stay a separate step after it.
- **The login screen's messages go to the system journal**: `sudo journalctl -t swaydesk-greeter` (or without sudo in the `adm` or `systemd-journal` group), on every distribution. Before, nothing it logged was kept anywhere, so a session that failed to start left no trace. Passwords are never logged, and a user name typed in **Other user…** is replaced by `<user>` in the logged errors.

### Changed

- **Window borders are 3 px instead of 2**, on tiled and floating windows. A window alone on its workspace still has none, and the focused one still stands out by its color. A `default_border` of your own in `~/.config/sway/config.d/50-local.conf` still wins.
- **The search and password fields look alike**: the lock screen's password field is no longer a pill and takes the style of the app grid's and the clipboard's search fields (8 px corners, a thin border, the accent color on focus), and the launcher's (rofi) search field gets the same border, always in the accent color since it always has the focus. In the app grid and the clipboard, the magnifier now sits as far from the text as in the launcher.
- A new account's `~/.config/sway/config.d/50-local.conf` now comes with its comments in the session's language (English, Portuguese or Spanish); it always came in Portuguese. An existing file is never touched.
- On Debian and Ubuntu, `swaydesk-desktop` also brings the new `swaydesk-efi` package, which keeps the firmware's fallback boot path working under Secure Boot on systems installed from a Swaydesk image (see **Fixed** and **Upgrading from 0.1.3.x**). On any other system, including one that only added the Swaydesk repository, it does nothing: it never touches the ESP, the boot entries or another system's files. To turn it off: `sudo mkdir -p /etc/swaydesk && echo off | sudo tee /etc/swaydesk/efi-fallback`. To see why it did or didn't act on a system, run `sudo SWAYDESK_EFI_VERBOSE=1 /usr/libexec/swaydesk/swaydesk-efi-fallback`.
- On Debian and Ubuntu, the new `swaydesk-dpkg-includes` package keeps Swaydesk's translations on systems whose dpkg drops them (see **Fixed**). It only adds a dpkg filter for Swaydesk's own translation files.
- **Arch: `swaydesk-desktop` depends on paku** (0.3.0 or later), the pacman and AUR wrapper Swaydesk uses for updates, from the Swaydesk repository, and on `archlinux-contrib` (for the services check). paku uses paru, yay or pikaur if one is installed (or the one named in `~/.config/paku/config`), and pacman alone otherwise. Swaydesk still never installs an AUR helper.
- **Your settings folders are now called `swaydesk`**: `~/.config/swaydesk`, `~/.local/share/swaydesk` and `~/.local/state/swaydesk` (they were `sway-desktop`), and the Waybar folder `~/.config/swaydesk-desktop/waybar` moves to `~/.config/swaydesk/waybar`. They move at the first login after the upgrade, never in the middle of a session, and the old names stay as links: the old paths keep working through them, with nothing to do. The cache starts over in `~/.cache/swaydesk`. See **Upgrading from 0.1.3.x**.
- The READMEs and the site now say it plainly: Swaydesk is an independent project built on top of [Sway](https://swaywm.org), not affiliated with, or endorsed by, the Sway project or its developers.

### Fixed

- **The dock under SwayFX 0.6** never showed up: SwayFX stops redrawing a panel that is almost entirely off screen, and the dock's slide-in started from there. It now keeps a small part on screen while it slides, and nothing is left on screen once it's hidden.
- **The app grid gone after login, or Super doing nothing**, when another user on the same computer had the app grid or the lock screen running: Swaydesk's scripts mistook that user's processes for yours. Every process lookup now sees only your own. The app grid also restarts by itself if it stops, like the dock already did.
- **Bluetooth asking for the administrator password at every login** on Debian and Ubuntu, for an account outside the `sudo` and `netdev` groups: blueman's KillSwitch plugin is now off in the Swaydesk session. Turning Bluetooth on in the popup now also lifts a software block (the airplane-mode key, `rfkill block`) without a password.
- **Desktop Theme lost outside changes**: with anything pending in the window, a change made outside it (`swaydesk-theme set` in a terminal, the automatic light/dark switch) was ignored, and **Apply** reverted it. Now the fields you didn't touch in the window follow the change, and only the ones you edited stay.
- **Swaydesk in English on minimized Ubuntu** (the server and cloud images), whose dpkg drops every package's translations: Swaydesk now keeps its own. The other packages' translations, the language packs' included, come back with `sudo unminimize`, and on such a system the language-pack notice shows `sudo unminimize` first.
- **No boot under Secure Boot after losing the boot entry**, on Debian 13, Debian testing and Ubuntu systems installed from the 0.1.3.x images: the firmware's fallback path (`EFI/BOOT/BOOTX64.EFI`) held GRUB, which Secure Boot rejects, so a firmware reset or the disk moved to another PC left the system unable to boot. The fallback path now holds the signed shim, with everything needed to boot and recreate the entry. Debian 13 and Debian testing systems get the fix with the upgrade; Ubuntu systems with the next GRUB or shim update (see **Upgrading from 0.1.3.x**).
- **The keyboard layout's name** in the corner of the login screen and on the lock screen is now in the system's language ("Português (Brasil)", "Portugués (Brasil)" instead of "Portuguese (Brazil)"), from xkeyboard-config's own translations. On Ubuntu and Mint these come with the language pack (`language-pack-pt-base`, `language-pack-es-base`); without it, the name stays in English.
- **The date on the login screen and the lock screen** follows the same language as the session's clock: the `locale` key in `~/.config/swaydesk/config.toml` (for the lock screen) or in `/etc/swaydesk-greeter/config.toml` (for the login screen), if set; otherwise `LC_ALL`, `LC_TIME`, then `LANG` (on the login screen, from `/etc/locale.conf` or `/etc/default/locale`). The login screen used to read only `LANG`. An `LC_TIME=C.UTF-8` you set on purpose still gives ISO dates. If the chosen language isn't generated on the system, the date now uses the language of the screen's texts instead of the C format. On Arch, after editing `/etc/locale.conf` by hand, run `sudo locale-gen`.
- Arch: the system's language, from `/etc/locale.conf`, is now enabled in `/etc/locale.gen` and generated when `swaydesk-desktop` is installed or upgraded. Before, a language that wasn't generated left the session in English.

### Install images

- **Images only for Debian 13 and Arch from 0.1.4 on**: Ubuntu 26.04 and Debian testing install through the packages, from the Swaydesk repository on a system you install yourself (Debian testing is best effort). Systems installed from the 0.1.3.x Ubuntu and Debian testing images keep getting Swaydesk's updates from the repository, the Secure Boot fix included (see **Upgrading from 0.1.3.x**).
- **"Swaydesk" in the boot menu**: on the Debian 13 image, the firmware's boot entry and the GRUB menu are named Swaydesk, and the boot files live in `EFI/swaydesk`, through every later GRUB update. Next to a plain Debian or Ubuntu on the same disk, each system keeps its own folder.
- **Secure Boot fallback**: on the Debian 13 image too, the firmware's fallback path holds the signed shim, so the installed system boots with Secure Boot even after it loses its boot entry, and the entry comes back by itself. Before, the fallback path held GRUB, which Secure Boot rejects.
- The Debian 13 image is about 8% smaller (xz compression, as the 0.1.3.x Debian testing image used); the live session takes about 4 seconds longer to start (42 s instead of 38 s from a USB stick).

### Upgrading from 0.1.3.x

- **Debian 13 and Debian testing installed from a Swaydesk image**: the Secure Boot fix arrives with `apt upgrade`, and there's nothing to do.
- **Ubuntu installed from a Swaydesk image**: the Secure Boot fix arrives with the next GRUB or shim update; to have it now, run `sudo apt install --reinstall grub-efi-amd64-signed shim-signed`. Everyday booting isn't affected either way: only a system that loses its boot entry needs the fix. That GRUB update moves the boot entry to `EFI/swaydesk`, still one "Swaydesk" entry, and the old `EFI/ubuntu` folder stays unused. You can remove it by hand, but only if no other Ubuntu or Linux Mint is installed on the same disk, since they use that same folder; `sudo efibootmgr -v` shows which entries point at it.
- **Arch, if you installed pacu**: it's now called paku, and pacu is gone from the Swaydesk repository. Run `sudo pacman -Syu paku` and answer `y` to remove pacu. `swaydesk-desktop` brings paku in any case.
- **GTK theme**: existing accounts keep the theme they had, such as `Adwaita-dark`. To have the accent color in GTK 3 apps, pick **adw-gtk3** in Desktop Theme.
- **Settings folders**: the session open while you upgrade keeps using the old folders; they move at the next login. If an old and a new folder both exist with an item of the same name, nothing moves: the old folder stays in use and the session log says so (`grep kept: ~/.local/state/sway/sway.log`); merge them by hand to switch. Going back to 0.1.3.x needs nothing, since it reads the old names through the links. The old `~/.cache/sway-desktop` can be deleted.
- **Ubuntu installed from the packages**: to have GTK's own texts (the file chooser, the dialogs) in your language, run the command from the login notice, `sudo apt install language-pack-gnome-<lang>`, such as `language-pack-gnome-pt`. On a minimized Ubuntu, run `sudo unminimize` first.

### Known limitations

- With adw-gtk3 on Debian 13 (GTK 4.18), plain GTK 4 apps that don't use libadwaita (such as `gtk4-widget-factory`) show adw-gtk3's light style even with a dark theme. libadwaita apps (most GNOME apps, such as Text Editor) and GTK 3 apps are dark. On Ubuntu 26.04 and Arch they all follow the dark style.
- With another desktop installed next to Swaydesk, the accent color doesn't reach GTK apps, since the files it needs would color that desktop too.
- On Debian 13, a new account's first login still asks for the keyring password once; that comes from a gnome-keyring 48 bug, fixed in Debian 14.
- With GNOME or KDE open on another VT at the same time as Swaydesk, they share one systemd user environment: while Swaydesk is open, that desktop's autostart also skips blueman and nm-applet, and an app it starts after your Swaydesk login closes when you log out of Swaydesk.

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

- **App store**: GNOME Software with Flathub, on the four images. On Debian and Ubuntu it also installs and updates the distro's packages and the firmware; on Arch it handles Flatpak apps, and the update notice handles the rest.
- **Debian testing image** (`swaydesk-0.1.3-debiantesting-amd64.iso`): a live and installer image on Debian testing (forky), built like the Debian 13 one: the same installer, the app store (GNOME Software with Flathub, Debian's packages and firmware updates), English, Portuguese and Spanish, and the NVIDIA driver under Secure Boot. The installed system follows testing (`testing`, `testing-updates`, `testing-security`), so its packages keep moving; for a stable system, use the Debian 13 image.
- **Spanish** in the installer, on its codecs page and in the notices after the install, and offline in the live session.
- **NVIDIA under Secure Boot on Debian**: with Secure Boot on, the codecs and drivers box now installs the NVIDIA driver too, signed with a key of the computer. After the first login a notice asks you to run `sudo swaydesk-extras`, which shows a one-time password of 8 digits, and at the next restart you enroll the key on MokManager's blue screen with that password. Before, the driver was left out until you turned Secure Boot off.
- On Debian and Ubuntu, the installer's keyboard test field now follows the layout you pick. In 0.1.2 that worked only on Arch.
- On Ubuntu, the screen shown while updates install at restart is in the installed system's language (Portuguese or Spanish; English otherwise).

### Known limitations

- Debian testing and Arch are rolling releases: Swaydesk is tested on them at every release, but updates to the distributions themselves may need fixes afterwards.
- The Debian testing image is a snapshot of its build day: after a new testing kernel, the NVIDIA driver (DKMS) can lag behind for a while. It has no HP printer drivers (`hplip`, not in testing right now); install them with apt when they return.
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
