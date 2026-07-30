mod layout;
mod splits;

pub use layout::confirm_unsaved_layout;
pub use splits::confirm_unsaved_splits;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingDocument {
    Splits,
    Layout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingChangeDecision {
    Save,
    Discard,
    Cancel,
}

fn confirm_unsaved_document(
    parent: &gtk::ApplicationWindow,
    message: &str,
    detail: &str,
    save_label: &str,
    decided: impl FnOnce(PendingChangeDecision) + 'static,
) {
    let dialog = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .modal(true)
        .build();
    dialog.set_buttons(&[save_label, "Discard Changes", "Cancel"]);
    dialog.set_cancel_button(2);
    dialog.set_default_button(0);
    dialog.choose(
        Some(parent),
        gio::Cancellable::NONE,
        move |response| match response.ok() {
            Some(0) => decided(PendingChangeDecision::Save),
            Some(1) => decided(PendingChangeDecision::Discard),
            _ => decided(PendingChangeDecision::Cancel),
        },
    );
}
