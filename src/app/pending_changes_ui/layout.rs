use super::{confirm_unsaved_document, PendingChangeDecision};

pub fn confirm_unsaved_layout(
    parent: &gtk::ApplicationWindow,
    decided: impl FnOnce(PendingChangeDecision) + 'static,
) {
    confirm_unsaved_document(
        parent,
        "Save changes to your layout?",
        "Your layout has unsaved changes.",
        "Save Layout",
        decided,
    );
}
