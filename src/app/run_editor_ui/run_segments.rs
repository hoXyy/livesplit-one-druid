use super::*;

pub(super) fn populate_run_segments(
    list: &gtk::ListBox,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    column_groups: &Rc<Vec<gtk::SizeGroup>>,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut ImageCache::new(), livesplit_core::Lang::English);
    for (index, segment) in state.segments.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let grid = gtk::Grid::builder()
            .column_spacing(8)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        let icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let drag_handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        drag_handle.set_tooltip_text(Some("Drag to reorder segment"));
        drag_handle.set_cursor_from_name(Some("grab"));
        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        drag_source.set_content(Some(&gdk::ContentProvider::for_value(
            &(index as i32).to_value(),
        )));
        drag_handle.add_controller(drag_source);
        icon_box.append(&drag_handle);
        let icon_preview = gtk::Image::new();
        icon_preview.set_pixel_size(32);
        icon_preview.set_size_request(36, 36);
        if let Some(editor) = editor.borrow().as_ref() {
            set_icon_preview(&icon_preview, editor.run().segment(index).icon().data());
        }
        icon_box.append(&icon_preview);
        let choose_icon = gtk::Button::from_icon_name("image-x-generic-symbolic");
        choose_icon.set_tooltip_text(Some("Choose segment icon"));
        let choose_editor = editor.clone();
        let selected_preview = icon_preview.clone();
        choose_icon.connect_clicked(move |button| {
            let icon_editor = choose_editor.clone();
            let preview = selected_preview.clone();
            open_icon_file(button, move |image| {
                set_icon_preview(&preview, image.data());
                if let Some(editor) = icon_editor.borrow_mut().as_mut() {
                    editor.select_only(index);
                    editor.active_segment().set_icon(image);
                }
            });
        });
        icon_box.append(&choose_icon);
        let remove_icon = gtk::Button::from_icon_name("edit-delete-symbolic");
        remove_icon.set_tooltip_text(Some("Remove segment icon"));
        let remove_editor = editor.clone();
        let removed_preview = icon_preview.clone();
        remove_icon.connect_clicked(move |_| {
            if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                editor.select_only(index);
                editor.active_segment().remove_icon();
            }
            set_icon_preview(&removed_preview, &[]);
        });
        icon_box.append(&remove_icon);
        if let Some(group) = column_groups.first() {
            group.add_widget(&icon_box);
        }
        grid.attach(&icon_box, 0, 0, 1, 1);
        for (column, (text, kind)) in [
            (segment.name.as_str(), 0_u8),
            (segment.split_time.as_str(), 1),
            (segment.segment_time.as_str(), 2),
            (segment.best_segment_time.as_str(), 3),
        ]
        .into_iter()
        .enumerate()
        {
            let entry = gtk::Entry::builder()
                .text(text)
                .hexpand(true)
                .width_chars(if column == 0 { 24 } else { 14 })
                .build();
            entry.set_tooltip_text(Some(match kind {
                0 => "Segment name",
                1 => "Split time",
                2 => "Segment time",
                _ => "Best segment time",
            }));
            let entry_editor = editor.clone();
            entry.connect_changed(move |entry| {
                let valid = if let Some(editor) = entry_editor.borrow_mut().as_mut() {
                    editor.select_only(index);
                    let text = entry.text();
                    match kind {
                        0 => {
                            editor.active_segment().set_name(text.as_str());
                            true
                        }
                        1 => editor
                            .active_segment()
                            .parse_and_set_split_time(text.as_str(), livesplit_core::Lang::English)
                            .is_ok(),
                        2 => editor
                            .active_segment()
                            .parse_and_set_segment_time(
                                text.as_str(),
                                livesplit_core::Lang::English,
                            )
                            .is_ok(),
                        _ => editor
                            .active_segment()
                            .parse_and_set_best_segment_time(
                                text.as_str(),
                                livesplit_core::Lang::English,
                            )
                            .is_ok(),
                    }
                } else {
                    false
                };
                if valid {
                    entry.remove_css_class("error");
                } else {
                    entry.add_css_class("error");
                }
            });
            if let Some(group) = column_groups.get(column + 1) {
                group.add_widget(&entry);
            }
            grid.attach(&entry, column as i32 + 1, 0, 1, 1);
        }
        for (comparison_index, comparison) in state.comparison_names.iter().enumerate() {
            let entry = gtk::Entry::builder()
                .text(&segment.comparison_times[comparison_index])
                .hexpand(true)
                .width_chars(14)
                .build();
            entry.set_tooltip_text(Some(comparison));
            let entry_editor = editor.clone();
            let comparison = comparison.clone();
            entry.connect_changed(move |entry| {
                let valid = entry_editor.borrow_mut().as_mut().is_some_and(|editor| {
                    editor.select_only(index);
                    editor
                        .active_segment()
                        .parse_and_set_comparison_time(
                            &comparison,
                            entry.text().as_str(),
                            livesplit_core::Lang::English,
                        )
                        .is_ok()
                });
                if valid {
                    entry.remove_css_class("error");
                } else {
                    entry.add_css_class("error");
                }
            });
            if let Some(group) = column_groups.get(comparison_index + 5) {
                group.add_widget(&entry);
            }
            grid.attach(&entry, comparison_index as i32 + 5, 0, 1, 1);
        }
        row.set_child(Some(&grid));
        let drop_target = gtk::DropTarget::new(i32::static_type(), gdk::DragAction::MOVE);
        let drop_editor = editor.clone();
        let drop_list = list.clone();
        let drop_groups = column_groups.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            let Ok(source) = value.get::<i32>() else {
                return false;
            };
            let source = source as usize;
            if source == index {
                return false;
            }
            if let Some(editor) = drop_editor.borrow_mut().as_mut() {
                editor.select_only(source);
                if source < index {
                    for _ in source..index {
                        editor.move_segments_down();
                    }
                } else {
                    for _ in index..source {
                        editor.move_segments_up();
                    }
                }
            }
            let refresh_editor = drop_editor.clone();
            let refresh_list = drop_list.clone();
            let refresh_groups = drop_groups.clone();
            glib::idle_add_local_once(move || {
                populate_run_segments(&refresh_list, &refresh_editor, &refresh_groups);
            });
            true
        });
        row.add_controller(drop_target);
        list.append(&row);
        if segment.selected.is_selected_or_active() {
            list.select_row(Some(&row));
        }
    }
}
