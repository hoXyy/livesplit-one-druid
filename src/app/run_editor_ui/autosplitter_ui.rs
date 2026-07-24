use super::*;

#[cfg(feature = "auto-splitting")]
fn auto_splitter_summary(association: Option<&AutoSplitterAssociation>) -> String {
    match association {
        Some(AutoSplitterAssociation::Local { path }) => format!("Local — {}", path.display()),
        Some(AutoSplitterAssociation::Registry {
            game, cached_path, ..
        }) => format!("Registry — {game} ({})", cached_path.display()),
        None => "No auto splitter associated".to_owned(),
    }
}

#[cfg(feature = "auto-splitting")]
pub(super) fn build_run_auto_splitter_section(
    parent: &adw::ApplicationWindow,
    game: &str,
    pending: &Rc<RefCell<Option<AutoSplitterAssociation>>>,
    runtime: &Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
    timer: &SharedTimer,
    can_associate: bool,
) -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title("Auto Splitter")
        .subtitle(auto_splitter_summary(pending.borrow().as_ref()))
        .build();
    let settings = gtk::Button::with_label("Settings…");
    settings.set_sensitive(pending.borrow().is_some());
    let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
    remove.set_tooltip_text(Some("Remove auto splitter"));
    let registry = gtk::Button::with_label("Find in Registry");
    registry.set_sensitive(can_associate);
    let registry_game = game.to_owned();
    let registry_pending = pending.clone();
    let registry_runtime = runtime.clone();
    let registry_timer = timer.clone();
    let registry_row = row.clone();
    let registry_settings = settings.clone();
    registry.connect_clicked(move |button| {
        if registry_game.trim().is_empty() {
            registry_row.set_subtitle("Enter a game name before searching the registry");
            return;
        }
        button.set_sensitive(false);
        registry_row.set_subtitle("Checking the LiveSplit auto splitter registry…");
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let game = registry_game.clone();
        std::thread::spawn(move || {
            let result = crate::autosplitter_registry::load_cached_or_refresh()
                .and_then(|entries| {
                    crate::autosplitter_registry::matching(&entries, &game)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("No registry entry matches {game}"))
                })
                .and_then(|entry| {
                    if !entry.installable {
                        anyhow::bail!("The matching entry is {}", entry.compatibility.label());
                    }
                    let download = crate::autosplitter_registry::download_to_temporary(&entry)?;
                    Ok((entry, download))
                })
                .map_err(|error| error.to_string());
            let _ = result_sender.send(result);
        });
        let pending = registry_pending.clone();
        let runtime = registry_runtime.clone();
        let timer = registry_timer.clone();
        let status = registry_row.clone();
        let button = button.clone();
        let settings = registry_settings.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let Ok(result) = result_receiver.try_recv() else {
                return glib::ControlFlow::Continue;
            };
            button.set_sensitive(true);
            match result {
                Ok((entry, (temporary, destination, url))) => {
                    if timer.read().unwrap().current_phase()
                        != livesplit_core::TimerPhase::NotRunning
                    {
                        let _ = std::fs::remove_file(temporary);
                        status.set_subtitle("Reset the timer before changing the auto splitter");
                    } else if let Err(error) = runtime.load(temporary.clone(), timer.clone()) {
                        let _ = std::fs::remove_file(temporary);
                        status.set_subtitle(&format!("Auto splitter failed to load: {error}"));
                    } else if let Err(error) = std::fs::rename(&temporary, &destination) {
                        let _ = runtime.unload();
                        status.set_subtitle(&format!("Could not install auto splitter: {error}"));
                    } else {
                        let association = AutoSplitterAssociation::Registry {
                            game: entry.game,
                            last_url: url,
                            cached_path: destination,
                        };
                        status.set_subtitle(&auto_splitter_summary(Some(&association)));
                        *pending.borrow_mut() = Some(association);
                        settings.set_sensitive(true);
                    }
                }
                Err(error) => status.set_subtitle(&error),
            }
            glib::ControlFlow::Break
        });
    });
    row.add_suffix(&registry);

    let local = gtk::Button::with_label("Select Local…");
    local.set_sensitive(can_associate);
    let local_parent = parent.clone();
    let local_pending = pending.clone();
    let local_runtime = runtime.clone();
    let local_timer = timer.clone();
    let local_row = row.clone();
    let local_settings = settings.clone();
    local.connect_clicked(move |_| {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("WASM Auto Splitters"));
        filter.add_pattern("*.wasm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Select Local WASM Auto Splitter")
            .filters(&filters)
            .build();
        let pending = local_pending.clone();
        let runtime = local_runtime.clone();
        let timer = local_timer.clone();
        let status = local_row.clone();
        let settings = local_settings.clone();
        dialog.open(Some(&local_parent), gio::Cancellable::NONE, move |result| {
            let Some(path) = result.ok().and_then(|file| file.path()) else {
                return;
            };
            if timer.read().unwrap().current_phase() != livesplit_core::TimerPhase::NotRunning {
                status.set_subtitle("Reset the timer before changing the auto splitter");
                return;
            }
            match runtime.load(path.clone(), timer.clone()) {
                Ok(()) => {
                    let association = AutoSplitterAssociation::Local { path };
                    status.set_subtitle(&auto_splitter_summary(Some(&association)));
                    *pending.borrow_mut() = Some(association);
                    settings.set_sensitive(true);
                }
                Err(error) => {
                    status.set_subtitle(&format!("Auto splitter failed to load: {error}"));
                }
            }
        });
    });
    row.add_suffix(&local);

    let settings_parent = parent.clone();
    let settings_runtime = runtime.clone();
    settings.connect_clicked(move |_| {
        open_auto_splitter_settings(&settings_parent, &settings_runtime);
    });
    row.add_suffix(&settings);
    let remove_pending = pending.clone();
    let remove_runtime = runtime.clone();
    let remove_row = row.clone();
    let remove_settings = settings.clone();
    remove.connect_clicked(move |_| match remove_runtime.unload() {
        Ok(()) => {
            *remove_pending.borrow_mut() = None;
            remove_row.set_subtitle(&auto_splitter_summary(None));
            remove_settings.set_sensitive(false);
        }
        Err(error) => remove_row.set_subtitle(&format!("Could not unload: {error}")),
    });
    row.add_suffix(&remove);
    if !can_associate {
        row.set_subtitle("Save the splits before associating an auto splitter");
    }
    row.upcast()
}

#[cfg(feature = "auto-splitting")]
fn open_auto_splitter_settings(
    parent: &adw::ApplicationWindow,
    runtime: &Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
) {
    use livesplit_core::auto_splitting::settings::{Value, WidgetKind};

    let window = adw::ApplicationWindow::builder()
        .title("Auto Splitter Settings")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(520)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Auto Splitter")
        .description("Changes are part of the Splits Editor transaction")
        .build();
    let widgets = runtime.settings_widgets().unwrap_or_default();
    let settings_map = runtime.settings_map().unwrap_or_default();
    if widgets.is_empty() {
        let loading = adw::ActionRow::builder()
            .title("Loading settings…")
            .subtitle("Waiting for the auto splitter to initialize")
            .build();
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        loading.add_suffix(&spinner);
        group.add(&loading);
        let attempts = Rc::new(Cell::new(0_u8));
        let retry_runtime = runtime.clone();
        let retry_parent = parent.clone();
        let retry_window = window.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if retry_runtime
                .settings_widgets()
                .is_some_and(|widgets| !widgets.is_empty())
            {
                retry_window.close();
                open_auto_splitter_settings(&retry_parent, &retry_runtime);
                return glib::ControlFlow::Break;
            }
            let next = attempts.get() + 1;
            attempts.set(next);
            if next >= 20 {
                spinner.set_spinning(false);
                spinner.set_visible(false);
                loading.set_title("No configurable settings");
                loading.set_subtitle("The loaded auto splitter did not expose any settings");
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    for widget in widgets.iter() {
        match &widget.kind {
            WidgetKind::Title { .. } => {
                let title = adw::ActionRow::builder()
                    .title(widget.description.as_ref())
                    .build();
                title.add_css_class("property");
                group.add(&title);
            }
            WidgetKind::Bool { default_value } => {
                let value = settings_map
                    .get(widget.key.as_ref())
                    .and_then(|value| match value {
                        Value::Bool(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(*default_value);
                let row = adw::SwitchRow::builder()
                    .title(widget.description.as_ref())
                    .active(value)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let runtime = runtime.clone();
                row.connect_active_notify(move |row| {
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::Bool(row.is_active()));
                    runtime.set_settings_map(map);
                });
                group.add(&row);
            }
            WidgetKind::Choice {
                default_option_key,
                options,
            } => {
                let model = gtk::StringList::new(
                    &options
                        .iter()
                        .map(|option| option.description.as_ref())
                        .collect::<Vec<_>>(),
                );
                let current_key = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .unwrap_or(default_option_key);
                let selected = options
                    .iter()
                    .position(|option| option.key.as_ref() == current_key.as_ref())
                    .unwrap_or(0) as u32;
                let row = adw::ComboRow::builder()
                    .title(widget.description.as_ref())
                    .model(&model)
                    .selected(selected)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let options = options.clone();
                let runtime = runtime.clone();
                row.connect_selected_notify(move |row| {
                    let Some(option) = options.get(row.selected() as usize) else {
                        return;
                    };
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::String(option.key.clone()));
                    runtime.set_settings_map(map);
                });
                group.add(&row);
            }
            WidgetKind::FileSelect { filters } => {
                let current_path = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .map_or_else(String::new, ToString::to_string);
                let row = adw::ActionRow::builder()
                    .title(widget.description.as_ref())
                    .subtitle(current_path)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let choose = gtk::Button::with_label("Choose…");
                let key = widget.key.clone();
                let runtime = runtime.clone();
                let parent = window.clone();
                let status = row.clone();
                let filters = filters.clone();
                choose.connect_clicked(move |_| {
                    let dialog = gtk::FileDialog::builder().title("Select File").build();
                    let gtk_filters = gio::ListStore::new::<gtk::FileFilter>();
                    for filter in filters.iter() {
                        let gtk_filter = gtk::FileFilter::new();
                        match filter {
                            livesplit_core::auto_splitting::settings::FileFilter::Name {
                                description,
                                pattern,
                            } => {
                                gtk_filter.set_name(description.as_deref());
                                for pattern in pattern.split_ascii_whitespace() {
                                    gtk_filter.add_pattern(pattern);
                                }
                            }
                            livesplit_core::auto_splitting::settings::FileFilter::MimeType(
                                mime,
                            ) => {
                                gtk_filter.set_name(Some(mime));
                                gtk_filter.add_mime_type(mime);
                            }
                        }
                        gtk_filters.append(&gtk_filter);
                    }
                    if gtk_filters.n_items() != 0 {
                        dialog.set_filters(Some(&gtk_filters));
                    }
                    let key = key.clone();
                    let runtime = runtime.clone();
                    let status = status.clone();
                    dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
                        let Some(path) = result.ok().and_then(|file| file.path()) else {
                            return;
                        };
                        let Some(path_value) =
                            livesplit_core::auto_splitting::wasi_path::from_native(&path)
                        else {
                            return;
                        };
                        let mut map = runtime.settings_map().unwrap_or_default();
                        map.insert(key.clone(), Value::String(path_value.into()));
                        runtime.set_settings_map(map);
                        status.set_subtitle(&path.to_string_lossy());
                    });
                });
                row.add_suffix(&choose);
                group.add(&row);
            }
        }
    }
    page.add(&group);
    toolbar.set_content(Some(&page));
    window.set_content(Some(&toolbar));
    window.present();
}
