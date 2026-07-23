#![allow(dead_code)]

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

use adw::prelude::*;
use gtk::{gdk, glib};
use livesplit_core::{
    layout::{Layout, LayoutState},
    rendering::software::Renderer,
    settings::ImageCache,
    HotkeySystem, SharedTimer, Timer,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

#[cfg(feature = "auto-splitting")]
use crate::config::AutoSplitterAssociation;
use crate::{
    config::Config,
    platform::{Platform, TimerWindowPlatform},
};

#[derive(Clone)]
pub struct LayoutDraft(Arc<Mutex<Option<Layout>>>);

#[derive(Clone)]
pub struct RunDraft(Arc<Mutex<Option<livesplit_core::Run>>>);

impl std::fmt::Debug for RunDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RunDraft(..)")
    }
}

impl std::fmt::Debug for LayoutDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LayoutDraft(..)")
    }
}

pub struct LayoutData {
    pub layout: Layout,
    pub layout_state: LayoutState,
    pub is_modified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorKind {
    Run,
    Layout,
    Window,
    Hotkeys,
    Notes,
    NotesViewer,
    #[cfg(feature = "auto-splitting")]
    AutoSplitter,
}

#[derive(Default)]
pub struct OpenEditors(HashMap<EditorKind, adw::ApplicationWindow>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Intent {
    #[default]
    None,
    NewSplits,
    OpenSplits,
    Exit,
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    NewSplits,
    OpenSplits(PathBuf),
    NewLayout,
    OpenLayout(PathBuf),
    Exit,
}

fn relevant_changes(action: &PendingAction, splits: bool, layout: bool) -> (bool, bool) {
    match action {
        PendingAction::NewSplits => (splits, false),
        PendingAction::OpenSplits(_) | PendingAction::Exit => (splits, layout),
        PendingAction::NewLayout | PendingAction::OpenLayout(_) => (false, layout),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TimerCommand {
    StartOrSplit,
    Reset,
    Pause,
    UndoSplit,
    SkipSplit,
    UndoAllPauses,
}

#[derive(Clone, Debug)]
pub enum ResetAction {
    ResetOnly,
    SaveSplits,
    SaveSplitsAs(PathBuf),
}

#[derive(Clone, Copy)]
enum FileChoice {
    OpenSplits,
    SaveSplits,
    OpenLayout,
    SaveLayout,
}

#[derive(Clone, Copy)]
enum DirectAction {
    NewSplits,
    SaveSplits,
    NewLayout,
    SaveLayout,
}

#[derive(Debug)]
pub enum AppMsg {
    RenderTick,
    TimerCommand(TimerCommand),
    ResetConfirmationFinished(bool, ResetAction),
    SaveSplitsConfirmed,
    SaveSplitsAsConfirmed(PathBuf),
    Scroll(f64),
    SetComparison(String),
    SetTimingMethod(livesplit_core::TimingMethod),
    WorldRecordResponse {
        index: usize,
        url: String,
        body: Option<String>,
    },
    BeginIntent(Intent),
    OpenEditor(EditorKind),
    EditorFinished(EditorKind),
    MainWindowConfigured,
    MainWindowCloseRequested,
    RequestAction(PendingAction),
    ExecuteAction(PendingAction),
    SaveBeforeAction(PendingAction),
    SaveSplitsAsThen(PathBuf, PendingAction),
    SaveLayoutAsThen(PathBuf, PendingAction),
    NewSplits,
    OpenSplits(PathBuf),
    SaveSplits,
    SaveSplitsAs(PathBuf),
    NewLayout,
    OpenLayout(PathBuf),
    SaveLayout,
    SaveLayoutAs(PathBuf),
    ApplyWindowSettings(bool),
    ApplyLayout(LayoutDraft),
    ApplyRun(RunDraft),
    ApplyHotkeys(livesplit_core::HotkeyConfig),
    #[cfg(feature = "auto-splitting")]
    ApplyAutoSplitterAssociation(Option<AutoSplitterAssociation>),
}

pub struct AppModel {
    pub timer: SharedTimer,
    pub layout: RefCell<LayoutData>,
    pub config: RefCell<Config>,
    pub image_cache: Rc<RefCell<ImageCache>>,
    pub hotkey_system: Option<HotkeySystem<SharedTimer>>,
    pub render_size: Cell<(u32, u32)>,
    pub open_editors: OpenEditors,
    pub pending_intent: Intent,
    pub pending_world_records: HashMap<usize, Instant>,
    active_layout_editor: Option<Rc<RefCell<Option<livesplit_core::LayoutEditor>>>>,
    active_run_editor: Option<Rc<RefCell<Option<livesplit_core::RunEditor>>>>,
    notes_viewer: Option<NotesViewerUi>,
    #[cfg(feature = "auto-splitting")]
    pub auto_splitter: Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
    renderer: RefCell<Renderer>,
    picture: gtk::Picture,
    platform: Platform,
    window: gtk::ApplicationWindow,
    closing_allowed: Rc<Cell<bool>>,
    mouse_passthrough: Cell<bool>,
}

struct NotesViewerUi {
    state: crate::notes::NotesViewer,
    title: gtk::Label,
    buffer: gtk::TextBuffer,
}

#[derive(Clone)]
#[allow(deprecated)]
struct CompletionEntryRow {
    row: adw::ActionRow,
    entry: gtk::Entry,
    combo: gtk::ComboBoxText,
}

#[allow(deprecated)]
impl CompletionEntryRow {
    fn new(title: &str, text: &str) -> Self {
        let row = adw::ActionRow::builder().title(title).build();
        let combo = gtk::ComboBoxText::with_entry();
        combo.set_hexpand(true);
        combo.set_valign(gtk::Align::Center);
        combo.set_size_request(320, -1);
        combo.add_css_class("completion-combo");
        let entry = combo
            .child()
            .and_then(|child| child.downcast::<gtk::Entry>().ok())
            .expect("editable combo box contains a GTK entry");
        entry.set_text(text);
        entry.set_width_chars(28);
        entry.set_valign(gtk::Align::Center);
        row.add_suffix(&combo);
        row.set_activatable_widget(Some(&entry));
        Self { row, entry, combo }
    }

    fn connect_changed(&self, callback: impl Fn(&gtk::Entry) + 'static) {
        self.entry.connect_changed(callback);
    }

    fn set_text(&self, text: &str) {
        self.entry.set_text(text);
    }

    fn text(&self) -> glib::GString {
        self.entry.text()
    }

    fn add_suffix(&self, widget: &impl IsA<gtk::Widget>) {
        self.row.add_suffix(widget);
    }

    fn set_tooltip_text(&self, text: Option<&str>) {
        self.entry.set_tooltip_text(text);
    }

    fn clear_items(&self) {
        self.combo.remove_all();
    }

    fn set_items(&self, items: &[&str]) {
        self.combo.remove_all();
        for item in items {
            self.combo.append_text(item);
        }
    }

    fn connect_selected(&self, callback: impl Fn(usize) + 'static) {
        self.combo.connect_changed(move |combo| {
            let index = combo.property::<i32>("active");
            if index >= 0 {
                callback(index as usize);
            }
        });
    }

    fn select(&self, index: usize) {
        self.combo.set_property("active", index as i32);
    }

    fn show_dropdown(&self) {
        let active_window = self
            .entry
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
            .is_some_and(|window| window.is_active());
        if active_window
            && self.entry.has_focus()
            && self
                .combo
                .model()
                .is_some_and(|model| model.iter_n_children(None) > 0)
        {
            self.combo.popup();
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = Config;
    type Input = AppMsg;
    type Output = ();

    view! {
        #[root]
        main_window = gtk::ApplicationWindow {
            set_title: Some("LiveSplit One"),
            set_default_width: width,
            set_default_height: height,
            set_decorated: false,
            set_resizable: true,
            add_css_class: "timer-window",

            #[name(picture)]
            gtk::Picture {
                set_can_shrink: true,
                set_content_fit: gtk::ContentFit::Fill,
            },
        }
    }

    fn init(
        mut config: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        config.setup_logging();
        let run = config.parse_run_or_default();
        let mut timer = Timer::new(run).expect("the default run contains a segment");
        config.configure_timer(&mut timer);
        let layout = config.parse_layout_or_default(&timer);
        let timer = timer.into_shared();
        #[cfg(feature = "auto-splitting")]
        let auto_splitter = Rc::new(livesplit_core::auto_splitting::Runtime::new());
        #[cfg(feature = "auto-splitting")]
        config.maybe_load_auto_splitter(&auto_splitter, timer.clone());
        let hotkey_system = Some(config.configure_hotkeys(timer.clone()));
        let (window_width, window_height) = config.window_size();
        let width = window_width.round() as i32;
        let height = window_height.round() as i32;
        let widgets = view_output!();
        let picture = widgets.picture.clone();
        let platform = Platform::detect();
        platform.configure(&root, &config);

        install_timer_interactions(&root, &picture, &timer, &config, platform, &sender);
        let closing_allowed = Rc::new(Cell::new(false));
        let close_guard = closing_allowed.clone();
        let close_sender = sender.clone();
        root.connect_close_request(move |_| {
            if close_guard.get() {
                glib::Propagation::Proceed
            } else {
                close_sender.input(AppMsg::RequestAction(PendingAction::Exit));
                glib::Propagation::Stop
            }
        });

        let model = AppModel {
            timer: timer.clone(),
            layout: RefCell::new(LayoutData {
                layout,
                layout_state: LayoutState::default(),
                is_modified: false,
            }),
            config: RefCell::new(config),
            image_cache: Rc::new(RefCell::new(ImageCache::new())),
            hotkey_system,
            render_size: Cell::new((width as u32, height as u32)),
            open_editors: OpenEditors::default(),
            pending_intent: Intent::None,
            pending_world_records: HashMap::new(),
            active_layout_editor: None,
            active_run_editor: None,
            notes_viewer: None,
            #[cfg(feature = "auto-splitting")]
            auto_splitter,
            renderer: RefCell::new(Renderer::new()),
            picture,
            platform,
            window: root,
            closing_allowed,
            mouse_passthrough: Cell::new(false),
        };

        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            match sender.input_sender().send(AppMsg::RenderTick) {
                Ok(()) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::RenderTick => {
                self.poll_world_records(&sender);
                self.update_notes_viewer();
                self.update_mouse_passthrough();
                self.render();
            }
            AppMsg::TimerCommand(command) => {
                if matches!(command, TimerCommand::Reset) {
                    self.request_reset(ResetAction::ResetOnly, &sender);
                    return;
                }
                let mut timer = self.timer.write().unwrap();
                match command {
                    TimerCommand::StartOrSplit => drop(timer.split_or_start()),
                    TimerCommand::Reset => unreachable!(),
                    TimerCommand::Pause => drop(timer.toggle_pause_or_start()),
                    TimerCommand::UndoSplit => drop(timer.undo_split()),
                    TimerCommand::SkipSplit => drop(timer.skip_split()),
                    TimerCommand::UndoAllPauses => drop(timer.undo_all_pauses()),
                }
            }
            AppMsg::ResetConfirmationFinished(update_times, action) => {
                let _ = self.timer.write().unwrap().reset(update_times);
                sender.input(match action {
                    ResetAction::ResetOnly => return,
                    ResetAction::SaveSplits => AppMsg::SaveSplitsConfirmed,
                    ResetAction::SaveSplitsAs(path) => AppMsg::SaveSplitsAsConfirmed(path),
                });
            }
            AppMsg::Scroll(delta) => {
                scroll_layout(&self.layout, delta);
            }
            AppMsg::SetComparison(comparison) => {
                if self
                    .timer
                    .write()
                    .unwrap()
                    .set_current_comparison(comparison.as_str())
                    .is_ok()
                {
                    self.config.borrow_mut().set_comparison(comparison);
                }
            }
            AppMsg::SetTimingMethod(method) => {
                self.timer
                    .write()
                    .unwrap()
                    .set_current_timing_method(method);
                self.config.borrow_mut().set_timing_method(method);
            }
            AppMsg::WorldRecordResponse { index, url, body } => {
                if let Some(body) = body {
                    self.pending_world_records.remove(&index);
                    let timer = self.timer.read().unwrap();
                    let snapshot = timer.snapshot();
                    let mut layout = self.layout.borrow_mut();
                    let still_current = layout
                        .layout
                        .world_record_request_urls(&snapshot)
                        .into_iter()
                        .any(|(candidate_index, candidate_url)| {
                            candidate_index == index && candidate_url == url
                        });
                    if still_current {
                        layout.layout.world_record_parse_response(index, &body);
                    }
                }
            }
            AppMsg::BeginIntent(intent) => self.pending_intent = intent,
            AppMsg::EditorFinished(kind) => {
                self.open_editors.0.remove(&kind);
                if kind == EditorKind::Layout {
                    self.active_layout_editor = None;
                } else if kind == EditorKind::Run {
                    self.active_run_editor = None;
                } else if kind == EditorKind::Window {
                    if let Some(system) = self.hotkey_system.as_mut() {
                        let _ = system.activate();
                    }
                } else if kind == EditorKind::NotesViewer {
                    self.notes_viewer = None;
                }
            }
            AppMsg::OpenEditor(kind) => match kind {
                EditorKind::Layout => self.open_layout_editor(&sender),
                EditorKind::Run => self.open_run_editor(&sender),
                EditorKind::Window => self.open_settings_editor(&sender),
                EditorKind::Notes => self.open_notes_editor(&sender),
                EditorKind::NotesViewer => self.open_notes_viewer(&sender),
                _ => {}
            },
            AppMsg::MainWindowConfigured => {}
            AppMsg::MainWindowCloseRequested => self.pending_intent = Intent::Exit,
            AppMsg::RequestAction(action) => self.request_action(action, &sender),
            AppMsg::ExecuteAction(action) => self.execute_action(action, &sender),
            AppMsg::SaveBeforeAction(action) => self.save_before_action(action, &sender),
            AppMsg::SaveSplitsAsThen(path, action) => {
                let result = self.config.borrow_mut().save_splits_as(
                    &mut self.timer.write().unwrap(),
                    #[cfg(feature = "auto-splitting")]
                    &self.auto_splitter,
                    path,
                );
                if result.is_ok() {
                    self.set_notes_actions_enabled(true);
                    sender.input(AppMsg::SaveBeforeAction(action));
                }
                crate::config::or_show_error(result);
            }
            AppMsg::SaveLayoutAsThen(path, action) => {
                let settings = self.layout.borrow().layout.settings();
                let result = self.config.borrow_mut().save_layout_as(
                    &mut self.timer.write().unwrap(),
                    settings,
                    path,
                );
                if result.is_ok() {
                    self.layout.borrow_mut().is_modified = false;
                    sender.input(AppMsg::SaveBeforeAction(action));
                }
                crate::config::or_show_error(result);
            }
            AppMsg::NewSplits => {
                self.close_notes_windows();
                self.config
                    .borrow_mut()
                    .new_splits(&mut self.timer.write().unwrap());
                self.set_notes_actions_enabled(false);
            }
            AppMsg::OpenSplits(path) => {
                let result = self.config.borrow_mut().open_splits(
                    &self.timer,
                    &mut self.layout.borrow_mut(),
                    #[cfg(feature = "auto-splitting")]
                    &self.auto_splitter,
                    path,
                );
                let opened = result.is_ok();
                crate::config::or_show_error(result);
                if opened {
                    self.close_notes_windows();
                }
                self.set_notes_actions_enabled(self.config.borrow().splits_path().is_some());
            }
            AppMsg::SaveSplits => {
                self.request_reset(ResetAction::SaveSplits, &sender);
            }
            AppMsg::SaveSplitsConfirmed => {
                if self.config.borrow().can_directly_save_splits() {
                    let result = self.config.borrow_mut().save_splits(
                        &mut self.timer.write().unwrap(),
                        #[cfg(feature = "auto-splitting")]
                        &self.auto_splitter,
                    );
                    crate::config::or_show_error(result);
                } else {
                    select_file(&self.window, true, FileChoice::SaveSplits, &sender);
                }
            }
            AppMsg::SaveSplitsAs(path) => {
                self.request_reset(ResetAction::SaveSplitsAs(path), &sender);
            }
            AppMsg::SaveSplitsAsConfirmed(path) => {
                let result = self.config.borrow_mut().save_splits_as(
                    &mut self.timer.write().unwrap(),
                    #[cfg(feature = "auto-splitting")]
                    &self.auto_splitter,
                    path,
                );
                if result.is_ok() {
                    self.set_notes_actions_enabled(true);
                }
                crate::config::or_show_error(result);
            }
            AppMsg::NewLayout => self.config.borrow_mut().new_layout(
                Some(&mut self.timer.write().unwrap()),
                &mut self.layout.borrow_mut(),
            ),
            AppMsg::OpenLayout(path) => {
                let result = self.config.borrow_mut().open_layout(
                    Some(&mut self.timer.write().unwrap()),
                    &mut self.layout.borrow_mut(),
                    &path,
                );
                crate::config::or_show_error(result);
            }
            AppMsg::SaveLayout => {
                if self.config.borrow().can_directly_save_layout() {
                    let settings = self.layout.borrow().layout.settings();
                    let result = self.config.borrow().save_layout(settings);
                    if result.is_ok() {
                        self.layout.borrow_mut().is_modified = false;
                    }
                    crate::config::or_show_error(result);
                } else {
                    select_file(&self.window, true, FileChoice::SaveLayout, &sender);
                }
            }
            AppMsg::SaveLayoutAs(path) => {
                let settings = self.layout.borrow().layout.settings();
                let result = self.config.borrow_mut().save_layout_as(
                    &mut self.timer.write().unwrap(),
                    settings,
                    path,
                );
                if result.is_ok() {
                    self.layout.borrow_mut().is_modified = false;
                }
                crate::config::or_show_error(result);
            }
            AppMsg::ApplyWindowSettings(enabled) => {
                self.config
                    .borrow_mut()
                    .set_mouse_pass_through_while_running(enabled);
            }
            AppMsg::ApplyLayout(draft) => {
                if let Some(layout) = draft.0.lock().unwrap().take() {
                    let mut layout_data = self.layout.borrow_mut();
                    layout_data.layout = layout;
                    layout_data.is_modified = true;
                }
            }
            AppMsg::ApplyRun(draft) => {
                if let Some(run) = draft.0.lock().unwrap().take() {
                    let _ = self.timer.write().unwrap().set_run(run);
                }
            }
            AppMsg::ApplyHotkeys(config) => {
                if let Some(system) = self.hotkey_system.as_mut() {
                    let _ = system.set_config(config);
                }
                self.config.borrow_mut().set_hotkeys(config);
            }
            #[cfg(feature = "auto-splitting")]
            AppMsg::ApplyAutoSplitterAssociation(association) => {
                self.config
                    .borrow_mut()
                    .set_auto_splitter_association(association);
            }
        }
    }
}

fn scroll_layout(layout_data: &RefCell<LayoutData>, delta: f64) {
    let mut layout_data = layout_data.borrow_mut();
    if delta > 0.0 {
        layout_data.layout.scroll_down();
    } else if delta < 0.0 {
        layout_data.layout.scroll_up();
    }
}

fn should_mouse_passthrough(
    configured: bool,
    phase: livesplit_core::TimerPhase,
    window_active: bool,
) -> bool {
    configured && phase == livesplit_core::TimerPhase::Running && !window_active
}

fn physical_to_logical_size(size: [f32; 2], scale: u32) -> (i32, i32) {
    let scale = scale.max(1) as f32;
    (
        (size[0] / scale).ceil().max(1.0) as i32,
        (size[1] / scale).ceil().max(1.0) as i32,
    )
}

impl AppModel {
    fn request_reset(&self, action: ResetAction, sender: &ComponentSender<Self>) {
        if !self
            .timer
            .read()
            .unwrap()
            .current_attempt_has_new_best_times()
        {
            sender.input(AppMsg::ResetConfirmationFinished(true, action));
            return;
        }

        let dialog = gtk::AlertDialog::builder()
            .message("Update your best times?")
            .detail("This attempt contains new best times. Should they be saved when resetting?")
            .modal(true)
            .build();
        dialog.set_buttons(&["Cancel", "Don't Update", "Update Times"]);
        dialog.set_cancel_button(0);
        dialog.set_default_button(2);
        let sender = sender.clone();
        dialog.choose(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |response| match response.ok() {
                Some(1) => sender.input(AppMsg::ResetConfirmationFinished(false, action.clone())),
                Some(2) => sender.input(AppMsg::ResetConfirmationFinished(true, action)),
                _ => {}
            },
        );
    }

    fn update_mouse_passthrough(&self) {
        let enabled = should_mouse_passthrough(
            self.config.borrow().get_mouse_pass_through_while_running(),
            self.timer.read().unwrap().current_phase(),
            self.window.is_active(),
        );
        if self.mouse_passthrough.replace(enabled) != enabled {
            self.platform.set_mouse_passthrough(&self.window, enabled);
        }
    }

    fn close_notes_windows(&mut self) {
        for kind in [EditorKind::Notes, EditorKind::NotesViewer] {
            if let Some(window) = self.open_editors.0.remove(&kind) {
                window.close();
            }
        }
        self.notes_viewer = None;
    }

    fn set_notes_actions_enabled(&self, enabled: bool) {
        for name in ["edit-notes", "show-notes"] {
            if let Some(action) = self.window.lookup_action(name) {
                if let Ok(action) = action.downcast::<gio::SimpleAction>() {
                    action.set_enabled(enabled);
                }
            }
        }
    }

    fn notes_context(&self) -> Option<(PathBuf, Vec<String>)> {
        let splits_path = self.config.borrow().splits_path()?.to_owned();
        let names = self
            .timer
            .read()
            .unwrap()
            .run()
            .segments()
            .iter()
            .map(|segment| segment.name().to_owned())
            .collect();
        Some((crate::notes::sidecar_path(&splits_path), names))
    }

    fn open_notes_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Notes) {
            window.present();
            return;
        }
        let Some((path, names)) = self.notes_context() else {
            return;
        };
        let document = Rc::new(RefCell::new(crate::notes::NotesDocument::load(names, path)));
        let window = build_notes_editor(&self.window, document, sender);
        self.open_editors
            .0
            .insert(EditorKind::Notes, window.clone());
        window.present();
    }

    fn open_notes_viewer(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::NotesViewer) {
            window.present();
            return;
        }
        let Some((path, names)) = self.notes_context() else {
            return;
        };
        let state = crate::notes::NotesViewer::load(names, path);
        let (window, title, buffer) = build_notes_viewer(&self.window, &state, sender);
        self.notes_viewer = Some(NotesViewerUi {
            state,
            title,
            buffer,
        });
        self.open_editors
            .0
            .insert(EditorKind::NotesViewer, window.clone());
        window.present();
    }

    fn update_notes_viewer(&mut self) {
        let Some(viewer) = self.notes_viewer.as_mut() else {
            return;
        };
        let index = self
            .timer
            .read()
            .unwrap()
            .current_split_index()
            .unwrap_or(0);
        if viewer.state.follow_split(index) {
            viewer.title.set_label(&viewer.state.title());
            set_markdown_buffer(&viewer.buffer, viewer.state.note());
        }
    }

    fn has_unsaved_changes(&self) -> (bool, bool) {
        let splits = self.timer.read().unwrap().run().has_been_modified();
        let layout = self.layout.borrow().is_modified;
        (splits, layout)
    }

    fn relevant_unsaved_changes(&self, action: &PendingAction) -> (bool, bool) {
        let (splits, layout) = self.has_unsaved_changes();
        relevant_changes(action, splits, layout)
    }

    fn request_action(&self, action: PendingAction, sender: &ComponentSender<Self>) {
        let (splits, layout) = self.relevant_unsaved_changes(&action);
        if !splits && !layout {
            sender.input(AppMsg::ExecuteAction(action));
            return;
        }
        let what = match (splits, layout) {
            (true, true) => "your splits and layout",
            (true, false) => "your splits",
            (false, true) => "your layout",
            (false, false) => unreachable!(),
        };
        let dialog = gtk::AlertDialog::builder()
            .message("Save changes before continuing?")
            .detail(format!("There are unsaved changes to {what}."))
            .modal(true)
            .build();
        dialog.set_buttons(&["Cancel", "Discard Changes", "Save"]);
        dialog.set_cancel_button(0);
        dialog.set_default_button(2);
        let sender = sender.clone();
        dialog.choose(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |response| match response.ok() {
                Some(1) => sender.input(AppMsg::ExecuteAction(action)),
                Some(2) => sender.input(AppMsg::SaveBeforeAction(action)),
                _ => {}
            },
        );
    }

    fn execute_action(&self, action: PendingAction, sender: &ComponentSender<Self>) {
        sender.input(match action {
            PendingAction::NewSplits => AppMsg::NewSplits,
            PendingAction::OpenSplits(path) => AppMsg::OpenSplits(path),
            PendingAction::NewLayout => AppMsg::NewLayout,
            PendingAction::OpenLayout(path) => AppMsg::OpenLayout(path),
            PendingAction::Exit => {
                let width = self.window.width();
                let height = self.window.height();
                if width > 0 && height > 0 {
                    let mut config = self.config.borrow_mut();
                    config.set_window_size((width as f64, height as f64));
                    self.platform.persist_placement(&self.window, &mut config);
                }
                self.closing_allowed.set(true);
                self.window.close();
                return;
            }
        });
    }

    fn save_before_action(&self, action: PendingAction, sender: &ComponentSender<Self>) {
        let (save_splits, save_layout) = self.relevant_unsaved_changes(&action);
        if save_splits {
            if self.config.borrow().can_directly_save_splits() {
                let result = self.config.borrow_mut().save_splits(
                    &mut self.timer.write().unwrap(),
                    #[cfg(feature = "auto-splitting")]
                    &self.auto_splitter,
                );
                crate::config::or_show_error(result);
                if !self.timer.read().unwrap().run().has_been_modified() {
                    sender.input(AppMsg::SaveBeforeAction(action));
                }
            } else {
                let sender = sender.clone();
                choose_save_path(&self.window, "Save Splits", move |path| {
                    sender.input(AppMsg::SaveSplitsAsThen(path, action));
                });
            }
            return;
        }
        if save_layout {
            if self.config.borrow().can_directly_save_layout() {
                let settings = self.layout.borrow().layout.settings();
                let result = self.config.borrow().save_layout(settings);
                if result.is_ok() {
                    self.layout.borrow_mut().is_modified = false;
                    sender.input(AppMsg::SaveBeforeAction(action));
                }
                crate::config::or_show_error(result);
            } else {
                let sender = sender.clone();
                choose_save_path(&self.window, "Save Layout", move |path| {
                    sender.input(AppMsg::SaveLayoutAsThen(path, action));
                });
            }
            return;
        }
        sender.input(AppMsg::ExecuteAction(action));
    }

    fn open_settings_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Window) {
            window.present();
            return;
        }
        if let Some(system) = self.hotkey_system.as_mut() {
            let _ = system.deactivate();
        }
        let draft = Rc::new(RefCell::new(self.config.borrow().hotkeys()));
        let window = build_settings_editor(
            &self.window,
            self.config.borrow().get_mouse_pass_through_while_running(),
            self.platform.backend(),
            draft,
            sender,
        );
        self.open_editors
            .0
            .insert(EditorKind::Window, window.clone());
        window.present();
    }

    fn open_run_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Run) {
            window.present();
            return;
        }
        let run = self.timer.read().unwrap().run().clone();
        let Ok(editor) = livesplit_core::RunEditor::new(run) else {
            return;
        };
        let editor = Rc::new(RefCell::new(Some(editor)));
        let window = build_run_editor(
            &self.window,
            editor.clone(),
            sender,
            #[cfg(feature = "auto-splitting")]
            self.config.borrow().auto_splitter_association().cloned(),
            #[cfg(feature = "auto-splitting")]
            self.auto_splitter.clone(),
            #[cfg(feature = "auto-splitting")]
            self.timer.clone(),
            #[cfg(feature = "auto-splitting")]
            self.config.borrow().splits_path().is_some(),
        );
        self.active_run_editor = Some(editor);
        self.open_editors.0.insert(EditorKind::Run, window.clone());
        window.present();
    }

    fn open_layout_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Layout) {
            window.present();
            return;
        }
        let layout = self.layout.borrow().layout.clone();
        let Ok(editor) = livesplit_core::LayoutEditor::new(layout) else {
            return;
        };
        let editor = Rc::new(RefCell::new(Some(editor)));
        let window = build_layout_editor(
            &self.window,
            editor.clone(),
            self.image_cache.clone(),
            sender,
        );
        self.active_layout_editor = Some(editor);
        self.open_editors
            .0
            .insert(EditorKind::Layout, window.clone());
        window.present();
    }

    fn poll_world_records(&mut self, sender: &ComponentSender<Self>) {
        let requests = {
            let timer = self.timer.read().unwrap();
            let snapshot = timer.snapshot();
            self.layout
                .borrow_mut()
                .layout
                .world_record_request_urls(&snapshot)
        };
        for (index, url) in requests {
            let should_request = self
                .pending_world_records
                .get(&index)
                .is_none_or(|started| started.elapsed() >= std::time::Duration::from_secs(30));
            if !should_request {
                continue;
            }
            self.pending_world_records.insert(index, Instant::now());
            let sender = sender.clone();
            std::thread::spawn(move || {
                let body = ureq::get(&url)
                    .call()
                    .ok()
                    .and_then(|response| response.into_string().ok());
                let _ =
                    sender
                        .input_sender()
                        .send(AppMsg::WorldRecordResponse { index, url, body });
            });
        }
    }

    fn render(&self) {
        let logical_width = self.picture.width().max(1) as u32;
        let logical_height = self.picture.height().max(1) as u32;
        let scale = self.picture.scale_factor().max(1) as u32;
        let width = logical_width.saturating_mul(scale);
        let height = logical_height.saturating_mul(scale);
        let timer = self.timer.read().unwrap();
        let snapshot = timer.snapshot();
        let mut layout = self.layout.borrow_mut();
        let mut cache = self.image_cache.borrow_mut();
        if let Some(editor) = &self.active_layout_editor {
            if let Some(editor) = editor.borrow_mut().as_mut() {
                editor.update_layout_state(
                    &mut layout.layout_state,
                    &mut cache,
                    &snapshot,
                    livesplit_core::Lang::English,
                );
            }
        } else {
            layout.layout_state =
                layout
                    .layout
                    .state(&mut cache, &snapshot, livesplit_core::Lang::English);
        }
        let mut renderer = self.renderer.borrow_mut();
        let requested_size = renderer.render(&layout.layout_state, &cache, [width, height]);
        let bytes = glib::Bytes::from_owned(renderer.image_data().to_vec());
        let texture = gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &bytes,
            (width * 4) as usize,
        );
        self.picture.set_paintable(Some(&texture));
        self.render_size.set((logical_width, logical_height));
        if let Some(size) = requested_size {
            let (width, height) = physical_to_logical_size(size, scale);
            if width != self.window.width() || height != self.window.height() {
                self.window.set_default_size(width, height);
            }
        }
        cache.collect();
    }
}

fn install_timer_interactions(
    window: &gtk::ApplicationWindow,
    picture: &gtk::Picture,
    timer: &SharedTimer,
    config: &Config,
    platform: Platform,
    sender: &ComponentSender<AppModel>,
) {
    const RESIZE_MARGIN: f64 = 8.0;

    let move_or_resize = gtk::GestureClick::new();
    move_or_resize.set_button(1);
    let drag_window = window.clone();
    let drag_picture = picture.clone();
    move_or_resize.connect_pressed(move |gesture, _, x, y| {
        let Some(event) = gesture.current_event() else {
            return;
        };
        if let Some(edge) = timer_resize_edge(
            x,
            y,
            drag_picture.width() as f64,
            drag_picture.height() as f64,
            RESIZE_MARGIN,
        ) {
            platform.begin_resize(&drag_window, &event, edge);
        } else {
            platform.begin_drag(&drag_window, &event);
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    picture.add_controller(move_or_resize);

    let resize_cursor = gtk::EventControllerMotion::new();
    let cursor_picture = picture.clone();
    resize_cursor.connect_motion(move |controller, x, y| {
        let cursor = timer_resize_edge(
            x,
            y,
            cursor_picture.width() as f64,
            cursor_picture.height() as f64,
            RESIZE_MARGIN,
        )
        .map(resize_cursor_name);
        if let Some(widget) = controller.widget() {
            widget.set_cursor_from_name(cursor);
        }
    });
    resize_cursor.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor_from_name(None);
        }
    });
    picture.add_controller(resize_cursor);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let scroll_sender = sender.clone();
    scroll.connect_scroll(move |_, _dx, dy| {
        scroll_sender.input(AppMsg::Scroll(dy));
        glib::Propagation::Stop
    });
    picture.add_controller(scroll);

    let menu = gio::Menu::new();

    let splits = gio::Menu::new();
    splits.append(Some("Edit…"), Some("win.edit-splits"));
    let splits_files = gio::Menu::new();
    splits_files.append(Some("New"), Some("win.new-splits"));
    splits_files.append(Some("Open…"), Some("win.open-splits"));
    let recent = gio::Menu::new();
    let mut recent_count = 0;
    for (game_index, (game, categories)) in config.splits_history().iter().enumerate() {
        let game_menu = gio::Menu::new();
        for (category_index, (category, paths)) in categories.iter().enumerate() {
            let category_menu = gio::Menu::new();
            for (path_index, path) in paths.iter().enumerate() {
                let action_name = format!("recent-{game_index}-{category_index}-{path_index}");
                let label = path
                    .file_name()
                    .map_or_else(|| "Untitled".into(), |name| name.to_string_lossy());
                category_menu.append(Some(&label), Some(&format!("win.{action_name}")));
                let action = gio::SimpleAction::new(&action_name, None);
                let path = path.to_path_buf();
                let action_sender = sender.clone();
                action.connect_activate(move |_, _| {
                    action_sender.input(AppMsg::RequestAction(PendingAction::OpenSplits(
                        path.clone(),
                    )));
                });
                window.add_action(&action);
                recent_count += 1;
            }
            game_menu.append_submenu(Some(category), &category_menu);
        }
        recent.append_submenu(Some(game), &game_menu);
    }
    if recent_count == 0 {
        recent.append(Some("No recent splits available"), Some("win.open-recent"));
    }
    splits_files.append_submenu(Some("Open Recent"), &recent);
    splits_files.append(Some("Save"), Some("win.save-splits"));
    splits_files.append(Some("Save As…"), Some("win.save-splits-as"));
    splits.append_section(None, &splits_files);
    menu.append_submenu(Some("Splits"), &splits);

    let layout = gio::Menu::new();
    layout.append(Some("Edit…"), Some("win.edit-layout"));
    let layout_files = gio::Menu::new();
    layout_files.append(Some("New"), Some("win.new-layout"));
    layout_files.append(Some("Open…"), Some("win.open-layout"));
    layout_files.append(Some("Save"), Some("win.save-layout"));
    layout_files.append(Some("Save As…"), Some("win.save-layout-as"));
    layout.append_section(None, &layout_files);
    menu.append_submenu(Some("Layout"), &layout);

    let control = gio::Menu::new();
    control.append(Some("Start / Split"), Some("win.start-or-split"));
    control.append(Some("Reset"), Some("win.reset"));
    let split_control = gio::Menu::new();
    split_control.append(Some("Undo Split"), Some("win.undo-split"));
    split_control.append(Some("Skip Split"), Some("win.skip-split"));
    control.append_section(None, &split_control);
    let pause_control = gio::Menu::new();
    pause_control.append(Some("Pause / Resume"), Some("win.pause"));
    pause_control.append(Some("Undo All Pauses"), Some("win.undo-all-pauses"));
    control.append_section(None, &pause_control);
    menu.append_submenu(Some("Control"), &control);

    let comparisons = gio::Menu::new();
    let timer_guard = timer.read().unwrap();
    for comparison in timer_guard.run().comparisons() {
        let item = gio::MenuItem::new(Some(comparison), None);
        item.set_action_and_target_value(Some("win.comparison"), Some(&comparison.to_variant()));
        comparisons.append_item(&item);
    }
    let comparison_action = gio::SimpleAction::new_stateful(
        "comparison",
        Some(&String::static_variant_type()),
        &timer_guard.current_comparison().to_variant(),
    );
    drop(timer_guard);
    let comparison_sender = sender.clone();
    comparison_action.connect_activate(move |action, parameter| {
        let Some(comparison) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        action.set_state(&comparison.to_variant());
        comparison_sender.input(AppMsg::SetComparison(comparison));
    });
    window.add_action(&comparison_action);
    menu.append_submenu(Some("Compare Against"), &comparisons);

    let timing = gio::Menu::new();
    for (label, value) in [("Real Time", "real-time"), ("Game Time", "game-time")] {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some("win.timing-method"), Some(&value.to_variant()));
        timing.append_item(&item);
    }
    let initial_timing = if timer.read().unwrap().current_timing_method()
        == livesplit_core::TimingMethod::RealTime
    {
        "real-time"
    } else {
        "game-time"
    };
    let timing_action = gio::SimpleAction::new_stateful(
        "timing-method",
        Some(&String::static_variant_type()),
        &initial_timing.to_variant(),
    );
    let timing_sender = sender.clone();
    timing_action.connect_activate(move |action, parameter| {
        let Some(method) = parameter.and_then(|value| value.str()) else {
            return;
        };
        action.set_state(&method.to_variant());
        timing_sender.input(AppMsg::SetTimingMethod(if method == "game-time" {
            livesplit_core::TimingMethod::GameTime
        } else {
            livesplit_core::TimingMethod::RealTime
        }));
    });
    window.add_action(&timing_action);
    menu.append_submenu(Some("Timing Method"), &timing);

    let tools = gio::Menu::new();
    tools.append(Some("Settings"), Some("win.window-settings"));
    tools.append(Some("Edit Split Notes"), Some("win.edit-notes"));
    tools.append(Some("Show Split Notes"), Some("win.show-notes"));
    menu.append_section(None, &tools);

    let application = gio::Menu::new();
    application.append(Some("Exit"), Some("win.exit"));
    menu.append_section(None, &application);

    for (name, command) in [
        ("start-or-split", TimerCommand::StartOrSplit),
        ("undo-split", TimerCommand::UndoSplit),
        ("skip-split", TimerCommand::SkipSplit),
        ("pause", TimerCommand::Pause),
        ("undo-all-pauses", TimerCommand::UndoAllPauses),
        ("reset", TimerCommand::Reset),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| action_sender.input(AppMsg::TimerCommand(command)));
        window.add_action(&action);
    }

    // Keep the former menu discoverable while its editor and file-intent
    // components are being ported. A disabled action is preferable to an item
    // that either disappears or silently discards the user's request.
    let open_recent = gio::SimpleAction::new("open-recent", None);
    open_recent.set_enabled(false);
    window.add_action(&open_recent);

    let window_settings = gio::SimpleAction::new("window-settings", None);
    let settings_sender = sender.clone();
    window_settings.connect_activate(move |_, _| {
        settings_sender.input(AppMsg::OpenEditor(EditorKind::Window));
    });
    window.add_action(&window_settings);

    for (name, kind) in [
        ("edit-notes", EditorKind::Notes),
        ("show-notes", EditorKind::NotesViewer),
    ] {
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(config.splits_path().is_some());
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| {
            action_sender.input(AppMsg::OpenEditor(kind));
        });
        window.add_action(&action);
    }

    let edit_layout = gio::SimpleAction::new("edit-layout", None);
    let layout_sender = sender.clone();
    edit_layout.connect_activate(move |_, _| {
        layout_sender.input(AppMsg::OpenEditor(EditorKind::Layout));
    });
    window.add_action(&edit_layout);

    let edit_splits = gio::SimpleAction::new("edit-splits", None);
    let splits_sender = sender.clone();
    edit_splits.connect_activate(move |_, _| {
        splits_sender.input(AppMsg::OpenEditor(EditorKind::Run));
    });
    window.add_action(&edit_splits);

    for (name, direct_action) in [
        ("new-splits", DirectAction::NewSplits),
        ("save-splits", DirectAction::SaveSplits),
        ("new-layout", DirectAction::NewLayout),
        ("save-layout", DirectAction::SaveLayout),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| {
            action_sender.input(match direct_action {
                DirectAction::NewSplits => AppMsg::RequestAction(PendingAction::NewSplits),
                DirectAction::SaveSplits => AppMsg::SaveSplits,
                DirectAction::NewLayout => AppMsg::RequestAction(PendingAction::NewLayout),
                DirectAction::SaveLayout => AppMsg::SaveLayout,
            });
        });
        window.add_action(&action);
    }

    for (name, save, choice) in [
        ("open-splits", false, FileChoice::OpenSplits),
        ("save-splits-as", true, FileChoice::SaveSplits),
        ("open-layout", false, FileChoice::OpenLayout),
        ("save-layout-as", true, FileChoice::SaveLayout),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let action_window = window.clone();
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| {
            select_file(&action_window, save, choice, &action_sender);
        });
        window.add_action(&action);
    }

    let exit = gio::SimpleAction::new("exit", None);
    let exit_sender = sender.clone();
    exit.connect_activate(move |_, _| {
        exit_sender.input(AppMsg::RequestAction(PendingAction::Exit));
    });
    window.add_action(&exit);

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(picture);
    popover.set_has_arrow(false);
    let click = gtk::GestureClick::new();
    click.set_button(3);
    click.connect_pressed(move |gesture, _, x, y| {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    picture.add_controller(click);
}

fn timer_resize_edge(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    margin: f64,
) -> Option<gdk::SurfaceEdge> {
    let west = x < margin;
    let east = x >= width - margin;
    let north = y < margin;
    let south = y >= height - margin;
    match (west, east, north, south) {
        (true, _, true, _) => Some(gdk::SurfaceEdge::NorthWest),
        (_, true, true, _) => Some(gdk::SurfaceEdge::NorthEast),
        (true, _, _, true) => Some(gdk::SurfaceEdge::SouthWest),
        (_, true, _, true) => Some(gdk::SurfaceEdge::SouthEast),
        (true, _, _, _) => Some(gdk::SurfaceEdge::West),
        (_, true, _, _) => Some(gdk::SurfaceEdge::East),
        (_, _, true, _) => Some(gdk::SurfaceEdge::North),
        (_, _, _, true) => Some(gdk::SurfaceEdge::South),
        _ => None,
    }
}

fn resize_cursor_name(edge: gdk::SurfaceEdge) -> &'static str {
    match edge {
        gdk::SurfaceEdge::NorthWest => "nw-resize",
        gdk::SurfaceEdge::North => "n-resize",
        gdk::SurfaceEdge::NorthEast => "ne-resize",
        gdk::SurfaceEdge::West => "w-resize",
        gdk::SurfaceEdge::East => "e-resize",
        gdk::SurfaceEdge::SouthWest => "sw-resize",
        gdk::SurfaceEdge::South => "s-resize",
        gdk::SurfaceEdge::SouthEast => "se-resize",
        _ => "default",
    }
}

fn build_notes_editor(
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

fn build_notes_viewer(
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

fn set_markdown_buffer(buffer: &gtk::TextBuffer, markdown: &str) {
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

fn build_settings_editor(
    parent: &gtk::ApplicationWindow,
    initial_pass_through: bool,
    backend: crate::platform::DisplayBackend,
    draft: Rc<RefCell<livesplit_core::HotkeyConfig>>,
    sender: &ComponentSender<AppModel>,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .title("Settings")
        .transient_for(parent)
        .default_width(620)
        .default_height(620)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let overlay = adw::ToastOverlay::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let warning = adw::Banner::builder()
        .title("That hotkey is already assigned to another action")
        .revealed(false)
        .build();
    content.append(&warning);
    let page = adw::PreferencesPage::new();
    page.set_vexpand(true);
    let behavior = adw::PreferencesGroup::builder()
        .title("Timer Window")
        .build();
    let pass_through = adw::SwitchRow::builder()
        .title("Ignore mouse while running")
        .subtitle("Pass pointer input through the timer while it is running")
        .active(initial_pass_through)
        .build();
    behavior.add(&pass_through);
    let backend_row = adw::ActionRow::builder()
        .title("Display Backend")
        .subtitle(match backend {
            crate::platform::DisplayBackend::X11 => "X11 — absolute placement and always on top",
            crate::platform::DisplayBackend::WaylandStandard => {
                "Wayland — compositor controls placement and stacking"
            }
        })
        .build();
    behavior.add(&backend_row);
    page.add(&behavior);
    let group = adw::PreferencesGroup::builder()
        .title("Global Hotkeys")
        .description("Click a shortcut to capture a new key combination")
        .build();
    let description = draft
        .borrow()
        .settings_description(livesplit_core::Lang::English);
    for (index, field) in description.fields.iter().enumerate() {
        let current = match &field.value {
            livesplit_core::settings::Value::Hotkey(value) => *value,
            _ => continue,
        };
        let row = adw::ActionRow::builder()
            .title(field.text.as_ref())
            .subtitle(field.tooltip.as_ref())
            .build();
        let shortcut = gtk::Button::with_label(
            &current.map_or_else(|| "Not assigned".to_owned(), |hotkey| hotkey.to_string()),
        );
        shortcut.add_css_class("flat");
        shortcut.set_valign(gtk::Align::Center);
        let capture_parent = window.clone();
        let capture_draft = draft.clone();
        let capture_label = shortcut.clone();
        let capture_warning = warning.clone();
        shortcut.connect_clicked(move |_| {
            open_hotkey_capture(
                &capture_parent,
                index,
                capture_draft.clone(),
                capture_label.clone(),
                capture_warning.clone(),
            );
        });
        row.add_suffix(&shortcut);
        let clear = gtk::Button::from_icon_name("edit-clear-symbolic");
        clear.set_tooltip_text(Some("Clear hotkey"));
        clear.set_valign(gtk::Align::Center);
        let clear_draft = draft.clone();
        let clear_label = shortcut.clone();
        clear.connect_clicked(move |_| {
            let _ = clear_draft
                .borrow_mut()
                .set_value(index, livesplit_core::settings::Value::Hotkey(None));
            clear_label.set_label("Not assigned");
        });
        row.add_suffix(&clear);
        group.add(&row);
    }
    page.add(&group);
    content.append(&page);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    buttons.append(&spacer);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&apply);
    content.append(&buttons);
    overlay.set_child(Some(&content));
    toolbar.set_content(Some(&overlay));
    window.set_content(Some(&toolbar));

    let cancel_window = window.clone();
    cancel.connect_clicked(move |_| cancel_window.close());
    let apply_window = window.clone();
    let apply_draft = draft.clone();
    let apply_sender = sender.clone();
    apply.connect_clicked(move |_| {
        apply_sender.input(AppMsg::ApplyWindowSettings(pass_through.is_active()));
        apply_sender.input(AppMsg::ApplyHotkeys(*apply_draft.borrow()));
        apply_window.close();
    });
    let close_sender = sender.clone();
    window.connect_close_request(move |_| {
        close_sender.input(AppMsg::EditorFinished(EditorKind::Window));
        glib::Propagation::Proceed
    });
    window
}

fn open_hotkey_capture(
    parent: &adw::ApplicationWindow,
    index: usize,
    draft: Rc<RefCell<livesplit_core::HotkeyConfig>>,
    shortcut_label: gtk::Button,
    warning: adw::Banner,
) {
    let dialog = adw::ApplicationWindow::builder()
        .title("Capture Hotkey")
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .default_height(260)
        .build();
    let status = adw::StatusPage::builder()
        .icon_name("preferences-desktop-keyboard-shortcuts-symbolic")
        .title("Press a key combination")
        .description("Press Escape to cancel. Modifier-only keys are ignored.")
        .build();
    let clear = gtk::Button::with_label("Clear Shortcut");
    clear.set_halign(gtk::Align::Center);
    let clear_dialog = dialog.clone();
    let clear_draft = draft.clone();
    let clear_label = shortcut_label.clone();
    clear.connect_clicked(move |_| {
        let _ = clear_draft
            .borrow_mut()
            .set_value(index, livesplit_core::settings::Value::Hotkey(None));
        clear_label.set_label("Not assigned");
        clear_dialog.close();
    });
    status.set_child(Some(&clear));
    dialog.set_content(Some(&status));
    let keys = gtk::EventControllerKey::new();
    let key_dialog = dialog.clone();
    let conflict_status = status.clone();
    keys.connect_key_pressed(move |_, key, _keycode, state| {
        if key == gdk::Key::Escape {
            key_dialog.close();
            return glib::Propagation::Stop;
        }
        let Some(hotkey) = hotkey_from_gdk(key, state) else {
            return glib::Propagation::Stop;
        };
        let value = livesplit_core::settings::Value::Hotkey(Some(hotkey));
        if draft.borrow_mut().set_value(index, value).is_ok() {
            shortcut_label.set_label(&hotkey.to_string());
            warning.set_revealed(false);
            key_dialog.close();
        } else {
            warning.set_revealed(true);
            conflict_status.set_description(Some(
                "That shortcut is already assigned. Press a different combination.",
            ));
        }
        glib::Propagation::Stop
    });
    dialog.add_controller(keys);
    dialog.present();
}

fn hotkey_from_gdk(
    key: gdk::Key,
    state: gdk::ModifierType,
) -> Option<livesplit_core::hotkey::Hotkey> {
    use livesplit_core::hotkey::{KeyCode, Modifiers};
    let name = key.name()?;
    let name = name.as_str();
    if matches!(
        name,
        "Shift_L"
            | "Shift_R"
            | "Control_L"
            | "Control_R"
            | "Alt_L"
            | "Alt_R"
            | "Super_L"
            | "Super_R"
            | "Meta_L"
            | "Meta_R"
    ) {
        return None;
    }
    let code_name = match name {
        "Return" | "ISO_Enter" => "Enter".to_owned(),
        "space" => "Space".to_owned(),
        "BackSpace" => "Backspace".to_owned(),
        "Left" => "ArrowLeft".to_owned(),
        "Right" => "ArrowRight".to_owned(),
        "Up" => "ArrowUp".to_owned(),
        "Down" => "ArrowDown".to_owned(),
        "Page_Up" => "PageUp".to_owned(),
        "Page_Down" => "PageDown".to_owned(),
        "KP_Add" => "NumpadAdd".to_owned(),
        "KP_Subtract" => "NumpadSubtract".to_owned(),
        "KP_Multiply" => "NumpadMultiply".to_owned(),
        "KP_Divide" => "NumpadDivide".to_owned(),
        "KP_Decimal" => "NumpadDecimal".to_owned(),
        "KP_Enter" => "NumpadEnter".to_owned(),
        "grave" => "Backquote".to_owned(),
        "backslash" => "Backslash".to_owned(),
        "bracketleft" => "BracketLeft".to_owned(),
        "bracketright" => "BracketRight".to_owned(),
        "comma" => "Comma".to_owned(),
        "equal" => "Equal".to_owned(),
        "minus" => "Minus".to_owned(),
        "period" => "Period".to_owned(),
        "apostrophe" => "Quote".to_owned(),
        "semicolon" => "Semicolon".to_owned(),
        "slash" => "Slash".to_owned(),
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic() => {
            format!("Key{}", value.to_ascii_uppercase())
        }
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_digit() => {
            format!("Digit{value}")
        }
        value if value.starts_with("KP_") && value[3..].parse::<u8>().is_ok() => {
            format!("Numpad{}", &value[3..])
        }
        value => value.replace('_', ""),
    };
    let key_code = code_name.parse::<KeyCode>().ok()?;
    let mut modifiers = Modifiers::empty();
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        modifiers.insert(Modifiers::SHIFT);
    }
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        modifiers.insert(Modifiers::CONTROL);
    }
    if state.contains(gdk::ModifierType::ALT_MASK) {
        modifiers.insert(Modifiers::ALT);
    }
    if state.contains(gdk::ModifierType::META_MASK) || state.contains(gdk::ModifierType::SUPER_MASK)
    {
        modifiers.insert(Modifiers::META);
    }
    Some(key_code.with_modifiers(modifiers))
}

fn build_run_editor(
    parent: &gtk::ApplicationWindow,
    editor: Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    sender: &ComponentSender<AppModel>,
    #[cfg(feature = "auto-splitting")] original_auto_splitter: Option<AutoSplitterAssociation>,
    #[cfg(feature = "auto-splitting")] auto_splitter: Rc<
        livesplit_core::auto_splitting::Runtime<SharedTimer>,
    >,
    #[cfg(feature = "auto-splitting")] timer: SharedTimer,
    #[cfg(feature = "auto-splitting")] can_associate_auto_splitter: bool,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .title("Edit Splits")
        .transient_for(parent)
        .default_width(980)
        .default_height(650)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let pages = adw::ViewStack::new();
    pages.set_vexpand(true);
    let page_switcher = adw::ViewSwitcher::builder()
        .stack(&pages)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let splits_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    #[cfg(feature = "auto-splitting")]
    let pending_auto_splitter = Rc::new(RefCell::new(original_auto_splitter.clone()));
    #[cfg(feature = "auto-splitting")]
    let original_auto_splitter_settings = auto_splitter.settings_map().unwrap_or_default();
    #[cfg(feature = "auto-splitting")]
    let auto_splitter_applied = Rc::new(Cell::new(false));

    let general = adw::PreferencesGroup::builder().title("Run").build();
    general.set_hexpand(true);
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut ImageCache::new(), livesplit_core::Lang::English);
    let details = build_run_details_page(&editor, &state);
    let game = CompletionEntryRow::new("Game", &state.game);
    let game_editor = editor.clone();
    game.connect_changed(move |row| {
        if let Some(editor) = game_editor.borrow_mut().as_mut() {
            editor.set_game_name(row.text().as_str());
        }
    });
    general.add(&game.row);
    let src_spinner = gtk::Spinner::new();
    src_spinner.set_tooltip_text(Some("Searching speedrun.com"));
    game.add_suffix(&src_spinner);
    let category = CompletionEntryRow::new("Category", &state.category);
    let category_editor = editor.clone();
    category.connect_changed(move |row| {
        if let Some(editor) = category_editor.borrow_mut().as_mut() {
            editor.set_category_name(row.text().as_str());
        }
    });
    general.add(&category.row);
    let src_platforms = details.platform_entry.clone();
    let src_regions = details.region_entry.clone();
    let game_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Game>::new()));
    let category_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Category>::new()));
    let platform_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Choice>::new()));
    let region_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Choice>::new()));
    let populating_suggestions = Rc::new(Cell::new(false));
    let search_generation = Rc::new(Cell::new(0_u64));
    let metadata_generation = Rc::new(Cell::new(0_u64));
    let variable_generation = Rc::new(Cell::new(0_u64));
    let initial_game_name = state.game.clone();
    let initial_category_name = state.category.clone();
    let auto_select_game = Rc::new(Cell::new(initial_game_name.trim().len() >= 2));
    let auto_select_category = Rc::new(Cell::new(false));
    let search_results = game_results.clone();
    let search_status = game.clone();
    let search_spinner = src_spinner.clone();
    let search_populating = populating_suggestions.clone();
    let changed_game_completion = game.clone();
    let search_generation_changed = search_generation.clone();
    let search_auto_select_game = auto_select_game.clone();
    let search_auto_select_category = auto_select_category.clone();
    let search_initial_game = initial_game_name.clone();
    game.connect_changed(move |row| {
        let query = row.text().trim().to_owned();
        let generation = search_generation_changed.get() + 1;
        search_generation_changed.set(generation);
        let completion = changed_game_completion.clone();
        let auto_select_game = search_auto_select_game.clone();
        let auto_select_category = search_auto_select_category.clone();
        let initial_game = search_initial_game.clone();
        glib::idle_add_local_once(move || completion.clear_items());
        if query.len() < 2 {
            search_spinner.set_spinning(false);
            search_status.set_tooltip_text(Some("Type at least two characters for suggestions"));
            return;
        }
        search_spinner.set_spinning(true);
        search_status.set_tooltip_text(Some("Waiting to search speedrun.com…"));
        let generation_state = search_generation_changed.clone();
        let results = search_results.clone();
        let status = search_status.clone();
        let spinner = search_spinner.clone();
        let populating = search_populating.clone();
        let completion = changed_game_completion.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
            if generation_state.get() != generation {
                return;
            }
            status.set_tooltip_text(Some("Searching speedrun.com…"));
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(
                    crate::speedrun_com::search_games(&query).map_err(|error| error.to_string()),
                );
            });
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let Ok(response) = rx.try_recv() else {
                    return glib::ControlFlow::Continue;
                };
                if generation_state.get() != generation {
                    return glib::ControlFlow::Break;
                }
                spinner.set_spinning(false);
                match response {
                    Ok(games) => {
                        let count = games.len();
                        let names = games
                            .iter()
                            .map(|game| game.name.as_str())
                            .collect::<Vec<_>>();
                        populating.set(true);
                        completion.set_items(&names);
                        *results.borrow_mut() = games;
                        populating.set(false);
                        completion.show_dropdown();
                        if auto_select_game.replace(false) {
                            if let Some(index) = results
                                .borrow()
                                .iter()
                                .position(|game| game.name.eq_ignore_ascii_case(&initial_game))
                            {
                                auto_select_category.set(true);
                                completion.select(index);
                            }
                        }
                        status
                            .set_tooltip_text(Some(&format!("Found {} speedrun.com games", count)));
                    }
                    Err(error) => status
                        .set_tooltip_text(Some(&format!("Speedrun.com search failed: {error}"))),
                }
                glib::ControlFlow::Break
            });
        });
    });
    let selected_games = game_results.clone();
    let selected_categories = category_results.clone();
    let selected_platforms = platform_results.clone();
    let selected_regions = region_results.clone();
    let selected_editor = editor.clone();
    let selected_game_entry = game.clone();
    let selected_status = game.clone();
    let selected_spinner = src_spinner.clone();
    let selected_populating = populating_suggestions.clone();
    let selected_auto_category = auto_select_category.clone();
    let selected_initial_category = initial_category_name.clone();
    let selected_category_completion = category.clone();
    let selected_platform_completion = details.platform_entry.clone();
    let selected_region_completion = details.region_entry.clone();
    let selected_metadata_generation = metadata_generation.clone();
    game.connect_selected(move |selected_index| {
        if selected_populating.get() {
            return;
        }
        let Some(game) = selected_games.borrow().get(selected_index).cloned() else {
            return;
        };
        let generation = selected_metadata_generation.get() + 1;
        selected_metadata_generation.set(generation);
        selected_game_entry.set_text(&game.name);
        if let Some(editor) = selected_editor.borrow_mut().as_mut() {
            editor.set_game_name(game.name.as_str());
        }
        selected_spinner.set_spinning(true);
        selected_status.set_tooltip_text(Some("Loading speedrun.com metadata…"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                Ok((
                    crate::speedrun_com::categories(&game.id)?,
                    crate::speedrun_com::platforms(&game.id)?,
                    crate::speedrun_com::regions(&game.id)?,
                ))
            })()
            .map_err(|error: anyhow::Error| error.to_string());
            let _ = tx.send(result);
        });
        let categories = selected_categories.clone();
        let platforms = selected_platforms.clone();
        let regions = selected_regions.clone();
        let status = selected_status.clone();
        let spinner = selected_spinner.clone();
        let populating = selected_populating.clone();
        let category_completion = selected_category_completion.clone();
        let platform_completion = selected_platform_completion.clone();
        let region_completion = selected_region_completion.clone();
        let auto_category = selected_auto_category.clone();
        let initial_category = selected_initial_category.clone();
        let generation_state = selected_metadata_generation.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let Ok(response) = rx.try_recv() else {
                return glib::ControlFlow::Continue;
            };
            if generation_state.get() != generation {
                return glib::ControlFlow::Break;
            }
            spinner.set_spinning(false);
            match response {
                Ok((found_categories, found_platforms, found_regions)) => {
                    populating.set(true);
                    let names = found_categories
                        .iter()
                        .map(|category| category.name.as_str())
                        .collect::<Vec<_>>();
                    category_completion.set_items(&names);

                    let names = found_platforms
                        .iter()
                        .map(|platform| platform.name.as_str())
                        .collect::<Vec<_>>();
                    platform_completion.set_items(&names);

                    let names = found_regions
                        .iter()
                        .map(|region| region.name.as_str())
                        .collect::<Vec<_>>();
                    region_completion.set_items(&names);

                    status.set_tooltip_text(Some(&format!(
                        "Found {} categories, {} platforms, and {} regions",
                        found_categories.len(),
                        found_platforms.len(),
                        found_regions.len()
                    )));
                    *categories.borrow_mut() = found_categories;
                    *platforms.borrow_mut() = found_platforms;
                    *regions.borrow_mut() = found_regions;
                    populating.set(false);
                    if auto_category.replace(false) {
                        if let Some(index) = categories.borrow().iter().position(|category| {
                            category.name.eq_ignore_ascii_case(&initial_category)
                        }) {
                            category_completion.select(index);
                        }
                    }
                }
                Err(error) => status.set_tooltip_text(Some(&format!(
                    "Speedrun.com metadata lookup failed: {error}"
                ))),
            }
            glib::ControlFlow::Break
        });
    });
    let chosen_platforms = platform_results.clone();
    let chosen_platform_editor = editor.clone();
    let platform_populating = populating_suggestions.clone();
    let selected_platform_entry = src_platforms.clone();
    src_platforms.connect_selected(move |index| {
        if platform_populating.get() {
            return;
        }
        let Some(platform) = chosen_platforms.borrow().get(index).cloned() else {
            return;
        };
        if let Some(editor) = chosen_platform_editor.borrow_mut().as_mut() {
            editor.set_platform_name(platform.name.as_str());
        }
        selected_platform_entry.set_text(&platform.name);
    });
    let chosen_regions = region_results.clone();
    let chosen_region_editor = editor.clone();
    let region_populating = populating_suggestions.clone();
    let selected_region_entry = src_regions.clone();
    src_regions.connect_selected(move |index| {
        if region_populating.get() {
            return;
        }
        let Some(region) = chosen_regions.borrow().get(index).cloned() else {
            return;
        };
        if let Some(editor) = chosen_region_editor.borrow_mut().as_mut() {
            editor.set_region_name(region.name.as_str());
        }
        selected_region_entry.set_text(&region.name);
    });
    let chosen_categories = category_results.clone();
    let chosen_editor = editor.clone();
    let chosen_category_entry = category.clone();
    let chosen_status = category.clone();
    let chosen_spinner = src_spinner.clone();
    let chosen_variables_group = details.src_variables.clone();
    let chosen_empty_variables = details.empty_variables.clone();
    let chosen_variable_rows = details.dynamic_variable_rows.clone();
    let category_populating = populating_suggestions;
    let chosen_variable_generation = variable_generation;
    category.connect_selected(move |index| {
        if category_populating.get() {
            return;
        }
        let Some(category) = chosen_categories.borrow().get(index).cloned() else {
            return;
        };
        let generation = chosen_variable_generation.get() + 1;
        chosen_variable_generation.set(generation);
        chosen_category_entry.set_text(&category.name);
        if let Some(editor) = chosen_editor.borrow_mut().as_mut() {
            editor.set_category_name(category.name.as_str());
        }
        chosen_spinner.set_spinning(true);
        chosen_status.set_tooltip_text(Some("Loading speedrun.com category variables…"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(
                crate::speedrun_com::variables(&category.id).map_err(|error| error.to_string()),
            );
        });
        let editor = chosen_editor.clone();
        let status = chosen_status.clone();
        let spinner = chosen_spinner.clone();
        let variables_group = chosen_variables_group.clone();
        let empty_variables = chosen_empty_variables.clone();
        let variable_rows = chosen_variable_rows.clone();
        let generation_state = chosen_variable_generation.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let Ok(response) = rx.try_recv() else {
                return glib::ControlFlow::Continue;
            };
            if generation_state.get() != generation {
                return glib::ControlFlow::Break;
            }
            spinner.set_spinning(false);
            match response {
                Ok(variables) => {
                    if let Some(editor) = editor.borrow_mut().as_mut() {
                        let old_variables = editor
                            .run()
                            .metadata()
                            .speedrun_com_variables()
                            .map(|(name, value)| (name.to_owned(), value.clone()))
                            .collect::<HashMap<_, _>>();
                        for name in old_variables.keys() {
                            editor.remove_speedrun_com_variable(name);
                        }
                        for variable in &variables {
                            let value = old_variables
                                .get(&variable.name)
                                .or(variable.default.as_ref())
                                .or_else(|| {
                                    variable
                                        .mandatory
                                        .then(|| variable.values.first())
                                        .flatten()
                                });
                            if let Some(value) = value {
                                editor.set_speedrun_com_variable(
                                    variable.name.as_str(),
                                    value.as_str(),
                                );
                            }
                        }
                    }
                    populate_speedrun_com_variable_rows(
                        &variables_group,
                        &empty_variables,
                        &variable_rows,
                        &editor,
                        &variables,
                    );
                    status.set_tooltip_text(Some(&format!(
                        "Loaded {} speedrun.com category variables",
                        variables.len()
                    )));
                }
                Err(error) => status.set_tooltip_text(Some(&format!(
                    "Speedrun.com variable lookup failed: {error}"
                ))),
            }
            glib::ControlFlow::Break
        });
    });
    if initial_game_name.trim().len() >= 2 {
        game.entry.emit_by_name::<()>("changed", &[]);
    }
    let offset = adw::EntryRow::builder()
        .title("Start Timer At")
        .text(&state.offset)
        .build();
    let offset_editor = editor.clone();
    offset.connect_changed(move |row| {
        let valid = offset_editor.borrow_mut().as_mut().is_some_and(|editor| {
            editor
                .parse_and_set_offset(row.text().as_str(), livesplit_core::Lang::English)
                .is_ok()
        });
        if valid {
            row.remove_css_class("error");
        } else {
            row.add_css_class("error");
        }
    });
    general.add(&offset);
    let attempts = adw::SpinRow::builder()
        .title("Attempts")
        .adjustment(&gtk::Adjustment::new(
            state.attempts as f64,
            0.0,
            u32::MAX as f64,
            1.0,
            10.0,
            0.0,
        ))
        .digits(0)
        .build();
    let attempts_editor = editor.clone();
    attempts.connect_value_notify(move |row| {
        if let Some(editor) = attempts_editor.borrow_mut().as_mut() {
            editor.set_attempt_count(row.value() as u32);
        }
    });
    general.add(&attempts);
    let game_icon = gtk::Box::new(gtk::Orientation::Vertical, 6);
    game_icon.set_halign(gtk::Align::Center);
    let game_icon_preview = gtk::Image::new();
    game_icon_preview.set_pixel_size(96);
    game_icon_preview.set_size_request(112, 112);
    game_icon_preview.add_css_class("icon-dropshadow");
    if let Some(editor) = editor.borrow().as_ref() {
        set_icon_preview(&game_icon_preview, editor.run().game_icon().data());
    }
    let game_icon_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    game_icon_frame.add_css_class("card");
    game_icon_frame.set_size_request(112, 112);
    game_icon_frame.set_halign(gtk::Align::Center);
    game_icon_frame.set_valign(gtk::Align::Start);
    game_icon_frame.append(&game_icon_preview);
    game_icon.append(&game_icon_frame);
    let game_icon_actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    game_icon_actions.set_halign(gtk::Align::Center);
    let choose_game_icon = gtk::Button::with_label("Choose…");
    let game_icon_editor = editor.clone();
    let selected_game_preview = game_icon_preview.clone();
    choose_game_icon.connect_clicked(move |button| {
        let icon_editor = game_icon_editor.clone();
        let preview = selected_game_preview.clone();
        open_icon_file(button, move |image| {
            set_icon_preview(&preview, image.data());
            if let Some(editor) = icon_editor.borrow_mut().as_mut() {
                editor.set_game_icon(image);
            }
        });
    });
    game_icon_actions.append(&choose_game_icon);
    let remove_game_icon = gtk::Button::from_icon_name("edit-delete-symbolic");
    remove_game_icon.set_tooltip_text(Some("Remove game icon"));
    let remove_icon_editor = editor.clone();
    let removed_game_preview = game_icon_preview.clone();
    remove_game_icon.connect_clicked(move |_| {
        if let Some(editor) = remove_icon_editor.borrow_mut().as_mut() {
            editor.remove_game_icon();
        }
        set_icon_preview(&removed_game_preview, &[]);
    });
    game_icon_actions.append(&remove_game_icon);
    game_icon.append(&game_icon_actions);
    let run_header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    run_header.append(&game_icon);
    run_header.append(&general);
    content.append(&run_header);
    content.append(&page_switcher);

    let use_layout = gtk::Switch::new();
    use_layout.set_valign(gtk::Align::Center);
    let linked_layout = editor
        .borrow()
        .as_ref()
        .and_then(|editor| editor.run().linked_layout().cloned())
        .unwrap_or(livesplit_core::run::LinkedLayout::Default);
    use_layout.set_active(
        editor
            .borrow()
            .as_ref()
            .is_some_and(|editor| editor.run().linked_layout().is_some()),
    );
    let linked_editor = editor.clone();
    use_layout.connect_active_notify(move |button| {
        if let Some(editor) = linked_editor.borrow_mut().as_mut() {
            editor.set_linked_layout(button.is_active().then(|| linked_layout.clone()));
        }
    });
    #[cfg(feature = "auto-splitting")]
    splits_page.append(&build_run_auto_splitter_section(
        &window,
        &state.game,
        &pending_auto_splitter,
        &auto_splitter,
        &timer,
        can_associate_auto_splitter,
    ));

    let segment_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let segment_title = gtk::Label::builder()
        .label("Segments")
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["heading"])
        .build();
    segment_header.append(&segment_title);
    let timing = gtk::DropDown::from_strings(&["Real Time", "Game Time"]);
    timing.set_selected(match state.timing_method {
        livesplit_core::TimingMethod::RealTime => 0,
        livesplit_core::TimingMethod::GameTime => 1,
    });
    segment_header.append(&timing);

    let columns = gtk::Grid::builder()
        .column_spacing(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let column_groups = Rc::new(
        (0..5 + state.comparison_names.len())
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect::<Vec<_>>(),
    );
    for (column, title) in ["Icon", "Name", "Split Time", "Segment Time", "Best Segment"]
        .iter()
        .enumerate()
    {
        let label = gtk::Label::builder().label(*title).xalign(0.0).build();
        label.add_css_class("dim-label");
        column_groups[column].add_widget(&label);
        columns.attach(&label, column as i32, 0, 1, 1);
    }
    for (index, comparison) in state.comparison_names.iter().enumerate() {
        let label = gtk::Label::builder().label(comparison).xalign(0.0).build();
        label.add_css_class("dim-label");
        column_groups[index + 5].add_widget(&label);
        columns.attach(&label, index as i32 + 5, 0, 1, 1);
    }

    let segments = gtk::ListBox::new();
    segments.set_selection_mode(gtk::SelectionMode::Multiple);
    segments.add_css_class("boxed-list");
    populate_run_segments(&segments, &editor, &column_groups);
    let selection_editor = editor.clone();
    segments.connect_selected_rows_changed(move |list| {
        let selected = list.selected_rows();
        let Some((first, rest)) = selected.split_first() else {
            return;
        };
        if let Some(editor) = selection_editor.borrow_mut().as_mut() {
            editor.select_only(first.index() as usize);
            for row in rest {
                editor.select_additionally(row.index() as usize);
            }
        }
    });
    let scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .min_content_height(220)
        .child(&segments)
        .build();
    let table = gtk::Box::new(gtk::Orientation::Vertical, 6);
    table.set_hexpand(true);
    table.append(&segment_header);
    let history_revealer = gtk::Revealer::new();
    history_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    let history_count = editor
        .borrow()
        .as_ref()
        .map_or(0, |editor| editor.run().attempt_history().len());
    let history_summary = adw::ActionRow::builder()
        .title(format!("{history_count} recorded attempts"))
        .subtitle("Use Other… to clear attempt history or all times")
        .build();
    history_revealer.set_child(Some(&history_summary));
    table.append(&history_revealer);
    table.append(&columns);
    table.append(&scroller);

    let command_rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    command_rail.set_size_request(132, -1);
    let history = gtk::Button::with_label("History ▾");
    let shown_history = history_revealer.clone();
    history.connect_clicked(move |button| {
        let reveal = !shown_history.reveals_child();
        shown_history.set_reveal_child(reveal);
        button.set_label(if reveal { "History ▴" } else { "History ▾" });
    });
    command_rail.append(&history);
    for (label, operation) in [
        ("Insert Above", 0_u8),
        ("Add Below", 1),
        ("Remove", 2),
        ("Move Up", 3),
        ("Move Down", 4),
    ] {
        let button = gtk::Button::with_label(label);
        let button_editor = editor.clone();
        let button_list = segments.clone();
        let button_groups = column_groups.clone();
        button.connect_clicked(move |_| {
            if let Some(editor) = button_editor.borrow_mut().as_mut() {
                match operation {
                    0 => editor.insert_segment_above(),
                    1 => editor.insert_segment_below(),
                    2 => editor.remove_segments(),
                    3 => editor.move_segments_up(),
                    _ => editor.move_segments_down(),
                }
            }
            populate_run_segments(&button_list, &button_editor, &button_groups);
        });
        command_rail.append(&button);
    }
    let comparisons_button = gtk::Button::with_label("Comparisons…");
    let comparison_pages = pages.clone();
    comparisons_button.connect_clicked(move |_| {
        comparison_pages.set_visible_child_name("comparisons");
    });
    command_rail.append(&comparisons_button);
    let tools = gtk::MenuButton::builder()
        .label("Other…")
        .menu_model(&run_tools_menu())
        .build();
    install_run_tool_actions(&window, &editor, &segments, &column_groups);
    command_rail.append(&tools);
    let editor_body = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    editor_body.append(&command_rail);
    editor_body.append(&table);
    splits_page.append(&editor_body);
    pages.add_titled_with_icon(&splits_page, Some("splits"), "Splits", "view-list-symbolic");

    pages.add_titled_with_icon(
        &details.page,
        Some("details"),
        "Additional Info",
        "document-properties-symbolic",
    );

    let comparisons_page = build_run_comparisons_page(&editor, &segments, &column_groups);
    pages.add_titled_with_icon(
        &comparisons_page,
        Some("comparisons"),
        "Comparisons",
        "view-sort-ascending-symbolic",
    );
    content.append(&pages);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let link_layout = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    link_layout.set_valign(gtk::Align::Center);
    link_layout.append(&use_layout);
    link_layout.append(&gtk::Label::new(Some("Link Layout")));
    actions.append(&link_layout);
    let action_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    action_spacer.set_hexpand(true);
    actions.append(&action_spacer);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    actions.append(&cancel);
    actions.append(&apply);
    content.append(&actions);

    let timing_editor = editor.clone();
    let timing_list = segments.clone();
    let timing_groups = column_groups.clone();
    timing.connect_selected_notify(move |dropdown| {
        if let Some(editor) = timing_editor.borrow_mut().as_mut() {
            editor.select_timing_method(if dropdown.selected() == 0 {
                livesplit_core::TimingMethod::RealTime
            } else {
                livesplit_core::TimingMethod::GameTime
            });
        }
        populate_run_segments(&timing_list, &timing_editor, &timing_groups);
    });

    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));
    let cancel_window = window.clone();
    cancel.connect_clicked(move |_| cancel_window.close());
    let apply_window = window.clone();
    let apply_editor = editor.clone();
    let apply_sender = sender.clone();
    #[cfg(feature = "auto-splitting")]
    let applied_on_apply = auto_splitter_applied.clone();
    apply.connect_clicked(move |_| {
        if let Some(run) = apply_editor
            .borrow_mut()
            .take()
            .map(|editor| editor.close())
        {
            apply_sender.input(AppMsg::ApplyRun(RunDraft(Arc::new(Mutex::new(Some(run))))));
        }
        #[cfg(feature = "auto-splitting")]
        {
            applied_on_apply.set(true);
            apply_sender.input(AppMsg::ApplyAutoSplitterAssociation(
                pending_auto_splitter.borrow().clone(),
            ));
        }
        apply_window.close();
    });
    let close_sender = sender.clone();
    #[cfg(feature = "auto-splitting")]
    let restore_runtime = auto_splitter.clone();
    #[cfg(feature = "auto-splitting")]
    let restore_timer = timer.clone();
    window.connect_close_request(move |_| {
        #[cfg(feature = "auto-splitting")]
        if !auto_splitter_applied.get() {
            let _ = restore_runtime.unload();
            if let Some(association) = &original_auto_splitter {
                if restore_runtime
                    .load(association.path().into(), restore_timer.clone())
                    .is_ok()
                {
                    restore_runtime.set_settings_map(original_auto_splitter_settings.clone());
                }
            }
        }
        close_sender.input(AppMsg::EditorFinished(EditorKind::Run));
        glib::Propagation::Proceed
    });
    window
}

fn populate_run_segments(
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

#[cfg(feature = "auto-splitting")]
fn auto_splitter_summary(association: Option<&AutoSplitterAssociation>) -> String {
    match association {
        Some(AutoSplitterAssociation::Local { path }) => format!("Local — {}", path.display()),
        Some(AutoSplitterAssociation::Registry {
            game, cached_path, ..
        }) => format!("Registry — {game} ({})", cached_path.display()),
        None => "No auto splitter associated".to_owned(),
    }
}

#[cfg(feature = "auto-splitting")]
fn build_run_auto_splitter_section(
    parent: &adw::ApplicationWindow,
    game: &str,
    pending: &Rc<RefCell<Option<AutoSplitterAssociation>>>,
    runtime: &Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
    timer: &SharedTimer,
    can_associate: bool,
) -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title("Auto Splitter")
        .subtitle(auto_splitter_summary(pending.borrow().as_ref()))
        .build();
    let settings = gtk::Button::with_label("Settings…");
    settings.set_sensitive(pending.borrow().is_some());
    let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
    remove.set_tooltip_text(Some("Remove auto splitter"));
    let registry = gtk::Button::with_label("Find in Registry");
    registry.set_sensitive(can_associate);
    let registry_game = game.to_owned();
    let registry_pending = pending.clone();
    let registry_runtime = runtime.clone();
    let registry_timer = timer.clone();
    let registry_row = row.clone();
    let registry_settings = settings.clone();
    registry.connect_clicked(move |button| {
        if registry_game.trim().is_empty() {
            registry_row.set_subtitle("Enter a game name before searching the registry");
            return;
        }
        button.set_sensitive(false);
        registry_row.set_subtitle("Checking the LiveSplit auto splitter registry…");
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let game = registry_game.clone();
        std::thread::spawn(move || {
            let result = crate::autosplitter_registry::load_cached_or_refresh()
                .and_then(|entries| {
                    crate::autosplitter_registry::matching(&entries, &game)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("No registry entry matches {game}"))
                })
                .and_then(|entry| {
                    if !entry.installable {
                        anyhow::bail!("The matching entry is {}", entry.compatibility.label());
                    }
                    let download = crate::autosplitter_registry::download_to_temporary(&entry)?;
                    Ok((entry, download))
                })
                .map_err(|error| error.to_string());
            let _ = result_sender.send(result);
        });
        let pending = registry_pending.clone();
        let runtime = registry_runtime.clone();
        let timer = registry_timer.clone();
        let status = registry_row.clone();
        let button = button.clone();
        let settings = registry_settings.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let Ok(result) = result_receiver.try_recv() else {
                return glib::ControlFlow::Continue;
            };
            button.set_sensitive(true);
            match result {
                Ok((entry, (temporary, destination, url))) => {
                    if timer.read().unwrap().current_phase()
                        != livesplit_core::TimerPhase::NotRunning
                    {
                        let _ = std::fs::remove_file(temporary);
                        status.set_subtitle("Reset the timer before changing the auto splitter");
                    } else if let Err(error) = runtime.load(temporary.clone(), timer.clone()) {
                        let _ = std::fs::remove_file(temporary);
                        status.set_subtitle(&format!("Auto splitter failed to load: {error}"));
                    } else if let Err(error) = std::fs::rename(&temporary, &destination) {
                        let _ = runtime.unload();
                        status.set_subtitle(&format!("Could not install auto splitter: {error}"));
                    } else {
                        let association = AutoSplitterAssociation::Registry {
                            game: entry.game,
                            last_url: url,
                            cached_path: destination,
                        };
                        status.set_subtitle(&auto_splitter_summary(Some(&association)));
                        *pending.borrow_mut() = Some(association);
                        settings.set_sensitive(true);
                    }
                }
                Err(error) => status.set_subtitle(&error),
            }
            glib::ControlFlow::Break
        });
    });
    row.add_suffix(&registry);

    let local = gtk::Button::with_label("Select Local…");
    local.set_sensitive(can_associate);
    let local_parent = parent.clone();
    let local_pending = pending.clone();
    let local_runtime = runtime.clone();
    let local_timer = timer.clone();
    let local_row = row.clone();
    let local_settings = settings.clone();
    local.connect_clicked(move |_| {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("WASM Auto Splitters"));
        filter.add_pattern("*.wasm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Select Local WASM Auto Splitter")
            .filters(&filters)
            .build();
        let pending = local_pending.clone();
        let runtime = local_runtime.clone();
        let timer = local_timer.clone();
        let status = local_row.clone();
        let settings = local_settings.clone();
        dialog.open(Some(&local_parent), gio::Cancellable::NONE, move |result| {
            let Some(path) = result.ok().and_then(|file| file.path()) else {
                return;
            };
            if timer.read().unwrap().current_phase() != livesplit_core::TimerPhase::NotRunning {
                status.set_subtitle("Reset the timer before changing the auto splitter");
                return;
            }
            match runtime.load(path.clone(), timer.clone()) {
                Ok(()) => {
                    let association = AutoSplitterAssociation::Local { path };
                    status.set_subtitle(&auto_splitter_summary(Some(&association)));
                    *pending.borrow_mut() = Some(association);
                    settings.set_sensitive(true);
                }
                Err(error) => {
                    status.set_subtitle(&format!("Auto splitter failed to load: {error}"));
                }
            }
        });
    });
    row.add_suffix(&local);

    let settings_parent = parent.clone();
    let settings_runtime = runtime.clone();
    settings.connect_clicked(move |_| {
        open_auto_splitter_settings(&settings_parent, &settings_runtime);
    });
    row.add_suffix(&settings);
    let remove_pending = pending.clone();
    let remove_runtime = runtime.clone();
    let remove_row = row.clone();
    let remove_settings = settings.clone();
    remove.connect_clicked(move |_| match remove_runtime.unload() {
        Ok(()) => {
            *remove_pending.borrow_mut() = None;
            remove_row.set_subtitle(&auto_splitter_summary(None));
            remove_settings.set_sensitive(false);
        }
        Err(error) => remove_row.set_subtitle(&format!("Could not unload: {error}")),
    });
    row.add_suffix(&remove);
    if !can_associate {
        row.set_subtitle("Save the splits before associating an auto splitter");
    }
    row.upcast()
}

#[cfg(feature = "auto-splitting")]
fn open_auto_splitter_settings(
    parent: &adw::ApplicationWindow,
    runtime: &Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
) {
    use livesplit_core::auto_splitting::settings::{Value, WidgetKind};

    let window = adw::ApplicationWindow::builder()
        .title("Auto Splitter Settings")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(520)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Auto Splitter")
        .description("Changes are part of the Splits Editor transaction")
        .build();
    let widgets = runtime.settings_widgets().unwrap_or_default();
    let settings_map = runtime.settings_map().unwrap_or_default();
    if widgets.is_empty() {
        let loading = adw::ActionRow::builder()
            .title("Loading settings…")
            .subtitle("Waiting for the auto splitter to initialize")
            .build();
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        loading.add_suffix(&spinner);
        group.add(&loading);
        let attempts = Rc::new(Cell::new(0_u8));
        let retry_runtime = runtime.clone();
        let retry_parent = parent.clone();
        let retry_window = window.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if retry_runtime
                .settings_widgets()
                .is_some_and(|widgets| !widgets.is_empty())
            {
                retry_window.close();
                open_auto_splitter_settings(&retry_parent, &retry_runtime);
                return glib::ControlFlow::Break;
            }
            let next = attempts.get() + 1;
            attempts.set(next);
            if next >= 20 {
                spinner.set_spinning(false);
                spinner.set_visible(false);
                loading.set_title("No configurable settings");
                loading.set_subtitle("The loaded auto splitter did not expose any settings");
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    for widget in widgets.iter() {
        match &widget.kind {
            WidgetKind::Title { .. } => {
                let title = adw::ActionRow::builder()
                    .title(widget.description.as_ref())
                    .build();
                title.add_css_class("property");
                group.add(&title);
            }
            WidgetKind::Bool { default_value } => {
                let value = settings_map
                    .get(widget.key.as_ref())
                    .and_then(|value| match value {
                        Value::Bool(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(*default_value);
                let row = adw::SwitchRow::builder()
                    .title(widget.description.as_ref())
                    .active(value)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let runtime = runtime.clone();
                row.connect_active_notify(move |row| {
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::Bool(row.is_active()));
                    runtime.set_settings_map(map);
                });
                group.add(&row);
            }
            WidgetKind::Choice {
                default_option_key,
                options,
            } => {
                let model = gtk::StringList::new(
                    &options
                        .iter()
                        .map(|option| option.description.as_ref())
                        .collect::<Vec<_>>(),
                );
                let current_key = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .unwrap_or(default_option_key);
                let selected = options
                    .iter()
                    .position(|option| option.key.as_ref() == current_key.as_ref())
                    .unwrap_or(0) as u32;
                let row = adw::ComboRow::builder()
                    .title(widget.description.as_ref())
                    .model(&model)
                    .selected(selected)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let options = options.clone();
                let runtime = runtime.clone();
                row.connect_selected_notify(move |row| {
                    let Some(option) = options.get(row.selected() as usize) else {
                        return;
                    };
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::String(option.key.clone()));
                    runtime.set_settings_map(map);
                });
                group.add(&row);
            }
            WidgetKind::FileSelect { filters } => {
                let current_path = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .map_or_else(String::new, ToString::to_string);
                let row = adw::ActionRow::builder()
                    .title(widget.description.as_ref())
                    .subtitle(current_path)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let choose = gtk::Button::with_label("Choose…");
                let key = widget.key.clone();
                let runtime = runtime.clone();
                let parent = window.clone();
                let status = row.clone();
                let filters = filters.clone();
                choose.connect_clicked(move |_| {
                    let dialog = gtk::FileDialog::builder().title("Select File").build();
                    let gtk_filters = gio::ListStore::new::<gtk::FileFilter>();
                    for filter in filters.iter() {
                        let gtk_filter = gtk::FileFilter::new();
                        match filter {
                            livesplit_core::auto_splitting::settings::FileFilter::Name {
                                description,
                                pattern,
                            } => {
                                gtk_filter.set_name(description.as_deref());
                                for pattern in pattern.split_ascii_whitespace() {
                                    gtk_filter.add_pattern(pattern);
                                }
                            }
                            livesplit_core::auto_splitting::settings::FileFilter::MimeType(
                                mime,
                            ) => {
                                gtk_filter.set_name(Some(mime));
                                gtk_filter.add_mime_type(mime);
                            }
                        }
                        gtk_filters.append(&gtk_filter);
                    }
                    if gtk_filters.n_items() != 0 {
                        dialog.set_filters(Some(&gtk_filters));
                    }
                    let key = key.clone();
                    let runtime = runtime.clone();
                    let status = status.clone();
                    dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
                        let Some(path) = result.ok().and_then(|file| file.path()) else {
                            return;
                        };
                        let Some(path_value) =
                            livesplit_core::auto_splitting::wasi_path::from_native(&path)
                        else {
                            return;
                        };
                        let mut map = runtime.settings_map().unwrap_or_default();
                        map.insert(key.clone(), Value::String(path_value.into()));
                        runtime.set_settings_map(map);
                        status.set_subtitle(&path.to_string_lossy());
                    });
                });
                row.add_suffix(&choose);
                group.add(&row);
            }
        }
    }
    page.add(&group);
    toolbar.set_content(Some(&page));
    window.set_content(Some(&toolbar));
    window.present();
}

fn open_icon_file(
    button: &gtk::Button,
    selected: impl Fn(livesplit_core::settings::Image) + 'static,
) {
    let Some(window) = button.root().and_downcast::<gtk::Window>() else {
        return;
    };
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    filter.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Select Icon")
        .filters(&filters)
        .build();
    dialog.open(Some(&window), gio::Cancellable::NONE, move |result| {
        let Some(path) = result.ok().and_then(|file| file.path()) else {
            return;
        };
        let mut buffer = Vec::new();
        if let Ok(image) = livesplit_core::settings::Image::from_file(
            path,
            &mut buffer,
            livesplit_core::settings::Image::ICON,
        ) {
            selected(image);
        }
    });
}

fn set_icon_preview(preview: &gtk::Image, data: &[u8]) {
    let texture = (!data.is_empty())
        .then(|| gdk::Texture::from_bytes(&glib::Bytes::from(data)))
        .and_then(Result::ok);
    if let Some(texture) = texture {
        preview.set_paintable(Some(&texture));
        preview.set_tooltip_text(Some("Current icon"));
    } else {
        preview.set_icon_name(Some("image-missing-symbolic"));
        preview.set_tooltip_text(Some("No icon"));
    }
}

struct RunDetailsPage {
    page: adw::PreferencesPage,
    platform_entry: CompletionEntryRow,
    region_entry: CompletionEntryRow,
    src_variables: adw::PreferencesGroup,
    empty_variables: adw::ActionRow,
    dynamic_variable_rows: Rc<RefCell<Vec<gtk::Widget>>>,
}

fn build_run_details_page(
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    state: &livesplit_core::run::editor::State,
) -> RunDetailsPage {
    let page = adw::PreferencesPage::new();
    let metadata = adw::PreferencesGroup::builder().title("Metadata").build();
    let platform_entry = CompletionEntryRow::new("Platform", &state.metadata.platform_name);
    let region_entry = CompletionEntryRow::new("Region", &state.metadata.region_name);
    for (row, field) in [(&platform_entry, 0_u8), (&region_entry, 1)] {
        let row_editor = editor.clone();
        row.connect_changed(move |row| {
            if let Some(editor) = row_editor.borrow_mut().as_mut() {
                match field {
                    0 => editor.set_platform_name(row.text().as_str()),
                    1 => editor.set_region_name(row.text().as_str()),
                    _ => unreachable!(),
                }
            }
        });
        metadata.add(&row.row);
    }
    let run_id = adw::EntryRow::builder()
        .title("speedrun.com Run ID")
        .text(&state.metadata.run_id)
        .build();
    let run_id_editor = editor.clone();
    run_id.connect_changed(move |row| {
        if let Some(editor) = run_id_editor.borrow_mut().as_mut() {
            editor.set_run_id(row.text().as_str());
        }
    });
    metadata.add(&run_id);
    let emulator = adw::SwitchRow::builder()
        .title("Emulator")
        .subtitle("This run was performed using an emulator")
        .active(state.metadata.uses_emulator)
        .build();
    let emulator_editor = editor.clone();
    emulator.connect_active_notify(move |row| {
        if let Some(editor) = emulator_editor.borrow_mut().as_mut() {
            editor.set_emulator_usage(row.is_active());
        }
    });
    metadata.add(&emulator);
    page.add(&metadata);

    let src_variables = adw::PreferencesGroup::builder()
        .title("speedrun.com Variables")
        .description("Category variables stored with this run")
        .build();
    let dynamic_variable_rows = Rc::new(RefCell::new(Vec::new()));
    for (name, value) in state.metadata.speedrun_com_variables() {
        let name = name.to_owned();
        let row = adw::EntryRow::builder().title(&name).text(value).build();
        let variable_editor = editor.clone();
        let name = name.clone();
        row.connect_changed(move |row| {
            if let Some(editor) = variable_editor.borrow_mut().as_mut() {
                if row.text().is_empty() {
                    editor.remove_speedrun_com_variable(&name);
                } else {
                    editor.set_speedrun_com_variable(name.as_str(), row.text().as_str());
                }
            }
        });
        src_variables.add(&row);
        dynamic_variable_rows.borrow_mut().push(row.upcast());
    }
    let empty_variables = adw::ActionRow::builder()
        .title("No category variables")
        .subtitle("Selecting a category suggestion can populate these values")
        .visible(state.metadata.speedrun_com_variables().next().is_none())
        .build();
    src_variables.add(&empty_variables);
    page.add(&src_variables);

    let custom_variables = adw::PreferencesGroup::builder()
        .title("Custom Variables")
        .build();
    for (name, variable) in state
        .metadata
        .custom_variables()
        .filter(|(_, variable)| variable.is_permanent)
    {
        let name = name.to_owned();
        let row = adw::EntryRow::builder()
            .title(&name)
            .text(&variable.value)
            .build();
        let variable_editor = editor.clone();
        let edit_name = name.clone();
        row.connect_changed(move |row| {
            if let Some(editor) = variable_editor.borrow_mut().as_mut() {
                editor.set_custom_variable(edit_name.as_str(), row.text().as_str());
            }
        });
        let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
        remove.set_tooltip_text(Some("Remove variable"));
        let remove_editor = editor.clone();
        let remove_name = name;
        let remove_row = row.clone();
        remove.connect_clicked(move |_| {
            if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                editor.remove_custom_variable(&remove_name);
            }
            remove_row.set_visible(false);
        });
        row.add_suffix(&remove);
        custom_variables.add(&row);
    }
    let add_variable = adw::EntryRow::builder()
        .title("Add Custom Variable")
        .show_apply_button(true)
        .build();
    let add_editor = editor.clone();
    add_variable.connect_apply(move |row| {
        let name = row.text();
        if !name.trim().is_empty() {
            if let Some(editor) = add_editor.borrow_mut().as_mut() {
                editor.add_custom_variable(name.as_str());
            }
            row.set_text("");
        }
    });
    custom_variables.add(&add_variable);
    page.add(&custom_variables);
    RunDetailsPage {
        page,
        platform_entry,
        region_entry,
        src_variables,
        empty_variables,
        dynamic_variable_rows,
    }
}

fn populate_speedrun_com_variable_rows(
    group: &adw::PreferencesGroup,
    empty: &adw::ActionRow,
    dynamic_rows: &Rc<RefCell<Vec<gtk::Widget>>>,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    variables: &[crate::speedrun_com::Variable],
) {
    for row in dynamic_rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    empty.set_visible(variables.is_empty());
    for variable in variables {
        let suffix = if variable.is_subcategory {
            " (subcategory)"
        } else if variable.mandatory {
            " (required)"
        } else if variable.user_defined {
            " (custom)"
        } else {
            ""
        };
        let title = format!("{}{suffix}", variable.name);
        let current = editor
            .borrow()
            .as_ref()
            .and_then(|editor| {
                editor
                    .run()
                    .metadata()
                    .speedrun_com_variables()
                    .find(|(name, _)| *name == variable.name.as_str())
                    .map(|(_, value)| value.clone())
            })
            .or_else(|| variable.default.clone())
            .unwrap_or_default();
        let row = CompletionEntryRow::new(&title, &current);
        let labels = variable
            .values
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        row.set_items(&labels);
        let variable_editor = editor.clone();
        let name = variable.name.clone();
        row.connect_changed(move |entry| {
            if let Some(editor) = variable_editor.borrow_mut().as_mut() {
                if entry.text().is_empty() {
                    editor.remove_speedrun_com_variable(&name);
                } else {
                    editor.set_speedrun_com_variable(name.as_str(), entry.text().as_str());
                }
            }
        });
        group.add(&row.row);
        dynamic_rows.borrow_mut().push(row.row.upcast());
    }
}

fn build_run_comparisons_page(
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    segments: &gtk::ListBox,
    column_groups: &Rc<Vec<gtk::SizeGroup>>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Custom Comparisons")
        .description(
            "Comparison times are edited in the segment table after selecting a timing method",
        )
        .build();
    let names = editor
        .borrow()
        .as_ref()
        .unwrap()
        .custom_comparisons()
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();
    for name in names {
        let row = adw::ActionRow::builder().title(&name).build();
        let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
        remove.set_tooltip_text(Some("Remove comparison"));
        let remove_editor = editor.clone();
        let remove_segments = segments.clone();
        let remove_groups = column_groups.clone();
        let remove_name = name.clone();
        let remove_row = row.clone();
        remove.connect_clicked(move |_| {
            if let Some(editor) = remove_editor.borrow_mut().as_mut() {
                editor.remove_comparison(&remove_name);
            }
            remove_row.set_visible(false);
            populate_run_segments(&remove_segments, &remove_editor, &remove_groups);
        });
        row.add_suffix(&remove);
        group.add(&row);
    }
    let add = adw::EntryRow::builder()
        .title("Add Comparison")
        .show_apply_button(true)
        .build();
    let add_editor = editor.clone();
    let add_segments = segments.clone();
    let add_groups = column_groups.clone();
    add.connect_apply(move |row| {
        let name = row.text();
        let valid = !name.trim().is_empty()
            && add_editor
                .borrow_mut()
                .as_mut()
                .is_some_and(|editor| editor.add_comparison(name.as_str()).is_ok());
        if valid {
            row.remove_css_class("error");
            row.set_text("");
            populate_run_segments(&add_segments, &add_editor, &add_groups);
        } else {
            row.add_css_class("error");
        }
    });
    group.add(&add);
    page.add(&group);
    page
}

fn run_tools_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Clear History"), Some("win.clear-run-history"));
    menu.append(Some("Clear Times"), Some("win.clear-run-times"));
    menu
}

fn install_run_tool_actions(
    window: &adw::ApplicationWindow,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    segments: &gtk::ListBox,
    column_groups: &Rc<Vec<gtk::SizeGroup>>,
) {
    for (name, clear_times) in [("clear-run-history", false), ("clear-run-times", true)] {
        let action = gio::SimpleAction::new(name, None);
        let action_editor = editor.clone();
        let action_segments = segments.clone();
        let action_groups = column_groups.clone();
        action.connect_activate(move |_, _| {
            if let Some(editor) = action_editor.borrow_mut().as_mut() {
                if clear_times {
                    editor.clear_times();
                } else {
                    editor.clear_history();
                }
            }
            populate_run_segments(&action_segments, &action_editor, &action_groups);
        });
        window.add_action(&action);
    }
}

fn build_layout_editor(
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
    }
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
        open_layout_settings_window(&double_parent, &double_editor, &double_cache, true);
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
            .build();
        let drag_handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        drag_handle.set_tooltip_text(Some("Drag to reorder"));
        drag_handle.set_cursor_from_name(Some("grab"));
        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        drag_source.set_content(Some(&gdk::ContentProvider::for_value(
            &(index as i32).to_value(),
        )));
        drag_handle.add_controller(drag_source);
        row_content.append(&label);
        row_content.append(&drag_handle);
        row.set_child(Some(&row_content));
        row.set_activatable(true);
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

fn select_file(
    window: &gtk::ApplicationWindow,
    save: bool,
    choice: FileChoice,
    sender: &ComponentSender<AppModel>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(match choice {
            FileChoice::OpenSplits => "Open Splits",
            FileChoice::SaveSplits => "Save Splits",
            FileChoice::OpenLayout => "Open Layout",
            FileChoice::SaveLayout => "Save Layout",
        })
        .build();
    let sender = sender.clone();
    let selected = move |result: Result<gio::File, glib::Error>| {
        let Some(path) = result.ok().and_then(|file| file.path()) else {
            return;
        };
        sender.input(match choice {
            FileChoice::OpenSplits => AppMsg::RequestAction(PendingAction::OpenSplits(path)),
            FileChoice::SaveSplits => AppMsg::SaveSplitsAs(path),
            FileChoice::OpenLayout => AppMsg::RequestAction(PendingAction::OpenLayout(path)),
            FileChoice::SaveLayout => AppMsg::SaveLayoutAs(path),
        });
    };
    if save {
        dialog.save(Some(window), gio::Cancellable::NONE, selected);
    } else {
        dialog.open(Some(window), gio::Cancellable::NONE, selected);
    }
}

fn choose_save_path(
    window: &gtk::ApplicationWindow,
    title: &str,
    selected: impl FnOnce(PathBuf) + 'static,
) {
    let dialog = gtk::FileDialog::builder().title(title).build();
    dialog.save(Some(window), gio::Cancellable::NONE, move |result| {
        if let Some(path) = result.ok().and_then(|file| file.path()) {
            selected(path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        physical_to_logical_size, relevant_changes, scroll_layout, should_mouse_passthrough,
        timer_resize_edge, Intent, LayoutData, PendingAction,
    };
    use gtk::gdk;
    use std::cell::RefCell;

    #[test]
    fn cancel_clears_pending_intent() {
        let mut pending = Intent::Exit;
        assert_eq!(pending, Intent::Exit);
        pending = Intent::None;
        assert_eq!(pending, Intent::None);
    }

    #[test]
    fn scrolling_releases_layout_borrow_before_rendering() {
        let layout = RefCell::new(LayoutData {
            layout: livesplit_core::Layout::default_layout(livesplit_core::Lang::English),
            layout_state: Default::default(),
            is_modified: false,
        });

        scroll_layout(&layout, 1.0);

        assert!(layout.try_borrow_mut().is_ok());
    }

    #[test]
    fn destructive_actions_only_guard_files_they_replace() {
        assert_eq!(
            relevant_changes(&PendingAction::NewSplits, true, true),
            (true, false)
        );
        assert_eq!(
            relevant_changes(&PendingAction::NewLayout, true, true),
            (false, true)
        );
        assert_eq!(
            relevant_changes(&PendingAction::Exit, true, true),
            (true, true)
        );
        assert_eq!(
            relevant_changes(
                &PendingAction::OpenSplits(std::path::PathBuf::from("next.lss")),
                true,
                true,
            ),
            (true, true)
        );
    }

    #[test]
    fn invisible_resize_border_classifies_edges_and_corners() {
        let edge = |x, y| timer_resize_edge(x, y, 300.0, 500.0, 8.0);
        assert_eq!(edge(0.0, 0.0), Some(gdk::SurfaceEdge::NorthWest));
        assert_eq!(edge(150.0, 0.0), Some(gdk::SurfaceEdge::North));
        assert_eq!(edge(299.0, 0.0), Some(gdk::SurfaceEdge::NorthEast));
        assert_eq!(edge(0.0, 250.0), Some(gdk::SurfaceEdge::West));
        assert_eq!(edge(299.0, 250.0), Some(gdk::SurfaceEdge::East));
        assert_eq!(edge(0.0, 499.0), Some(gdk::SurfaceEdge::SouthWest));
        assert_eq!(edge(150.0, 499.0), Some(gdk::SurfaceEdge::South));
        assert_eq!(edge(299.0, 499.0), Some(gdk::SurfaceEdge::SouthEast));
        assert_eq!(edge(150.0, 250.0), None);
    }

    #[test]
    fn pass_through_requires_running_and_an_inactive_window() {
        use livesplit_core::TimerPhase;

        assert!(should_mouse_passthrough(true, TimerPhase::Running, false));
        assert!(!should_mouse_passthrough(true, TimerPhase::Running, true));
        assert!(!should_mouse_passthrough(true, TimerPhase::Paused, false));
        assert!(!should_mouse_passthrough(false, TimerPhase::Running, false));
    }

    #[test]
    fn renderer_dimensions_are_converted_from_physical_pixels() {
        assert_eq!(physical_to_logical_size([601.0, 999.0], 2), (301, 500));
        assert_eq!(physical_to_logical_size([300.0, 500.0], 1), (300, 500));
        assert_eq!(physical_to_logical_size([0.0, 0.0], 0), (1, 1));
    }
}
