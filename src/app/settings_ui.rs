use super::*;

pub(super) fn build_settings_editor(
    parent: &gtk::ApplicationWindow,
    initial_pass_through: bool,
    backend: crate::platform::DisplayBackend,
    draft: Rc<RefCell<livesplit_core::HotkeyConfig>>,
    sender: &ComponentSender<AppModel>,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .title("Settings")
        .transient_for(parent)
        .default_width(620)
        .default_height(620)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let overlay = adw::ToastOverlay::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let warning = adw::Banner::builder()
        .title("That hotkey is already assigned to another action")
        .revealed(false)
        .build();
    content.append(&warning);
    let page = adw::PreferencesPage::new();
    page.set_vexpand(true);
    let behavior = adw::PreferencesGroup::builder()
        .title("Timer Window")
        .build();
    let pass_through = adw::SwitchRow::builder()
        .title("Ignore mouse while running")
        .subtitle("Pass pointer input through the timer while it is running")
        .active(initial_pass_through)
        .build();
    behavior.add(&pass_through);
    let backend_row = adw::ActionRow::builder()
        .title("Display Backend")
        .subtitle(match backend {
            crate::platform::DisplayBackend::X11 => "X11 — absolute placement and always on top",
            crate::platform::DisplayBackend::WaylandStandard => {
                "Wayland — compositor controls placement and stacking"
            }
        })
        .build();
    behavior.add(&backend_row);
    page.add(&behavior);
    let group = adw::PreferencesGroup::builder()
        .title("Global Hotkeys")
        .description("Click a shortcut to capture a new key combination")
        .build();
    let description = draft
        .borrow()
        .settings_description(livesplit_core::Lang::English);
    for (index, field) in description.fields.iter().enumerate() {
        let current = match &field.value {
            livesplit_core::settings::Value::Hotkey(value) => *value,
            _ => continue,
        };
        let row = adw::ActionRow::builder()
            .title(field.text.as_ref())
            .subtitle(field.tooltip.as_ref())
            .build();
        let shortcut = gtk::Button::with_label(
            &current.map_or_else(|| "Not assigned".to_owned(), |hotkey| hotkey.to_string()),
        );
        shortcut.add_css_class("flat");
        shortcut.set_valign(gtk::Align::Center);
        let capture_parent = window.clone();
        let capture_draft = draft.clone();
        let capture_label = shortcut.clone();
        let capture_warning = warning.clone();
        shortcut.connect_clicked(move |_| {
            open_hotkey_capture(
                &capture_parent,
                index,
                capture_draft.clone(),
                capture_label.clone(),
                capture_warning.clone(),
            );
        });
        row.add_suffix(&shortcut);
        let clear = gtk::Button::from_icon_name("edit-clear-symbolic");
        clear.set_tooltip_text(Some("Clear hotkey"));
        clear.set_valign(gtk::Align::Center);
        let clear_draft = draft.clone();
        let clear_label = shortcut.clone();
        clear.connect_clicked(move |_| {
            let _ = clear_draft
                .borrow_mut()
                .set_value(index, livesplit_core::settings::Value::Hotkey(None));
            clear_label.set_label("Not assigned");
        });
        row.add_suffix(&clear);
        group.add(&row);
    }
    page.add(&group);
    content.append(&page);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    buttons.append(&spacer);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&apply);
    content.append(&buttons);
    overlay.set_child(Some(&content));
    toolbar.set_content(Some(&overlay));
    window.set_content(Some(&toolbar));

    let cancel_window = window.clone();
    cancel.connect_clicked(move |_| cancel_window.close());
    let apply_window = window.clone();
    let apply_draft = draft.clone();
    let apply_sender = sender.clone();
    apply.connect_clicked(move |_| {
        apply_sender.input(AppMsg::ApplyWindowSettings(pass_through.is_active()));
        apply_sender.input(AppMsg::ApplyHotkeys(*apply_draft.borrow()));
        apply_window.close();
    });
    let close_sender = sender.clone();
    window.connect_close_request(move |_| {
        close_sender.input(AppMsg::EditorFinished(EditorKind::Window));
        glib::Propagation::Proceed
    });
    window
}

fn open_hotkey_capture(
    parent: &adw::ApplicationWindow,
    index: usize,
    draft: Rc<RefCell<livesplit_core::HotkeyConfig>>,
    shortcut_label: gtk::Button,
    warning: adw::Banner,
) {
    let dialog = adw::ApplicationWindow::builder()
        .title("Capture Hotkey")
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .default_height(260)
        .build();
    let status = adw::StatusPage::builder()
        .title("Press a key combination")
        .description("Press Escape to cancel. Modifier-only keys are ignored.")
        .build();
    let clear = gtk::Button::with_label("Clear Shortcut");
    clear.set_halign(gtk::Align::Center);
    let clear_dialog = dialog.clone();
    let clear_draft = draft.clone();
    let clear_label = shortcut_label.clone();
    clear.connect_clicked(move |_| {
        let _ = clear_draft
            .borrow_mut()
            .set_value(index, livesplit_core::settings::Value::Hotkey(None));
        clear_label.set_label("Not assigned");
        clear_dialog.close();
    });
    status.set_child(Some(&clear));
    dialog.set_content(Some(&status));
    let keys = gtk::EventControllerKey::new();
    let key_dialog = dialog.clone();
    let conflict_status = status.clone();
    keys.connect_key_pressed(move |_, key, _keycode, state| {
        if key == gdk::Key::Escape {
            key_dialog.close();
            return glib::Propagation::Stop;
        }
        let Some(hotkey) = hotkey_from_gdk(key, state) else {
            return glib::Propagation::Stop;
        };
        let value = livesplit_core::settings::Value::Hotkey(Some(hotkey));
        if draft.borrow_mut().set_value(index, value).is_ok() {
            shortcut_label.set_label(&hotkey.to_string());
            warning.set_revealed(false);
            key_dialog.close();
        } else {
            warning.set_revealed(true);
            conflict_status.set_description(Some(
                "That shortcut is already assigned. Press a different combination.",
            ));
        }
        glib::Propagation::Stop
    });
    dialog.add_controller(keys);
    dialog.present();
}

fn hotkey_from_gdk(
    key: gdk::Key,
    state: gdk::ModifierType,
) -> Option<livesplit_core::hotkey::Hotkey> {
    use livesplit_core::hotkey::{KeyCode, Modifiers};
    let name = key.name()?;
    let name = name.as_str();
    if matches!(
        name,
        "Shift_L"
            | "Shift_R"
            | "Control_L"
            | "Control_R"
            | "Alt_L"
            | "Alt_R"
            | "Super_L"
            | "Super_R"
            | "Meta_L"
            | "Meta_R"
    ) {
        return None;
    }
    let code_name = match name {
        "Return" | "ISO_Enter" => "Enter".to_owned(),
        "space" => "Space".to_owned(),
        "BackSpace" => "Backspace".to_owned(),
        "Left" => "ArrowLeft".to_owned(),
        "Right" => "ArrowRight".to_owned(),
        "Up" => "ArrowUp".to_owned(),
        "Down" => "ArrowDown".to_owned(),
        "Page_Up" => "PageUp".to_owned(),
        "Page_Down" => "PageDown".to_owned(),
        "KP_Add" => "NumpadAdd".to_owned(),
        "KP_Subtract" => "NumpadSubtract".to_owned(),
        "KP_Multiply" => "NumpadMultiply".to_owned(),
        "KP_Divide" => "NumpadDivide".to_owned(),
        "KP_Decimal" => "NumpadDecimal".to_owned(),
        "KP_Enter" => "NumpadEnter".to_owned(),
        "grave" => "Backquote".to_owned(),
        "backslash" => "Backslash".to_owned(),
        "bracketleft" => "BracketLeft".to_owned(),
        "bracketright" => "BracketRight".to_owned(),
        "comma" => "Comma".to_owned(),
        "equal" => "Equal".to_owned(),
        "minus" => "Minus".to_owned(),
        "period" => "Period".to_owned(),
        "apostrophe" => "Quote".to_owned(),
        "semicolon" => "Semicolon".to_owned(),
        "slash" => "Slash".to_owned(),
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic() => {
            format!("Key{}", value.to_ascii_uppercase())
        }
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_digit() => {
            format!("Digit{value}")
        }
        value if value.starts_with("KP_") && value[3..].parse::<u8>().is_ok() => {
            format!("Numpad{}", &value[3..])
        }
        value => value.replace('_', ""),
    };
    let key_code = code_name.parse::<KeyCode>().ok()?;
    let mut modifiers = Modifiers::empty();
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        modifiers.insert(Modifiers::SHIFT);
    }
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        modifiers.insert(Modifiers::CONTROL);
    }
    if state.contains(gdk::ModifierType::ALT_MASK) {
        modifiers.insert(Modifiers::ALT);
    }
    if state.contains(gdk::ModifierType::META_MASK) || state.contains(gdk::ModifierType::SUPER_MASK)
    {
        modifiers.insert(Modifiers::META);
    }
    Some(key_code.with_modifiers(modifiers))
}
