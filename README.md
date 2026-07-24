# LiveSplit One GTK

A Linux desktop version of LiveSplit One built with Relm4, GTK 4, libadwaita,
and [livesplit-core].

## Requirements

- Rust stable
- GTK 4.16 or newer
- libadwaita 1.5 or newer
- X11 development libraries for the X11 placement backend

Build and test with:

```bash
cargo build --release --locked
cargo test --all-features
```

Linux is the only supported platform. On X11, the timer restores absolute
placement and requests always-on-top behavior. Wayland uses an ordinary
transparent toplevel: movement and resizing are handled by the compositor,
exact placement and always-on-top are unavailable, and stored placement is
preserved rather than overwritten.

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

The web version is available at [one.livesplit.org].

[livesplit-core]: https://github.com/LiveSplit/livesplit-core
[one.livesplit.org]: https://one.livesplit.org/
