use super::*;

#[derive(Clone)]
#[allow(deprecated)]
pub(super) struct CompletionEntryRow {
    pub(super) row: adw::ActionRow,
    pub(super) entry: gtk::Entry,
    combo: gtk::ComboBoxText,
}

#[allow(deprecated)]
impl CompletionEntryRow {
    pub(super) fn new(title: &str, text: &str) -> Self {
        let row = adw::ActionRow::builder().title(title).build();
        let combo = gtk::ComboBoxText::with_entry();
        combo.set_hexpand(true);
        combo.set_valign(gtk::Align::Center);
        combo.set_size_request(320, -1);
        combo.add_css_class("completion-combo");
        let entry = combo
            .child()
            .and_then(|child| child.downcast::<gtk::Entry>().ok())
            .expect("editable combo box contains a GTK entry");
        entry.set_text(text);
        entry.set_width_chars(28);
        entry.set_valign(gtk::Align::Center);
        row.add_suffix(&combo);
        row.set_activatable_widget(Some(&entry));
        Self { row, entry, combo }
    }

    pub(super) fn connect_changed(&self, callback: impl Fn(&gtk::Entry) + 'static) {
        self.entry.connect_changed(callback);
    }

    pub(super) fn set_text(&self, text: &str) {
        self.entry.set_text(text);
    }

    pub(super) fn text(&self) -> glib::GString {
        self.entry.text()
    }

    pub(super) fn add_suffix(&self, widget: &impl IsA<gtk::Widget>) {
        self.row.add_suffix(widget);
    }

    pub(super) fn set_tooltip_text(&self, text: Option<&str>) {
        self.entry.set_tooltip_text(text);
    }

    pub(super) fn clear_items(&self) {
        self.combo.remove_all();
    }

    pub(super) fn set_items(&self, items: &[&str]) {
        self.combo.remove_all();
        for item in items {
            self.combo.append_text(item);
        }
    }

    pub(super) fn connect_selected(&self, callback: impl Fn(usize) + 'static) {
        self.combo.connect_changed(move |combo| {
            let index = combo.property::<i32>("active");
            if index >= 0 {
                callback(index as usize);
            }
        });
    }

    pub(super) fn select(&self, index: usize) {
        self.combo.set_property("active", index as i32);
    }

    pub(super) fn show_dropdown(&self) {
        let active_window = self
            .entry
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
            .is_some_and(|window| window.is_active());
        if active_window
            && self.entry.has_focus()
            && self
                .combo
                .model()
                .is_some_and(|model| model.iter_n_children(None) > 0)
        {
            self.combo.popup();
        }
    }
}
