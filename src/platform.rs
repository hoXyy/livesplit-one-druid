#![allow(dead_code)]

use gtk::{gdk, prelude::*};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, PropMode,
    },
    wrapper::ConnectionExt as _,
};

use crate::config::Config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayBackend {
    X11,
    WaylandStandard,
}

pub trait TimerWindowPlatform {
    fn configure(&self, window: &gtk::ApplicationWindow, config: &Config);
    fn begin_drag(&self, window: &gtk::ApplicationWindow, event: &gdk::Event);
    fn begin_resize(
        &self,
        window: &gtk::ApplicationWindow,
        event: &gdk::Event,
        edge: gdk::SurfaceEdge,
    );
    fn set_mouse_passthrough(&self, window: &gtk::ApplicationWindow, enabled: bool);
    fn persist_placement(&self, window: &gtk::ApplicationWindow, config: &mut Config);
}

#[derive(Clone, Copy)]
pub struct Platform {
    backend: DisplayBackend,
}

impl Platform {
    pub fn detect() -> Self {
        let name = gdk::Display::default()
            .map(|display| display.type_().name())
            .unwrap_or_default();
        let backend = choose_backend(name);
        Self { backend }
    }

    pub fn backend(self) -> DisplayBackend {
        self.backend
    }
}

fn choose_backend(display_name: &str) -> DisplayBackend {
    if display_name.contains("X11") {
        DisplayBackend::X11
    } else {
        DisplayBackend::WaylandStandard
    }
}

impl TimerWindowPlatform for Platform {
    fn configure(&self, window: &gtk::ApplicationWindow, config: &Config) {
        window.set_decorated(false);
        window.set_resizable(true);
        window.add_css_class("timer-window");
        if self.backend == DisplayBackend::X11 {
            let position = config.window_position();
            window.connect_realize(move |window| configure_x11(window, position));
        }
    }

    fn begin_drag(&self, window: &gtk::ApplicationWindow, event: &gdk::Event) {
        let (Some(surface), Some(device), Some((x, y))) =
            (window.surface(), event.device(), event.position())
        else {
            return;
        };
        if let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() {
            toplevel.begin_move(&device, 1, x, y, event.time());
        }
    }

    fn begin_resize(
        &self,
        window: &gtk::ApplicationWindow,
        event: &gdk::Event,
        edge: gdk::SurfaceEdge,
    ) {
        let (Some(surface), Some(device), Some((x, y))) =
            (window.surface(), event.device(), event.position())
        else {
            return;
        };
        if let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() {
            toplevel.begin_resize(edge, Some(&device), 1, x, y, event.time());
        }
    }

    fn set_mouse_passthrough(&self, window: &gtk::ApplicationWindow, enabled: bool) {
        let Some(surface) = window.surface() else {
            return;
        };
        if enabled {
            let empty = gtk::cairo::Region::create();
            surface.set_input_region(Some(&empty));
        } else {
            surface.set_input_region(None);
        }
    }

    fn persist_placement(&self, window: &gtk::ApplicationWindow, config: &mut Config) {
        if self.backend == DisplayBackend::X11 {
            let Some(surface) = window
                .surface()
                .and_then(|surface| surface.downcast::<gdk4_x11::X11Surface>().ok())
            else {
                return;
            };
            let Ok((connection, screen)) = x11rb::connect(None) else {
                return;
            };
            let root = connection.setup().roots[screen].root;
            if let Ok(cookie) = connection.translate_coordinates(surface.xid() as u32, root, 0, 0) {
                if let Ok(position) = cookie.reply() {
                    config.set_window_position((position.dst_x as f64, position.dst_y as f64));
                }
            };
        }
        // Standard Wayland deliberately does not overwrite x/y. X11 absolute
        // positioning is persisted by the X11 backend once its surface exists.
    }
}

fn configure_x11(window: &gtk::ApplicationWindow, position: Option<(f64, f64)>) {
    let Some(surface) = window
        .surface()
        .and_then(|surface| surface.downcast::<gdk4_x11::X11Surface>().ok())
    else {
        return;
    };
    let Ok((connection, screen)) = x11rb::connect(None) else {
        return;
    };
    let xid = surface.xid() as u32;
    let root = connection.setup().roots[screen].root;
    if let Some((x, y)) = position {
        let _ = connection.configure_window(
            xid,
            &ConfigureWindowAux::new()
                .x(x.round() as i32)
                .y(y.round() as i32),
        );
    }
    let Ok(wm_state_cookie) = connection.intern_atom(false, b"_NET_WM_STATE") else {
        return;
    };
    let Ok(wm_state) = wm_state_cookie.reply().map(|reply| reply.atom) else {
        return;
    };
    let Ok(above_cookie) = connection.intern_atom(false, b"_NET_WM_STATE_ABOVE") else {
        return;
    };
    let Ok(above) = above_cookie.reply().map(|reply| reply.atom) else {
        return;
    };
    let event = ClientMessageEvent::new(32, xid, wm_state, [1, above, 0, 1, 0]);
    let _ = connection.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    );
    let _ = connection.change_property32(
        PropMode::REPLACE,
        xid,
        wm_state,
        x11rb::protocol::xproto::AtomEnum::ATOM,
        &[above],
    );
    let _ = connection.flush();
}

#[cfg(test)]
mod tests {
    use super::{choose_backend, DisplayBackend};

    #[test]
    fn standard_wayland_is_a_distinct_placement_policy() {
        assert_ne!(DisplayBackend::WaylandStandard, DisplayBackend::X11);
    }

    #[test]
    fn backend_selection_uses_native_x11_or_wayland() {
        assert_eq!(choose_backend("GdkX11Display"), DisplayBackend::X11);
        assert_eq!(
            choose_backend("GdkWaylandDisplay"),
            DisplayBackend::WaylandStandard
        );
    }
}
