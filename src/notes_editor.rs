use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use anyhow::{Context, Result};
use druid::{
    piet::{FontFamily, FontStyle, FontWeight},
    text::RichText,
    widget::{Button, Checkbox, Flex, Label, Scroll, TextBox},
    Color, Data, Lens, Widget, WidgetExt,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::consts::{BUTTON_SPACING, MARGIN};

const FILE_HEADER: &str = "# LiveSplit Split Notes\n\n<!-- livesplit-one-split-notes:v1 -->\n";

#[derive(Clone, Data, Lens)]
pub struct State {
    split_names: Arc<Vec<String>>,
    notes: Arc<Vec<String>>,
    selected: usize,
    note: String,
    preview: bool,
    status: String,
    #[data(ignore)]
    path: PathBuf,
}

impl State {
    pub fn load(split_names: Vec<String>, path: PathBuf) -> Self {
        let mut notes = vec![String::new(); split_names.len()];
        let mut status = format!("Notes file: {}", path.display());

        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(markdown) => {
                    notes = parse(&markdown, split_names.len());
                }
                Err(error) => status = format!("Could not read notes: {error}"),
            }
        }

        let note = notes.first().cloned().unwrap_or_default();
        Self {
            split_names: Arc::new(split_names),
            notes: Arc::new(notes),
            selected: 0,
            note,
            preview: false,
            status,
            path,
        }
    }

    fn select(&mut self, index: usize) {
        if index >= self.split_names.len() {
            return;
        }
        self.store_current();
        self.selected = index;
        self.note = self.notes[index].clone();
    }

    fn store_current(&mut self) {
        if self.selected >= self.notes.len() {
            return;
        }
        Arc::make_mut(&mut self.notes)[self.selected] = self.note.clone();
    }

    fn save(&mut self) {
        self.store_current();
        match save(&self.path, &self.split_names, &self.notes) {
            Ok(()) => self.status = format!("Saved {}", self.path.display()),
            Err(error) => self.status = format!("Could not save notes: {error:#}"),
        }
    }

    fn reload(&mut self) {
        match fs::read_to_string(&self.path) {
            Ok(markdown) => {
                self.notes = Arc::new(parse(&markdown, self.split_names.len()));
                self.note = self.notes.get(self.selected).cloned().unwrap_or_default();
                self.status = format!("Reloaded {}", self.path.display());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.notes = Arc::new(vec![String::new(); self.split_names.len()]);
                self.note.clear();
                self.status = "There is no notes file yet.".into();
            }
            Err(error) => self.status = format!("Could not reload notes: {error}"),
        }
    }
}

pub fn sidecar_path(splits_path: &Path) -> PathBuf {
    let mut file_name = splits_path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".notes.md");
    splits_path.with_file_name(file_name)
}

fn save(path: &Path, split_names: &[String], notes: &[String]) -> Result<()> {
    let mut markdown = String::from(FILE_HEADER);
    for (index, name) in split_names.iter().enumerate() {
        markdown.push_str(&format!(
            "\n<!-- split:{} -->\n## {}. {}\n\n",
            index,
            index + 1,
            name.replace('\n', " ")
        ));
        if let Some(note) = notes.get(index) {
            markdown.push_str(note.trim_end());
        }
        markdown.push('\n');
    }
    fs::write(path, markdown).with_context(|| format!("Failed writing {}", path.display()))
}

fn parse(markdown: &str, count: usize) -> Vec<String> {
    let mut notes = vec![String::new(); count];
    let mut current = None;
    let mut body = String::new();

    let finish = |current: Option<usize>, body: &mut String, notes: &mut Vec<String>| {
        if let Some(index) = current.filter(|index| *index < notes.len()) {
            notes[index] = body.trim().to_owned();
        }
        body.clear();
    };

    for line in markdown.lines() {
        if let Some(index) = line
            .strip_prefix("<!-- split:")
            .and_then(|value| value.strip_suffix(" -->"))
            .and_then(|value| value.parse::<usize>().ok())
        {
            finish(current, &mut body, &mut notes);
            current = Some(index);
        } else if current.is_some() {
            // The generated heading is metadata. The note starts after it.
            if body.is_empty() && line.starts_with("## ") {
                continue;
            }
            body.push_str(line);
            body.push('\n');
        }
    }
    finish(current, &mut body, &mut notes);
    notes
}

fn append_markdown(data: &mut State, markdown: &str) {
    if !data.note.is_empty() && !data.note.ends_with('\n') {
        data.note.push('\n');
    }
    data.note.push_str(markdown);
}

#[derive(Clone, Copy)]
enum Style {
    Strong,
    Emphasis,
    Strikethrough,
    Heading(HeadingLevel),
    Code,
    Link,
}

struct RichMarkdown {
    text: String,
    spans: Vec<(usize, usize, Style)>,
    styles: Vec<(Style, usize)>,
    lists: Vec<Option<u64>>,
    item_pending: bool,
    quote_depth: usize,
}

impl RichMarkdown {
    fn new() -> Self {
        Self {
            text: String::new(),
            spans: Vec::new(),
            styles: Vec::new(),
            lists: Vec::new(),
            item_pending: false,
            quote_depth: 0,
        }
    }

    fn at_line_start(&self) -> bool {
        self.text.is_empty() || self.text.ends_with('\n')
    }

    fn newline(&mut self) {
        if !self.at_line_start() {
            self.text.push('\n');
        }
    }

    fn blank_line(&mut self) {
        self.newline();
        if !self.text.is_empty() && !self.text.ends_with("\n\n") {
            self.text.push('\n');
        }
    }

    fn item_prefix(&mut self, task: Option<bool>) {
        if !self.item_pending {
            return;
        }
        self.item_pending = false;
        self.text
            .push_str(&"  ".repeat(self.lists.len().saturating_sub(1)));
        if let Some(checked) = task {
            self.text.push_str(if checked { "☑ " } else { "☐ " });
        } else if let Some(Some(next)) = self.lists.last_mut() {
            self.text.push_str(&format!("{next}. "));
            *next += 1;
        } else {
            self.text.push_str("• ");
        }
    }

    fn push_text(&mut self, text: &str) {
        self.item_prefix(None);
        if self.quote_depth > 0 && self.at_line_start() {
            self.text.push_str(&"│ ".repeat(self.quote_depth));
        }
        self.text.push_str(text);
    }

    fn start_style(&mut self, style: Style) {
        self.item_prefix(None);
        self.styles.push((style, self.text.len()));
    }

    fn end_style(&mut self, matches: impl Fn(Style) -> bool) {
        if let Some(index) = self.styles.iter().rposition(|(style, _)| matches(*style)) {
            let (style, start) = self.styles.remove(index);
            self.spans.push((start, self.text.len(), style));
        }
    }

    fn build(mut self) -> RichText {
        let mut rich = RichText::new(std::mem::take(&mut self.text).into());
        for (start, end, style) in self.spans {
            if start == end {
                continue;
            }
            rich.add_attribute(
                start..end,
                match style {
                    Style::Strong => druid::text::Attribute::weight(FontWeight::BOLD),
                    Style::Emphasis => druid::text::Attribute::style(FontStyle::Italic),
                    Style::Strikethrough => druid::text::Attribute::Strikethrough(true),
                    Style::Heading(level) => {
                        let size = match level {
                            HeadingLevel::H1 => 26.0,
                            HeadingLevel::H2 => 23.0,
                            HeadingLevel::H3 => 20.0,
                            HeadingLevel::H4 => 18.0,
                            HeadingLevel::H5 => 16.0,
                            HeadingLevel::H6 => 14.0,
                        };
                        druid::text::Attribute::size(size)
                    }
                    Style::Code => druid::text::Attribute::font_family(FontFamily::MONOSPACE),
                    Style::Link => druid::text::Attribute::text_color(Color::rgb8(80, 140, 220)),
                },
            );
            if matches!(style, Style::Heading(_)) {
                rich.add_attribute(start..end, druid::text::Attribute::weight(FontWeight::BOLD));
            } else if matches!(style, Style::Link) {
                rich.add_attribute(start..end, druid::text::Attribute::underline(true));
            }
        }
        rich
    }
}

fn render_markdown(markdown: &str) -> RichText {
    let mut out = RichMarkdown::new();
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Paragraph) => {
                if !out.item_pending {
                    out.blank_line();
                }
            }
            Event::End(TagEnd::Paragraph) => out.blank_line(),
            Event::Start(Tag::Heading { level, .. }) => {
                out.blank_line();
                out.start_style(Style::Heading(level));
            }
            Event::End(TagEnd::Heading(level)) => {
                out.end_style(|style| matches!(style, Style::Heading(value) if value == level));
                out.blank_line();
            }
            Event::Start(Tag::Strong) => out.start_style(Style::Strong),
            Event::End(TagEnd::Strong) => out.end_style(|style| matches!(style, Style::Strong)),
            Event::Start(Tag::Emphasis) => out.start_style(Style::Emphasis),
            Event::End(TagEnd::Emphasis) => out.end_style(|style| matches!(style, Style::Emphasis)),
            Event::Start(Tag::Strikethrough) => out.start_style(Style::Strikethrough),
            Event::End(TagEnd::Strikethrough) => {
                out.end_style(|style| matches!(style, Style::Strikethrough))
            }
            Event::Start(Tag::Link { .. }) => out.start_style(Style::Link),
            Event::End(TagEnd::Link) => out.end_style(|style| matches!(style, Style::Link)),
            Event::Start(Tag::CodeBlock(kind)) => {
                out.blank_line();
                if let CodeBlockKind::Fenced(language) = kind {
                    if !language.is_empty() {
                        out.push_text(&format!("{language}\n"));
                    }
                }
                out.start_style(Style::Code);
            }
            Event::End(TagEnd::CodeBlock) => {
                out.end_style(|style| matches!(style, Style::Code));
                out.blank_line();
            }
            Event::Start(Tag::List(start)) => {
                if out.lists.is_empty() {
                    out.blank_line();
                }
                out.lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                out.lists.pop();
                out.newline();
            }
            Event::Start(Tag::Item) => {
                out.newline();
                out.item_pending = true;
            }
            Event::End(TagEnd::Item) => {
                out.item_prefix(None);
                out.newline();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                out.newline();
                out.quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                out.quote_depth = out.quote_depth.saturating_sub(1);
                out.blank_line();
            }
            Event::Text(text) => out.push_text(&text),
            Event::Code(code) => {
                out.item_prefix(None);
                let start = out.text.len();
                out.push_text(&code);
                out.spans.push((start, out.text.len(), Style::Code));
            }
            Event::TaskListMarker(checked) => out.item_prefix(Some(checked)),
            Event::SoftBreak | Event::HardBreak => {
                out.item_prefix(None);
                out.text.push('\n');
            }
            Event::Rule => {
                out.blank_line();
                out.push_text("────────");
                out.blank_line();
            }
            Event::Html(html) | Event::InlineHtml(html) => out.push_text(&html),
            Event::FootnoteReference(name) => out.push_text(&format!("[{name}]")),
            Event::InlineMath(math) | Event::DisplayMath(math) => out.push_text(&math),
            _ => {}
        }
    }
    while out.text.ends_with('\n') {
        out.text.pop();
    }
    out.build()
}

fn markdown_view<T: Data>(markdown: impl Fn(&T) -> String + 'static) -> impl Widget<T> {
    struct MarkdownLens<T>(Arc<dyn Fn(&T) -> String>);

    impl<T> Clone for MarkdownLens<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T> Lens<T, RichText> for MarkdownLens<T> {
        fn with<V, F: FnOnce(&RichText) -> V>(&self, data: &T, f: F) -> V {
            f(&render_markdown(&(self.0)(data)))
        }

        fn with_mut<V, F: FnOnce(&mut RichText) -> V>(&self, data: &mut T, f: F) -> V {
            // The preview is derived data. RawLabel never mutates its text value.
            f(&mut render_markdown(&(self.0)(data)))
        }
    }

    Label::<RichText>::raw()
        .with_line_break_mode(druid::widget::LineBreaking::WordWrap)
        .lens(MarkdownLens(Arc::new(markdown)))
        .padding(8.0)
        .expand_width()
}

pub fn root_widget() -> impl Widget<State> {
    let title = Label::dynamic(|data: &State, _| {
        if data.split_names.is_empty() {
            "This run has no splits".into()
        } else {
            format!(
                "Split {} of {}: {}",
                data.selected + 1,
                data.split_names.len(),
                data.split_names[data.selected]
            )
        }
    })
    .with_text_size(16.0);

    let navigation = Flex::row()
        .with_child(Button::new("Previous").on_click(|_, data: &mut State, _| {
            if data.selected > 0 {
                data.select(data.selected - 1);
            }
        }))
        .with_spacer(BUTTON_SPACING)
        .with_flex_child(title, 1.0)
        .with_spacer(BUTTON_SPACING)
        .with_child(Button::new("Next").on_click(|_, data: &mut State, _| {
            data.select(data.selected + 1);
        }));

    let toolbar = Flex::row()
        .with_child(Button::new("Heading").on_click(|_, data: &mut State, _| {
            append_markdown(data, "### Heading");
        }))
        .with_spacer(BUTTON_SPACING)
        .with_child(Button::new("Bold").on_click(|_, data: &mut State, _| {
            append_markdown(data, "**bold text**");
        }))
        .with_spacer(BUTTON_SPACING)
        .with_child(Button::new("Checklist").on_click(|_, data: &mut State, _| {
            append_markdown(data, "- [ ] item");
        }))
        .with_spacer(BUTTON_SPACING)
        .with_child(Button::new("Link").on_click(|_, data: &mut State, _| {
            append_markdown(data, "[label](https://example.com)");
        }))
        .with_flex_spacer(1.0)
        .with_child(Checkbox::new("Preview").lens(State::preview));

    let editor = TextBox::multiline()
        .with_placeholder("Add notes for this split…")
        .lens(State::note)
        .expand();
    let rendered = Scroll::new(markdown_view(|data: &State| data.note.clone()))
        .vertical()
        .expand();

    let content = druid::widget::Either::new(|data: &State, _| data.preview, rendered, editor);

    let footer = Flex::row()
        .with_flex_child(
            Label::dynamic(|data: &State, _| data.status.clone())
                .with_line_break_mode(druid::widget::LineBreaking::WordWrap),
            1.0,
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(Button::new("Reload").on_click(|_, data: &mut State, _| data.reload()))
        .with_spacer(BUTTON_SPACING)
        .with_child(Button::new("Save Notes").on_click(|_, data: &mut State, _| data.save()));

    Flex::column()
        .with_child(navigation)
        .with_spacer(BUTTON_SPACING)
        .with_child(toolbar)
        .with_spacer(BUTTON_SPACING)
        .with_flex_child(content, 1.0)
        .with_spacer(BUTTON_SPACING)
        .with_child(footer)
        .padding(MARGIN)
}

#[derive(Clone, Data, Lens)]
pub struct ViewerState {
    split_names: Arc<Vec<String>>,
    notes: Arc<Vec<String>>,
    selected: usize,
    #[data(ignore)]
    path: PathBuf,
    #[data(ignore)]
    modified: Option<SystemTime>,
}

impl ViewerState {
    pub fn load(split_names: Vec<String>, path: PathBuf) -> Self {
        let modified = modified_time(&path);
        let notes = fs::read_to_string(&path)
            .map(|markdown| parse(&markdown, split_names.len()))
            .unwrap_or_else(|_| vec![String::new(); split_names.len()]);
        Self {
            split_names: Arc::new(split_names),
            notes: Arc::new(notes),
            selected: 0,
            path,
            modified,
        }
    }

    pub fn follow_split(&mut self, index: usize) {
        self.reload_if_changed();
        if index < self.split_names.len() {
            self.selected = index;
        }
    }

    fn reload_if_changed(&mut self) {
        let modified = modified_time(&self.path);
        if modified == self.modified {
            return;
        }
        self.modified = modified;
        self.notes = Arc::new(
            fs::read_to_string(&self.path)
                .map(|markdown| parse(&markdown, self.split_names.len()))
                .unwrap_or_else(|_| vec![String::new(); self.split_names.len()]),
        );
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

pub fn viewer_widget() -> impl Widget<ViewerState> {
    let title = Label::dynamic(|data: &ViewerState, _| {
        data.split_names
            .get(data.selected)
            .map(|name| format!("{}. {}", data.selected + 1, name))
            .unwrap_or_else(|| "No current split".into())
    })
    .with_text_size(18.0);

    let note = markdown_view(|data: &ViewerState| {
        data.notes
            .get(data.selected)
            .cloned()
            .filter(|note| !note.is_empty())
            .unwrap_or_else(|| "No notes for this split.".into())
    });

    Flex::column()
        .with_child(title)
        .with_spacer(BUTTON_SPACING)
        .with_flex_child(Scroll::new(note).vertical(), 1.0)
        .padding(MARGIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_notes_by_split_index() {
        let source = concat!(
            "# LiveSplit Split Notes\n\n",
            "<!-- split:0 -->\n## 1. Intro\n\nFirst **note**\n\n",
            "<!-- split:1 -->\n## 2. Boss\n\n- dodge\n- hit\n",
        );
        assert_eq!(parse(source, 2), vec!["First **note**", "- dodge\n- hit"]);
    }

    #[test]
    fn sidecar_keeps_original_extension_visible() {
        assert_eq!(
            sidecar_path(Path::new("/runs/game.lss")),
            PathBuf::from("/runs/game.lss.notes.md")
        );
    }

    #[test]
    fn renders_lists_and_task_lists() {
        use druid::piet::TextStorage;

        let rendered = render_markdown("- first\n- [ ] todo\n- [x] done\n\n1. one\n2. two");
        assert_eq!(
            rendered.as_str(),
            "• first\n☐ todo\n☑ done\n\n1. one\n2. two"
        );
    }
}
