use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
#[cfg(test)]
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

const FILE_HEADER: &str = "# LiveSplit Split Notes\n\n<!-- livesplit-one-split-notes:v1 -->\n";

pub struct NotesDocument {
    pub split_names: Vec<String>,
    pub notes: Vec<String>,
    pub selected: usize,
    pub note: String,
    pub path: PathBuf,
    pub status: String,
    pub modified: bool,
}

impl NotesDocument {
    pub fn load(split_names: Vec<String>, path: PathBuf) -> Self {
        let mut notes = vec![String::new(); split_names.len()];
        let mut status = format!("Notes file: {}", path.display());
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(markdown) => notes = parse(&markdown, split_names.len()),
                Err(error) => status = format!("Could not read notes: {error}"),
            }
        }
        let note = notes.first().cloned().unwrap_or_default();
        Self {
            split_names,
            notes,
            selected: 0,
            note,
            path,
            status,
            modified: false,
        }
    }

    pub fn select(&mut self, index: usize) {
        if index >= self.split_names.len() {
            return;
        }
        self.store_current();
        self.selected = index;
        self.note.clone_from(&self.notes[index]);
    }

    pub fn update_note(&mut self, note: String) {
        if self.note != note {
            self.note = note;
            self.modified = true;
        }
    }

    pub fn append_markdown(&mut self, markdown: &str) {
        if !self.note.is_empty() && !self.note.ends_with('\n') {
            self.note.push('\n');
        }
        self.note.push_str(markdown);
        self.modified = true;
    }

    pub fn save(&mut self) -> Result<()> {
        self.store_current();
        save(&self.path, &self.split_names, &self.notes)?;
        self.modified = false;
        self.status = format!("Saved {}", self.path.display());
        Ok(())
    }

    pub fn reload(&mut self) -> Result<()> {
        self.notes = match fs::read_to_string(&self.path) {
            Ok(markdown) => parse(&markdown, self.split_names.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                vec![String::new(); self.split_names.len()]
            }
            Err(error) => return Err(error.into()),
        };
        self.note = self.notes.get(self.selected).cloned().unwrap_or_default();
        self.modified = false;
        self.status = if self.path.exists() {
            format!("Reloaded {}", self.path.display())
        } else {
            "There is no notes file yet.".into()
        };
        Ok(())
    }

    fn store_current(&mut self) {
        if let Some(note) = self.notes.get_mut(self.selected) {
            note.clone_from(&self.note);
        }
    }
}

pub struct NotesViewer {
    pub split_names: Vec<String>,
    pub notes: Vec<String>,
    pub selected: usize,
    pub path: PathBuf,
    modified: Option<SystemTime>,
}

impl NotesViewer {
    pub fn load(split_names: Vec<String>, path: PathBuf) -> Self {
        let modified = modified_time(&path);
        let notes = fs::read_to_string(&path)
            .map(|markdown| parse(&markdown, split_names.len()))
            .unwrap_or_else(|_| vec![String::new(); split_names.len()]);
        Self {
            split_names,
            notes,
            selected: 0,
            path,
            modified,
        }
    }

    pub fn follow_split(&mut self, index: usize) -> bool {
        let reloaded = self.reload_if_changed();
        let selected = index.min(self.split_names.len().saturating_sub(1));
        let changed = selected != self.selected;
        self.selected = selected;
        reloaded || changed
    }

    pub fn title(&self) -> String {
        self.split_names
            .get(self.selected)
            .map(|name| format!("{}. {name}", self.selected + 1))
            .unwrap_or_else(|| "No current split".into())
    }

    pub fn note(&self) -> &str {
        self.notes
            .get(self.selected)
            .filter(|note| !note.is_empty())
            .map_or("No notes for this split.", String::as_str)
    }

    fn reload_if_changed(&mut self) -> bool {
        let modified = modified_time(&self.path);
        if modified == self.modified {
            return false;
        }
        self.modified = modified;
        self.notes = fs::read_to_string(&self.path)
            .map(|markdown| parse(&markdown, self.split_names.len()))
            .unwrap_or_else(|_| vec![String::new(); self.split_names.len()]);
        true
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
            "\n<!-- split:{index} -->\n## {}. {}\n\n",
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

pub fn parse(markdown: &str, count: usize) -> Vec<String> {
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

#[cfg(test)]
pub fn render_markdown(markdown: &str) -> String {
    let mut output = String::new();
    let mut list_depth: usize = 0;
    let mut ordered = Vec::new();
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Text(text) | Event::Code(text) => output.push_str(&text),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Start(Tag::List(start)) => {
                list_depth += 1;
                ordered.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                ordered.pop();
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
            Event::Start(Tag::Item) => {
                if !output.is_empty() && !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                if let Some(Some(index)) = ordered.last_mut() {
                    output.push_str(&format!("{index}. "));
                    *index += 1;
                } else {
                    output.push_str("• ");
                }
            }
            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "☑ " } else { "☐ " });
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => {
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
            Event::Rule => output.push_str("────────\n"),
            _ => {}
        }
    }
    output.trim().to_owned()
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
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
        assert_eq!(
            render_markdown("- first\n- [ ] todo\n- [x] done\n\n1. one\n2. two"),
            "• first\n• ☐ todo\n• ☑ done\n1. one\n2. two"
        );
    }

    #[test]
    fn switching_splits_preserves_each_draft() {
        let mut document = NotesDocument::load(
            vec!["First".into(), "Second".into()],
            PathBuf::from("/path/that/does/not/exist.notes.md"),
        );
        document.update_note("first note".into());
        document.select(1);
        document.update_note("second note".into());
        document.select(0);

        assert_eq!(document.note, "first note");
        assert_eq!(document.notes, ["first note", "second note"]);
        assert!(document.modified);
    }
}
