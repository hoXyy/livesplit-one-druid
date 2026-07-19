use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use druid::{
    commands,
    lens::Identity,
    piet::{ImageBuf, ImageFormat, InterpolationMode},
    theme,
    widget::{
        Button, ClipBox, Container, Controller, CrossAxisAlignment, Either, Flex, Label, List,
        ListIter, Scroll, Switch, TextBox,
    },
    BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LensExt, LifeCycle, LifeCycleCtx,
    LinearGradient, Menu, MenuItem, PaintCtx, RenderContext, Selector, Size, Target, TextAlignment,
    UnitPoint, UpdateCtx, Widget, WidgetExt,
};
#[cfg(feature = "auto-splitting")]
use livesplit_core::auto_splitting::{settings::Map as AutoSplitterSettings, Runtime};
#[cfg(feature = "auto-splitting")]
use livesplit_core::SharedTimer;
use livesplit_core::{
    run::editor,
    settings::{Image, ImageCache, ImageId},
    RunEditor, TimeSpan, TimingMethod,
};

#[cfg(feature = "auto-splitting")]
use crate::config::AutoSplitterAssociation;
use crate::{
    config::Config,
    consts::{
        switch_style, ATTEMPTS_OFFSET_WIDTH, BUTTON_ACTIVE_BOTTOM, BUTTON_ACTIVE_TOP,
        BUTTON_BORDER, BUTTON_HEIGHT, BUTTON_SPACING, COLUMN_LABEL_FONT, DIALOG_BUTTON_HEIGHT,
        DIALOG_BUTTON_WIDTH, GRID_BORDER, ICON_SIZE, MARGIN, SPACING, TABLE_HORIZONTAL_MARGIN,
        TIME_COLUMN_WIDTH,
    },
    formatter_scope::{formatted, optional_time_span, validated, OnFocusLoss},
    speedrun_com, MainState,
};

#[cfg(feature = "auto-splitting")]
pub const OPEN_AUTOSPLITTER_SETTINGS: Selector =
    Selector::new("run-editor-open-autosplitter-settings");
#[cfg(feature = "auto-splitting")]
const SELECT_LOCAL_AUTOSPLITTER: Selector = Selector::new("run-editor-select-local-autosplitter");
#[cfg(feature = "auto-splitting")]
const LOCAL_AUTOSPLITTER_RESULT: Selector<druid::FileInfo> =
    Selector::new("run-editor-local-autosplitter-result");
#[cfg(feature = "auto-splitting")]
const REMOVE_AUTOSPLITTER: Selector = Selector::new("run-editor-remove-autosplitter");
#[cfg(feature = "auto-splitting")]
const REGISTRY_RESULT: Selector<(
    String,
    Option<crate::autosplitter_registry::Entry>,
    Option<String>,
)> = Selector::new("run-editor-autosplitter-registry-result");
#[cfg(feature = "auto-splitting")]
const REGISTRY_DOWNLOAD_RESULT: Selector<(
    crate::autosplitter_registry::Entry,
    Option<(PathBuf, PathBuf, String)>,
    Option<String>,
)> = Selector::new("run-editor-autosplitter-download-result");
#[cfg(feature = "auto-splitting")]
const REGISTRY_AVAILABILITY: Selector<(
    String,
    Option<crate::autosplitter_registry::Entry>,
    Option<String>,
)> = Selector::new("run-editor-autosplitter-registry-availability");

const GAME_RESULTS: Selector<(u64, String, Arc<[ApiGame]>, Option<String>)> =
    Selector::new("run-editor-src-game-results");
const GAME_DETAILS: Selector<(u64, String, Arc<ApiDetails>, Option<String>)> =
    Selector::new("run-editor-src-game-details");
const VARIABLE_RESULTS: Selector<(u64, String, Arc<[ApiVariable]>, Option<String>)> =
    Selector::new("run-editor-src-variable-results");
const SET_PLATFORM: Selector<String> = Selector::new("run-editor-src-set-platform");
const SET_REGION: Selector<String> = Selector::new("run-editor-src-set-region");
const SET_VARIABLE: Selector<(String, String)> = Selector::new("run-editor-src-set-variable");
const SELECT_GAME: Selector<usize> = Selector::new("run-editor-src-select-game");
const SELECT_CATEGORY: Selector<usize> = Selector::new("run-editor-src-select-category");
const REQUEST_SEGMENT_ICON: Selector<usize> = Selector::new("run-editor-request-segment-icon");
const SEGMENT_ICON_RESULT: Selector<(usize, PathBuf)> =
    Selector::new("run-editor-segment-icon-result");
const REMOVE_SEGMENT_ICON: Selector<usize> = Selector::new("run-editor-remove-segment-icon");
const REQUEST_GAME_ICON: Selector = Selector::new("run-editor-request-game-icon");
const GAME_ICON_RESULT: Selector<PathBuf> = Selector::new("run-editor-game-icon-result");
const REMOVE_GAME_ICON: Selector = Selector::new("run-editor-remove-game-icon");
static REQUEST_GENERATION: AtomicU64 = AtomicU64::new(0);

fn next_request_generation() -> u64 {
    REQUEST_GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

#[derive(Clone, Data)]
struct ApiGame {
    id: String,
    name: String,
}

#[derive(Clone, Data)]
struct ApiCategory {
    id: String,
    name: String,
    rules: String,
}

#[derive(Clone, Data)]
struct ApiVariable {
    name: String,
    values: Arc<[String]>,
    default: Option<String>,
    user_defined: bool,
    mandatory: bool,
    is_subcategory: bool,
}

#[derive(Clone, Data, Default)]
struct ApiDetails {
    categories: Arc<[ApiCategory]>,
    platforms: Arc<[String]>,
    regions: Arc<[String]>,
}

#[derive(Clone, Data, Default)]
struct ApiState {
    game_results: Arc<[ApiGame]>,
    details: Arc<ApiDetails>,
    variables: Arc<[ApiVariable]>,
    selected_game_id: String,
    selected_category_id: String,
    request_generation: u64,
    loading: bool,
    message: String,
    initial_lookup_started: bool,
    auto_select_game: bool,
    auto_select_category: bool,
}

struct SegmentWidget<T> {
    inner: T,
}

impl<T> SegmentWidget<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Widget<Segment>> Widget<Segment> for SegmentWidget<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Segment, env: &Env) {
        if let Event::MouseDown(event) = event {
            if !data.state.segments[data.index]
                .selected
                .is_selected_or_active()
            {
                ctx.request_focus();
                if event.mods.shift() {
                    data.select_range = true;
                } else if event.mods.ctrl() {
                    data.select_additionally = true;
                } else {
                    data.select_only = true;
                }
            } else if event.mods.ctrl() {
                data.unselect = true;
            }
        }
        self.inner.event(ctx, event, data, env)
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &Segment, env: &Env) {
        // if let &LifeCycle::FocusChanged(has_now_focus) = event {
        //     let is_selected = data.state.segments[data.index]
        //         .selected
        //         .is_selected_or_active();
        //     if has_now_focus && !is_selected {
        //         data.select = true;
        //     }
        // }
        self.inner.lifecycle(ctx, event, data, env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &Segment, data: &Segment, env: &Env) {
        // TODO: We honestly really only need to care about its selected state
        if !old_data.same(data) {
            ctx.request_paint();
        }
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &Segment,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Segment, env: &Env) {
        let rect = ctx.size().to_rect();
        if data.state.segments[data.index]
            .selected
            .is_selected_or_active()
        {
            ctx.fill(
                rect,
                &LinearGradient::new(
                    UnitPoint::TOP,
                    UnitPoint::BOTTOM,
                    (Color::rgb8(0x33, 0x73, 0xf4), Color::rgb8(0x15, 0x35, 0x74)),
                ),
            );
        } else {
            let color = if data.index & 1 == 0 {
                Color::grey8(0x12)
            } else {
                Color::grey8(0xb)
            };
            ctx.fill(rect, &color);
        }
        self.inner.paint(ctx, data, env)
    }
}

#[derive(Clone, Data)]
pub struct State {
    state: Rc<editor::State>,
    config: Rc<RefCell<Config>>,
    // image: Rc<ImageBuf>,
    #[data(ignore)]
    pub editor: Rc<RefCell<Option<RunEditor>>>,
    #[data(ignore)]
    pub closed_with_ok: bool,
    image_cache: Rc<RefCell<ImageCache>>,
    api: ApiState,
    additional_info_tab: bool,
    #[cfg(feature = "auto-splitting")]
    #[data(ignore)]
    runtime: Rc<Runtime<SharedTimer>>,
    #[cfg(feature = "auto-splitting")]
    #[data(ignore)]
    timer: SharedTimer,
    #[cfg(feature = "auto-splitting")]
    #[data(ignore)]
    original_auto_splitter: Option<AutoSplitterAssociation>,
    #[cfg(feature = "auto-splitting")]
    #[data(ignore)]
    pending_auto_splitter: Option<AutoSplitterAssociation>,
    #[cfg(feature = "auto-splitting")]
    #[data(ignore)]
    original_auto_splitter_settings: AutoSplitterSettings,
    #[cfg(feature = "auto-splitting")]
    auto_splitter_summary: String,
    #[cfg(feature = "auto-splitting")]
    registry_status: String,
}

impl State {
    pub fn new(
        editor: RunEditor,
        config: Rc<RefCell<Config>>,
        image_cache: Rc<RefCell<ImageCache>>,
        #[cfg(feature = "auto-splitting")] runtime: Rc<Runtime<SharedTimer>>,
        #[cfg(feature = "auto-splitting")] timer: SharedTimer,
    ) -> Self {
        let state =
            Rc::new(editor.state(&mut image_cache.borrow_mut(), livesplit_core::Lang::English));
        // let image = image::load_from_memory(state.icon_change.as_deref().unwrap())
        //     .unwrap()
        //     .into_rgba8();
        // let image = Rc::new(ImageBuf::from_raw(
        //     image.as_raw().as_slice(),
        //     ImageFormat::RgbaSeparate,
        //     image.width() as _,
        //     image.height() as _,
        // ));

        #[cfg(feature = "auto-splitting")]
        let original_auto_splitter = config.borrow().auto_splitter_association().cloned();
        #[cfg(feature = "auto-splitting")]
        let original_auto_splitter_settings = runtime.settings_map().unwrap_or_default();
        #[cfg(feature = "auto-splitting")]
        let auto_splitter_summary = association_summary(original_auto_splitter.as_ref());
        Self {
            state,
            config,
            // image,
            editor: Rc::new(RefCell::new(Some(editor))),
            closed_with_ok: false,
            image_cache,
            api: ApiState::default(),
            additional_info_tab: false,
            #[cfg(feature = "auto-splitting")]
            runtime,
            #[cfg(feature = "auto-splitting")]
            timer,
            #[cfg(feature = "auto-splitting")]
            pending_auto_splitter: original_auto_splitter.clone(),
            #[cfg(feature = "auto-splitting")]
            original_auto_splitter,
            #[cfg(feature = "auto-splitting")]
            original_auto_splitter_settings,
            #[cfg(feature = "auto-splitting")]
            auto_splitter_summary,
            #[cfg(feature = "auto-splitting")]
            registry_status: "Checking registry availability…".to_owned(),
        }
    }

    #[cfg(feature = "auto-splitting")]
    pub fn commit_auto_splitter(&self) {
        self.config
            .borrow_mut()
            .set_auto_splitter_association(self.pending_auto_splitter.clone());
    }

    #[cfg(feature = "auto-splitting")]
    pub fn revert_auto_splitter(&self) {
        // Always remove the pending module first. If restoration fails, this
        // guarantees that the newly selected module is not left active.
        let _ = self.runtime.unload();
        if let Some(original) = &self.original_auto_splitter {
            if let Err(error) = self
                .runtime
                .load(original.path().into(), self.timer.clone())
            {
                log::error!("Failed restoring Auto Splitter: {error}");
                return;
            }
            self.runtime
                .set_settings_map(self.original_auto_splitter_settings.clone());
        }
    }
}

#[cfg(feature = "auto-splitting")]
fn association_summary(association: Option<&AutoSplitterAssociation>) -> String {
    match association {
        Some(AutoSplitterAssociation::Local { path }) => {
            format!("Local — {}", path.display())
        }
        Some(AutoSplitterAssociation::Registry {
            game, cached_path, ..
        }) => {
            format!("Registry-managed ({game}) — {}", cached_path.display())
        }
        None => "None".to_owned(),
    }
}

#[cfg(feature = "auto-splitting")]
fn autosplitter_section() -> impl Widget<State> {
    let has_saved_path = |state: &State, _: &Env| state.config.borrow().splits_path().is_some();
    let has_module = |state: &State, _: &Env| state.pending_auto_splitter.is_some();
    Flex::column()
        .with_child(Label::new("Auto-splitter").with_text_size(16.0))
        .with_spacer(4.0)
        .with_child(Label::dynamic(|state: &State, _| {
            state.auto_splitter_summary.clone()
        }))
        .with_spacer(4.0)
        .with_child(
            Label::dynamic(|state: &State, _| state.registry_status.clone())
                .with_text_color(Color::grey8(0xb0)),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Flex::row()
                .with_child(
                    Button::new("Select / Change…")
                        .on_click(|ctx, state: &mut State, _| {
                            let game = state.state.game.clone();
                            let sink = ctx.get_external_handle();
                            std::thread::spawn(move || {
                                let result = crate::autosplitter_registry::load_cached_or_refresh()
                                    .map(|entries| {
                                        crate::autosplitter_registry::matching(&entries, &game)
                                            .cloned()
                                    });
                                let (entry, error) = match result {
                                    Ok(entry) => (entry, None),
                                    Err(error) => (None, Some(error.to_string())),
                                };
                                let _ = sink.submit_command(
                                    REGISTRY_RESULT,
                                    (game, entry, error),
                                    Target::Auto,
                                );
                            });
                        })
                        .disabled_if(move |state, env| !has_saved_path(state, env)),
                )
                .with_spacer(BUTTON_SPACING)
                .with_child(
                    Button::new("Settings…")
                        .on_click(|ctx, _, _| ctx.submit_command(OPEN_AUTOSPLITTER_SETTINGS))
                        .disabled_if(move |state, env| !has_module(state, env)),
                )
                .with_spacer(BUTTON_SPACING)
                .with_child(
                    Button::new("Remove")
                        .on_click(|ctx, _, _| ctx.submit_command(REMOVE_AUTOSPLITTER))
                        .disabled_if(move |state, env| !has_module(state, env)),
                ),
        )
        .padding(8.0)
        .border(BUTTON_BORDER, 1.0)
}

#[cfg(feature = "auto-splitting")]
struct AutoSplitterController;

#[cfg(feature = "auto-splitting")]
fn request_registry_availability(sink: druid::ExtEventSink, game: String) {
    std::thread::spawn(move || {
        let result = crate::autosplitter_registry::load_cached_or_refresh()
            .map(|entries| crate::autosplitter_registry::matching(&entries, &game).cloned());
        let (entry, error) = match result {
            Ok(entry) => (entry, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let _ = sink.submit_command(REGISTRY_AVAILABILITY, (game, entry, error), Target::Auto);
    });
}

#[cfg(feature = "auto-splitting")]
impl<W: Widget<State>> Controller<State, W> for AutoSplitterController {
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut State,
        env: &Env,
    ) {
        if let Event::Command(command) = event {
            if let Some((game, entry, error)) = command.get(REGISTRY_AVAILABILITY) {
                // Ignore stale responses after the game name has changed.
                if data.state.game == *game {
                    data.registry_status = match entry {
                        Some(entry) if entry.installable => format!(
                            "Registry auto-splitter available: {} ({})",
                            entry.game,
                            entry.compatibility.label()
                        ),
                        Some(entry) => format!(
                            "Registry entry available, but unsupported: {}",
                            entry.compatibility.label()
                        ),
                        None if error.is_some() => {
                            "Registry availability could not be checked.".to_owned()
                        }
                        None => "No registry auto-splitter found for this game.".to_owned(),
                    };
                }
                ctx.set_handled();
                return;
            }
            if command.is(SELECT_LOCAL_AUTOSPLITTER) {
                ctx.submit_command(
                    commands::SHOW_OPEN_PANEL.with(
                        druid::FileDialogOptions::new()
                            .title("Select Local WASM Auto-splitter")
                            .allowed_types(vec![druid::FileSpec {
                                name: "WASM Auto-splitters",
                                extensions: &["wasm"],
                            }])
                            .accept_command(LOCAL_AUTOSPLITTER_RESULT),
                    ),
                );
                return;
            }
            if let Some((game, entry, error)) = command.get(REGISTRY_RESULT) {
                let install = entry.as_ref().filter(|entry| entry.installable).and_then(|entry| {
                    native_dialog::DialogBuilder::message()
                        .set_title("Select Auto-splitter")
                        .set_text(&format!(
                            "{} — {}. {} Install this registry-managed auto-splitter? Choose No to select a local WASM file.",
                            entry.game,
                            entry.compatibility.label(),
                            entry.description
                        ))
                        .confirm()
                        .show()
                        .ok()
                        .filter(|answer| *answer)
                        .map(|_| entry.clone())
                });
                if let Some(entry) = install {
                    let sink = ctx.get_external_handle();
                    std::thread::spawn(move || {
                        let result = crate::autosplitter_registry::download_to_temporary(&entry);
                        let (download, error) = match result {
                            Ok(download) => (Some(download), None),
                            Err(error) => (None, Some(error.to_string())),
                        };
                        let _ = sink.submit_command(
                            REGISTRY_DOWNLOAD_RESULT,
                            (entry, download, error),
                            Target::Auto,
                        );
                    });
                } else {
                    if let Some(error) = error {
                        log::warn!("Could not refresh Auto Splitter registry: {error}");
                    } else if entry.is_none() {
                        log::info!("No registry Auto Splitter exactly matches {game}");
                    }
                    ctx.submit_command(SELECT_LOCAL_AUTOSPLITTER);
                }
                ctx.set_handled();
                return;
            }
            if let Some((entry, download, error)) = command.get(REGISTRY_DOWNLOAD_RESULT) {
                if let Some(error) = error {
                    crate::config::show_error(anyhow::anyhow!(
                        "Auto Splitter installation failed: {error}"
                    ));
                } else if data.timer.read().unwrap().current_phase()
                    != livesplit_core::TimerPhase::NotRunning
                {
                    log::info!("Discarding Auto Splitter update while timer is running");
                    if let Some((temporary, _, _)) = download {
                        let _ = std::fs::remove_file(temporary);
                    }
                } else if let Some((temporary, destination, url)) = download {
                    match data.runtime.load(temporary.clone(), data.timer.clone()) {
                        Ok(()) => {
                            if let Err(error) = std::fs::rename(&temporary, &destination) {
                                let _ = data.runtime.unload();
                                crate::config::show_error(anyhow::anyhow!(
                                    "Could not install Auto Splitter: {error}"
                                ));
                            } else {
                                data.pending_auto_splitter =
                                    Some(AutoSplitterAssociation::Registry {
                                        game: entry.game.clone(),
                                        last_url: url.clone(),
                                        cached_path: destination.clone(),
                                    });
                                data.auto_splitter_summary = format!(
                                    "{}\n{}\n{}",
                                    association_summary(data.pending_auto_splitter.as_ref()),
                                    entry.compatibility.label(),
                                    entry.description
                                );
                            }
                        }
                        Err(error) => {
                            let _ = std::fs::remove_file(temporary);
                            crate::config::show_error(anyhow::anyhow!(
                                "Downloaded Auto Splitter failed to load: {error}"
                            ));
                        }
                    }
                }
                ctx.set_handled();
                return;
            }
            if let Some(file) = command.get(LOCAL_AUTOSPLITTER_RESULT) {
                let association = AutoSplitterAssociation::Local {
                    path: file.path().to_path_buf(),
                };
                match data
                    .runtime
                    .load(association.path().into(), data.timer.clone())
                {
                    Ok(()) => {
                        data.pending_auto_splitter = Some(association);
                        data.auto_splitter_summary =
                            association_summary(data.pending_auto_splitter.as_ref());
                    }
                    Err(error) => crate::config::show_error(anyhow::anyhow!(
                        "Auto Splitter failed to load: {error}"
                    )),
                }
                ctx.set_handled();
                return;
            }
            if command.is(REMOVE_AUTOSPLITTER) {
                if let Err(error) = data.runtime.unload() {
                    crate::config::show_error(anyhow::anyhow!(
                        "Auto Splitter failed to unload: {error}"
                    ));
                } else {
                    data.pending_auto_splitter = None;
                    data.auto_splitter_summary = association_summary(None);
                }
                ctx.set_handled();
                return;
            }
        }
        child.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        child: &mut W,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &State,
        env: &Env,
    ) {
        if matches!(event, LifeCycle::WidgetAdded) {
            request_registry_availability(ctx.get_external_handle(), data.state.game.clone());
        }
        child.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        child: &mut W,
        ctx: &mut UpdateCtx,
        old_data: &State,
        data: &State,
        env: &Env,
    ) {
        if old_data.state.game != data.state.game {
            request_registry_availability(ctx.get_external_handle(), data.state.game.clone());
        }
        child.update(ctx, old_data, data, env);
    }
}

fn refresh(state: &mut State) {
    let mut editor = state.editor.borrow_mut();
    state.state = Rc::new(editor.as_mut().unwrap().state(
        &mut state.image_cache.borrow_mut(),
        livesplit_core::Lang::English,
    ));
    state.image_cache.borrow_mut().collect();
}

fn request_game_search(ctx: &mut EventCtx, state: &mut State, auto_select: bool) {
    let query = state.state.game.trim().to_owned();
    let generation = next_request_generation();
    state.api.request_generation = generation;
    state.api.selected_game_id.clear();
    state.api.selected_category_id.clear();
    state.api.game_results = Arc::new([]);
    state.api.details = Arc::new(ApiDetails::default());
    state.api.variables = Arc::new([]);
    state.api.auto_select_game = auto_select;
    state.api.auto_select_category = false;
    if query.len() < 2 {
        state.api.loading = false;
        state.api.message.clear();
        return;
    }
    state.api.loading = true;
    state.api.message = "Searching speedrun.com…".into();
    let sink = ctx.get_external_handle();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        if REQUEST_GENERATION.load(Ordering::Relaxed) != generation {
            return;
        }
        let result = speedrun_com::search_games(&query);
        let (games, error): (Arc<[ApiGame]>, Option<String>) = match result {
            Ok(games) => (
                games
                    .into_iter()
                    .map(|game| ApiGame {
                        id: game.id,
                        name: game.name,
                    })
                    .collect::<Vec<_>>()
                    .into(),
                None,
            ),
            Err(error) => (Arc::new([]), Some(error.to_string())),
        };
        let _ = sink.submit_command(
            GAME_RESULTS,
            (generation, query, games, error),
            Target::Auto,
        );
    });
}

fn select_game(ctx: &mut EventCtx, state: &mut State, index: usize) {
    let Some(game) = state.api.game_results.get(index).cloned() else {
        return;
    };
    state
        .editor
        .borrow_mut()
        .as_mut()
        .unwrap()
        .set_game_name(game.name.clone());
    refresh(state);
    let generation = next_request_generation();
    state.api.request_generation = generation;
    state.api.selected_game_id = game.id.clone();
    state.api.selected_category_id.clear();
    state.api.game_results = Arc::new([]);
    state.api.loading = true;
    state.api.message = "Loading categories and platforms…".into();
    let sink = ctx.get_external_handle();
    std::thread::spawn(move || {
        let result = (|| {
            let categories = speedrun_com::categories(&game.id)?
                .into_iter()
                .map(|category| ApiCategory {
                    id: category.id,
                    name: category.name,
                    rules: category.rules,
                })
                .collect::<Vec<_>>();
            let platforms = speedrun_com::platforms(&game.id)?
                .into_iter()
                .map(|choice| choice.name)
                .collect::<Vec<_>>();
            let regions = speedrun_com::regions(&game.id)?
                .into_iter()
                .map(|choice| choice.name)
                .collect::<Vec<_>>();
            Ok::<_, anyhow::Error>(ApiDetails {
                categories: categories.into(),
                platforms: platforms.into(),
                regions: regions.into(),
            })
        })();
        let (details, error) = match result {
            Ok(details) => (Arc::new(details), None),
            Err(error) => (Arc::new(ApiDetails::default()), Some(error.to_string())),
        };
        let _ = sink.submit_command(
            GAME_DETAILS,
            (generation, game.id, details, error),
            Target::Auto,
        );
    });
}

fn select_category(ctx: &mut EventCtx, state: &mut State, index: usize) {
    let Some(category) = state.api.details.categories.get(index).cloned() else {
        return;
    };
    state
        .editor
        .borrow_mut()
        .as_mut()
        .unwrap()
        .set_category_name(category.name.clone());
    refresh(state);
    let generation = next_request_generation();
    state.api.request_generation = generation;
    state.api.selected_category_id = category.id.clone();
    state.api.variables = Arc::new([]);
    state.api.loading = true;
    state.api.message = "Loading category options…".into();
    let sink = ctx.get_external_handle();
    std::thread::spawn(move || {
        let result = speedrun_com::variables(&category.id);
        let (variables, error): (Arc<[ApiVariable]>, Option<String>) = match result {
            Ok(variables) => (
                variables
                    .into_iter()
                    .map(|variable| ApiVariable {
                        name: variable.name,
                        values: variable.values.into(),
                        default: variable.default,
                        user_defined: variable.user_defined,
                        mandatory: variable.mandatory,
                        is_subcategory: variable.is_subcategory,
                    })
                    .collect::<Vec<_>>()
                    .into(),
                None,
            ),
            Err(error) => (Arc::new([]), Some(error.to_string())),
        };
        let _ = sink.submit_command(
            VARIABLE_RESULTS,
            (generation, category.id, variables, error),
            Target::Auto,
        );
    });
}

struct GameIcon {
    id: Option<ImageId>,
    image: Option<ImageBuf>,
}

impl GameIcon {
    fn new() -> Self {
        Self {
            id: None,
            image: None,
        }
    }

    fn update_image(&mut self, data: &State) {
        let id = data.state.icon;
        if self.id == Some(id) {
            return;
        }
        self.id = Some(id);
        self.image = if id.is_empty() {
            None
        } else {
            data.image_cache
                .borrow()
                .lookup(&id)
                .and_then(|image| image::load_from_memory(image.data()).ok())
                .map(|image| image.into_rgba8())
                .map(|image| {
                    let (width, height) = image.dimensions();
                    ImageBuf::from_raw(
                        image.into_raw(),
                        ImageFormat::RgbaSeparate,
                        width as usize,
                        height as usize,
                    )
                })
        };
    }
}

impl Widget<State> for GameIcon {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, _env: &Env) {
        if let Event::MouseDown(mouse) = event {
            if mouse.button == druid::MouseButton::Left && mouse.count == 2 {
                ctx.submit_command(REQUEST_GAME_ICON);
                ctx.set_handled();
            } else if mouse.button == druid::MouseButton::Right {
                let has_icon = !data.state.icon.is_empty();
                let mut menu = Menu::new("Game Icon").entry(
                    MenuItem::new(if has_icon {
                        "Replace Icon…"
                    } else {
                        "Set Icon…"
                    })
                    .command(REQUEST_GAME_ICON),
                );
                if has_icon {
                    menu = menu.entry(MenuItem::new("Remove Icon").command(REMOVE_GAME_ICON));
                }
                ctx.show_context_menu::<MainState>(menu, mouse.window_pos);
                ctx.set_handled();
            }
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &State,
        _env: &Env,
    ) {
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old: &State, data: &State, _env: &Env) {
        if old.state.icon != data.state.icon {
            self.id = None;
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &State,
        _env: &Env,
    ) -> Size {
        bc.constrain(Size::new(ICON_SIZE, ICON_SIZE))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &State, env: &Env) {
        self.update_image(data);
        let rect = ctx.size().to_rect();
        let bounds = rect.inset(-BUTTON_SPACING);
        ctx.stroke(rect, &BUTTON_BORDER, 1.0);
        if let Some(image) = &self.image {
            let piet_image = image.to_image(ctx.render_ctx);
            let image_size = image.size();
            let scale = (bounds.width() / image_size.width)
                .min(bounds.height() / image_size.height)
                .min(1.0);
            let size = image_size * scale;
            let target = size.to_rect().with_origin(druid::Point::new(
                0.5 * (ctx.size().width - size.width),
                0.5 * (ctx.size().height - size.height),
            ));
            ctx.draw_image(&piet_image, target, InterpolationMode::Bilinear);
        } else {
            let center = ctx.size().to_rect().center();
            let color = env.get(theme::PLACEHOLDER_COLOR);
            ctx.stroke(
                druid::kurbo::Line::new((center.x - 8.0, center.y), (center.x + 8.0, center.y)),
                &color,
                1.0,
            );
            ctx.stroke(
                druid::kurbo::Line::new((center.x, center.y - 8.0), (center.x, center.y + 8.0)),
                &color,
                1.0,
            );
        }
    }
}

fn game_icon() -> impl Widget<State> {
    GameIcon::new().fix_size(ICON_SIZE, ICON_SIZE)
}

fn game_name() -> impl Widget<State> {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new("Game"))
        .with_spacer(BUTTON_SPACING)
        .with_child(
            TextBox::new()
                .lens(Identity.map(
                    |state: &State| state.state.game.clone(),
                    |state: &mut State, name: String| {
                        let mut editor = state.editor.borrow_mut();
                        let editor = editor.as_mut().unwrap();
                        editor.set_game_name(name);
                        state.state = Rc::new(editor.state(
                            &mut state.image_cache.borrow_mut(),
                            livesplit_core::Lang::English,
                        ));
                        state.image_cache.borrow_mut().collect();
                    },
                ))
                .controller(GameSearchController)
                .expand_width(),
        )
}

struct GameSearchController;

impl<W: Widget<State>> Controller<State, W> for GameSearchController {
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut State,
        env: &Env,
    ) {
        child.event(ctx, event, data, env);
        if matches!(event, Event::KeyUp(_)) {
            request_game_search(ctx, data, false);
        }
    }
}

fn category_name() -> impl Widget<State> {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new("Category"))
        .with_spacer(BUTTON_SPACING)
        .with_child(
            TextBox::new()
                .lens(Identity.map(
                    |state: &State| state.state.category.clone(),
                    |state: &mut State, name: String| {
                        let mut editor = state.editor.borrow_mut();
                        let editor = editor.as_mut().unwrap();
                        editor.set_category_name(name);
                        state.state = Rc::new(editor.state(
                            &mut state.image_cache.borrow_mut(),
                            livesplit_core::Lang::English,
                        ));
                        state.image_cache.borrow_mut().collect();
                    },
                ))
                .expand_width(),
        )
}

fn offset() -> impl Widget<State> {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new("Start Timer at").align_right())
        .with_spacer(BUTTON_SPACING)
        .with_child(
            validated(
                TextBox::new().with_text_alignment(TextAlignment::End),
                |offset| TimeSpan::parse(offset, livesplit_core::Lang::English).is_ok(),
            )
            .lens(Identity.map(
                |state: &State| state.state.offset.clone(),
                |state: &mut State, value: String| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    let _ = editor.parse_and_set_offset(&value, livesplit_core::Lang::English);
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                },
            ))
            .expand_width(),
        )
}

fn attempts() -> impl Widget<State> {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new("Attempts").align_right())
        .with_spacer(BUTTON_SPACING)
        .with_child(
            formatted(
                TextBox::new().with_text_alignment(TextAlignment::End),
                |buf, val| {
                    use std::fmt::Write;
                    let _ = write!(buf, "{}", val);
                },
                |val| val.parse().ok(),
            )
            .lens(Identity.map(
                |state: &State| state.state.attempts,
                |state: &mut State, value: u32| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    editor.set_attempt_count(value);
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                },
            ))
            .expand_width(),
        )
}

fn header() -> impl Widget<State> {
    Flex::column()
        .with_child(
            Flex::row()
                .with_flex_child(game_name(), 2.0)
                .with_spacer(SPACING)
                .with_child(offset().fix_width(ATTEMPTS_OFFSET_WIDTH)),
        )
        .with_spacer(SPACING)
        .with_child(
            Flex::row()
                .with_flex_child(category_name(), 2.0)
                .with_spacer(SPACING)
                .with_child(attempts().fix_width(ATTEMPTS_OFFSET_WIDTH)),
        )
        .with_child(game_suggestions())
        .with_child(category_suggestions())
}

fn game_suggestions() -> impl Widget<State> {
    Either::new(
        |state: &State, _| !state.api.game_results.is_empty(),
        Button::new(|state: &State, _env: &Env| {
            format!(
                "Select matching game… ({} found)",
                state.api.game_results.len()
            )
        })
        .on_click(|ctx, state: &mut State, _| {
            let mut menu = Menu::new("Matching Games");
            for (index, game) in state.api.game_results.iter().enumerate() {
                menu = menu.entry(
                    MenuItem::new(game.name.clone()).command(druid::Command::new(
                        SELECT_GAME,
                        index,
                        Target::Auto,
                    )),
                );
            }
            ctx.show_context_menu::<MainState>(
                menu,
                ctx.to_window(druid::Point::new(0.0, ctx.size().height)),
            );
        })
        .expand_width()
        .padding((0.0, BUTTON_SPACING, 0.0, 0.0)),
        Container::new(Label::new("")),
    )
}

fn category_suggestions() -> impl Widget<State> {
    Either::new(
        |state: &State, _| {
            !state.api.details.categories.is_empty() && state.api.selected_category_id.is_empty()
        },
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Label::new("Category on speedrun.com"))
            .with_child(
                Button::new(|state: &State, _env: &Env| {
                    format!(
                        "Select leaderboard category… ({} found)",
                        state.api.details.categories.len()
                    )
                })
                .on_click(|ctx, state: &mut State, _| {
                    let mut menu = Menu::new("Categories");
                    for (index, category) in state.api.details.categories.iter().enumerate() {
                        menu =
                            menu.entry(MenuItem::new(category.name.clone()).command(
                                druid::Command::new(SELECT_CATEGORY, index, Target::Auto),
                            ));
                    }
                    ctx.show_context_menu::<MainState>(
                        menu,
                        ctx.to_window(druid::Point::new(0.0, ctx.size().height)),
                    );
                })
                .expand_width(),
            ),
        Container::new(Label::new("")),
    )
}

fn side_buttons() -> impl Widget<State> {
    Flex::column()
        .with_child(
            Button::new("Insert Above")
                .on_click(|_, state: &mut State, _| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    editor.insert_segment_above();
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                })
                .expand_width()
                .fix_height(BUTTON_HEIGHT),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Button::new("Insert Below")
                .on_click(|_, state: &mut State, _| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    editor.insert_segment_below();
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                })
                .expand_width()
                .fix_height(BUTTON_HEIGHT),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Button::new("Remove Segment")
                .on_click(|_, state: &mut State, _| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    editor.remove_segments();
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                })
                .expand_width()
                .fix_height(BUTTON_HEIGHT),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Button::new("Move Up")
                .on_click(|_, state: &mut State, _| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    editor.move_segments_up();
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                })
                .expand_width()
                .fix_height(BUTTON_HEIGHT),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Button::new("Move Down")
                .on_click(|_, state: &mut State, _| {
                    let mut editor = state.editor.borrow_mut();
                    let editor = editor.as_mut().unwrap();
                    editor.move_segments_down();
                    state.state = Rc::new(editor.state(
                        &mut state.image_cache.borrow_mut(),
                        livesplit_core::Lang::English,
                    ));
                    state.image_cache.borrow_mut().collect();
                })
                .expand_width()
                .fix_height(BUTTON_HEIGHT),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(OtherButtonWidget::new(
            Button::new("Other...")
                .expand_width()
                .fix_height(BUTTON_HEIGHT),
        ))
}

impl ListIter<Segment> for State {
    fn for_each(&self, mut cb: impl FnMut(&Segment, usize)) {
        let mut segment = Segment {
            index: 0,
            state: self.state.clone(),
            image_cache: self.image_cache.clone(),
            new_name: None,
            new_split_time: None,
            new_segment_time: None,
            new_best_segment_time: None,
            select_only: false,
            select_additionally: false,
            select_range: false,
            unselect: false,
        };
        for index in 0..self.data_len() {
            segment.index = index;
            cb(&segment, index);
        }
    }

    fn for_each_mut(&mut self, mut cb: impl FnMut(&mut Segment, usize)) {
        let mut segment = Segment {
            index: 0,
            state: self.state.clone(),
            image_cache: self.image_cache.clone(),
            new_name: None,
            new_split_time: None,
            new_segment_time: None,
            new_best_segment_time: None,
            select_only: false,
            select_additionally: false,
            select_range: false,
            unselect: false,
        };
        let mut editor = self.editor.borrow_mut();
        let editor = editor.as_mut().unwrap();
        let mut changed = false;

        for index in 0..self.data_len() {
            segment.index = index;
            cb(&mut segment, index);
            if let Some(new_name) = segment.new_name.take() {
                editor.select_only(index);
                editor.active_segment().set_name(new_name);
                changed = true;
            }
            if let Some(new_split_time) = segment.new_split_time.take() {
                editor.select_only(index);
                let _ = editor
                    .active_segment()
                    .parse_and_set_split_time(&new_split_time, livesplit_core::Lang::English);
                changed = true;
            }
            if let Some(new_segment_time) = segment.new_segment_time.take() {
                editor.select_only(index);
                let _ = editor
                    .active_segment()
                    .parse_and_set_segment_time(&new_segment_time, livesplit_core::Lang::English);
                changed = true;
            }
            if let Some(new_best_segment_time) = segment.new_best_segment_time.take() {
                editor.select_only(index);
                let _ = editor.active_segment().parse_and_set_best_segment_time(
                    &new_best_segment_time,
                    livesplit_core::Lang::English,
                );
                changed = true;
            }
            if segment.select_only {
                editor.select_only(index);
                segment.select_only = false;
                changed = true;
            }
            if segment.select_additionally {
                editor.select_additionally(index);
                segment.select_additionally = false;
                changed = true;
            }
            if segment.select_range {
                editor.select_range(index);
                segment.select_range = false;
                changed = true;
            }
            if segment.unselect {
                editor.unselect(index);
                segment.unselect = false;
                changed = true;
            }
        }

        if changed {
            self.state = Rc::new(editor.state(
                &mut self.image_cache.borrow_mut(),
                livesplit_core::Lang::English,
            ));
        }
        self.image_cache.borrow_mut().collect();
    }

    fn data_len(&self) -> usize {
        self.state.segments.len()
    }
}

#[derive(Clone, Data)]
struct Segment {
    index: usize,
    state: Rc<editor::State>,
    #[data(ignore)]
    image_cache: Rc<RefCell<ImageCache>>,
    new_name: Option<String>,
    new_split_time: Option<String>,
    new_segment_time: Option<String>,
    new_best_segment_time: Option<String>,
    select_only: bool,
    select_additionally: bool,
    select_range: bool,
    unselect: bool,
}

struct SegmentIcon {
    id: Option<ImageId>,
    image: Option<ImageBuf>,
}

impl SegmentIcon {
    fn new() -> Self {
        Self {
            id: None,
            image: None,
        }
    }

    fn update_image(&mut self, data: &Segment) {
        let id = data.state.segments[data.index].icon;
        if self.id == Some(id) {
            return;
        }
        self.id = Some(id);
        self.image = if id.is_empty() {
            None
        } else {
            data.image_cache
                .borrow()
                .lookup(&id)
                .and_then(|image| image::load_from_memory(image.data()).ok())
                .map(|image| image.into_rgba8())
                .map(|image| {
                    let (width, height) = image.dimensions();
                    ImageBuf::from_raw(
                        image.into_raw(),
                        ImageFormat::RgbaSeparate,
                        width as usize,
                        height as usize,
                    )
                })
        };
    }
}

impl Widget<Segment> for SegmentIcon {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Segment, _env: &Env) {
        if let Event::MouseDown(mouse) = event {
            if mouse.button == druid::MouseButton::Left && mouse.count == 2 {
                ctx.submit_command(REQUEST_SEGMENT_ICON.with(data.index));
                ctx.set_handled();
            } else if mouse.button == druid::MouseButton::Right {
                let has_icon = !data.state.segments[data.index].icon.is_empty();
                let mut menu = Menu::new("Segment Icon").entry(
                    MenuItem::new(if has_icon {
                        "Replace Icon…"
                    } else {
                        "Set Icon…"
                    })
                    .command(REQUEST_SEGMENT_ICON.with(data.index)),
                );
                if has_icon {
                    menu = menu.entry(
                        MenuItem::new("Remove Icon").command(REMOVE_SEGMENT_ICON.with(data.index)),
                    );
                }
                ctx.show_context_menu::<MainState>(menu, mouse.window_pos);
                ctx.set_handled();
            }
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &Segment,
        _env: &Env,
    ) {
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old: &Segment, data: &Segment, _env: &Env) {
        if old.state.segments[old.index].icon != data.state.segments[data.index].icon {
            self.id = None;
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &Segment,
        _env: &Env,
    ) -> Size {
        bc.constrain(Size::new(50.0, 25.0))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Segment, env: &Env) {
        self.update_image(data);
        let bounds = ctx.size().to_rect().inset(-3.0);
        if let Some(image) = &self.image {
            let piet_image = image.to_image(ctx.render_ctx);
            let image_size = image.size();
            let scale = (bounds.width() / image_size.width)
                .min(bounds.height() / image_size.height)
                .min(1.0);
            let size = image_size * scale;
            let target = size.to_rect().with_origin(druid::Point::new(
                0.5 * (ctx.size().width - size.width),
                0.5 * (ctx.size().height - size.height),
            ));
            ctx.draw_image(&piet_image, target, InterpolationMode::Bilinear);
        } else {
            let center = ctx.size().to_rect().center();
            let color = env.get(theme::PLACEHOLDER_COLOR);
            ctx.stroke(
                druid::kurbo::Line::new((center.x - 4.0, center.y), (center.x + 4.0, center.y)),
                &color,
                1.0,
            );
            ctx.stroke(
                druid::kurbo::Line::new((center.x, center.y - 4.0), (center.x, center.y + 4.0)),
                &color,
                1.0,
            );
        }
    }
}

fn segments() -> impl Widget<State> {
    Flex::column()
        .with_child(
            Flex::row()
                .with_spacer(TABLE_HORIZONTAL_MARGIN)
                .with_child(
                    Label::new("Icon")
                        .with_font(COLUMN_LABEL_FONT)
                        .center()
                        .fix_width(50.0),
                )
                .with_spacer(GRID_BORDER)
                .with_flex_child(
                    ClipBox::unmanaged(Label::new("Segment Name").with_font(COLUMN_LABEL_FONT))
                        .expand_width(),
                    1.0,
                )
                .with_spacer(GRID_BORDER)
                .with_child(
                    ClipBox::unmanaged(Label::new("Split Time").with_font(COLUMN_LABEL_FONT))
                        .align_right()
                        .fix_width(TIME_COLUMN_WIDTH),
                )
                .with_spacer(GRID_BORDER)
                .with_child(
                    ClipBox::unmanaged(Label::new("Segment Time").with_font(COLUMN_LABEL_FONT))
                        .align_right()
                        .fix_width(TIME_COLUMN_WIDTH),
                )
                .with_spacer(GRID_BORDER)
                .with_child(
                    ClipBox::unmanaged(Label::new("Best Segment").with_font(COLUMN_LABEL_FONT))
                        .align_right()
                        .fix_width(TIME_COLUMN_WIDTH),
                )
                .with_spacer(TABLE_HORIZONTAL_MARGIN)
                .fix_height(26.0)
                .border(BUTTON_BORDER, 1.0),
        )
        // .with_spacer(GRID_BORDER)
        .with_flex_child(
            Scroll::new(
                List::new(|| {
                    SegmentWidget::new(
                        Flex::row()
                            .with_spacer(TABLE_HORIZONTAL_MARGIN)
                            .with_child(SegmentIcon::new().fix_width(50.0))
                            .with_spacer(GRID_BORDER)
                            .with_flex_child(
                                TextBox::new()
                                    .lens(Identity.map(
                                        |s: &Segment| s.state.segments[s.index].name.clone(),
                                        |state: &mut Segment, name: String| {
                                            if name != state.state.segments[state.index].name {
                                                state.new_name = Some(name);
                                            }
                                        },
                                    ))
                                    .expand_width(),
                                1.0,
                            )
                            .with_spacer(GRID_BORDER)
                            .with_child(
                                OnFocusLoss::new(optional_time_span(
                                    TextBox::new().with_text_alignment(TextAlignment::End),
                                ))
                                .lens(Identity.map(
                                    |s: &Segment| s.state.segments[s.index].split_time.clone(),
                                    |state: &mut Segment, split_time: String| {
                                        if split_time
                                            != state.state.segments[state.index].split_time
                                        {
                                            state.new_split_time = Some(split_time);
                                        }
                                    },
                                ))
                                .fix_width(TIME_COLUMN_WIDTH),
                            )
                            .with_spacer(GRID_BORDER)
                            .with_child(
                                OnFocusLoss::new(optional_time_span(
                                    TextBox::new().with_text_alignment(TextAlignment::End),
                                ))
                                .lens(Identity.map(
                                    |s: &Segment| s.state.segments[s.index].segment_time.clone(),
                                    |state: &mut Segment, segment_time: String| {
                                        if segment_time
                                            != state.state.segments[state.index].segment_time
                                        {
                                            state.new_segment_time = Some(segment_time);
                                        }
                                    },
                                ))
                                .fix_width(TIME_COLUMN_WIDTH),
                            )
                            .with_spacer(GRID_BORDER)
                            .with_child(
                                OnFocusLoss::new(optional_time_span(
                                    TextBox::new().with_text_alignment(TextAlignment::End),
                                ))
                                .lens(Identity.map(
                                    |s: &Segment| {
                                        s.state.segments[s.index].best_segment_time.clone()
                                    },
                                    |state: &mut Segment, best_segment_time: String| {
                                        if best_segment_time
                                            != state.state.segments[state.index].best_segment_time
                                        {
                                            state.new_best_segment_time = Some(best_segment_time);
                                        }
                                    },
                                ))
                                .fix_width(TIME_COLUMN_WIDTH),
                            )
                            .with_spacer(TABLE_HORIZONTAL_MARGIN),
                    )
                })
                .border(BUTTON_BORDER, 1.0),
            )
            .vertical(),
            1.0,
        )
        .env_scope(|env, _| {
            env.set(theme::TEXTBOX_BORDER_RADIUS, 0.0);
            env.set(theme::TEXTBOX_BORDER_WIDTH, 0.0);
            env.set(theme::BACKGROUND_LIGHT, Color::rgba8(0, 0, 0, 0));
        })
}

fn tabs() -> impl Widget<State> {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(
            Flex::row()
                .with_child(
                    Button::new("Real Time")
                        .on_click(|_, state: &mut State, _| {
                            let mut editor = state.editor.borrow_mut();
                            let editor = editor.as_mut().unwrap();
                            editor.select_timing_method(TimingMethod::RealTime);
                            state.state = Rc::new(editor.state(
                                &mut state.image_cache.borrow_mut(),
                                livesplit_core::Lang::English,
                            ));
                            state.image_cache.borrow_mut().collect();
                        })
                        .env_scope(|env, data: &State| {
                            if data.state.timing_method == TimingMethod::RealTime {
                                env.set(theme::BUTTON_LIGHT, BUTTON_ACTIVE_TOP);
                                env.set(theme::BUTTON_DARK, BUTTON_ACTIVE_BOTTOM);
                            }
                        }),
                )
                .with_child(
                    Button::new("Game Time")
                        .on_click(|_, state: &mut State, _| {
                            let mut editor = state.editor.borrow_mut();
                            let editor = editor.as_mut().unwrap();
                            editor.select_timing_method(TimingMethod::GameTime);
                            state.state = Rc::new(editor.state(
                                &mut state.image_cache.borrow_mut(),
                                livesplit_core::Lang::English,
                            ));
                            state.image_cache.borrow_mut().collect();
                        })
                        .env_scope(|env, data: &State| {
                            if data.state.timing_method == TimingMethod::GameTime {
                                env.set(theme::BUTTON_LIGHT, BUTTON_ACTIVE_TOP);
                                env.set(theme::BUTTON_DARK, BUTTON_ACTIVE_BOTTOM);
                            }
                        }),
                )
                .env_scope(|env, _| {
                    env.set(theme::BUTTON_BORDER_RADIUS, 0.0);
                }),
        )
        .with_flex_child(segments(), 1.0)
}

fn body() -> impl Widget<State> {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(side_buttons().fix_width(ICON_SIZE))
        .with_spacer(SPACING)
        .with_flex_child(tabs(), 1.0)
}

fn metadata_text_field(
    label: &'static str,
    getter: fn(&State) -> String,
    setter: fn(&mut RunEditor, String),
) -> impl Widget<State> {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new(label))
        .with_spacer(BUTTON_SPACING)
        .with_child(
            TextBox::new()
                .lens(Identity.map(getter, move |state: &mut State, value| {
                    setter(state.editor.borrow_mut().as_mut().unwrap(), value);
                    refresh(state);
                }))
                .expand_width(),
        )
}

fn platform_field() -> impl Widget<State> {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::End)
        .with_flex_child(
            metadata_text_field(
                "Platform",
                |state| state.state.metadata.platform_name.clone(),
                |editor, value| editor.set_platform_name(value),
            ),
            1.0,
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Button::new("Choose…").on_click(|ctx, state: &mut State, _| {
                let mut menu = Menu::new("Platform");
                for platform in state.api.details.platforms.iter() {
                    menu = menu.entry(MenuItem::new(platform.clone()).command(
                        druid::Command::new(SET_PLATFORM, platform.clone(), Target::Auto),
                    ));
                }
                ctx.show_context_menu::<MainState>(
                    menu,
                    ctx.to_window(druid::Point::new(0.0, ctx.size().height)),
                );
            }),
        )
}

fn region_field() -> impl Widget<State> {
    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::End)
        .with_flex_child(
            metadata_text_field(
                "Region",
                |state| state.state.metadata.region_name.clone(),
                |editor, value| editor.set_region_name(value),
            ),
            1.0,
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Button::new("Choose…").on_click(|ctx, state: &mut State, _| {
                let mut menu = Menu::new("Region");
                for region in state.api.details.regions.iter() {
                    menu = menu.entry(MenuItem::new(region.clone()).command(druid::Command::new(
                        SET_REGION,
                        region.clone(),
                        Target::Auto,
                    )));
                }
                ctx.show_context_menu::<MainState>(
                    menu,
                    ctx.to_window(druid::Point::new(0.0, ctx.size().height)),
                );
            }),
        )
}

fn variable_row(index: usize) -> impl Widget<State> {
    Either::new(
        move |state: &State, _| state.api.variables.get(index).is_some(),
        Flex::row()
            .with_child(
                Label::new(move |state: &State, _env: &Env| {
                    state
                        .api
                        .variables
                        .get(index)
                        .map(|variable| {
                            let suffix = if variable.is_subcategory {
                                " (subcategory)"
                            } else if variable.mandatory {
                                " (required)"
                            } else {
                                ""
                            };
                            format!("{}{}", variable.name, suffix)
                        })
                        .unwrap_or_default()
                })
                .fix_width(190.0),
            )
            .with_flex_child(
                TextBox::new()
                    .lens(Identity.map(
                        move |state: &State| {
                            let Some(variable) = state.api.variables.get(index) else {
                                return String::new();
                            };
                            state
                                .state
                                .metadata
                                .speedrun_com_variables()
                                .find(|(name, _)| *name == variable.name.as_str())
                                .map(|(_, value)| value.clone())
                                .or_else(|| variable.default.clone())
                                .unwrap_or_default()
                        },
                        move |state: &mut State, value| {
                            let Some(variable) = state.api.variables.get(index) else {
                                return;
                            };
                            let name = variable.name.clone();
                            state
                                .editor
                                .borrow_mut()
                                .as_mut()
                                .unwrap()
                                .set_speedrun_com_variable(name, value);
                            refresh(state);
                        },
                    ))
                    .expand_width(),
                1.0,
            )
            .with_spacer(BUTTON_SPACING)
            .with_child(
                Button::new("Choose…").on_click(move |ctx, state: &mut State, _| {
                    let Some(variable) = state.api.variables.get(index) else {
                        return;
                    };
                    let mut menu = Menu::new(variable.name.clone());
                    for value in variable.values.iter() {
                        menu =
                            menu.entry(MenuItem::new(value.clone()).command(druid::Command::new(
                                SET_VARIABLE,
                                (variable.name.clone(), value.clone()),
                                Target::Auto,
                            )));
                    }
                    ctx.show_context_menu::<MainState>(
                        menu,
                        ctx.to_window(druid::Point::new(0.0, ctx.size().height)),
                    );
                }),
            ),
        Container::new(Label::new("")),
    )
}

fn additional_info() -> impl Widget<State> {
    let mut variables = Flex::column();
    for index in 0..16 {
        variables.add_child(variable_row(index));
        variables.add_spacer(BUTTON_SPACING);
    }

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Label::new("Additional Info").with_text_size(15.0))
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Flex::row()
                .with_flex_child(platform_field(), 1.0)
                .with_spacer(SPACING)
                .with_flex_child(region_field(), 1.0)
                .with_spacer(SPACING)
                .with_child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_child(Label::new("Emulator"))
                        .with_spacer(BUTTON_SPACING)
                        .with_child(
                            Switch::new()
                                .lens(Identity.map(
                                    |state: &State| state.state.metadata.uses_emulator,
                                    |state: &mut State, value| {
                                        state
                                            .editor
                                            .borrow_mut()
                                            .as_mut()
                                            .unwrap()
                                            .set_emulator_usage(value);
                                        refresh(state);
                                    },
                                ))
                                .env_scope(|env, _| switch_style(env)),
                        ),
                ),
        )
        .with_spacer(BUTTON_SPACING)
        .with_child(
            Scroll::new(variables)
                .vertical()
                .fix_height(140.0)
                .expand_width(),
        )
        .with_child(Label::new(|state: &State, _env: &Env| {
            state.api.message.clone()
        }))
        .padding(BUTTON_SPACING)
        .border(BUTTON_BORDER, 1.0)
}

fn run_editor() -> impl Widget<State> {
    Flex::column()
        .with_child(
            Flex::row()
                .with_child(game_icon())
                .with_spacer(SPACING)
                .with_flex_child(header(), 1.0),
        )
        .with_spacer(SPACING)
        .with_child(
            Flex::row()
                .with_child(
                    Button::new("Splits")
                        .on_click(|_, state: &mut State, _| state.additional_info_tab = false)
                        .env_scope(|env, state| {
                            if !state.additional_info_tab {
                                env.set(theme::BUTTON_LIGHT, BUTTON_ACTIVE_TOP);
                                env.set(theme::BUTTON_DARK, BUTTON_ACTIVE_BOTTOM);
                            }
                        }),
                )
                .with_child(
                    Button::new("Additional Info")
                        .on_click(|_, state: &mut State, _| state.additional_info_tab = true)
                        .env_scope(|env, state| {
                            if state.additional_info_tab {
                                env.set(theme::BUTTON_LIGHT, BUTTON_ACTIVE_TOP);
                                env.set(theme::BUTTON_DARK, BUTTON_ACTIVE_BOTTOM);
                            }
                        }),
                )
                .env_scope(|env, _| env.set(theme::BUTTON_BORDER_RADIUS, 0.0)),
        )
        .with_flex_child(
            Either::new(
                |state: &State, _| state.additional_info_tab,
                additional_info().expand(),
                body().expand(),
            ),
            1.0,
        )
}

struct SpeedrunComController;

impl<W: Widget<State>> Controller<State, W> for SpeedrunComController {
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut State,
        env: &Env,
    ) {
        if matches!(event, Event::WindowConnected) && !data.api.initial_lookup_started {
            data.api.initial_lookup_started = true;
            if data.state.game.trim().len() >= 2 {
                request_game_search(ctx, data, true);
            }
        }
        if let Event::Command(command) = event {
            if command.is(REQUEST_GAME_ICON) {
                let sink = ctx.get_external_handle();
                std::thread::spawn(move || {
                    let dialog = native_dialog::DialogBuilder::file();
                    #[cfg(not(target_os = "macos"))]
                    let dialog = dialog
                        .add_filter(
                            "Images",
                            &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico"],
                        )
                        .add_filter("All Files", &["*"]);
                    if let Ok(Some(path)) = dialog.open_single_file().show() {
                        let _ = sink.submit_command(GAME_ICON_RESULT, path, Target::Auto);
                    }
                });
                ctx.set_handled();
                return;
            }
            if let Some(path) = command.get(GAME_ICON_RESULT) {
                let mut bytes = Vec::new();
                if let Ok(image) = Image::from_file(path, &mut bytes, Image::ICON) {
                    {
                        data.editor
                            .borrow_mut()
                            .as_mut()
                            .unwrap()
                            .set_game_icon(image);
                    }
                    refresh(data);
                }
                ctx.set_handled();
                return;
            }
            if command.is(REMOVE_GAME_ICON) {
                {
                    data.editor
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .remove_game_icon();
                }
                refresh(data);
                ctx.set_handled();
                return;
            }
            if let Some(index) = command.get(REQUEST_SEGMENT_ICON) {
                let index = *index;
                let sink = ctx.get_external_handle();
                std::thread::spawn(move || {
                    let dialog = native_dialog::DialogBuilder::file();
                    #[cfg(not(target_os = "macos"))]
                    let dialog = dialog
                        .add_filter(
                            "Images",
                            &["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico"],
                        )
                        .add_filter("All Files", &["*"]);
                    if let Ok(Some(path)) = dialog.open_single_file().show() {
                        let _ =
                            sink.submit_command(SEGMENT_ICON_RESULT, (index, path), Target::Auto);
                    }
                });
                ctx.set_handled();
                return;
            }
            if let Some((index, path)) = command.get(SEGMENT_ICON_RESULT) {
                if *index < data.state.segments.len() {
                    let mut bytes = Vec::new();
                    if let Ok(image) = Image::from_file(path, &mut bytes, Image::ICON) {
                        {
                            let mut editor = data.editor.borrow_mut();
                            let editor = editor.as_mut().unwrap();
                            editor.select_only(*index);
                            editor.active_segment().set_icon(image);
                        }
                        refresh(data);
                    }
                }
                ctx.set_handled();
                return;
            }
            if let Some(index) = command.get(REMOVE_SEGMENT_ICON) {
                if *index < data.state.segments.len() {
                    {
                        let mut editor = data.editor.borrow_mut();
                        let editor = editor.as_mut().unwrap();
                        editor.select_only(*index);
                        editor.active_segment().remove_icon();
                    }
                    refresh(data);
                }
                ctx.set_handled();
                return;
            }
            if let Some(index) = command.get(SELECT_GAME) {
                select_game(ctx, data, *index);
                return;
            }
            if let Some(index) = command.get(SELECT_CATEGORY) {
                select_category(ctx, data, *index);
                return;
            }
            if let Some((generation, query, games, error)) = command.get(GAME_RESULTS) {
                if *generation == data.api.request_generation && *query == data.state.game {
                    data.api.game_results = games.clone();
                    data.api.loading = false;
                    data.api.message = error
                        .as_ref()
                        .map(|error| format!("speedrun.com: {error}"))
                        .unwrap_or_else(|| {
                            if games.is_empty() {
                                "No speedrun.com games found.".into()
                            } else {
                                "Select a matching game.".into()
                            }
                        });
                    if data.api.auto_select_game {
                        data.api.auto_select_game = false;
                        if let Some(index) = games
                            .iter()
                            .position(|game| game.name.eq_ignore_ascii_case(query))
                        {
                            select_game(ctx, data, index);
                            data.api.auto_select_category = true;
                        }
                    }
                }
                return;
            }
            if let Some((generation, game_id, details, error)) = command.get(GAME_DETAILS) {
                if *generation == data.api.request_generation
                    && *game_id == data.api.selected_game_id
                {
                    data.api.details = details.clone();
                    data.api.loading = false;
                    data.api.message = error
                        .as_ref()
                        .map(|error| format!("speedrun.com: {error}"))
                        .unwrap_or_else(|| "Select a leaderboard category.".into());
                    if data.api.auto_select_category {
                        data.api.auto_select_category = false;
                        let category_name = data.state.category.clone();
                        if let Some(index) = details
                            .categories
                            .iter()
                            .position(|category| category.name.eq_ignore_ascii_case(&category_name))
                        {
                            select_category(ctx, data, index);
                        }
                    }
                }
                return;
            }
            if let Some((generation, category_id, variables, error)) = command.get(VARIABLE_RESULTS)
            {
                if *generation == data.api.request_generation
                    && *category_id == data.api.selected_category_id
                {
                    data.api.variables = variables.clone();
                    data.api.loading = false;
                    data.api.message = error
                        .as_ref()
                        .map(|error| format!("speedrun.com: {error}"))
                        .unwrap_or_else(|| "Leaderboard metadata loaded.".into());
                }
                return;
            }
            if let Some(platform) = command.get(SET_PLATFORM) {
                data.editor
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .set_platform_name(platform.clone());
                refresh(data);
                return;
            }
            if let Some(region) = command.get(SET_REGION) {
                data.editor
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .set_region_name(region.clone());
                refresh(data);
                return;
            }
            if let Some((name, value)) = command.get(SET_VARIABLE) {
                data.editor
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .set_speedrun_com_variable(name.clone(), value.clone());
                refresh(data);
                return;
            }
        }
        child.event(ctx, event, data, env);
    }
}

struct RunEditorWidget<T> {
    inner: T,
}

impl<T> RunEditorWidget<T> {
    #[allow(dead_code)]
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Widget<Option<State>>> Widget<Option<State>> for RunEditorWidget<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Option<State>, env: &Env) {
        self.inner.event(ctx, event, data, env)
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &Option<State>,
        env: &Env,
    ) {
        self.inner.lifecycle(ctx, event, data, env)
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &Option<State>,
        data: &Option<State>,
        env: &Env,
    ) {
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &Option<State>,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Option<State>, env: &Env) {
        self.inner.paint(ctx, data, env)
    }
}

struct LinkLayout<W>(W);

impl<W: Widget<bool>> Widget<State> for LinkLayout<W> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        let had_linked_layout = has_linked_layout(data);
        let mut has_linked_layout = had_linked_layout;

        self.0.event(ctx, event, &mut has_linked_layout, env);

        if has_linked_layout && !had_linked_layout {
            data.config
                .borrow()
                .link_layout(data.editor.borrow_mut().as_mut().unwrap());
        } else if !has_linked_layout && had_linked_layout {
            let mut editor = data.editor.borrow_mut();
            let editor = editor.as_mut().unwrap();
            editor.set_linked_layout(None);
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &State, env: &Env) {
        self.0.lifecycle(ctx, event, &has_linked_layout(data), env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &State, data: &State, env: &Env) {
        self.0.update(
            ctx,
            &has_linked_layout(old_data),
            &has_linked_layout(data),
            env,
        )
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        self.0.layout(ctx, bc, &has_linked_layout(data), env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &State, env: &Env) {
        self.0.paint(ctx, &has_linked_layout(data), env)
    }
}

fn has_linked_layout(data: &State) -> bool {
    data.editor
        .borrow()
        .as_ref()
        .unwrap()
        .run()
        .linked_layout()
        .is_some()
}

fn link_layout_toggle() -> impl Widget<State> {
    LinkLayout(Switch::new().env_scope(|env, _| switch_style(env)))
}

pub fn root_widget() -> impl Widget<State> {
    let content = Flex::column().with_flex_child(run_editor(), 1.0);
    #[cfg(feature = "auto-splitting")]
    let content = content.with_child(autosplitter_section());
    let content = content
        .with_spacer(MARGIN)
        .with_child(
            Flex::row()
                .with_child(link_layout_toggle())
                .with_spacer(BUTTON_SPACING)
                .with_child(Label::new("Link Layout"))
                .with_flex_spacer(1.0)
                .with_child(
                    Button::new("OK")
                        .on_click(|ctx, state: &mut State, _| {
                            state.closed_with_ok = true;
                            ctx.submit_command(commands::CLOSE_WINDOW);
                        })
                        .fix_size(DIALOG_BUTTON_WIDTH, DIALOG_BUTTON_HEIGHT),
                )
                .with_spacer(BUTTON_SPACING)
                .with_child(
                    Button::new("Cancel")
                        .on_click(|ctx, _state, _| {
                            ctx.submit_command(commands::CLOSE_WINDOW);
                        })
                        .fix_size(DIALOG_BUTTON_WIDTH, DIALOG_BUTTON_HEIGHT),
                ),
        )
        .padding(MARGIN)
        .controller(SpeedrunComController);
    #[cfg(feature = "auto-splitting")]
    let content = content.controller(AutoSplitterController);
    content
}

struct OtherButtonWidget<T> {
    inner: T,
}

impl<T> OtherButtonWidget<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

const CLEAR_HISTORY: Selector = Selector::new("run-editor-clear-history");
const CLEAR_TIMES: Selector = Selector::new("run-editor-clear-times");
const CLEAN_SUM_OF_BEST: Selector = Selector::new("run-editor-clean-sum-of-best");
const GENERATE_GOAL_COMPARISON: Selector = Selector::new("run-editor-generate-goal-comparison");

impl<T: Widget<State>> Widget<State> for OtherButtonWidget<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        if let Event::MouseDown(event) = event {
            ctx.show_context_menu::<MainState>(
                Menu::new("Other")
                    .entry(MenuItem::new("Clear History").command(CLEAR_HISTORY))
                    .entry(MenuItem::new("Clear Times").command(CLEAR_TIMES))
                    .entry(
                        MenuItem::new("Clean Sum of Best")
                            .command(CLEAN_SUM_OF_BEST)
                            .enabled(false),
                    )
                    .entry(
                        MenuItem::new("Generate Goal Comparison")
                            .command(GENERATE_GOAL_COMPARISON)
                            .enabled(false),
                    ),
                event.window_pos,
            );
            return;
        } else if let Event::Command(command) = event {
            if command.is(CLEAR_HISTORY) {
                let mut editor = data.editor.borrow_mut();
                let editor = editor.as_mut().unwrap();
                editor.clear_history();
                data.state = Rc::new(editor.state(
                    &mut data.image_cache.borrow_mut(),
                    livesplit_core::Lang::English,
                ));
            } else if command.is(CLEAR_TIMES) {
                let mut editor = data.editor.borrow_mut();
                let editor = editor.as_mut().unwrap();
                editor.clear_times();
                data.state = Rc::new(editor.state(
                    &mut data.image_cache.borrow_mut(),
                    livesplit_core::Lang::English,
                ));
            }
        }
        data.image_cache.borrow_mut().collect();
        self.inner.event(ctx, event, data, env)
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &State, env: &Env) {
        self.inner.lifecycle(ctx, event, data, env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &State, data: &State, env: &Env) {
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &State, env: &Env) {
        self.inner.paint(ctx, data, env)
    }
}
