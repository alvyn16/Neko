mod icons;
pub mod input;

use crate::{
    config::{self, Config, RotationInterval, WallpaperFit},
    local,
    model::{Provider, Tab, Wallpaper},
    sources, wallpaper,
};
use gpui::{WindowControlArea, prelude::*, *};
use icons::icon;
use input::{InputEvent, TextInput};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const BG: u32 = 0x141416;
const SURFACE: u32 = 0x222226;
const TEXT: u32 = 0xeeeef0;
const MUTED: u32 = 0x85858f;
const ACCENT: u32 = 0xc4b1ef;

actions!(neko, [Refresh, ChooseFolder, FocusSearch, Dismiss]);

#[derive(Clone, Copy)]
struct DialogParent(RawWindowHandle);

impl HasWindowHandle for DialogParent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // The GPUI window remains alive while the picker is open, so its HWND is valid.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

impl HasDisplayHandle for DialogParent {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dialog {
    Preview,
    Rename,
    Delete,
    Settings,
}

struct Neko {
    config: Config,
    cache: PathBuf,
    query: Entity<TextInput>,
    rename_input: Entity<TextInput>,
    focus: FocusHandle,
    items: Vec<Wallpaper>,
    visible: Vec<usize>,
    selected: Option<Wallpaper>,
    dialog: Option<Dialog>,
    loading: bool,
    busy: bool,
    status: String,
    error: bool,
    generation: u64,
    rotation_generation: u64,
    active_query: String,
    page: u32,
    last_page: u32,
    total: usize,
    color: Option<String>,
    inflight: HashSet<String>,
    failed_thumbs: HashSet<String>,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

fn job<T: Send + 'static>(
    cx: &mut Context<Neko>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut Neko, T, &mut Context<Neko>) + 'static,
) {
    let task = cx.background_executor().spawn(async move { work() });
    cx.spawn(async move |this, cx| {
        let result = task.await;
        let _ = this.update(cx, |this, cx| done(this, result, cx));
    })
    .detach();
}

fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    glyph: &'static str,
    primary: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(36.))
        .px_3()
        .rounded(px(9.))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .bg(rgb(if primary { ACCENT } else { SURFACE }))
        .text_color(rgb(if primary { BG } else { TEXT }))
        .hover(move |s| s.bg(rgb(if primary { 0xd6c6f8 } else { 0x323238 })))
        .active(|s| s.opacity(0.75))
        .child(icon(glyph).absolute().left(px(12.)).top(px(10.)))
        .child(div().w_full().text_center().child(label.into()))
}

fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(36.))
        .px_3()
        .text_center()
        .rounded(px(9.))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .bg(rgb(if primary { ACCENT } else { SURFACE }))
        .text_color(rgb(if primary { BG } else { TEXT }))
        .hover(move |s| s.bg(rgb(if primary { 0xd6c6f8 } else { 0x323238 })))
        .active(|s| s.opacity(0.75))
        .child(div().w_full().text_center().child(label.into()))
}

impl Neko {
    fn new(cx: &mut Context<Self>) -> Self {
        let (config, initial_error) = match Config::load() {
            Ok(c) => (c, None),
            Err(e) => (
                Config::default(),
                Some(format!("Could not load settings: {e}")),
            ),
        };
        let (cache, cache_error) = match config::cache_dir() {
            Ok(p) => (p, None),
            Err(e) => (
                std::env::temp_dir().join("neko-cache"),
                Some(format!("Using temporary cache: {e}")),
            ),
        };
        let query = cx.new(|cx| TextInput::new("Search wallpapers…", cx));
        let rename_input = cx.new(|cx| TextInput::new("Wallpaper name", cx));
        let subscriptions = vec![
            cx.subscribe(&query, |this, _, event, cx| match event {
                InputEvent::Changed if this.config.last_tab == Tab::Local => {
                    this.filter_local(cx);
                }
                InputEvent::Submit if this.dialog.is_none() => this.refresh(false, cx),
                InputEvent::Escape => {
                    this.dialog = None;
                    cx.notify();
                }
                _ => {}
            }),
            cx.subscribe(&rename_input, |this, _, event, cx| match event {
                InputEvent::Submit => this.commit_rename(cx),
                InputEvent::Escape => {
                    this.dialog = Some(Dialog::Preview);
                    cx.notify();
                }
                _ => {}
            }),
        ];
        let mut this = Self {
            config,
            cache,
            query,
            rename_input,
            focus: cx.focus_handle(),
            items: vec![],
            visible: vec![],
            selected: None,
            dialog: None,
            loading: false,
            busy: false,
            status: initial_error.or(cache_error).unwrap_or_default(),
            error: false,
            generation: 0,
            rotation_generation: 0,
            active_query: String::new(),
            page: 0,
            last_page: 0,
            total: 0,
            color: None,
            inflight: HashSet::new(),
            failed_thumbs: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.refresh(false, cx);
        this.schedule_rotation(cx);
        this
    }

    fn persist(&mut self) {
        if let Err(e) = self.config.save() {
            self.status = format!("Could not remember settings: {e}");
            self.error = true;
        }
    }
    fn set_fit(&mut self, fit: WallpaperFit, cx: &mut Context<Self>) {
        if self.config.wallpaper_fit == fit {
            return;
        }
        self.config.wallpaper_fit = fit;
        self.error = false;
        self.persist();
        if !self.error {
            self.status = "Wallpaper fit updated".into();
        }
        cx.notify();
    }
    fn set_rotation(&mut self, interval: RotationInterval, cx: &mut Context<Self>) {
        if self.config.rotation_interval == interval {
            return;
        }
        self.config.rotation_interval = interval;
        self.error = false;
        self.persist();
        self.schedule_rotation(cx);
        if !self.error {
            self.status = if interval == RotationInterval::Off {
                "Automatic rotation is off".into()
            } else {
                "Automatic rotation is ready".into()
            };
        }
        cx.notify();
    }
    fn schedule_rotation(&mut self, cx: &mut Context<Self>) {
        self.rotation_generation = self.rotation_generation.wrapping_add(1);
        let generation = self.rotation_generation;
        let Some(duration) = self.config.rotation_interval.duration() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            Timer::after(duration).await;
            let _ = this.update(cx, move |this, cx| {
                if this.rotation_generation == generation {
                    this.rotate_wallpaper(cx);
                }
            });
        })
        .detach();
    }
    fn rotate_wallpaper(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            self.schedule_rotation(cx);
            return;
        }
        let Some(folder) = self.config.wallpaper_folder.clone() else {
            self.fail("Automatic rotation needs a wallpaper folder".into());
            self.schedule_rotation(cx);
            cx.notify();
            return;
        };
        self.busy = true;
        self.status = "Choosing the next wallpaper…".into();
        self.error = false;
        let cache = self.cache.clone();
        let fit = self.config.wallpaper_fit;
        let generation = self.rotation_generation;
        job(
            cx,
            move || -> anyhow::Result<String> {
                let paths = local::image_paths(&folder)?;
                anyhow::ensure!(
                    !paths.is_empty(),
                    "The wallpaper folder has no supported images"
                );
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let path = &paths[(seed % paths.len() as u128) as usize];
                let title = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                wallpaper::apply(path, &cache, fit)?;
                Ok(title)
            },
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(title) => {
                        this.status = format!("Rotated · {title}");
                        this.error = false;
                    }
                    Err(e) => this.fail(format!("Could not rotate wallpaper: {e:#}")),
                }
                if this.rotation_generation == generation {
                    this.schedule_rotation(cx);
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.config.last_tab == tab {
            return;
        }
        self.config.last_tab = tab;
        self.query.update(cx, |q, cx| q.set_value("", cx));
        self.persist();
        self.refresh(false, cx);
    }
    fn set_source(&mut self, source: Provider, cx: &mut Context<Self>) {
        if self.config.last_source == source {
            return;
        }
        self.config.last_source = source;
        self.color = None;
        self.persist();
        self.refresh(false, cx);
    }
    fn filter_local(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_lowercase();
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, w)| w.title.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
        cx.notify();
    }
    fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        self.generation += 1;
        self.items.clear();
        self.visible.clear();
        self.inflight.clear();
        self.failed_thumbs.clear();
        self.selected = None;
        self.dialog = None;
        self.page = 0;
        self.last_page = 0;
        self.total = 0;
        self.error = false;
        self.status.clear();
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
        if self.config.last_tab == Tab::Search {
            self.active_query = self.query.read(cx).value();
            self.search_page(1, force, cx);
            return;
        }
        self.loading = false;
        let Some(folder) = self.config.wallpaper_folder.clone() else {
            cx.notify();
            return;
        };
        self.loading = true;
        let cache = self.cache.clone();
        let generation = self.generation;
        job(
            cx,
            move || local::scan(&folder, &cache),
            move |this, result, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(items) => {
                        this.total = items.len();
                        this.items = items;
                        this.filter_local(cx);
                    }
                    Err(e) => this.fail(format!("Could not open folder: {e:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn search_page(&mut self, page: u32, force: bool, cx: &mut Context<Self>) {
        if page > 1 && self.query.read(cx).value() != self.active_query {
            self.refresh(false, cx);
            return;
        }
        self.loading = true;
        let generation = self.generation;
        let provider = self.config.last_source;
        let query = self.active_query.clone();
        let color = self.color.clone();
        let cache = self.cache.clone();
        job(
            cx,
            move || sources::search(provider, &query, color.as_deref(), page, &cache, force),
            move |this, result, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(result) => {
                        this.page = result.page;
                        this.last_page = result.last_page;
                        this.total = result.total;
                        this.status = result.notice.unwrap_or_default();
                        this.error = false;
                        let existing: HashSet<_> =
                            this.items.iter().map(|w| w.id.clone()).collect();
                        this.items.extend(
                            result
                                .items
                                .into_iter()
                                .filter(|w| !existing.contains(&w.id)),
                        );
                        this.visible = (0..this.items.len()).collect();
                        this.load_thumbnails(cx);
                    }
                    Err(e) => this.fail(format!("Could not load wallpapers: {e:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn load_thumbnails(&mut self, cx: &mut Context<Self>) {
        if self.config.last_tab != Tab::Search {
            return;
        }
        let available = 4_usize.saturating_sub(self.inflight.len());
        let pending: Vec<_> = self
            .items
            .iter()
            .filter(|w| {
                w.thumbnail_path.is_none()
                    && !self.inflight.contains(&w.id)
                    && !self.failed_thumbs.contains(&w.id)
            })
            .take(available)
            .cloned()
            .collect();
        for item in pending {
            let id = item.id.clone();
            self.inflight.insert(id.clone());
            let generation = self.generation;
            let cache = self.cache.clone();
            job(
                cx,
                move || sources::thumbnail(&item, &cache),
                move |this, result, cx| {
                    if this.generation != generation {
                        return;
                    }
                    this.inflight.remove(&id);
                    match result {
                        Ok(path) => {
                            if let Some(w) = this.items.iter_mut().find(|w| w.id == id) {
                                w.thumbnail_path = Some(path.clone());
                            }
                            if let Some(w) = this.selected.as_mut().filter(|w| w.id == id) {
                                w.thumbnail_path = Some(path);
                            }
                        }
                        Err(_) => {
                            this.failed_thumbs.insert(id);
                        }
                    }
                    this.load_thumbnails(cx);
                    cx.notify();
                },
            );
        }
    }
    fn fail(&mut self, message: String) {
        self.status = message;
        self.error = true;
    }
    fn choose_folder(&mut self, save: Option<Wallpaper>, window: &Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let current = self.config.wallpaper_folder.clone();
        let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose wallpaper folder");
        if let Some(raw) = HasWindowHandle::window_handle(window)
            .ok()
            .map(|handle| handle.as_raw())
            && matches!(raw, RawWindowHandle::Win32(_))
        {
            let parent = DialogParent(raw);
            dialog = dialog.set_parent(&parent);
        }
        if let Some(path) = current {
            dialog = dialog.set_directory(path);
        }
        let task = dialog.pick_folder();
        cx.spawn(async move |this, cx| {
            let path = task.await.map(|handle| handle.path().to_path_buf());
            let _ = this.update(cx, move |this, cx| {
                this.busy = false;
                if let Some(path) = path {
                    this.config.wallpaper_folder = Some(path);
                    this.persist();
                    if let Some(item) = save {
                        this.save_item(item, None, cx);
                    } else if this.config.last_tab == Tab::Local {
                        this.refresh(false, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn apply_item(&mut self, item: Wallpaper, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "Preparing wallpaper…".into();
        self.error = false;
        let cache = self.cache.clone();
        let fit = self.config.wallpaper_fit;
        let title = item.title.clone();
        job(
            cx,
            move || {
                let path = match item.local_path {
                    Some(p) => p,
                    None => sources::download(&item, &cache)?,
                };
                wallpaper::apply(&path, &cache, fit)
            },
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.status = format!("Applied · {title}");
                        this.error = false;
                    }
                    Err(e) => this.fail(format!("Could not apply wallpaper: {e:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn save_item(&mut self, item: Wallpaper, window: Option<&Window>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(folder) = self.config.wallpaper_folder.clone() else {
            if let Some(window) = window {
                self.choose_folder(Some(item), window, cx);
            } else {
                self.fail("Choose a wallpaper folder before saving".into());
                cx.notify();
            }
            return;
        };
        self.busy = true;
        self.status = "Saving to your folder…".into();
        self.error = false;
        let cache = self.cache.clone();
        job(
            cx,
            move || {
                let path = sources::download(&item, &cache)?;
                local::save_download(&path, &folder, &item.title)
            },
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(path) => {
                        this.status = format!(
                            "Saved · {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        );
                        this.error = false;
                    }
                    Err(e) => this.fail(format!("Could not save wallpaper: {e:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.dialog != Some(Dialog::Rename) {
            return;
        }
        let Some(path) = self.selected.as_ref().and_then(|w| w.local_path.clone()) else {
            return;
        };
        let name = self.rename_input.read(cx).value();
        self.busy = true;
        job(
            cx,
            move || local::rename(&path, &name),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(_) => {
                        this.refresh(false, cx);
                        this.status = "Wallpaper renamed".into();
                    }
                    Err(e) => this.fail(format!("Could not rename: {e:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn commit_delete(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.dialog != Some(Dialog::Delete) {
            return;
        }
        let Some(path) = self.selected.as_ref().and_then(|w| w.local_path.clone()) else {
            return;
        };
        self.busy = true;
        job(
            cx,
            move || local::delete(&path),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.refresh(false, cx);
                        this.status = "Moved to Recycle Bin".into();
                    }
                    Err(e) => this.fail(format!("Could not delete: {e:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn open_item(&mut self, item: Wallpaper, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(item);
        self.dialog = Some(Dialog::Preview);
        window.focus(&self.focus);
        cx.notify();
    }

    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(58.))
            .px(px(24.))
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap(px(10.))
                    .h_full()
                    .window_control_area(WindowControlArea::Drag)
                    .child(icon("cat").size(px(25.)).text_color(rgb(ACCENT)))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(17.))
                            .child("neko"),
                    ),
            )
            .child(
                div()
                    .id("settings")
                    .size(px(28.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(rgb(MUTED))
                    .hover(|s| s.bg(rgb(SURFACE)).text_color(rgb(TEXT)))
                    .child(icon("settings").size(px(14.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dialog = Some(Dialog::Settings);
                        this.selected = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("minimize")
                    .size(px(28.))
                    .rounded_full()
                    .bg(rgb(SURFACE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x35353b)))
                    .child(icon("minus").size(px(13.)))
                    .on_click(|_, window, _| window.minimize_window()),
            )
            .child(
                div()
                    .id("close")
                    .size(px(28.))
                    .rounded_full()
                    .bg(rgb(SURFACE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x594048)))
                    .child(icon("close").size(px(13.)))
                    .on_click(cx.listener(|_, _, window, _| window.remove_window())),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let online = self.config.last_tab == Tab::Search;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .px(px(24.))
            .pt_2()
            .pb(px(18.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_1()
                    .bg(rgb(0x0d0d0f))
                    .p_1()
                    .rounded(px(12.))
                    .children(
                        [(Tab::Local, "Local"), (Tab::Search, "Explore")]
                            .into_iter()
                            .map(|(tab, label)| {
                                let active = self.config.last_tab == tab;
                                div()
                                    .id(label)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(34.))
                                    .w(px(104.))
                                    .px(px(16.))
                                    .text_center()
                                    .rounded(px(9.))
                                    .cursor_pointer()
                                    .text_color(rgb(if active { TEXT } else { MUTED }))
                                    .bg(rgb(if active { 0x2b2b31 } else { 0x0d0d0f }))
                                    .text_size(px(12.))
                                    .hover(|s| s.text_color(rgb(TEXT)))
                                    .child(div().w_full().text_center().child(label))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)),
                                    )
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .w(px(if online { 310. } else { 230. }))
                    .child(self.query.clone()),
            )
            .when(online, |s| {
                s.child(
                    text_button("search", "Search", true)
                        .w(px(106.))
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(false, cx))),
                )
            })
            .when(!online, |s| {
                s.child(
                    button(
                        "choose-folder",
                        if self.config.wallpaper_folder.is_some() {
                            "Change folder"
                        } else {
                            "Choose folder"
                        },
                        "folder",
                        false,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.choose_folder(None, window, cx)),
                    ),
                )
            })
    }

    fn sourcebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_3()
            .px(px(24.))
            .pb(px(18.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("SOURCE"),
            )
            .children(
                [
                    (Provider::Wallhaven, "Wallhaven"),
                    (Provider::Bjarneo, "bjarneo"),
                ]
                .into_iter()
                .map(|(provider, label)| {
                    let active = self.config.last_source == provider;
                    div()
                        .id(label)
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .h(px(28.))
                        .rounded_full()
                        .cursor_pointer()
                        .text_size(px(11.))
                        .bg(rgb(if active { 0x302a3e } else { SURFACE }))
                        .text_color(rgb(if active { ACCENT } else { MUTED }))
                        .child(div().size(px(5.)).rounded_full().bg(rgb(if active {
                            ACCENT
                        } else {
                            0x55555d
                        })))
                        .child(label)
                        .hover(|s| s.text_color(rgb(TEXT)))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_source(provider, cx)))
                }),
            )
            .child(div().flex_1())
            .when(self.config.last_source == Provider::Wallhaven, |s| {
                s.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child("SFW wallpapers · No account needed"),
                )
            })
            .when(self.config.last_source == Provider::Bjarneo, |s| {
                s.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .mr_1()
                                .text_color(rgb(MUTED))
                                .child("Color"),
                        )
                        .child(
                            div()
                                .id("all-colors")
                                .text_size(px(10.))
                                .px_2()
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .cursor_pointer()
                                .bg(rgb(if self.color.is_none() {
                                    0x38323f
                                } else {
                                    SURFACE
                                }))
                                .child("All")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.color = None;
                                    this.refresh(false, cx);
                                })),
                        )
                        .children(
                            [
                                ("red", 0xd77378),
                                ("orange", 0xdca26f),
                                ("yellow", 0xe0ce80),
                                ("green", 0x8cb58c),
                                ("blue", 0x81a8ce),
                                ("cyan", 0x75c9c7),
                                ("purple", 0xad8fca),
                                ("pink", 0xd493b4),
                                ("monochrome", 0x77777f),
                            ]
                            .into_iter()
                            .map(|(name, color)| {
                                div()
                                    .id(name)
                                    .size(px(18.))
                                    .p(px(3.))
                                    .rounded_full()
                                    .border_1()
                                    .cursor_pointer()
                                    .border_color(rgb(if self.color.as_deref() == Some(name) {
                                        TEXT
                                    } else {
                                        BG
                                    }))
                                    .hover(|s| s.border_color(rgb(MUTED)))
                                    .child(div().size_full().rounded_full().bg(rgb(color)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.color = Some(name.into());
                                        this.refresh(false, cx);
                                    }))
                            }),
                        ),
                )
            })
    }

    fn collection_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let online = self.config.last_tab == Tab::Search;
        let title = if online {
            if self.query.read(cx).value().is_empty() {
                "Discover something new".to_string()
            } else {
                format!("Results for “{}”", self.query.read(cx).value())
            }
        } else {
            "Your collection".into()
        };
        let subtitle = if online {
            match self.config.last_source {
                Provider::Wallhaven => "A fresh perspective, one wallpaper at a time.".to_string(),
                Provider::Bjarneo => "A curated collection by bjarneo & contributors.".to_string(),
            }
        } else {
            self.config
                .wallpaper_folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Your favorite wallpapers, together in one place.".into())
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .px(px(24.))
            .pb(px(18.))
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(if self.loading {
                        "Loading…".into()
                    } else {
                        format!(
                            "{} wallpapers",
                            if online {
                                self.total
                            } else {
                                self.visible.len()
                            }
                        )
                    }),
            )
            .child(
                div()
                    .id("refresh")
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_color(rgb(MUTED))
                    .hover(|s| s.bg(rgb(SURFACE)).text_color(rgb(TEXT)))
                    .child(icon("refresh").size(px(14.)))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(true, cx))),
            )
    }

    fn card(
        &self,
        item: &Wallpaper,
        index: usize,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preview = item.clone();
        let apply = item.clone();
        let failed = self.failed_thumbs.contains(&item.id);
        div()
            .id(("wallpaper", index))
            .group("card")
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(width * 0.625 + 29.))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_item(preview.clone(), window, cx)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(width * 0.625))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(0x2a2a2e))
                    .group_hover("card", |s| s.border_color(rgb(ACCENT)))
                    .when_some(item.thumbnail_path.clone(), |s, path| {
                        s.child(
                            img(Arc::<Path>::from(path))
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        )
                    })
                    .when(item.thumbnail_path.is_none(), |s| {
                        s.child(
                            div()
                                .size_full()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .items_center()
                                .justify_center()
                                .text_color(rgb(0x50505c))
                                .child(icon("image").size(px(27.)))
                                .when(failed, |s| {
                                    s.child(div().text_size(px(10.)).child("Preview unavailable"))
                                }),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top(px(7.))
                            .right(px(7.))
                            .invisible()
                            .group_hover("card", |s| s.visible())
                            .child(
                                div()
                                    .id(("quick-apply", index))
                                    .size(px(29.))
                                    .rounded(px(8.))
                                    .bg(rgba(0x121216e8))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|s| s.bg(rgb(ACCENT)).text_color(rgb(BG)))
                                    .child(icon("monitor").size(px(14.)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.apply_item(apply.clone(), cx);
                                    })),
                            ),
                    )
                    .when(item.width > 0, |s| {
                        s.child(
                            div()
                                .absolute()
                                .bottom(px(7.))
                                .left(px(7.))
                                .px(px(6.))
                                .py(px(3.))
                                .rounded(px(5.))
                                .bg(rgba(0x121216c8))
                                .text_size(px(9.))
                                .text_color(rgb(TEXT))
                                .invisible()
                                .group_hover("card", |s| s.visible())
                                .child(format!("{} × {}", item.width, item.height)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pt(px(8.))
                    .px(px(2.))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .group_hover("card", |s| s.text_color(rgb(TEXT)))
                            .child(item.title.clone()),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x595963))
                            .child(if item.width >= 3840 {
                                "4K"
                            } else if item.width >= 1920 {
                                "HD"
                            } else {
                                ""
                            }),
                    ),
            )
            .into_any_element()
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let local = self.config.last_tab == Tab::Local;
        let no_folder = local && self.config.wallpaper_folder.is_none();
        let (title, subtitle) = if self.loading {
            (
                "Finding your next view",
                "Loading wallpapers. This only takes a moment.",
            )
        } else if self.error {
            (
                "A little interruption",
                "Check your connection or try refreshing the collection.",
            )
        } else if no_folder {
            (
                "Make yourself at home",
                "Choose a folder to bring your wallpapers together.",
            )
        } else if local && self.items.is_empty() {
            (
                "Room for a new view",
                "Save a few wallpapers here, or explore the online collections.",
            )
        } else {
            (
                "No wallpapers found",
                "Try a different search or choose another color.",
            )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .pb(px(30.))
            .child(
                div()
                    .size(px(68.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(20.))
                    .bg(rgb(0x27232f))
                    .text_color(rgb(ACCENT))
                    .child(icon(if local { "folder" } else { "image" }).size(px(30.))),
            )
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(subtitle),
            )
            .when(!self.loading, |s| {
                s.child(
                    div()
                        .flex()
                        .gap_3()
                        .mt_2()
                        .when(no_folder, |s| {
                            s.child(
                                button("empty-folder", "Choose wallpaper folder", "folder", true)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_folder(None, window, cx)
                                    })),
                            )
                        })
                        .when(local, |s| {
                            s.child(
                                button("empty-explore", "Explore wallpapers", "arrow", !no_folder)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.set_tab(Tab::Search, cx)),
                                    ),
                            )
                        })
                        .when(!local && self.error, |s| {
                            s.child(
                                button("retry", "Try again", "refresh", true)
                                    .on_click(cx.listener(|this, _, _, cx| this.refresh(true, cx))),
                            )
                        }),
                )
            })
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(45.))
            .px(px(24.))
            .border_t_1()
            .border_color(rgb(0x252529))
            .text_size(px(10.))
            .child(div().size(px(5.)).rounded_full().bg(rgb(if self.error {
                0xea9696
            } else if self.busy || self.loading {
                ACCENT
            } else {
                0x8bab98
            })))
            .when(self.config.last_tab == Tab::Search && self.page > 0, |s| {
                s.child(div().text_color(rgb(MUTED)).child(format!(
                    "{} of {}",
                    self.items.len(),
                    self.total
                )))
            })
            .when(
                self.config.last_tab == Tab::Search && self.page < self.last_page,
                |s| {
                    s.child(
                        button(
                            "load-more",
                            if self.loading {
                                "Loading…"
                            } else {
                                "Load more"
                            },
                            "arrow",
                            false,
                        )
                        .h(px(28.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.loading {
                                this.search_page(this.page + 1, false, cx);
                            }
                        })),
                    )
                },
            )
            .when(self.config.last_tab == Tab::Local, |s| {
                s.child(div().text_color(rgb(0x55555f)).child("F5 refresh"))
            })
    }

    fn settings_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let fit_options = [
            (WallpaperFit::Fill, "Fill"),
            (WallpaperFit::Fit, "Fit"),
            (WallpaperFit::Stretch, "Stretch"),
            (WallpaperFit::Center, "Center"),
            (WallpaperFit::Tile, "Tile"),
        ];
        let rotation_options = [
            (RotationInterval::Off, "Off"),
            (RotationInterval::FifteenMinutes, "15 min"),
            (RotationInterval::Hourly, "1 hour"),
            (RotationInterval::SixHours, "6 hours"),
            (RotationInterval::Daily, "Daily"),
        ];
        let panel = div()
            .w(px(590.))
            .rounded(px(16.))
            .bg(rgb(0x1b1b1f))
            .border_1()
            .border_color(rgb(0x3a3a43))
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .p_4()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(
                        div()
                            .id("close-settings")
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .bg(rgb(SURFACE))
                            .hover(|s| s.bg(rgb(0x594048)))
                            .child("×")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px_4()
                    .pb_4()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Wallpaper fit"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("Controls how Windows places each wallpaper on the desktop."),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(fit_options.into_iter().enumerate().map(|(index, (fit, label))| {
                                let active = self.config.wallpaper_fit == fit;
                                div()
                                    .id(("wallpaper-fit", index))
                                    .flex_1()
                                    .h(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .text_size(px(11.))
                                    .bg(rgb(if active { 0x403653 } else { SURFACE }))
                                    .text_color(rgb(if active { ACCENT } else { MUTED }))
                                    .hover(|s| s.text_color(rgb(TEXT)).bg(rgb(0x323238)))
                                    .child(label)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.set_fit(fit, cx)),
                                    )
                            })),
                    ),
            )
            .child(div().h(px(1.)).mx_4().bg(rgb(0x303036)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Automatic rotation"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("Picks a random image from your local wallpaper folder while Neko is running."),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(rotation_options.into_iter().enumerate().map(|(index, (interval, label))| {
                                let active = self.config.rotation_interval == interval;
                                div()
                                    .id(("rotation-interval", index))
                                    .flex_1()
                                    .h(px(34.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .text_size(px(11.))
                                    .bg(rgb(if active { 0x403653 } else { SURFACE }))
                                    .text_color(rgb(if active { ACCENT } else { MUTED }))
                                    .hover(|s| s.text_color(rgb(TEXT)).bg(rgb(0x323238)))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_rotation(interval, cx)
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                button("rotate-now", "Rotate now", "refresh", true).on_click(
                                    cx.listener(|this, _, _, cx| this.rotate_wallpaper(cx)),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(10.))
                                    .text_color(rgb(MUTED))
                                    .child(if self.config.wallpaper_folder.is_some() {
                                        "Uses your selected wallpaper folder"
                                    } else {
                                        "Choose a wallpaper folder before rotating"
                                    }),
                            ),
                    )
                    .when(!self.status.is_empty(), |s| {
                        s.child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(if self.error { 0xe8a2a2 } else { MUTED }))
                                .child(self.status.clone()),
                        )
                    }),
            );
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x050507c8))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .occlude()
            .child(panel)
            .into_any_element()
    }

    fn modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = self.dialog else {
            return div().into_any_element();
        };
        if dialog == Dialog::Settings {
            return self.settings_modal(cx);
        }
        let Some(item) = self.selected.clone() else {
            return div().into_any_element();
        };
        let local = item.local_path.is_some();
        let source_label = match item.source {
            Some(Provider::Wallhaven) => "Wallhaven",
            Some(Provider::Bjarneo) => "bjarneo",
            None if local => "Local folder",
            None => "Online wallpaper",
        };
        let color_label = item
            .color
            .as_deref()
            .map(|color| format!("  ·  {color}"))
            .unwrap_or_default();
        let apply = item.clone();
        let save = item.clone();
        let width = (f32::from(window.viewport_size().width) - 100.).min(690.);
        let mut panel = div()
            .w(px(width))
            .rounded(px(16.))
            .bg(rgb(0x1b1b1f))
            .border_1()
            .border_color(rgb(0x3a3a43))
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(match dialog {
                                Dialog::Preview => item.title.clone(),
                                Dialog::Rename => "Rename wallpaper".into(),
                                Dialog::Delete => "Move to Recycle Bin?".into(),
                                Dialog::Settings => unreachable!(),
                            }),
                    )
                    .child(
                        div()
                            .id("close-dialog")
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(SURFACE))
                            .cursor_pointer()
                            .rounded(px(8.))
                            .text_size(px(19.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .hover(|s| s.bg(rgb(0x594048)))
                            .child("×")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog = None;
                                cx.notify();
                            })),
                    ),
            );
        match dialog {
            Dialog::Preview => {
                panel =
                    panel
                        .child(
                            div()
                                .mx_4()
                                .rounded(px(10.))
                                .overflow_hidden()
                                .h(px((width * 0.51)
                                    .min(f32::from(window.viewport_size().height) - 270.)))
                                .bg(rgb(0x0e0e10))
                                .flex()
                                .justify_center()
                                .items_center()
                                .when_some(item.thumbnail_path.clone(), |s, p| {
                                    s.child(
                                        img(Arc::<Path>::from(p))
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    )
                                })
                                .when(item.thumbnail_path.is_none(), |s| {
                                    s.child(icon("image").size(px(50.)).text_color(rgb(MUTED)))
                                }),
                        )
                        .child(
                            div()
                                .px_4()
                                .pt_3()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .child(
                                    format!(
                                        "{}{}",
                                        if item.width > 0 {
                                            format!("{} × {}  ·  ", item.width, item.height)
                                        } else {
                                            String::new()
                                        },
                                        item.attribution.as_deref().unwrap_or(source_label)
                                    ) + &color_label,
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .p_4()
                                .child(
                                    button(
                                        "apply",
                                        if self.busy {
                                            "Working…"
                                        } else {
                                            "Apply wallpaper"
                                        },
                                        "monitor",
                                        true,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| this.apply_item(apply.clone(), cx),
                                    )),
                                )
                                .when(!local, |s| {
                                    s.child(
                                        button("save", "Save to folder", "download", false)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.save_item(save.clone(), Some(window), cx)
                                            })),
                                    )
                                })
                                .when(local, |s| {
                                    s.child(button("rename", "Rename", "edit", false).on_click(
                                        cx.listener(|this, _, window, cx| {
                                            let name = this
                                                .selected
                                                .as_ref()
                                                .and_then(|w| w.local_path.as_ref())
                                                .and_then(|p| p.file_stem())
                                                .map(|s| s.to_string_lossy().into_owned())
                                                .unwrap_or_default();
                                            this.rename_input
                                                .update(cx, |i, cx| i.set_value(&name, cx));
                                            this.dialog = Some(Dialog::Rename);
                                            this.rename_input.focus_handle(cx).focus(window);
                                            cx.notify();
                                        }),
                                    ))
                                    .child(
                                        button("reveal", "Show in folder", "folder", false)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(path) = this
                                                    .selected
                                                    .as_ref()
                                                    .and_then(|w| w.local_path.clone())
                                                    && let Err(e) = local::show_in_folder(&path)
                                                {
                                                    this.fail(format!("Could not show file: {e}"));
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        button("delete", "Delete", "trash", false)
                                            .text_color(rgb(0xe29a9e))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.dialog = Some(Dialog::Delete);
                                                cx.notify();
                                            })),
                                    )
                                }),
                        );
            }
            Dialog::Rename => {
                panel = panel
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .child("Give this wallpaper a new name. Its file type stays the same."),
                    )
                    .child(div().px_4().child(self.rename_input.clone()))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .p_4()
                            .child(button("cancel-rename", "Cancel", "close", false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.dialog = Some(Dialog::Preview);
                                    cx.notify();
                                }),
                            ))
                            .child(
                                button("confirm-rename", "Rename", "check", true)
                                    .on_click(cx.listener(|this, _, _, cx| this.commit_rename(cx))),
                            ),
                    );
            }
            Dialog::Delete => {
                panel=panel.child(div().px_4().text_size(px(12.)).text_color(rgb(MUTED)).child(format!("“{}” will be moved to the Recycle Bin. You can restore it from there.",item.title)))
                .child(div().flex().justify_end().gap_2().p_4().child(button("cancel-delete","Keep wallpaper","close",false).on_click(cx.listener(|this,_,_,cx|{this.dialog=Some(Dialog::Preview);cx.notify();})))
                    .child(button("confirm-delete","Move to Recycle Bin","trash",true).bg(rgb(0xe4a3a7)).on_click(cx.listener(|this,_,_,cx|this.commit_delete(cx)))));
            }
            Dialog::Settings => unreachable!(),
        }
        if !self.status.is_empty() {
            panel = panel.child(
                div()
                    .px_4()
                    .pb_4()
                    .text_size(px(11.))
                    .text_color(rgb(if self.error { 0xe8a2a2 } else { MUTED }))
                    .child(self.status.clone()),
            );
        }
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x050507c8))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .occlude()
            .child(panel)
            .into_any_element()
    }
}

impl Render for Neko {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content_width = f32::from(window.viewport_size().width) - 50.;
        let columns = if content_width < 720. { 3 } else { 4 };
        let card_width = (content_width - ((columns - 1) as f32 * 12.)) / columns as f32;
        let rows = self.visible.len().div_ceil(columns);
        let modal = self.modal(window, cx);
        div()
            .id("neko")
            .relative()
            .size_full()
            .rounded(px(14.))
            .overflow_hidden()
            .bg(rgba(0x141416f5))
            .border_1()
            .border_color(rgb(0x34343a))
            .text_color(rgb(TEXT))
            .font_family("Segoe UI")
            .text_size(px(13.))
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .key_context("Neko")
            .on_action(cx.listener(|this, _: &Refresh, _, cx| {
                if this.dialog.is_none() {
                    this.refresh(true, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ChooseFolder, window, cx| {
                if this.dialog.is_none() {
                    this.choose_folder(None, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                if this.dialog.is_none() {
                    this.query.focus_handle(cx).focus(window);
                }
            }))
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| {
                this.dialog = None;
                cx.notify();
            }))
            .child(self.titlebar(cx))
            .child(self.toolbar(cx))
            .when(self.config.last_tab == Tab::Search, |s| {
                s.child(self.sourcebar(cx))
            })
            .child(self.collection_header(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .mx(px(24.))
                    .mb_3()
                    .when(self.visible.is_empty(), |s| s.child(self.empty(cx)))
                    .when(!self.visible.is_empty(), |s| {
                        s.child(
                            uniform_list(
                                "gallery",
                                rows,
                                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                    range
                                        .map(|row| {
                                            div().flex().gap(px(12.)).pb(px(13.)).children(
                                                (row * columns
                                                    ..((row + 1) * columns)
                                                        .min(this.visible.len()))
                                                    .map(|position| {
                                                        let i = this.visible[position];
                                                        this.card(&this.items[i], i, card_width, cx)
                                                    }),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .size_full()
                            .track_scroll(self.scroll.clone()),
                        )
                    }),
            )
            .child(self.footer(cx))
            .when(self.dialog.is_some(), |s| s.child(modal))
    }
}

pub fn run() {
    Application::new()
        .with_assets(icons::Icons)
        .run(|cx: &mut App| {
            input::init(cx);
            cx.bind_keys([
                KeyBinding::new("f5", Refresh, Some("Neko")),
                KeyBinding::new("ctrl-o", ChooseFolder, Some("Neko")),
                KeyBinding::new("ctrl-f", FocusSearch, Some("Neko")),
                KeyBinding::new("escape", Dismiss, Some("Neko")),
            ]);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(1000.), px(720.)), cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    titlebar: None,
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(760.), px(560.))),
                    app_id: Some("Neko".into()),
                    window_background: WindowBackgroundAppearance::Blurred,
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Neko — Wallpapers");
                    let view = cx.new(Neko::new);
                    window.focus(&view.read(cx).focus);
                    view
                },
            ) {
                let _ = rfd::MessageDialog::new()
                    .set_title("Neko couldn't start")
                    .set_description(format!("{error:#}"))
                    .set_level(rfd::MessageLevel::Error)
                    .show();
                cx.quit();
            }
            cx.activate(true);
        });
}
