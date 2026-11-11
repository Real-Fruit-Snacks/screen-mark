//! The tray icon and its menu.
//!
//! Clicks arrive as `TrayEvent`s on a channel, and a `wake` callback nudges
//! the winit event loop so they're handled right away.
//!
//! On Windows the icon lives on the main (winit) thread. On Linux it must
//! live on a thread running the GTK main loop: tray-icon builds its menu
//! with GTK, and 2.0 created it on the winit thread with GTK never
//! initialised, which panicked at startup ("GTK has not been initialized").
//! `start` spawns that thread; `TrayHandle` forwards updates to it.

use std::sync::mpsc::Sender;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// A tray menu entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Region,
    Screenshot,
    LastRegion,
    Window,
    Delay(u32),
    OpenFolder,
    History,
    Autostart,
    Settings,
    /// Check for updates, or install the one that was found.
    Update,
    About,
    Quit,
}

/// What the user did with the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click on the icon itself (Windows).
    IconClicked,
    Menu(Item),
}

const REGION_LABEL: &str = "Capture region";
const SCREENSHOT_LABEL: &str = "Screenshot (save + copy)";
const LAST_REGION_LABEL: &str = "Last region again (save + copy)";
/// The update item before anything is known; `set_update` changes it.
pub const UPDATE_LABEL: &str = "Check for updates";

/// The menu and icon. Owned by the thread that created them.
struct TrayState {
    region_item: MenuItem,
    screenshot_item: MenuItem,
    last_region_item: MenuItem,
    autostart_check: CheckMenuItem,
    update_item: MenuItem,
    _tray: tray_icon::TrayIcon,
}

impl TrayState {
    fn set_bindings(&self, region: &str, screenshot: &str, last_region: &str) {
        self.region_item.set_text(with_key(REGION_LABEL, region));
        self.screenshot_item
            .set_text(with_key(SCREENSHOT_LABEL, screenshot));
        self.last_region_item
            .set_text(with_key(LAST_REGION_LABEL, last_region));
    }
    fn set_autostart(&self, on: bool) {
        self.autostart_check.set_checked(on);
    }
    fn set_update(&self, text: &str) {
        self.update_item.set_text(text);
    }
}

fn with_key(label: &str, binding: &str) -> String {
    match crate::keys::Binding::parse(binding) {
        // Windows right-aligns text after a tab as the shortcut column.
        Ok(Some(b)) if cfg!(target_os = "windows") => format!("{label}\t{b}"),
        Ok(Some(b)) => format!("{label}   ({b})"),
        _ => label.to_string(),
    }
}

fn build(
    events: Sender<TrayEvent>,
    wake: impl Fn() + Send + Sync + Clone + 'static,
) -> Result<TrayState, String> {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let icon_img = image::load_from_memory(icon_bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (w, h) = icon_img.dimensions();
    let icon = Icon::from_rgba(icon_img.into_raw(), w, h).map_err(|e| e.to_string())?;

    let menu = Menu::new();
    let mut ids: Vec<(MenuId, Item)> = Vec::new();
    let region_item = MenuItem::new(REGION_LABEL, true, None);
    let screenshot_item = MenuItem::new(SCREENSHOT_LABEL, true, None);
    let last_region_item = MenuItem::new(LAST_REGION_LABEL, true, None);
    let autostart_check =
        CheckMenuItem::new("Start on login", true, crate::autostart::is_enabled(), None);
    let plain = |text: &str| MenuItem::new(text, true, None);
    let window_item = plain("Capture window");
    let d3 = plain("Capture in 3s");
    let d5 = plain("Capture in 5s");
    let d10 = plain("Capture in 10s");
    let folder = plain("Open captures folder");
    let history = plain("History");
    let settings = plain("Settings");
    let update_item = plain(UPDATE_LABEL);
    let about = plain("About");
    let quit = plain("Quit");
    for (item, kind) in [
        (&region_item, Item::Region),
        (&screenshot_item, Item::Screenshot),
        (&last_region_item, Item::LastRegion),
        (&window_item, Item::Window),
        (&d3, Item::Delay(3)),
        (&d5, Item::Delay(5)),
        (&d10, Item::Delay(10)),
        (&folder, Item::OpenFolder),
        (&history, Item::History),
        (&settings, Item::Settings),
        (&update_item, Item::Update),
        (&about, Item::About),
        (&quit, Item::Quit),
    ] {
        ids.push((item.id().clone(), kind));
    }
    ids.push((autostart_check.id().clone(), Item::Autostart));

    let sep = || PredefinedMenuItem::separator();
    let err = |e: tray_icon::menu::Error| e.to_string();
    menu.append_items(&[
        &region_item,
        &screenshot_item,
        &last_region_item,
        &window_item,
    ])
    .map_err(err)?;
    menu.append_items(&[&d3, &d5, &d10]).map_err(err)?;
    menu.append(&sep()).map_err(err)?;
    menu.append_items(&[&folder, &history]).map_err(err)?;
    menu.append(&autostart_check).map_err(err)?;
    menu.append_items(&[&settings, &update_item, &about])
        .map_err(err)?;
    menu.append(&sep()).map_err(err)?;
    menu.append(&quit).map_err(err)?;

    // Route clicks to the app and wake its event loop.
    {
        let events = events.clone();
        let wake = wake.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, item)) = ids.iter().find(|(id, _)| *id == e.id) {
                let _ = events.send(TrayEvent::Menu(*item));
                wake();
            }
        }));
    }
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = e
        {
            let _ = events.send(TrayEvent::IconClicked);
            wake();
        }
    }));

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("screen-mark")
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(TrayState {
        region_item,
        screenshot_item,
        last_region_item,
        autostart_check,
        update_item,
        _tray: tray,
    })
}

/// Updates to the tray from the app's thread.
pub struct TrayHandle {
    #[cfg(not(target_os = "linux"))]
    state: TrayState,
}

impl TrayHandle {
    /// Show the current hotkeys next to the three hotkey capture items.
    pub fn set_bindings(&self, region: &str, screenshot: &str, last_region: &str) {
        #[cfg(not(target_os = "linux"))]
        self.state.set_bindings(region, screenshot, last_region);
        #[cfg(target_os = "linux")]
        {
            let (r, s, l) = (
                region.to_string(),
                screenshot.to_string(),
                last_region.to_string(),
            );
            gtk::glib::idle_add_once(move || {
                GTK_TRAY.with(|t| {
                    if let Some(t) = t.borrow().as_ref() {
                        t.set_bindings(&r, &s, &l);
                    }
                })
            });
        }
    }

    /// Relabel the update item: "Check for updates", "Install screen-mark
    /// 2.3.0", and so on.
    pub fn set_update(&self, text: &str) {
        #[cfg(not(target_os = "linux"))]
        self.state.set_update(text);
        #[cfg(target_os = "linux")]
        {
            let text = text.to_string();
            gtk::glib::idle_add_once(move || {
                GTK_TRAY.with(|t| {
                    if let Some(t) = t.borrow().as_ref() {
                        t.set_update(&text);
                    }
                })
            });
        }
    }

    pub fn set_autostart(&self, on: bool) {
        #[cfg(not(target_os = "linux"))]
        self.state.set_autostart(on);
        #[cfg(target_os = "linux")]
        gtk::glib::idle_add_once(move || {
            GTK_TRAY.with(|t| {
                if let Some(t) = t.borrow().as_ref() {
                    t.set_autostart(on);
                }
            })
        });
    }
}

#[cfg(target_os = "linux")]
thread_local! {
    static GTK_TRAY: std::cell::RefCell<Option<TrayState>> = const { std::cell::RefCell::new(None) };
}

/// Create the tray. `wake` is called (from any thread) after each event.
pub fn start(
    events: Sender<TrayEvent>,
    wake: impl Fn() + Send + Sync + Clone + 'static,
) -> Result<TrayHandle, String> {
    #[cfg(not(target_os = "linux"))]
    {
        Ok(TrayHandle {
            state: build(events, wake)?,
        })
    }
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("tray-gtk".into())
            .spawn(move || {
                if let Err(e) = gtk::init() {
                    let _ = tx.send(Err(format!("GTK init failed: {e}")));
                    return;
                }
                match build(events, wake) {
                    Ok(state) => {
                        GTK_TRAY.with(|t| *t.borrow_mut() = Some(state));
                        let _ = tx.send(Ok(()));
                        gtk::main();
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        // A missing appindicator library panics inside tray-icon; that only
        // ends the tray thread, and we carry on without a tray.
        match rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(TrayHandle {}),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                Err("tray thread did not start (is libayatana-appindicator3 installed?)".into())
            }
        }
    }
}
