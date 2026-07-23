use super::*;

pub(super) fn build_notes_editor(
    parent: &gtk::ApplicationWindow,
    document: Rc<RefCell<crate::notes::NotesDocument>>,
    sender: &ComponentSender<AppModel>,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .title("Split Notes")
        .transient_for(parent)
        .default_width(760)
        .default_height(580)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let overlay = adw::ToastOverlay::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let banner = adw::Banner::new("");
    let initial_status = document.borrow().status.clone();
    if initial_status.starts_with("Could not") {
        banner.set_title(&initial_status);
        banner.set_revealed(true);
    }
    content.append(&banner);

    let note_buffer = gtk::TextBuffer::new(None);
    note_buffer.set_text(&document.borrow().note);
    let editor = gtk::TextView::with_buffer(&note_buffer);
    editor.set_wrap_mode(gtk::WrapMode::WordChar);
    editor.set_left_margin(8);
    editor.set_right_margin(8);
    editor.set_top_margin(8);
    editor.set_bottom_margin(8);
    let editor_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&editor)
        .build();

    let preview_buffer = gtk::TextBuffer::new(None);
    set_markdown_buffer(&preview_buffer, &document.borrow().note);
    let preview = gtk::TextView::with_buffer(&preview_buffer);
    preview.set_editable(false);
    preview.set_cursor_visible(false);
    preview.set_wrap_mode(gtk::WrapMode::WordChar);
    preview.set_left_margin(12);
    preview.set_right_margin(12);
    preview.set_top_margin(12);
    preview.set_bottom_margin(12);
    let preview_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&preview)
        .build();

    let modes = adw::ViewStack::new();
    modes.set_vexpand(true);
    modes.add_titled_with_icon(
        &editor_scroll,
        Some("edit"),
        "Edit",
        "document-edit-symbolic",
    );
    modes.add_titled_with_icon(
        &preview_scroll,
        Some("preview"),
        "Preview",
        "view-reveal-symbolic",
    );
    let mode_switcher = adw::ViewSwitcher::builder()
        .stack(&modes)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();

    let split_list = gtk::ListBox::new();
    split_list.set_selection_mode(gtk::SelectionMode::Single);
    for (index, name) in document.borrow().split_names.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::builder()
            .label(format!("{}. {name}", index + 1))
            .xalign(0.0)
            .margin_start(10)
            .margin_end(10)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        row.set_child(Some(&label));
        split_list.append(&row);
    }
    if let Some(row) = split_list.row_at_index(0) {
        split_list.select_row(Some(&row));
    }
    let sidebar = gtk::ScrolledWindow::builder()
        .min_content_width(190)
        .child(&split_list)
        .build();
    let main = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let edit_tools = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (label, markdown) in [
        ("Heading", "### Heading"),
        ("Bold", "**bold text**"),
        ("Checklist", "- [ ] item"),
        ("Link", "[label](https://example.com)"),
    ] {
        let button = gtk::Button::with_label(label);
        let tool_document = document.clone();
        let tool_buffer = note_buffer.clone();
        button.connect_clicked(move |_| {
            let note = {
                let mut document = tool_document.borrow_mut();
                document.append_markdown(markdown);
                document.note.clone()
            };
            tool_buffer.set_text(&note);
        });
        edit_tools.append(&button);
    }
    let tool_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tool_spacer.set_hexpand(true);
    edit_tools.append(&tool_spacer);
    edit_tools.append(&mode_switcher);
    main.append(&edit_tools);
    main.append(&modes);
    let split_view = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&sidebar)
        .end_child(&main)
        .resize_start_child(false)
        .shrink_start_child(false)
        .wide_handle(true)
        .vexpand(true)
        .build();
    content.append(&split_view);

    let changed_document = document.clone();
    let changed_preview = preview_buffer.clone();
    note_buffer.connect_changed(move |buffer| {
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        changed_document.borrow_mut().update_note(text.to_string());
        set_markdown_buffer(&changed_preview, text.as_str());
    });
    let selected_document = document.clone();
    let selected_buffer = note_buffer.clone();
    split_list.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            return;
        };
        let note = {
            let mut document = selected_document.borrow_mut();
            document.select(row.index() as usize);
            document.note.clone()
        };
        selected_buffer.set_text(&note);
    });

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let status = gtk::Label::builder()
        .label(&initial_status)
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .build();
    status.add_css_class("dim-label");
    footer.append(&status);
    let reload = gtk::Button::with_label("Reload");
    let reload_document = document.clone();
    let reload_buffer = note_buffer.clone();
    let reload_status = status.clone();
    let reload_banner = banner.clone();
    reload.connect_clicked(move |_| {
        let result = {
            let mut document = reload_document.borrow_mut();
            document
                .reload()
                .map(|()| (document.note.clone(), document.status.clone()))
        };
        match result {
            Ok((note, status)) => {
                reload_buffer.set_text(&note);
                reload_status.set_label(&status);
                reload_banner.set_revealed(false);
            }
            Err(error) => {
                reload_banner.set_title(&format!("Could not reload notes: {error:#}"));
                reload_banner.set_revealed(true);
            }
        }
    });
    footer.append(&reload);
    let save = gtk::Button::with_label("Save Notes");
    save.add_css_class("suggested-action");
    let save_document = document.clone();
    let save_status = status.clone();
    let save_banner = banner.clone();
    let save_overlay = overlay.clone();
    save.connect_clicked(move |_| {
        let result = {
            let mut document = save_document.borrow_mut();
            document.save()
        };
        match result {
            Ok(()) => {
                save_status.set_label(&save_document.borrow().status);
                save_banner.set_revealed(false);
                save_overlay.add_toast(adw::Toast::new("Split notes saved"));
            }
            Err(error) => {
                save_banner.set_title(&format!("Could not save notes: {error:#}"));
                save_banner.set_revealed(true);
            }
        }
    });
    footer.append(&save);
    content.append(&footer);
    overlay.set_child(Some(&content));
    toolbar.set_content(Some(&overlay));
    window.set_content(Some(&toolbar));
    let close_sender = sender.clone();
    window.connect_close_request(move |_| {
        close_sender.input(AppMsg::EditorFinished(EditorKind::Notes));
        glib::Propagation::Proceed
    });
    window
}

pub(super) fn build_notes_viewer(
    parent: &gtk::ApplicationWindow,
    state: &crate::notes::NotesViewer,
    sender: &ComponentSender<AppModel>,
) -> (adw::ApplicationWindow, gtk::Label, gtk::TextBuffer) {
    let window = adw::ApplicationWindow::builder()
        .title("Split Notes")
        .transient_for(parent)
        .default_width(440)
        .default_height(320)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let title = gtk::Label::builder()
        .label(state.title())
        .xalign(0.0)
        .css_classes(["title-2"])
        .build();
    content.append(&title);
    let buffer = gtk::TextBuffer::new(None);
    set_markdown_buffer(&buffer, state.note());
    let note = gtk::TextView::with_buffer(&buffer);
    note.set_editable(false);
    note.set_cursor_visible(false);
    note.set_wrap_mode(gtk::WrapMode::WordChar);
    note.set_left_margin(8);
    note.set_right_margin(8);
    note.set_top_margin(8);
    note.set_bottom_margin(8);
    content.append(
        &gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&note)
            .build(),
    );
    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));
    let close_sender = sender.clone();
    window.connect_close_request(move |_| {
        close_sender.input(AppMsg::EditorFinished(EditorKind::NotesViewer));
        glib::Propagation::Proceed
    });
    (window, title, buffer)
}

pub(super) fn set_markdown_buffer(buffer: &gtk::TextBuffer, markdown: &str) {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    for (name, properties) in [
        (
            "strong",
            vec![("weight", &700_i32 as &dyn glib::value::ToValue)],
        ),
        (
            "emphasis",
            vec![(
                "style",
                &gtk::pango::Style::Italic as &dyn glib::value::ToValue,
            )],
        ),
        (
            "strike",
            vec![("strikethrough", &true as &dyn glib::value::ToValue)],
        ),
        (
            "code",
            vec![("family", &"monospace" as &dyn glib::value::ToValue)],
        ),
        (
            "heading",
            vec![
                ("weight", &700_i32 as &dyn glib::value::ToValue),
                ("scale", &1.25_f64 as &dyn glib::value::ToValue),
            ],
        ),
        (
            "link",
            vec![(
                "underline",
                &gtk::pango::Underline::Single as &dyn glib::value::ToValue,
            )],
        ),
    ] {
        if buffer.tag_table().lookup(name).is_none() {
            buffer.create_tag(Some(name), &properties);
        }
    }
    buffer.set_text("");
    let mut iter = buffer.end_iter();
    let mut active = Vec::<&'static str>::new();
    let mut lists = Vec::<Option<u64>>::new();
    let insert =
        |buffer: &gtk::TextBuffer, iter: &mut gtk::TextIter, text: &str, active: &[&str]| {
            buffer.insert_with_tags_by_name(iter, text, active);
        };
    let remove = |active: &mut Vec<&'static str>, name| {
        if let Some(index) = active.iter().rposition(|candidate| *candidate == name) {
            active.remove(index);
        }
    };
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Strong) => active.push("strong"),
            Event::End(TagEnd::Strong) => remove(&mut active, "strong"),
            Event::Start(Tag::Emphasis) => active.push("emphasis"),
            Event::End(TagEnd::Emphasis) => remove(&mut active, "emphasis"),
            Event::Start(Tag::Strikethrough) => active.push("strike"),
            Event::End(TagEnd::Strikethrough) => remove(&mut active, "strike"),
            Event::Start(Tag::Link { .. }) => active.push("link"),
            Event::End(TagEnd::Link) => remove(&mut active, "link"),
            Event::Start(Tag::Heading { .. }) => active.push("heading"),
            Event::End(TagEnd::Heading(_)) => {
                remove(&mut active, "heading");
                insert(buffer, &mut iter, "\n", &active);
            }
            Event::Start(Tag::CodeBlock(_)) => active.push("code"),
            Event::End(TagEnd::CodeBlock) => {
                remove(&mut active, "code");
                insert(buffer, &mut iter, "\n", &active);
            }
            Event::Start(Tag::List(start)) => lists.push(start),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                if iter.offset() > 0 {
                    insert(buffer, &mut iter, "\n", &active);
                }
            }
            Event::Start(Tag::Item) => {
                let indent = "  ".repeat(lists.len().saturating_sub(1));
                let marker = if let Some(Some(number)) = lists.last_mut() {
                    let marker = format!("{number}. ");
                    *number += 1;
                    marker
                } else {
                    "• ".to_owned()
                };
                insert(buffer, &mut iter, &format!("{indent}{marker}"), &active);
            }
            Event::End(TagEnd::Item | TagEnd::Paragraph) => {
                insert(buffer, &mut iter, "\n", &active);
            }
            Event::Text(text) => insert(buffer, &mut iter, &text, &active),
            Event::Code(text) => {
                let mut tags = active.clone();
                tags.push("code");
                insert(buffer, &mut iter, &text, &tags);
            }
            Event::TaskListMarker(checked) => {
                insert(
                    buffer,
                    &mut iter,
                    if checked { "☑ " } else { "☐ " },
                    &active,
                );
            }
            Event::SoftBreak | Event::HardBreak => insert(buffer, &mut iter, "\n", &active),
            Event::Rule => insert(buffer, &mut iter, "────────\n", &active),
            _ => {}
        }
    }
}
