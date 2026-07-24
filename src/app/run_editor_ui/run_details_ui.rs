use super::*;

pub(super) fn open_icon_file(
    button: &impl IsA<gtk::Widget>,
    selected: impl Fn(livesplit_core::settings::Image) + 'static,
) {
    let Some(window) = button.root().and_downcast::<gtk::Window>() else {
        return;
    };
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    filter.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Select Icon")
        .filters(&filters)
        .build();
    dialog.open(Some(&window), gio::Cancellable::NONE, move |result| {
        let Some(path) = result.ok().and_then(|file| file.path()) else {
            return;
        };
        let mut buffer = Vec::new();
        if let Ok(image) = livesplit_core::settings::Image::from_file(
            path,
            &mut buffer,
            livesplit_core::settings::Image::ICON,
        ) {
            selected(image);
        }
    });
}

pub(super) type IconChanged = Rc<dyn Fn(livesplit_core::settings::Image)>;
pub(super) type IconRemoved = Rc<dyn Fn()>;
pub(super) type IconActionAvailable = Rc<dyn Fn() -> bool>;

pub(super) fn build_icon_picker(
    data: &[u8],
    pixel_size: i32,
    size: i32,
    choose: IconChanged,
    remove: IconRemoved,
    apply_to_selected: Option<(IconChanged, IconActionAvailable)>,
) -> gtk::MenuButton {
    let preview = gtk::Image::new();
    preview.set_pixel_size(pixel_size);
    preview.set_size_request(size, size);
    set_icon_preview(&preview, data);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&preview));
    let indicator = gtk::Image::from_icon_name("pan-down-symbolic");
    indicator.set_halign(gtk::Align::End);
    indicator.set_valign(gtk::Align::End);
    indicator.set_margin_end(2);
    indicator.set_margin_bottom(2);
    indicator.add_css_class("dim-label");
    overlay.add_overlay(&indicator);

    let picker = gtk::MenuButton::new();
    picker.add_css_class("flat");
    picker.set_child(Some(&overlay));
    picker.set_tooltip_text(Some(
        "Click for icon options; double-click to choose an image",
    ));

    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    menu.set_margin_start(6);
    menu.set_margin_end(6);
    menu.set_margin_top(6);
    menu.set_margin_bottom(6);
    let remove_button = gtk::Button::with_label("Remove Icon");
    remove_button.add_css_class("flat");
    remove_button.set_sensitive(!data.is_empty());
    let choose_button = gtk::Button::with_label("Choose Image…");
    choose_button.add_css_class("flat");
    let choose_picker = picker.clone();
    let choose_preview = preview.clone();
    let choose_changed = choose.clone();
    let choose_remove = remove_button.clone();
    choose_button.connect_clicked(move |_| {
        choose_picker.popdown();
        let preview = choose_preview.clone();
        let changed = choose_changed.clone();
        let remove = choose_remove.clone();
        open_icon_file(&choose_picker, move |image| {
            set_icon_preview(&preview, image.data());
            remove.set_sensitive(true);
            changed(image);
        });
    });
    menu.append(&choose_button);

    if let Some((apply, available)) = apply_to_selected {
        let apply_button = gtk::Button::with_label("Choose for Selected Splits…");
        apply_button.add_css_class("flat");
        apply_button.set_visible(available());
        let apply_picker = picker.clone();
        apply_button.connect_clicked(move |_| {
            apply_picker.popdown();
            let apply = apply.clone();
            open_icon_file(&apply_picker, move |image| apply(image));
        });
        let visible_button = apply_button.clone();
        picker.connect_active_notify(move |picker| {
            if picker.is_active() {
                visible_button.set_visible(available());
            }
        });
        menu.append(&apply_button);
    }

    let remove_picker = picker.clone();
    let remove_preview = preview.clone();
    remove_button.connect_clicked(move |button| {
        remove_picker.popdown();
        remove();
        set_icon_preview(&remove_preview, &[]);
        button.set_sensitive(false);
    });
    menu.append(&remove_button);

    let popover = gtk::Popover::new();
    popover.set_child(Some(&menu));
    picker.set_popover(Some(&popover));

    let double_click = gtk::GestureClick::new();
    double_click.set_button(1);
    let double_picker = picker.clone();
    let double_preview = preview;
    let double_remove = remove_button;
    double_click.connect_released(move |gesture, presses, _, _| {
        if presses != 2 {
            return;
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
        double_picker.popdown();
        let preview = double_preview.clone();
        let changed = choose.clone();
        let remove = double_remove.clone();
        open_icon_file(&double_picker, move |image| {
            set_icon_preview(&preview, image.data());
            remove.set_sensitive(true);
            changed(image);
        });
    });
    overlay.add_controller(double_click);
    picker
}

pub(super) fn set_icon_preview(preview: &gtk::Image, data: &[u8]) {
    let texture = (!data.is_empty())
        .then(|| gdk::Texture::from_bytes(&glib::Bytes::from(data)))
        .and_then(Result::ok);
    if let Some(texture) = texture {
        preview.set_paintable(Some(&texture));
        preview.set_tooltip_text(Some("Current icon"));
    } else {
        preview.set_icon_name(Some("image-missing-symbolic"));
        preview.set_tooltip_text(Some("No icon"));
    }
}

pub(super) struct RunDetailsPage {
    pub(super) page: adw::PreferencesPage,
    pub(super) platform_entry: CompletionEntryRow,
    pub(super) region_entry: CompletionEntryRow,
    pub(super) src_variables: adw::PreferencesGroup,
    pub(super) empty_variables: adw::ActionRow,
    pub(super) dynamic_variable_rows: Rc<RefCell<Vec<gtk::Widget>>>,
}

pub(super) fn build_run_details_page(
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    state: &livesplit_core::run::editor::State,
) -> RunDetailsPage {
    let page = adw::PreferencesPage::new();
    let metadata = adw::PreferencesGroup::builder().title("Metadata").build();
    let platform_entry = CompletionEntryRow::new("Platform", &state.metadata.platform_name);
    let region_entry = CompletionEntryRow::new("Region", &state.metadata.region_name);
    for (row, field) in [(&platform_entry, 0_u8), (&region_entry, 1)] {
        let row_editor = editor.clone();
        row.connect_changed(move |row| {
            if let Some(editor) = row_editor.borrow_mut().as_mut() {
                match field {
                    0 => editor.set_platform_name(row.text().as_str()),
                    1 => editor.set_region_name(row.text().as_str()),
                    _ => unreachable!(),
                }
            }
        });
        metadata.add(&row.row);
    }
    let emulator = adw::SwitchRow::builder()
        .title("Emulator")
        .subtitle("This run was performed using an emulator")
        .active(state.metadata.uses_emulator)
        .build();
    let emulator_editor = editor.clone();
    emulator.connect_active_notify(move |row| {
        if let Some(editor) = emulator_editor.borrow_mut().as_mut() {
            editor.set_emulator_usage(row.is_active());
        }
    });
    metadata.add(&emulator);
    page.add(&metadata);

    let src_variables = adw::PreferencesGroup::builder()
        .title("speedrun.com Variables")
        .description("Category variables stored with this run")
        .build();
    let dynamic_variable_rows = Rc::new(RefCell::new(Vec::new()));
    for (name, value) in state.metadata.speedrun_com_variables() {
        let name = name.to_owned();
        let row = adw::EntryRow::builder().title(&name).text(value).build();
        let variable_editor = editor.clone();
        let name = name.clone();
        row.connect_changed(move |row| {
            if let Some(editor) = variable_editor.borrow_mut().as_mut() {
                if row.text().is_empty() {
                    editor.remove_speedrun_com_variable(&name);
                } else {
                    editor.set_speedrun_com_variable(name.as_str(), row.text().as_str());
                }
            }
        });
        src_variables.add(&row);
        dynamic_variable_rows.borrow_mut().push(row.upcast());
    }
    let empty_variables = adw::ActionRow::builder()
        .title("No category variables")
        .subtitle("Selecting a category suggestion can populate these values")
        .visible(state.metadata.speedrun_com_variables().next().is_none())
        .build();
    src_variables.add(&empty_variables);
    page.add(&src_variables);

    let custom_variables = adw::PreferencesGroup::builder()
        .title("Custom Variables")
        .build();
    for (name, variable) in state
        .metadata
        .custom_variables()
        .filter(|(_, variable)| variable.is_permanent)
    {
        let name = name.to_owned();
        let row = adw::EntryRow::builder()
            .title(&name)
            .text(&variable.value)
            .build();
        let variable_editor = editor.clone();
        let edit_name = name.clone();
        row.connect_changed(move |row| {
            if let Some(editor) = variable_editor.borrow_mut().as_mut() {
                editor.set_custom_variable(edit_name.as_str(), row.text().as_str());
            }
        });
        let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
        remove.set_tooltip_text(Some("Remove variable"));
        let remove_editor = editor.clone();
        let remove_name = name;
        let remove_row = row.clone();
        remove.connect_clicked(move |_| {
            if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                editor.remove_custom_variable(&remove_name);
            }
            remove_row.set_visible(false);
        });
        row.add_suffix(&remove);
        custom_variables.add(&row);
    }
    let add_variable = adw::EntryRow::builder()
        .title("Add Custom Variable")
        .show_apply_button(true)
        .build();
    let add_editor = editor.clone();
    add_variable.connect_apply(move |row| {
        let name = row.text();
        if !name.trim().is_empty() {
            if let Some(editor) = add_editor.borrow_mut().as_mut() {
                editor.add_custom_variable(name.as_str());
            }
            row.set_text("");
        }
    });
    custom_variables.add(&add_variable);
    page.add(&custom_variables);
    RunDetailsPage {
        page,
        platform_entry,
        region_entry,
        src_variables,
        empty_variables,
        dynamic_variable_rows,
    }
}

pub(super) fn populate_speedrun_com_variable_rows(
    group: &adw::PreferencesGroup,
    empty: &adw::ActionRow,
    dynamic_rows: &Rc<RefCell<Vec<gtk::Widget>>>,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    variables: &[crate::speedrun_com::Variable],
) {
    for row in dynamic_rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    empty.set_visible(variables.is_empty());
    for variable in variables {
        let suffix = if variable.is_subcategory {
            " (subcategory)"
        } else if variable.mandatory {
            " (required)"
        } else if variable.user_defined {
            " (custom)"
        } else {
            ""
        };
        let title = format!("{}{suffix}", variable.name);
        let current = editor
            .borrow()
            .as_ref()
            .and_then(|editor| {
                editor
                    .run()
                    .metadata()
                    .speedrun_com_variables()
                    .find(|(name, _)| *name == variable.name.as_str())
                    .map(|(_, value)| value.clone())
            })
            .or_else(|| variable.default.clone())
            .unwrap_or_default();
        let row = CompletionEntryRow::new(&title, &current);
        let labels = variable
            .values
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        row.set_items(&labels);
        let variable_editor = editor.clone();
        let name = variable.name.clone();
        row.connect_changed(move |entry| {
            if let Some(editor) = variable_editor.borrow_mut().as_mut() {
                if entry.text().is_empty() {
                    editor.remove_speedrun_com_variable(&name);
                } else {
                    editor.set_speedrun_com_variable(name.as_str(), entry.text().as_str());
                }
            }
        });
        group.add(&row.row);
        dynamic_rows.borrow_mut().push(row.row.upcast());
    }
}

pub(super) fn build_run_comparisons_page(
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    segments: &gtk::ListBox,
    column_groups: &Rc<Vec<gtk::SizeGroup>>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Custom Comparisons")
        .description(
            "Comparison times are edited in the segment table after selecting a timing method",
        )
        .build();
    let names = editor
        .borrow()
        .as_ref()
        .unwrap()
        .custom_comparisons()
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();
    for name in names {
        let row = adw::ActionRow::builder().title(&name).build();
        let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
        remove.set_tooltip_text(Some("Remove comparison"));
        let remove_editor = editor.clone();
        let remove_segments = segments.clone();
        let remove_groups = column_groups.clone();
        let remove_name = name.clone();
        let remove_row = row.clone();
        remove.connect_clicked(move |_| {
            if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                editor.remove_comparison(&remove_name);
            }
            remove_row.set_visible(false);
            populate_run_segments(&remove_segments, &remove_editor, &remove_groups);
        });
        row.add_suffix(&remove);
        group.add(&row);
    }
    let add = adw::EntryRow::builder()
        .title("Add Comparison")
        .show_apply_button(true)
        .build();
    let add_editor = editor.clone();
    let add_segments = segments.clone();
    let add_groups = column_groups.clone();
    add.connect_apply(move |row| {
        let name = row.text();
        let valid = !name.trim().is_empty()
            && add_editor
                .borrow_mut()
                .as_mut()
                .is_some_and(|editor| editor.add_comparison(name.as_str()).is_ok());
        if valid {
            row.remove_css_class("error");
            row.set_text("");
            populate_run_segments(&add_segments, &add_editor, &add_groups);
        } else {
            row.add_css_class("error");
        }
    });
    group.add(&add);
    page.add(&group);
    page
}

pub(super) fn run_tools_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Clear History"), Some("win.clear-run-history"));
    menu.append(Some("Clear Times"), Some("win.clear-run-times"));
    menu
}

pub(super) fn install_run_tool_actions(
    window: &adw::ApplicationWindow,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    segments: &gtk::ListBox,
    column_groups: &Rc<Vec<gtk::SizeGroup>>,
) {
    for (name, clear_times) in [("clear-run-history", false), ("clear-run-times", true)] {
        let action = gio::SimpleAction::new(name, None);
        let action_editor = editor.clone();
        let action_segments = segments.clone();
        let action_groups = column_groups.clone();
        action.connect_activate(move |_, _| {
            if let Some(editor) = action_editor.borrow_mut().as_mut() {
                if clear_times {
                    editor.clear_times();
                } else {
                    editor.clear_history();
                }
            }
            populate_run_segments(&action_segments, &action_editor, &action_groups);
        });
        window.add_action(&action);
    }
}
