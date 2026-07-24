use super::*;

fn active_segment_index(editor: &livesplit_core::RunEditor) -> usize {
    editor
        .state(&mut ImageCache::new(), livesplit_core::Lang::English)
        .rows
        .iter()
        .find_map(|row| match row {
            livesplit_core::run::editor::RowState::Segment(segment)
                if segment.selected == livesplit_core::run::editor::SelectionState::Active =>
            {
                Some(segment.segment_index)
            }
            _ => None,
        })
        .unwrap_or(0)
}

fn selected_segment_indices(editor: &livesplit_core::RunEditor) -> Vec<usize> {
    editor
        .state(&mut ImageCache::new(), livesplit_core::Lang::English)
        .rows
        .iter()
        .filter_map(|row| match row {
            livesplit_core::run::editor::RowState::Segment(segment)
                if segment.selected.is_selected_or_active() =>
            {
                Some(segment.segment_index)
            }
            _ => None,
        })
        .collect()
}

fn restore_segment_selection(editor: &mut livesplit_core::RunEditor, selected: &[usize]) {
    if let Some((&first, rest)) = selected.split_first() {
        editor.select_only(first);
        for &index in rest {
            editor.select_additionally(index);
        }
    }
}

fn set_segment_icons(
    editor: &mut livesplit_core::RunEditor,
    indices: &[usize],
    image: &livesplit_core::settings::Image,
) {
    let selected = selected_segment_indices(editor);
    for &index in indices {
        editor.select_only(index);
        editor.active_segment().set_icon(image.clone());
    }
    restore_segment_selection(editor, &selected);
}

fn remove_segment_icon(editor: &mut livesplit_core::RunEditor, index: usize) {
    let selected = selected_segment_indices(editor);
    editor.select_only(index);
    editor.active_segment().remove_icon();
    restore_segment_selection(editor, &selected);
}

fn move_selected_segment_to(editor: &mut livesplit_core::RunEditor, target: usize) {
    let limit = editor.run().len().saturating_mul(2);
    for _ in 0..limit {
        let current = active_segment_index(editor);
        if current == target {
            break;
        }
        if current < target {
            editor.move_segments_down();
        } else {
            editor.move_segments_up();
        }
    }
}

fn move_segment_into_group(
    editor: &mut livesplit_core::RunEditor,
    source: usize,
    group_index: usize,
) {
    let Some(group) = editor.run().segment_groups().groups().get(group_index) else {
        return;
    };
    let (start, end) = (group.start(), group.end());
    if (start..end).contains(&source) {
        return;
    }

    editor.select_only(source);
    let steps = if source < start {
        start - source
    } else {
        source - end + 1
    };
    for _ in 0..steps {
        if source < start {
            editor.move_segments_down();
        } else {
            editor.move_segments_up();
        }
    }
}

fn insert_segment_into_group(editor: &mut livesplit_core::RunEditor, group_index: usize) {
    let Some(group) = editor.run().segment_groups().groups().get(group_index) else {
        return;
    };
    let start = group.start();
    let end = group.end();
    let name = group.name().map(str::to_owned);
    let icon = group.icon().cloned();

    let _ = editor.select_segment_group(group_index);
    let _ = editor.remove_selected_segment_groups();
    editor.select_only(end - 1);
    editor.insert_segment_above();
    editor.select_only(start);
    editor.select_range(end);
    if editor
        .create_segment_group_from_selection(name.as_deref())
        .is_err()
    {
        return;
    }

    if let Some(icon) = icon {
        let group_index = editor
            .run()
            .segment_groups()
            .groups()
            .iter()
            .position(|group| group.start() == start && group.end() == end + 1);
        if let Some(group_index) = group_index {
            let _ = editor.set_segment_group_icon(group_index, icon);
        }
    }
}

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
    for state_row in &state.rows {
        let livesplit_core::run::editor::RowState::Segment(segment) = state_row else {
            let livesplit_core::run::editor::RowState::SegmentGroup(group) = state_row else {
                unreachable!();
            };
            let row = segment_group_row(group, editor, list, column_groups);
            list.append(&row);
            if group.selected {
                list.select_row(Some(&row));
            }
            continue;
        };
        let index = segment.segment_index;
        let row = gtk::ListBoxRow::new();
        if segment.is_indented {
            row.add_css_class("group-member");
        }
        let grid = gtk::Grid::builder()
            .column_spacing(8)
            .margin_start(if segment.is_indented { 32 } else { 8 })
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        let icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let drag_handle = gtk::Image::from_icon_name("open-menu-symbolic");
        drag_handle.add_css_class("drag-handle");
        drag_handle.set_tooltip_text(Some("Drag to reorder segment"));
        drag_handle.set_cursor_from_name(Some("grab"));
        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        drag_source.set_content(Some(&gdk::ContentProvider::for_value(
            &(index as i32).to_value(),
        )));
        drag_handle.add_controller(drag_source);
        icon_box.append(&drag_handle);
        let icon_data = editor
            .borrow()
            .as_ref()
            .map(|editor| editor.run().segment(index).icon().data().to_vec())
            .unwrap_or_default();
        let choose_editor = editor.clone();
        let remove_editor = editor.clone();
        let selected_editor = editor.clone();
        let available_editor = editor.clone();
        let icon_picker = build_icon_picker(
            &icon_data,
            32,
            36,
            Rc::new(move |image| {
                if let Some(editor) = choose_editor.borrow_mut().as_mut() {
                    set_segment_icons(editor, &[index], &image);
                }
            }),
            Rc::new(move || {
                if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                    remove_segment_icon(editor, index);
                }
            }),
            Some((
                Rc::new(move |image| {
                    if let Some(editor) = selected_editor.borrow_mut().as_mut() {
                        let selected = selected_segment_indices(editor);
                        set_segment_icons(editor, &selected, &image);
                    }
                }),
                Rc::new(move || {
                    available_editor
                        .borrow()
                        .as_ref()
                        .is_some_and(|editor| selected_segment_indices(editor).len() > 1)
                }),
            )),
        );
        icon_box.append(&icon_picker);
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
                move_selected_segment_to(editor, index);
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

fn segment_group_row(
    group: &livesplit_core::run::editor::SegmentGroupState,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    list: &gtk::ListBox,
    column_groups: &Rc<Vec<gtk::SizeGroup>>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("accent");
    row.add_css_class("group-header");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(6);
    content.set_margin_bottom(6);

    let collapse = gtk::ToggleButton::new();
    collapse.set_icon_name("pan-down-symbolic");
    collapse.add_css_class("flat");
    collapse.set_tooltip_text(Some("Collapse or expand this group"));
    let collapse_row = row.clone();
    collapse.connect_toggled(move |button| {
        button.set_icon_name(if button.is_active() {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        });
        let mut sibling = collapse_row.next_sibling();
        while let Some(widget) = sibling {
            let next = widget.next_sibling();
            if !widget.has_css_class("group-member") {
                break;
            }
            widget.set_visible(!button.is_active());
            sibling = next;
        }
    });
    content.append(&collapse);

    let icon_data = editor
        .borrow()
        .as_ref()
        .and_then(|editor| {
            editor.run().segment_groups().groups()[group.group_index]
                .icon()
                .map(|icon| icon.data().to_vec())
        })
        .unwrap_or_default();
    let group_index = group.group_index;
    let choose_editor = editor.clone();
    let remove_editor = editor.clone();
    let icon_picker = build_icon_picker(
        &icon_data,
        24,
        28,
        Rc::new(move |image| {
            if let Some(editor) = choose_editor.borrow_mut().as_mut() {
                let _ = editor.set_segment_group_icon(group_index, image);
            }
        }),
        Rc::new(move || {
            if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                let _ = editor.remove_segment_group_icon(group_index);
            }
        }),
        None,
    );
    content.append(&icon_picker);

    let name = gtk::Entry::builder()
        .text(group.explicit_name.as_deref().unwrap_or_default())
        .placeholder_text(&group.name)
        .hexpand(true)
        .build();
    name.set_tooltip_text(Some(
        "Segment group name; leave empty to use the final segment's name",
    ));
    let name_editor = editor.clone();
    name.connect_changed(move |entry| {
        if let Some(editor) = name_editor.borrow_mut().as_mut() {
            let text = entry.text();
            let name = (!text.is_empty()).then_some(text.as_str());
            let _ = editor.rename_segment_group(group_index, name);
        }
    });
    content.append(&name);

    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some("Add a split inside this group"));
    let add_editor = editor.clone();
    let add_list = list.clone();
    let add_groups = column_groups.clone();
    add.connect_clicked(move |_| {
        if let Some(editor) = add_editor.borrow_mut().as_mut() {
            insert_segment_into_group(editor, group_index);
        }
        populate_run_segments(&add_list, &add_editor, &add_groups);
    });
    content.append(&add);

    let ungroup = gtk::Button::with_label("Ungroup");
    ungroup.set_tooltip_text(Some("Remove the group while keeping all of its splits"));
    let ungroup_editor = editor.clone();
    let ungroup_list = list.clone();
    let ungroup_groups = column_groups.clone();
    ungroup.connect_clicked(move |_| {
        if let Some(editor) = ungroup_editor.borrow_mut().as_mut() {
            let _ = editor.select_segment_group(group_index);
            let _ = editor.remove_selected_segment_groups();
        }
        populate_run_segments(&ungroup_list, &ungroup_editor, &ungroup_groups);
    });
    content.append(&ungroup);

    let drop_target = gtk::DropTarget::new(i32::static_type(), gdk::DragAction::MOVE);
    let drop_editor = editor.clone();
    let drop_list = list.clone();
    let drop_groups = column_groups.clone();
    drop_target.connect_drop(move |_, value, _, _| {
        let Ok(source) = value.get::<i32>() else {
            return false;
        };
        if source < 0 {
            return false;
        }
        if let Some(editor) = drop_editor.borrow_mut().as_mut() {
            move_segment_into_group(editor, source as usize, group_index);
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
    row.set_tooltip_text(Some(
        "Drop a split here to add it to this group; drag members outside to remove them",
    ));
    row.set_child(Some(&content));
    row
}

#[cfg(test)]
mod tests {
    use super::{
        insert_segment_into_group, move_segment_into_group, move_selected_segment_to,
        selected_segment_indices, set_segment_icons,
    };
    use livesplit_core::{Run, RunEditor, Segment};

    fn editor_with_group() -> RunEditor {
        let mut run = Run::new();
        for name in ["Intro", "A", "B", "Outro"] {
            run.push_segment(Segment::new(name));
        }
        let mut editor = RunEditor::new(run).unwrap();
        editor.select_only(1);
        editor.select_range(2);
        editor
            .create_segment_group_from_selection(Some("Chapter"))
            .unwrap();
        editor
    }

    #[test]
    fn dropping_a_split_on_a_group_adds_it_to_the_group() {
        let mut editor = editor_with_group();

        move_segment_into_group(&mut editor, 3, 0);

        let group = &editor.run().segment_groups().groups()[0];
        assert_eq!((group.start(), group.end()), (1, 4));
    }

    #[test]
    fn dragging_a_group_member_outside_removes_it_from_the_group() {
        let mut editor = editor_with_group();
        editor.select_only(1);

        move_selected_segment_to(&mut editor, 0);

        let group = &editor.run().segment_groups().groups()[0];
        assert_eq!((group.start(), group.end()), (2, 3));
        assert_eq!(editor.run().segment(0).name(), "A");
    }

    #[test]
    fn adding_to_a_single_split_group_keeps_both_splits_grouped() {
        let mut run = Run::new();
        run.push_segment(Segment::new("Only"));
        run.push_segment(Segment::new("Outro"));
        let mut editor = RunEditor::new(run).unwrap();
        editor
            .create_segment_group_from_selection(Some("Chapter"))
            .unwrap();

        insert_segment_into_group(&mut editor, 0);

        let group = &editor.run().segment_groups().groups()[0];
        assert_eq!((group.start(), group.end()), (0, 2));
        assert_eq!(group.name(), Some("Chapter"));
        assert_eq!(editor.run().segment(0).name(), "");
        assert_eq!(editor.run().segment(1).name(), "Only");
    }

    #[test]
    fn applying_an_icon_to_selected_splits_preserves_the_selection() {
        let mut editor = editor_with_group();
        editor.select_only(1);
        editor.select_additionally(2);
        let selected = selected_segment_indices(&editor);
        let image = livesplit_core::settings::Image::new(
            (&b"test icon"[..]).into(),
            livesplit_core::settings::Image::ICON,
        );

        set_segment_icons(&mut editor, &selected, &image);

        assert_eq!(editor.run().segment(1).icon(), &image);
        assert_eq!(editor.run().segment(2).icon(), &image);
        assert_eq!(selected_segment_indices(&editor), [1, 2]);
    }
}
