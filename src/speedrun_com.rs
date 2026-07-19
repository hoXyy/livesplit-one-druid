use std::{collections::HashMap, sync::Mutex, time::Duration};

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde_json::Value;

const API: &str = "https://www.speedrun.com/api/v1";

static GAME_SEARCH_CACHE: Lazy<Mutex<HashMap<String, Vec<Game>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static CATEGORY_CACHE: Lazy<Mutex<HashMap<String, Vec<Category>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PLATFORM_CACHE: Lazy<Mutex<HashMap<String, Vec<Choice>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static REGION_CACHE: Lazy<Mutex<HashMap<String, Vec<Choice>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static VARIABLE_CACHE: Lazy<Mutex<HashMap<String, Vec<Variable>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
pub struct Game {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub rules: String,
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Variable {
    pub name: String,
    pub values: Vec<String>,
    pub default: Option<String>,
    pub user_defined: bool,
    pub mandatory: bool,
    pub is_subcategory: bool,
}

fn get(path: &str, query: &[(&str, &str)]) -> Result<Value> {
    let mut request = ureq::get(&format!("{API}/{path}"))
        .timeout(Duration::from_secs(10))
        .set("User-Agent", "LiveSplit-One-Druid/0.7");
    for (key, value) in query {
        request = request.query(key, value);
    }
    let response = request.call().context("speedrun.com request failed")?;
    serde_json::from_reader(response.into_reader()).context("invalid speedrun.com response")
}

pub fn search_games(query: &str) -> Result<Vec<Game>> {
    let cache_key = query.trim().to_lowercase();
    if let Some(cached) = GAME_SEARCH_CACHE.lock().unwrap().get(&cache_key).cloned() {
        return Ok(cached);
    }
    let root = get("games", &[("name", query), ("max", "8")])?;
    let games = root["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|game| {
            Some(Game {
                id: game["id"].as_str()?.to_owned(),
                name: game["names"]["international"].as_str()?.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    GAME_SEARCH_CACHE
        .lock()
        .unwrap()
        .insert(cache_key, games.clone());
    Ok(games)
}

pub fn categories(game_id: &str) -> Result<Vec<Category>> {
    if let Some(cached) = CATEGORY_CACHE.lock().unwrap().get(game_id).cloned() {
        return Ok(cached);
    }
    let root = get(&format!("games/{game_id}/categories"), &[])?;
    let categories = root["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|category| category["type"].as_str() == Some("per-game"))
        .filter_map(|category| {
            Some(Category {
                id: category["id"].as_str()?.to_owned(),
                name: category["name"].as_str()?.to_owned(),
                rules: category["rules"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect::<Vec<_>>();
    CATEGORY_CACHE
        .lock()
        .unwrap()
        .insert(game_id.to_owned(), categories.clone());
    Ok(categories)
}

fn embedded_choices(
    game_id: &str,
    resource: &str,
    cache: &Mutex<HashMap<String, Vec<Choice>>>,
) -> Result<Vec<Choice>> {
    if let Some(cached) = cache.lock().unwrap().get(game_id).cloned() {
        return Ok(cached);
    }
    let root = get(&format!("games/{game_id}"), &[("embed", resource)])?;
    let choices = root["data"][resource]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Choice {
                name: item["name"].as_str()?.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    cache
        .lock()
        .unwrap()
        .insert(game_id.to_owned(), choices.clone());
    Ok(choices)
}

pub fn platforms(game_id: &str) -> Result<Vec<Choice>> {
    embedded_choices(game_id, "platforms", &PLATFORM_CACHE)
}

pub fn regions(game_id: &str) -> Result<Vec<Choice>> {
    embedded_choices(game_id, "regions", &REGION_CACHE)
}

pub fn variables(category_id: &str) -> Result<Vec<Variable>> {
    if let Some(cached) = VARIABLE_CACHE.lock().unwrap().get(category_id).cloned() {
        return Ok(cached);
    }
    let root = get(&format!("categories/{category_id}/variables"), &[])?;
    let variables = root["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|variable| {
            let values = variable["values"]["values"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, value)| value["label"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let default_id = variable["values"]["default"].as_str();
            let default = default_id.and_then(|id| {
                variable["values"]["values"][id]["label"]
                    .as_str()
                    .map(str::to_owned)
            });
            Some(Variable {
                name: variable["name"].as_str()?.to_owned(),
                values,
                default,
                user_defined: variable["user-defined"].as_bool().unwrap_or(false),
                mandatory: variable["mandatory"].as_bool().unwrap_or(false),
                is_subcategory: variable["is-subcategory"].as_bool().unwrap_or(false),
            })
        })
        .collect::<Vec<_>>();
    VARIABLE_CACHE
        .lock()
        .unwrap()
        .insert(category_id.to_owned(), variables.clone());
    Ok(variables)
}
