use super::*;

pub(super) fn select_file(
    window: &gtk::ApplicationWindow,
    save: bool,
    choice: FileChoice,
    sender: &ComponentSender<AppModel>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(match choice {
            FileChoice::OpenSplits => "Open Splits",
            FileChoice::SaveSplits => "Save Splits",
            FileChoice::OpenLayout => "Open Layout",
            FileChoice::SaveLayout => "Save Layout",
        })
        .build();
    let sender = sender.clone();
    let selected = move |result: Result<gio::File, glib::Error>| {
        let Some(path) = result.ok().and_then(|file| file.path()) else {
            return;
        };
        sender.input(match choice {
            FileChoice::OpenSplits => AppMsg::RequestAction(PendingAction::OpenSplits(path)),
            FileChoice::SaveSplits => AppMsg::SaveSplitsAs(path),
            FileChoice::OpenLayout => AppMsg::RequestAction(PendingAction::OpenLayout(path)),
            FileChoice::SaveLayout => AppMsg::SaveLayoutAs(path),
        });
    };
    if save {
        dialog.save(Some(window), gio::Cancellable::NONE, selected);
    } else {
        dialog.open(Some(window), gio::Cancellable::NONE, selected);
    }
}

pub(super) fn choose_save_path(
    window: &gtk::ApplicationWindow,
    title: &str,
    selected: impl FnOnce(PathBuf) + 'static,
) {
    let dialog = gtk::FileDialog::builder().title(title).build();
    dialog.save(Some(window), gio::Cancellable::NONE, move |result| {
        if let Some(path) = result.ok().and_then(|file| file.path()) {
            selected(path);
        }
    });
}
