use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{mpsc, Arc, Mutex};

use dbus::arg::{RefArg, Variant};
use dbus::blocking::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;
use dbus::message::SignalArgs;

mod dbus_ext;
mod dbus_interface;
mod freedesktop;
mod menu;
mod service;
mod tray;

use dbus_interface::{
    StatusNotifierItemNewAttentionIcon, StatusNotifierItemNewIcon,
    StatusNotifierItemNewOverlayIcon, StatusNotifierItemNewStatus, StatusNotifierItemNewTitle,
    StatusNotifierItemNewToolTip,
};
pub use menu::{MenuItem, TextDirection};
pub use service::TrayService;
pub use tray::{Category, Icon, Status, ToolTip};

pub trait Tray {
    /// Asks the status notifier item for activation, this is typically a
    /// consequence of user input, such as mouse left click over the graphical
    /// representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn activate(&mut self, _x: i32, _y: i32) {}

    /// Is to be considered a secondary and less important form of activation
    /// compared to Activate.
    /// This is typically a consequence of user input, such as mouse middle
    /// click over the graphical representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn secondary_activate(&mut self, _x: i32, _y: i32) {}

    /// The user asked for a scroll action. This is caused from input such as
    /// mouse wheel over the graphical representation of the item.
    ///
    /// The delta parameter represent the amount of scroll, the orientation
    /// parameter represent the horizontal or vertical orientation of the scroll
    /// request and its legal values are horizontal and vertical.
    fn scroll(&mut self, _delta: i32, _dir: &str) {}

    /// Describes the category of this item.
    fn category(&self) -> Category {
        tray::Category::ApplicationStatus
    }

    /// It's a name that should be unique for this application and consistent
    /// between sessions, such as the application name itself.
    fn id(&self) -> String {
        Default::default()
    }

    /// It's a name that describes the application, it can be more descriptive
    /// than Id.
    fn title(&self) -> String {
        Default::default()
    }

    /// Describes the status of this item or of the associated application.
    fn status(&self) -> Status {
        tray::Status::Active
    }

    // NOTE: u32 in org.freedesktop.StatusNotifierItem
    /// It's the windowing-system dependent identifier for a window, the
    /// application can chose one of its windows to be available through this
    /// property or just set 0 if it's not interested.
    fn window_id(&self) -> i32 {
        0
    }

    /// An additional path to add to the theme search path to find the icons.
    fn icon_theme_path(&self) -> String {
        Default::default()
    }

    /// The item only support the context menu, the visualization
    /// should prefer showing the menu or sending ContextMenu()
    /// instead of Activate()
    // fn item_is_menu() -> bool { false }

    /// The StatusNotifierItem can carry an icon that can be used by the
    /// visualization to identify the item.
    fn icon_name(&self) -> String {
        Default::default()
    }

    /// Carries an ARGB32 binary representation of the icon
    fn icon_pixmap(&self) -> Vec<Icon> {
        Default::default()
    }

    /// The Freedesktop-compliant name of an icon. This can be used by the
    /// visualization to indicate extra state information, for instance as an
    /// overlay for the main icon.
    fn overlay_icon_name(&self) -> String {
        Default::default()
    }

    /// ARGB32 binary representation of the overlay icon described in the
    /// previous paragraph.
    fn overlay_icon_pixmap(&self) -> Vec<Icon> {
        Default::default()
    }

    /// The Freedesktop-compliant name of an icon. this can be used by the
    /// visualization to indicate that the item is in RequestingAttention state.
    fn attention_icon_name(&self) -> String {
        Default::default()
    }

    /// ARGB32 binary representation of the requesting attention icon describe in
    /// the previous paragraph.
    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        Default::default()
    }

    /// An item can also specify an animation associated to the
    /// RequestingAttention state.
    /// This should be either a Freedesktop-compliant icon name or a full path.
    /// The visualization can chose between the movie or AttentionIconPixmap (or
    /// using neither of those) at its discretion.
    fn attention_movie_name(&self) -> String {
        Default::default()
    }

    /// Data structure that describes extra information associated to this item,
    /// that can be visualized for instance by a tooltip (or by any other mean
    /// the visualization consider appropriate.
    fn tool_tip(&self) -> ToolTip {
        Default::default()
    }

    /// Represents the way the text direction of the application.  This
    /// allows the server to handle mismatches intelligently.
    fn text_direction(&self) -> TextDirection {
        menu::TextDirection::LeftToRight
    }

    fn menu(&self) -> Vec<MenuItem> {
        Default::default()
    }
}

pub struct State<T: ?Sized> {
    tx: mpsc::Sender<dbus::Message>,
    inner: Arc<Mutex<T>>,
    prop_cache: Arc<Mutex<PropertiesCache>>,
}

impl<T: Tray> State<T> {
    pub fn update<F: Fn(&T)>(&self, f: F) {
        let inner = self.inner.lock().unwrap();
        (f)(&inner);
        self.update_properties();
    }

    // TODO: macro?
    fn update_properties(&self) {
        let sni_dbus_path: dbus::Path = service::SNI_PATH.into();
        let inner = self.inner.lock().unwrap();
        let mut cache = self.prop_cache.lock().unwrap();
        let mut dbusmenu_changed: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        let mut sni_changed: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();

        if let Some(text_direction) = cache.text_direction_changed(&*inner) {
            dbusmenu_changed.insert(
                "TextDirection".into(),
                Variant(Box::new(text_direction.to_string())),
            );
        }

        if let Some(tray_status) = cache.status_changed(&*inner) {
            let msg = StatusNotifierItemNewStatus {
                status: tray_status.to_string(),
            }
            .to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
            let menu_status = match tray_status {
                tray::Status::Passive | tray::Status::Active => menu::Status::Normal,
                tray::Status::NeedsAttention => menu::Status::Notice,
            };
            dbusmenu_changed.insert("Status".into(), Variant(Box::new(menu_status.to_string())));
        }

        if let Some(icon_theme_path) = cache.icon_theme_path_changed(&*inner) {
            dbusmenu_changed.insert(
                "IconThemePath".into(),
                Variant(Box::new(icon_theme_path.to_string())),
            );
            sni_changed.insert(
                "IconThemePath".into(),
                Variant(Box::new(vec![icon_theme_path.to_string()])),
            );
        }

        if !dbusmenu_changed.is_empty() {
            let msg = PropertiesPropertiesChanged {
                interface_name: "com.canonical.dbusmenu".to_owned(),
                changed_properties: dbusmenu_changed,
                invalidated_properties: Vec::new(),
            }
            .to_emit_message(&service::MENU_PATH.into());
            self.tx.send(msg).unwrap();
        }

        if let Some(category) = cache.category_changed(&*inner) {
            sni_changed.insert("Category".into(), Variant(Box::new(category.to_string())));
        }

        if let Some(window_id) = cache.window_id_changed(&*inner) {
            sni_changed.insert("WindowId".into(), Variant(Box::new(window_id.to_string())));
        }

        if !sni_changed.is_empty() {
            let msg = PropertiesPropertiesChanged {
                interface_name: "org.kde.StatusNotifierItem".to_owned(),
                changed_properties: sni_changed,
                invalidated_properties: Vec::new(),
            }
            .to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
        }

        // TODO: assert the id is consistent

        if cache.title_changed(&*inner) {
            let msg = StatusNotifierItemNewTitle {}.to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
        }
        if cache.icon_changed(&*inner) {
            let msg = StatusNotifierItemNewIcon {}.to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
        }
        if cache.overlay_icon_changed(&*inner) {
            let msg = StatusNotifierItemNewOverlayIcon {}.to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
        }
        if cache.attention_icon_changed(&*inner) {
            let msg = StatusNotifierItemNewAttentionIcon {}.to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
        }
        if cache.tool_tip_changed(&*inner) {
            let msg = StatusNotifierItemNewToolTip {}.to_emit_message(&sni_dbus_path);
            self.tx.send(msg).unwrap();
        }
    }
}

struct PropertiesCache {
    category: Category,
    title: u64,
    status: Status,
    window_id: i32,
    icon_theme_path: u64,
    icon: u64,
    overlay_icon: u64,
    attention_icon: u64,
    tool_tip: u64,
    text_direction: TextDirection,
}

impl PropertiesCache {
    fn new<T: Tray>(tray: &T) -> Self {
        PropertiesCache {
            category: tray.category(),
            title: hash_of(tray.title()),
            status: tray.status(),
            window_id: tray.window_id(),
            icon_theme_path: hash_of(tray.icon_theme_path()),
            icon: hash_of((tray.icon_name(), tray.icon_pixmap())),
            overlay_icon: hash_of((tray.overlay_icon_name(), tray.overlay_icon_pixmap())),
            attention_icon: hash_of((
                tray.attention_icon_name(),
                tray.attention_icon_pixmap(),
                tray.attention_movie_name(),
            )),
            tool_tip: hash_of(tray.tool_tip()),
            text_direction: tray.text_direction(),
        }
    }
    fn category_changed<T: Tray>(&mut self, t: &T) -> Option<Category> {
        let v = t.category();
        if self.category != v {
            self.category = v;
            Some(v)
        } else {
            None
        }
    }
    fn title_changed<T: Tray>(&mut self, t: &T) -> bool {
        let hash = hash_of(t.title());
        self.title != hash && {
            self.title = hash;
            true
        }
    }
    fn status_changed<T: Tray>(&mut self, t: &T) -> Option<Status> {
        let v = t.status();
        if self.status != v {
            self.status = v;
            Some(v)
        } else {
            None
        }
    }
    fn window_id_changed<T: Tray>(&mut self, t: &T) -> Option<i32> {
        let v = t.window_id();
        if self.window_id != v {
            self.window_id = v;
            Some(v)
        } else {
            None
        }
    }
    fn icon_theme_path_changed<T: Tray>(&mut self, t: &T) -> Option<String> {
        let v = t.icon_theme_path();
        let hash = hash_of(&v);
        if self.icon_theme_path != hash {
            self.icon_theme_path = hash;
            Some(v)
        } else {
            None
        }
    }
    fn icon_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of((tray.icon_name(), tray.icon_pixmap()));
        self.icon != hash && {
            self.icon = hash;
            true
        }
    }
    fn overlay_icon_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of((tray.overlay_icon_name(), tray.overlay_icon_pixmap()));
        self.overlay_icon != hash && {
            self.overlay_icon = hash;
            true
        }
    }
    fn attention_icon_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of((
            tray.attention_icon_name(),
            tray.attention_icon_pixmap(),
            tray.attention_movie_name(),
        ));
        self.attention_icon != hash && {
            self.attention_icon = hash;
            true
        }
    }
    fn tool_tip_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of(tray.tool_tip());
        self.tool_tip != hash && {
            self.tool_tip = hash;
            true
        }
    }
    fn text_direction_changed<T: Tray>(&mut self, t: &T) -> Option<TextDirection> {
        let v = t.text_direction();
        if self.text_direction != v {
            self.text_direction = v;
            Some(v)
        } else {
            None
        }
    }
}

fn hash_of<T: Hash>(v: T) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    v.hash(&mut hasher);
    hasher.finish()
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        State {
            tx: self.tx.clone(),
            inner: self.inner.clone(),
            prop_cache: self.prop_cache.clone(),
        }
    }
}
