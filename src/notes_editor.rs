use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use anyhow::{Context, Result};
use druid::{
    widget::{Button, Checkbox, Flex, Label, Scroll, TextBox},
    Data, Lens, Widget, WidgetExt,
};

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

fn preview(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            let line = line
                .trim_start_matches('#')
                .trim_start()
                .trim_start_matches("- ")
                .trim_start_matches("* ");
            line.replace("**", "").replace("__", "").replace('`', "")
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    let rendered = Scroll::new(
        Label::dynamic(|data: &State, _| preview(&data.note))
            .with_line_break_mode(druid::widget::LineBreaking::WordWrap)
            .padding(8.0)
            .expand_width(),
    )
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

    let note = Label::dynamic(|data: &ViewerState, _| {
        data.notes
            .get(data.selected)
            .map(|note| preview(note))
            .filter(|note| !note.is_empty())
            .unwrap_or_else(|| "No notes for this split.".into())
    })
    .with_line_break_mode(druid::widget::LineBreaking::WordWrap)
    .padding(8.0)
    .expand_width();

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
}
