# LiveSplit One GTK

An **unofficial** Linux desktop version of LiveSplit One built with Relm4, GTK 4, libadwaita,
and [livesplit-core]. Based on [AlexKnaught's version of livesplit-one-druid](https://github.com/AlexKnauth/livesplit-one-druid).

Available as a package:
- AUR: [livesplit-one-gtk-git](https://aur.archlinux.org/packages/livesplit-one-gtk-git)

## Build requirements

- Rust stable
- GTK 4.14 or newer
- libadwaita 1.5 or newer
- X11 development libraries for the X11 placement backend

Build and test with:

```bash
cargo build --release --locked
cargo test --all-features
```

## Global hotkeys on Wayland

On Wayland, LiveSplit's global hotkeys use Linux evdev input devices so that
keyboard and controller shortcuts work while another application is focused.
If LiveSplit cannot access those devices, it starts normally with global
hotkeys disabled. Timer controls and shortcut configuration remain available.

> **Security warning:** The following change allows every program running as
> your account to read raw keyboard and controller input, potentially including
> passwords and other sensitive keystrokes. It does not grant access only to
> LiveSplit. Leave global hotkeys disabled if you do not understand or accept
> this risk.

After reading and accepting that warning, deliberately enter this command in a
terminal:

```bash
sudo usermod -aG input "$USER"
```

`sudo` runs the group-management command as an administrator, `usermod -aG`
adds an account to a supplementary group without replacing its existing
groups, `input` is the privileged raw-input group, and `"$USER"` selects the
current account.

Fully sign out of the graphical session and sign back in afterward, then
restart LiveSplit. Closing LiveSplit or opening a new terminal is not enough.
Verify that `input` appears as a separate group name with:

```bash
id -nG
```

To remove the permission, deliberately enter:

```bash
sudo gpasswd -d "$USER" input
```

Then fully sign out and back in again.

If the system has no `input` group, consult your distribution's input-device
permission documentation. Do not create a group solely for LiveSplit, run
LiveSplit as root, use `chmod 666 /dev/input/event*`, or install a
world-readable udev rule.

## Configuration location

Configuration is stored here:

```text
~/.local/share/livesplitone/config.yml
```

# Note about autosplitters

Autosplitters may require `CAP_SYS_PTRACE` to be able to read other processes's memory:

```bash
sudo setcap CAP_SYS_PTRACE=+eip /usr/bin/livesplit-one-gtk
```

# Official versions of LiveSplit One

The official versions of LiveSplit One are:
- [OBS Plugin](https://github.com/livesplit/obs-livesplit-one)
- [Web version](https://one.livesplit.org/)

[livesplit-core]: https://github.com/LiveSplit/livesplit-core
[one.livesplit.org]: https://one.livesplit.org/
