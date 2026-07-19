use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use roxmltree::Document;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/LiveSplit/LiveSplit.AutoSplitters/master/LiveSplit.AutoSplitters.xml";
const MAX_REGISTRY_SIZE: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compatibility {
    CrossPlatformAsr,
    WindowsAsl,
    WindowsDotNet,
}

impl Compatibility {
    pub fn label(&self) -> &'static str {
        match self {
            Self::CrossPlatformAsr => "Cross-platform ASR/WASM",
            Self::WindowsAsl => "Windows-only ASL",
            Self::WindowsDotNet => "Windows-only .NET",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub game: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub website: Option<String>,
    pub urls: Vec<String>,
    pub compatibility: Compatibility,
    pub installable: bool,
}

pub fn plain_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => {
                in_tag = true;
                output.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse(xml: &str) -> Result<Vec<Entry>> {
    let document = Document::parse(xml).context("Malformed auto-splitter registry")?;
    let mut entries = Vec::new();
    for node in document
        .descendants()
        .filter(|n| n.has_tag_name("AutoSplitter"))
    {
        let text = |name| {
            node.children()
                .find(|n| n.has_tag_name(name))
                .and_then(|n| n.text())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        let games = node
            .descendants()
            .filter(|n| n.has_tag_name("Game") || n.has_tag_name("GameName"))
            .filter_map(|n| n.text())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let Some(game) = games.first().cloned() else {
            continue;
        };
        let kind = text("Type").unwrap_or_default();
        let script_type = text("ScriptType").unwrap_or_default();
        let urls = node
            .descendants()
            .filter(|n| n.has_tag_name("URL"))
            .filter_map(|n| n.text())
            .map(str::trim)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let compatibility = if kind == "Script" && script_type == "AutoSplittingRuntime" {
            Compatibility::CrossPlatformAsr
        } else if kind == "Script" {
            Compatibility::WindowsAsl
        } else {
            Compatibility::WindowsDotNet
        };
        let aliases = games.into_iter().skip(1).collect();
        entries.push(Entry {
            game,
            aliases,
            description: plain_text(text("Description").unwrap_or_default()),
            website: text("Website").map(str::to_owned),
            installable: compatibility == Compatibility::CrossPlatformAsr && !urls.is_empty(),
            compatibility,
            urls,
        });
    }
    Ok(entries)
}

pub fn matching<'a>(entries: &'a [Entry], game: &str) -> Option<&'a Entry> {
    entries.iter().find(|entry| {
        entry.game.eq_ignore_ascii_case(game)
            || entry
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(game))
    })
}

pub fn cache_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("org", "LiveSplit", "LiveSplit One")
        .context("Application data directory is unavailable")?;
    Ok(dirs.data_local_dir().join("autosplitters"))
}

pub fn managed_path(game: &str, url: &str) -> Result<PathBuf> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("Registry URL is not HTTP(S)");
    }
    let filename = Path::new(url)
        .file_name()
        .filter(|name| !name.is_empty())
        .context("Registry URL has no file name")?;
    let safe_game: String = game
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(cache_dir()?.join(safe_game).join(filename))
}

pub fn load_cached_or_refresh() -> Result<Vec<Entry>> {
    let cache = cache_dir()?.join("LiveSplit.AutoSplitters.xml");
    let fetched = (|| -> Result<String> {
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(10))
            .build()
            .get(REGISTRY_URL)
            .call()
            .context("Registry request failed")?;
        let mut body = String::new();
        response
            .into_reader()
            .take(MAX_REGISTRY_SIZE + 1)
            .read_to_string(&mut body)?;
        if body.len() as u64 > MAX_REGISTRY_SIZE {
            bail!("Registry response is too large");
        }
        parse(&body)?;
        Ok(body)
    })();
    match fetched {
        Ok(xml) => {
            if let Some(parent) = cache.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&cache, &xml)?;
            parse(&xml)
        }
        Err(error) => {
            let xml = fs::read_to_string(&cache)
                .with_context(|| format!("{error:#}; no cached registry is available"))?;
            parse(&xml)
        }
    }
}

pub fn download_to_temporary(entry: &Entry) -> Result<(PathBuf, PathBuf, String)> {
    let url = entry
        .urls
        .first()
        .context("Registry entry has no download URL")?;
    let destination = managed_path(&entry.game, url)?;
    let parent = destination
        .parent()
        .context("Managed auto-splitter has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(".autosplitter.download");
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get(url)
        .call()
        .context("Auto-splitter download failed")?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_REGISTRY_SIZE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REGISTRY_SIZE {
        bail!("Auto-splitter download is too large");
    }
    fs::write(&temporary, bytes)?;
    Ok((temporary, destination, url.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_and_matches_exact_aliases() {
        let xml = r#"<AutoSplitters>
          <AutoSplitter><Games><Game>Celeste</Game><Game>Celeste Classic</Game></Games><Description>ASR</Description>
          <Type>Script</Type><ScriptType>AutoSplittingRuntime</ScriptType>
          <URLs><URL>https://example.com/celeste.wasm</URL></URLs></AutoSplitter>
          <AutoSplitter><Games><Game>Old</Game></Games><Type>Script</Type><ScriptType>ASL</ScriptType></AutoSplitter>
          <AutoSplitter><Games><Game>Component</Game></Games><Type>Component</Type></AutoSplitter>
        </AutoSplitters>"#;
        let entries = parse(xml).unwrap();
        assert!(entries[0].installable);
        assert_eq!(entries[1].compatibility, Compatibility::WindowsAsl);
        assert_eq!(entries[2].compatibility, Compatibility::WindowsDotNet);
        assert_eq!(
            matching(&entries, "celeste classic").unwrap().game,
            "Celeste"
        );
        assert!(matching(&entries, "Celest").is_none());
    }

    #[test]
    fn rejects_non_http_downloads_and_sanitizes_paths() {
        assert!(managed_path("Game", "file:///tmp/a.wasm").is_err());
        let path = managed_path("../Game Name", "https://example.com/a.wasm").unwrap();
        assert_eq!(path.file_name().unwrap(), "a.wasm");
        assert!(path.to_string_lossy().contains("___Game_Name"));
    }

    #[test]
    fn strips_registry_html_for_native_dialogs() {
        assert_eq!(
            plain_text("Auto start.<br><b>By diggity</b>"),
            "Auto start. By diggity"
        );
    }
}
