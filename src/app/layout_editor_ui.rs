use super::*;

pub(super) fn build_layout_editor(
    parent: &gtk::ApplicationWindow,
    editor: Rc<RefCell<Option<livesplit_core::LayoutEditor>>>,
    image_cache: Rc<RefCell<ImageCache>>,
    sender: &ComponentSender<AppModel>,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .title("Layout Editor")
        .transient_for(parent)
        .default_width(620)
        .default_height(440)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let components = gtk::ListBox::new();
    components.set_selection_mode(gtk::SelectionMode::Single);
    // Match the C# editor: one click selects a component, while a double click
    // activates it and opens its settings.
    components.set_activate_on_single_click(false);
    populate_component_list(&components, &editor);
    let select_editor = editor.clone();
    components.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            if let Some(editor) = select_editor.borrow_mut().as_mut() {
                editor.select(row.index() as usize);
            }
        }
    });
    let sidebar = gtk::ScrolledWindow::builder()
        .min_content_height(180)
        .hexpand(true)
        .child(&components)
        .build();
    let component_area = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    component_area.set_margin_start(12);
    component_area.set_margin_end(12);
    component_area.set_margin_top(12);
    component_area.set_margin_bottom(6);
    let component_buttons = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let mut operation_buttons = Vec::new();
    for (label, operation) in [
        ("Remove", 0_u8),
        ("Duplicate", 1),
        ("Move Up", 2),
        ("Move Down", 3),
    ] {
        let button = gtk::Button::with_label(label);
        let button_editor = editor.clone();
        let button_list = components.clone();
        button.connect_clicked(move |_| {
            if let Some(editor) = button_editor.borrow_mut().as_mut() {
                match operation {
                    0 => editor.remove_component(),
                    1 => editor.duplicate_component(),
                    2 => editor.move_component_up(),
                    _ => editor.move_component_down(),
                }
            }
            populate_component_list(&button_list, &button_editor);
        });
        component_buttons.append(&button);
        operation_buttons.push(button);
    }
    let add_kind = gtk::DropDown::from_strings(&[
        "Current Comparison",
        "Current Pace",
        "Delta",
        "Detailed Timer",
        "Graph",
        "PB Chance",
        "Possible Time Save",
        "Previous Segment",
        "Segment Time",
        "Separator",
        "Splits",
        "Sum of Best",
        "Text",
        "Timer",
        "Title",
        "Total Playtime",
        "Alternate Timing Method",
        "Blank Space",
        "World Record",
        "Group",
        "Carousel",
    ]);
    add_kind.set_selected(10);
    add_kind.set_tooltip_text(Some(
        "Groups lay out children across the current direction; carousels rotate through children",
    ));
    component_buttons.append(&add_kind);
    let add = gtk::Button::with_label("Add");
    let add_editor = editor.clone();
    let add_list = components.clone();
    add.connect_clicked(move |_| {
        if let Some(editor) = add_editor.borrow_mut().as_mut() {
            editor.add_component(new_layout_component(add_kind.selected()));
        }
        populate_component_list(&add_list, &add_editor);
    });
    component_buttons.append(&add);
    component_area.append(&component_buttons);
    component_area.append(&sidebar);
    component_area.set_vexpand(true);
    content.append(&component_area);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_margin_start(12);
    buttons.set_margin_end(12);
    buttons.set_margin_bottom(12);
    let layout_settings = gtk::Button::with_label("Layout Settings…");
    let component_settings = gtk::Button::with_label("Component Settings…");
    buttons.append(&layout_settings);
    buttons.append(&component_settings);
    let sensitivity_editor = editor.clone();
    let sensitivity_settings = component_settings.clone();
    components.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            sensitivity_settings.set_sensitive(false);
            for button in &operation_buttons {
                button.set_sensitive(false);
            }
            return;
        };
        let editor = sensitivity_editor.borrow();
        let Some(editor) = editor.as_ref() else {
            return;
        };
        let state = editor.state(&mut ImageCache::new(), livesplit_core::Lang::English);
        let index = row.index() as usize;
        sensitivity_settings.set_sensitive(!state.is_placeholder[index]);
        for (button, sensitive) in operation_buttons.iter().zip([
            state.buttons.can_remove,
            state.buttons.can_duplicate,
            state.buttons.can_move_up,
            state.buttons.can_move_down,
        ]) {
            button.set_sensitive(sensitive);
        }
    });
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    buttons.append(&spacer);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&apply);
    content.append(&buttons);
    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));

    let component_parent = window.clone();
    let component_editor = editor.clone();
    let component_cache = image_cache.clone();
    component_settings.connect_clicked(move |_| {
        open_layout_settings_window(&component_parent, &component_editor, &component_cache, true);
    });
    let layout_parent = window.clone();
    let layout_editor = editor.clone();
    let layout_cache = image_cache.clone();
    layout_settings.connect_clicked(move |_| {
        open_layout_settings_window(&layout_parent, &layout_editor, &layout_cache, false);
    });
    let double_parent = window.clone();
    let double_editor = editor.clone();
    let double_cache = image_cache.clone();
    components.connect_row_activated(move |_, row| {
        if let Some(editor) = double_editor.borrow_mut().as_mut() {
            editor.select(row.index() as usize);
        }
        let is_placeholder = double_editor
            .borrow()
            .as_ref()
            .map(|editor| {
                editor
                    .state(&mut ImageCache::new(), livesplit_core::Lang::English)
                    .is_placeholder[row.index() as usize]
            })
            .unwrap_or(true);
        if !is_placeholder {
            open_layout_settings_window(&double_parent, &double_editor, &double_cache, true);
        }
    });

    let cancel_window = window.clone();
    cancel.connect_clicked(move |_| cancel_window.close());
    let apply_window = window.clone();
    let apply_editor = editor.clone();
    let apply_sender = sender.clone();
    apply.connect_clicked(move |_| {
        let draft = apply_editor
            .borrow_mut()
            .take()
            .map(|editor| editor.close());
        if let Some(layout) = draft {
            apply_sender.input(AppMsg::ApplyLayout(LayoutDraft(Arc::new(Mutex::new(
                Some(layout),
            )))));
        }
        apply_window.close();
    });
    let close_sender = sender.clone();
    window.connect_close_request(move |_| {
        close_sender.input(AppMsg::EditorFinished(EditorKind::Layout));
        glib::Propagation::Proceed
    });
    window
}

fn new_layout_component(index: u32) -> livesplit_core::layout::Component {
    use livesplit_core::{component, layout::Component};

    match index {
        0 => component::current_comparison::Component::new().into(),
        1 => component::current_pace::Component::new().into(),
        2 => component::delta::Component::new().into(),
        3 => Component::DetailedTimer(Box::new(component::detailed_timer::Component::new())),
        4 => component::graph::Component::new().into(),
        5 => component::pb_chance::Component::new().into(),
        6 => component::possible_time_save::Component::new().into(),
        7 => component::previous_segment::Component::new().into(),
        8 => component::segment_time::Component::new().into(),
        9 => component::separator::Component::new().into(),
        10 => component::splits::Component::new(livesplit_core::Lang::English).into(),
        11 => component::sum_of_best::Component::new().into(),
        12 => component::text::Component::new().into(),
        13 => component::timer::Component::new().into(),
        14 => component::title::Component::new().into(),
        15 => component::total_playtime::Component::new().into(),
        16 => component::alternate_timing_method::Component::new().into(),
        17 => component::blank_space::Component::new().into(),
        18 => component::world_record::Component::new().into(),
        19 => component::group::Component::new().into(),
        20 => component::carousel::Component::new().into(),
        _ => Component::from(component::splits::Component::new(
            livesplit_core::Lang::English,
        )),
    }
}

fn populate_component_list(
    list: &gtk::ListBox,
    editor: &Rc<RefCell<Option<livesplit_core::LayoutEditor>>>,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut ImageCache::new(), livesplit_core::Lang::English);
    for (index, name) in state.components.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let row_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .margin_start((state.indent_levels[index] * 20) as i32)
            .build();
        if state.is_placeholder[index] {
            label.add_css_class("dim-label");
        }
        let drag_handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        drag_handle.set_tooltip_text(Some("Drag to reorder"));
        drag_handle.set_cursor_from_name(Some("grab"));
        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        drag_source.set_content(Some(&gdk::ContentProvider::for_value(
            &(index as i32).to_value(),
        )));
        drag_handle.set_visible(!state.is_placeholder[index]);
        drag_handle.add_controller(drag_source);
        row_content.append(&label);
        row_content.append(&drag_handle);
        row.set_child(Some(&row_content));
        row.set_activatable(!state.is_placeholder[index]);
        row.set_margin_start(6);
        row.set_margin_end(6);
        row.set_margin_top(3);
        row.set_margin_bottom(3);
        row.set_tooltip_text(Some(&format!(
            "Component {} — double-click to edit settings",
            index + 1
        )));
        let drop_target = gtk::DropTarget::new(i32::static_type(), gdk::DragAction::MOVE);
        let drop_editor = editor.clone();
        let drop_list = list.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            let Ok(source) = value.get::<i32>() else {
                return false;
            };
            if source < 0 || source as usize == index {
                return false;
            }
            if let Some(editor) = drop_editor.borrow_mut().as_mut() {
                editor.select(source as usize);
                editor.move_component(index);
            }
            let refresh_editor = drop_editor.clone();
            let refresh_list = drop_list.clone();
            glib::idle_add_local_once(move || {
                populate_component_list(&refresh_list, &refresh_editor);
            });
            true
        });
        row.add_controller(drop_target);
        list.append(&row);
    }
    if let Some(row) = list.row_at_index(state.selected_component as i32) {
        list.select_row(Some(&row));
    }
}

fn open_layout_settings_window(
    parent: &adw::ApplicationWindow,
    editor: &Rc<RefCell<Option<livesplit_core::LayoutEditor>>>,
    image_cache: &Rc<RefCell<ImageCache>>,
    component: bool,
) {
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut image_cache.borrow_mut(), livesplit_core::Lang::English);
    let (title, snapshot) = if component {
        (
            format!(
                "{} Settings",
                state.components[state.selected_component as usize]
            ),
            state
                .component_settings
                .fields
                .iter()
                .map(|field| field.value.clone())
                .collect::<Vec<_>>(),
        )
    } else {
        (
            "Layout Settings".to_owned(),
            state
                .general_settings
                .fields
                .iter()
                .map(|field| field.value.clone())
                .collect::<Vec<_>>(),
        )
    };
    let window = adw::ApplicationWindow::builder()
        .title(&title)
        .transient_for(parent)
        .modal(true)
        .default_width(620)
        .default_height(620)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder().title(&title).build();
    if component {
        populate_component_settings(
            &group,
            &Rc::new(RefCell::new(Vec::new())),
            editor,
            image_cache,
        );
    } else {
        populate_general_settings(&group, editor, image_cache, &window);
    }
    page.add(&group);
    content.append(&page);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_end(12);
    buttons.set_margin_bottom(12);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&apply);
    content.append(&buttons);
    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));

    let accepted = Rc::new(Cell::new(false));
    let cancel_window = window.clone();
    cancel.connect_clicked(move |_| cancel_window.close());
    let apply_window = window.clone();
    let apply_accepted = accepted.clone();
    apply.connect_clicked(move |_| {
        apply_accepted.set(true);
        apply_window.close();
    });
    let restore_editor = editor.clone();
    let restore_cache = image_cache.clone();
    window.connect_close_request(move |_| {
        if !accepted.get() {
            if let Some(editor) = restore_editor.borrow_mut().as_mut() {
                for (index, value) in snapshot.iter().cloned().enumerate() {
                    if component {
                        editor.set_component_settings_value(index, value);
                    } else {
                        editor.set_general_settings_value(index, value, &restore_cache.borrow());
                    }
                }
            }
        }
        glib::Propagation::Proceed
    });
    window.present();
}

fn populate_component_settings(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<adw::PreferencesRow>>>,
    editor: &Rc<RefCell<Option<livesplit_core::LayoutEditor>>>,
    image_cache: &Rc<RefCell<ImageCache>>,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut image_cache.borrow_mut(), livesplit_core::Lang::English);
    let mut new_rows = rows.borrow_mut();
    for (index, field) in state.component_settings.fields.iter().enumerate() {
        let row_editor = editor.clone();
        let row = crate::setting_rows::build_setting_row(
            field,
            Rc::new(move |value| {
                if let Some(editor) = row_editor.borrow_mut().as_mut() {
                    editor.set_component_settings_value(index, value);
                }
            }),
        );
        group.add(&row);
        new_rows.push(row);
    }
}

fn populate_general_settings(
    group: &adw::PreferencesGroup,
    editor: &Rc<RefCell<Option<livesplit_core::LayoutEditor>>>,
    image_cache: &Rc<RefCell<ImageCache>>,
    parent: &adw::ApplicationWindow,
) {
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut image_cache.borrow_mut(), livesplit_core::Lang::English);
    for (index, field) in state.general_settings.fields.iter().enumerate() {
        let row_editor = editor.clone();
        let row_cache = image_cache.clone();
        let row = crate::setting_rows::build_setting_row(
            field,
            Rc::new(move |value| {
                if let Some(editor) = row_editor.borrow_mut().as_mut() {
                    editor.set_general_settings_value(index, value, &row_cache.borrow());
                }
            }),
        );
        group.add(&row);

        if let livesplit_core::settings::Value::LayoutBackground(background) = field.value {
            let actions = adw::ActionRow::builder()
                .title("Background Image")
                .subtitle("Choose an image or switch back to a gradient")
                .build();
            let choose = gtk::Button::with_label("Choose…");
            let choose_parent = parent.clone();
            let choose_editor = editor.clone();
            let choose_cache = image_cache.clone();
            choose.connect_clicked(move |_| {
                let dialog = gtk::FileDialog::builder()
                    .title("Choose Background Image")
                    .build();
                let filters = gio::ListStore::new::<gtk::FileFilter>();
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("Images"));
                filter.add_mime_type("image/*");
                filters.append(&filter);
                dialog.set_filters(Some(&filters));
                let editor = choose_editor.clone();
                let cache = choose_cache.clone();
                dialog.open(
                    Some(&choose_parent),
                    gio::Cancellable::NONE,
                    move |result| {
                        let Some(path) = result.ok().and_then(|file| file.path()) else {
                            return;
                        };
                        let Ok(image) = livesplit_core::settings::Image::from_file(
                            path,
                            &mut Vec::new(),
                            livesplit_core::settings::Image::LARGE,
                        ) else {
                            return;
                        };
                        let id = *image.id();
                        cache.borrow_mut().cache(&id, || image);
                        let image = match background {
                            livesplit_core::settings::LayoutBackground::Image(image) => {
                                image.map(id)
                            }
                            livesplit_core::settings::LayoutBackground::Gradient(_) => {
                                livesplit_core::settings::BackgroundImage {
                                    image: id,
                                    brightness: 1.0,
                                    opacity: 1.0,
                                    blur: 0.0,
                                }
                            }
                        };
                        if let Some(editor) = editor.borrow_mut().as_mut() {
                            editor.set_general_settings_value(
                                index,
                                livesplit_core::settings::Value::LayoutBackground(
                                    livesplit_core::settings::LayoutBackground::Image(image),
                                ),
                                &cache.borrow(),
                            );
                        }
                    },
                );
            });
            actions.add_suffix(&choose);
            if matches!(
                background,
                livesplit_core::settings::LayoutBackground::Image(_)
            ) {
                let gradient = gtk::Button::with_label("Use Gradient");
                let gradient_editor = editor.clone();
                let gradient_cache = image_cache.clone();
                gradient.connect_clicked(move |_| {
                    if let Some(editor) = gradient_editor.borrow_mut().as_mut() {
                        editor.set_general_settings_value(
                            index,
                            livesplit_core::settings::Value::LayoutBackground(
                                livesplit_core::settings::LayoutBackground::Gradient(
                                    livesplit_core::settings::Gradient::default(),
                                ),
                            ),
                            &gradient_cache.borrow(),
                        );
                    }
                });
                actions.add_suffix(&gradient);
            }
            group.add(&actions);
        }
    }
}
