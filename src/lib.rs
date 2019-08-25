use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbus::arg::{RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, MessageType, SignalArgs};

mod dbus_ext;
mod dbus_interface;
mod freedesktop;
pub mod menu;
pub mod tray;

use dbus_interface::StatusNotifierWatcher;
use menu::{MenuItem, TextDirection};
use tray::{Category, Icon, Status, ToolTip};

const SNI_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

static COUNTER: AtomicUsize = AtomicUsize::new(1);

pub trait TrayModel {
    type Err: std::fmt::Display;
    /// Asks the status notifier item for activation, this is typically a
    /// consequence of user input, such as mouse left click over the graphical
    /// representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn activate(_model: &Model<Self>, _x: i32, _y: i32) {}

    /// Is to be considered a secondary and less important form of activation
    /// compared to Activate.
    /// This is typically a consequence of user input, such as mouse middle
    /// click over the graphical representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn secondary_activate(_model: &Model<Self>, _x: i32, _y: i32) {}

    /// The user asked for a scroll action. This is caused from input such as
    /// mouse wheel over the graphical representation of the item.
    ///
    /// The delta parameter represent the amount of scroll, the orientation
    /// parameter represent the horizontal or vertical orientation of the scroll
    /// request and its legal values are horizontal and vertical.
    fn scroll(_model: &Model<Self>, _delta: i32, _dir: &str) {}

    /// Describes the category of this item.
    fn category(_model: &Model<Self>) -> Category {
        tray::Category::ApplicationStatus
    }

    /// It's a name that should be unique for this application and consistent
    /// between sessions, such as the application name itself.
    fn id(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// It's a name that describes the application, it can be more descriptive
    /// than Id.
    fn title(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// Describes the status of this item or of the associated application.
    fn status(_model: &Model<Self>) -> Status {
        tray::Status::Active
    }

    // NOTE: u32 in org.freedesktop.StatusNotifierItem
    /// It's the windowing-system dependent identifier for a window, the
    /// application can chose one of its windows to be available through this
    /// property or just set 0 if it's not interested.
    fn window_id(_model: &Model<Self>) -> i32 {
        0
    }

    /// An additional path to add to the theme search path to find the icons.
    fn icon_theme_path(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// The item only support the context menu, the visualization
    /// should prefer showing the menu or sending ContextMenu()
    /// instead of Activate()
    // fn item_is_menu() -> bool { false }

    /// The StatusNotifierItem can carry an icon that can be used by the
    /// visualization to identify the item.
    fn icon_name(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// Carries an ARGB32 binary representation of the icon
    fn icon_pixmap(_model: &Model<Self>) -> Vec<Icon> {
        Default::default()
    }

    /// The Freedesktop-compliant name of an icon. This can be used by the
    /// visualization to indicate extra state information, for instance as an
    /// overlay for the main icon.
    fn overlay_icon_name(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// ARGB32 binary representation of the overlay icon described in the
    /// previous paragraph.
    fn overlay_icon_pixmap(_model: &Model<Self>) -> Vec<Icon> {
        Default::default()
    }

    /// The Freedesktop-compliant name of an icon. this can be used by the
    /// visualization to indicate that the item is in RequestingAttention state.
    fn attention_icon_name(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// ARGB32 binary representation of the requesting attention icon describe in
    /// the previous paragraph.
    fn attention_icon_pixmap(_model: &Model<Self>) -> Vec<Icon> {
        Default::default()
    }

    /// An item can also specify an animation associated to the
    /// RequestingAttention state.
    /// This should be either a Freedesktop-compliant icon name or a full path.
    /// The visualization can chose between the movie or AttentionIconPixmap (or
    /// using neither of those) at its discretion.
    fn attention_movie_name(_model: &Model<Self>) -> String {
        Default::default()
    }

    /// Data structure that describes extra information associated to this item,
    /// that can be visualized for instance by a tooltip (or by any other mean
    /// the visualization consider appropriate.
    fn tool_tip(_model: &Model<Self>) -> ToolTip {
        Default::default()
    }

    /// Represents the way the text direction of the application.  This
    /// allows the server to handle mismatches intelligently.
    fn text_direction(_model: &Model<Self>) -> TextDirection {
        menu::TextDirection::LeftToRight
    }

    fn menu(_model: &Model<Self>) -> Vec<MenuItem> {
        Default::default()
    }
}

struct TrayService<T: TrayModel> {
    model: Model<T>,
    // A list of menu item and it's submenu
    menu: RefCell<Vec<(menu::RawMenuItem, Vec<usize>)>>,
    menu_path: dbus::Path<'static>,
}

pub struct Model<T: TrayModel + ?Sized> {
    inner: Arc<Mutex<T>>,
}

impl<T: TrayModel> Model<T> {
    pub fn update<F: Fn(&T)>(&self, f: F) {
        let inner = self.inner.lock().unwrap();
        (f)(&inner);
    }
}

impl<T: TrayModel> Clone for Model<T> {
    fn clone(&self) -> Self {
        Model {
            inner: self.inner.clone(),
        }
    }
}

impl<T: TrayModel> fmt::Debug for TrayService<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct(&format!("StatusNotifierItem")).finish()
    }
}

impl<T: TrayModel> dbus_interface::StatusNotifierItem for TrayService<T> {
    fn activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        TrayModel::activate(&self.model, x, y);
        Ok(())
    }
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        TrayModel::secondary_activate(&self.model, x, y);
        Ok(())
    }
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), dbus::tree::MethodErr> {
        TrayModel::scroll(&self.model, delta, dir);
        Ok(())
    }
    fn context_menu(&self, _x: i32, _y: i32) -> Result<(), dbus::tree::MethodErr> {
        Ok(())
    }
    fn get_item_is_menu(&self) -> Result<bool, dbus::tree::MethodErr> {
        Ok(false)
    }
    fn get_category(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::category(&self.model).to_string())
    }
    fn get_id(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::id(&self.model))
    }
    fn get_title(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::title(&self.model))
    }
    fn get_status(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::status(&self.model).to_string())
    }
    fn get_window_id(&self) -> Result<i32, dbus::tree::MethodErr> {
        Ok(TrayModel::window_id(&self.model))
    }
    fn get_menu(&self) -> Result<dbus::Path<'static>, dbus::tree::MethodErr> {
        Ok(MENU_PATH.into())
    }
    fn get_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::icon_name(&self.model))
    }
    fn get_icon_theme_path(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::icon_theme_path(&self.model))
    }
    fn get_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        Ok(TrayModel::icon_pixmap(&self.model)
            .into_iter()
            .map(Into::into)
            .collect())
    }
    fn get_overlay_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::overlay_icon_name(&self.model))
    }
    fn get_overlay_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        Ok(TrayModel::overlay_icon_pixmap(&self.model)
            .into_iter()
            .map(Into::into)
            .collect())
    }
    fn get_attention_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::attention_icon_name(&self.model))
    }
    fn get_attention_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        Ok(TrayModel::attention_icon_pixmap(&self.model)
            .into_iter()
            .map(Into::into)
            .collect())
    }
    fn get_attention_movie_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::attention_movie_name(&self.model))
    }
    fn get_tool_tip(
        &self,
    ) -> Result<(String, Vec<(i32, i32, Vec<u8>)>, String, String), dbus::tree::MethodErr> {
        Ok(TrayModel::tool_tip(&self.model).into())
    }
}

impl<T: TrayModel> dbus_interface::Dbusmenu for TrayService<T> {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<&str>,
    ) -> Result<
        (
            u32,
            (
                i32,
                HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
                Vec<Variant<Box<dyn RefArg + 'static>>>,
            ),
        ),
        dbus::tree::MethodErr,
    > {
        Ok((
            0,
            crate::menu::to_dbusmenu_variant(
                &self.menu.borrow(),
                parent_id as usize,
                if recursion_depth < 0 {
                    None
                } else {
                    Some(recursion_depth as usize)
                },
                property_names,
            ),
        ))
    }
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<&str>,
    ) -> Result<
        Vec<(i32, HashMap<String, Variant<Box<dyn RefArg + 'static>>>)>,
        dbus::tree::MethodErr,
    > {
        let r = ids
            .into_iter()
            .map(|id| {
                (
                    id,
                    self.menu.borrow()[id as usize]
                        .0
                        .to_dbus_map(&property_names),
                )
            })
            .collect();
        Ok(r)
    }
    fn get_property(
        &self,
        id: i32,
        name: &str,
    ) -> Result<Variant<Box<dyn RefArg + 'static>>, dbus::tree::MethodErr> {
        // FIXME
        Err(dbus::tree::MethodErr::failed(&"unimplemented"))
    }
    fn event(
        &self,
        id: i32,
        event_id: &str,
        _data: Variant<Box<dyn RefArg>>,
        _timestamp: u32,
    ) -> Result<(), dbus::tree::MethodErr> {
        match event_id {
            "clicked" => {
                let activate = self.menu.borrow()[id as usize].0.on_clicked.clone();
                let m = (activate)(&mut self.menu.borrow_mut(), id as usize);
                if let Some(msg) = m {
                    dbus_ext::with_current(|conn| conn.send(msg.to_emit_message(&self.menu_path)))
                        .unwrap()
                        .unwrap();
                };
            }
            _ => (),
        }
        Ok(())
    }
    fn event_group(
        &self,
        events: Vec<(i32, &str, Variant<Box<dyn RefArg>>, u32)>,
    ) -> Result<Vec<i32>, dbus::tree::MethodErr> {
        let (found, not_found) = events
            .into_iter()
            .partition::<Vec<_>, _>(|event| (event.0 as usize) < self.menu.borrow().len());
        if found.is_empty() {
            return Err(dbus::tree::MethodErr::invalid_arg(
                &"None of the id in the events can be found",
            ));
        }
        for (id, event_id, data, timestamp) in found {
            self.event(id, event_id, data, timestamp)?;
        }
        Ok(not_found.into_iter().map(|event| event.0).collect())
    }
    fn about_to_show(&self, _id: i32) -> Result<bool, dbus::tree::MethodErr> {
        Ok(false)
    }
    fn about_to_show_group(
        &self,
        _ids: Vec<i32>,
    ) -> Result<(Vec<i32>, Vec<i32>), dbus::tree::MethodErr> {
        // FIXME: the DBus message should set the no reply flag
        Ok(Default::default())
    }
    fn get_version(&self) -> Result<u32, dbus::tree::MethodErr> {
        Ok(3)
    }
    fn get_text_direction(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(TrayModel::text_direction(&self.model).to_string())
    }
    fn get_status(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(match TrayModel::status(&self.model) {
            tray::Status::Active | tray::Status::Passive => menu::Status::Normal,
            tray::Status::NeedsAttention => menu::Status::Notice,
        }
        .to_string())
    }
    fn get_icon_theme_path(&self) -> Result<Vec<String>, dbus::tree::MethodErr> {
        let path = TrayModel::icon_theme_path(&self.model);
        Ok(if path.is_empty() {
            Default::default()
        } else {
            vec![path]
        })
    }
}

fn register_to_watcher(conn: &Connection, name: String) -> Result<(), dbus::Error> {
    let status_notifier_watcher = conn.with_proxy(
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        Duration::from_millis(1000),
    );
    status_notifier_watcher.register_status_notifier_item(&name)?;

    status_notifier_watcher.match_signal(
        move |h: freedesktop::NameOwnerChanged, c: &Connection| {
            if h.name == "org.kde.StatusNotifierWatcher" {
                c.with_proxy(
                    "org.kde.StatusNotifierWatcher",
                    "/StatusNotifierWatcher",
                    Duration::from_millis(1000),
                )
                .register_status_notifier_item(&name)
                .unwrap_or_default();
            }
            true
        },
    )?;
    Ok(())
}

pub fn run<T: TrayModel + 'static>(tray: T) -> Result<(), dbus::Error> {
    let name = format!(
        "org.kde.StatusNotifierItem-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::AcqRel)
    );
    let mut conn = Connection::new_session()?;
    conn.request_name(&name, true, true, false)?;

    let model = Model {
        inner: Arc::new(Mutex::new(tray)),
    };
    let menu = RefCell::new(menu::menu_flatten(T::menu(&model)));
    let tray_service = Rc::new(TrayService {
        model: model,
        menu: menu,
        menu_path: MENU_PATH.into(),
    });

    let tray_service_clone = tray_service.clone();
    let f = dbus::tree::Factory::new_fn::<()>();
    let sni_interface = dbus_interface::status_notifier_item_server(&f, (), move |_| {
        tray_service_clone.clone() as Rc<dyn dbus_interface::StatusNotifierItem>
    });
    let menu_interface = dbus_interface::dbusmenu_server(&f, (), move |_| {
        tray_service.clone() as Rc<dyn dbus_interface::Dbusmenu>
    });
    let tree = f
        .tree(())
        .add(
            f.object_path(SNI_PATH, ())
                .introspectable()
                .add(sni_interface),
        )
        .add(
            f.object_path(MENU_PATH, ())
                .introspectable()
                .add(menu_interface),
        )
        // Add root path, to help introspection from debugging tools
        .add(f.object_path("/", ()).introspectable());
    let mut rule = MatchRule::new();
    rule.msg_type = Some(MessageType::MethodCall);
    conn.start_receive(
        rule,
        Box::new(move |msg, c| {
            dbus_ext::with_conn(c, || {
                if let Some(replies) = tree.handle(&msg) {
                    for r in replies {
                        let _ = c.send(r);
                    }
                }
            });
            true
        }),
    );

    register_to_watcher(&conn, name)?;

    loop {
        conn.process(Duration::from_millis(500))?;
    }
}
