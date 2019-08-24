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
    fn activate(_model: Model<Self>, _x: i32, _y: i32) {}

    /// Is to be considered a secondary and less important form of activation
    /// compared to Activate.
    /// This is typically a consequence of user input, such as mouse middle
    /// click over the graphical representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn secondary_activate(_model: Model<Self>, _x: i32, _y: i32) {}

    /// The user asked for a scroll action. This is caused from input such as
    /// mouse wheel over the graphical representation of the item.
    ///
    /// The delta parameter represent the amount of scroll, the orientation
    /// parameter represent the horizontal or vertical orientation of the scroll
    /// request and its legal values are horizontal and vertical.
    fn scroll(_model: Model<Self>, _delta: i32, _dir: &str) {}

    fn tray_properties() -> tray::Properties {
        Default::default()
    }
    fn menu_properties() -> menu::Properties {
        Default::default()
    }
    fn menu() -> Vec<menu::MenuItem> {
        Default::default()
    }
}

struct TrayService<T: TrayModel> {
    model: Model<T>,
    tray_properties: tray::Properties,
    menu_properties: menu::Properties,
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
        TrayModel::activate(self.model.clone(), x, y);
        Ok(())
    }
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        TrayModel::secondary_activate(self.model.clone(), x, y);
        Ok(())
    }
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), dbus::tree::MethodErr> {
        TrayModel::scroll(self.model.clone(), delta, dir);
        Ok(())
    }
    fn context_menu(&self, _x: i32, _y: i32) -> Result<(), dbus::tree::MethodErr> {
        Ok(())
    }
    fn get_item_is_menu(&self) -> Result<bool, dbus::tree::MethodErr> {
        Ok(self.tray_properties.item_is_menu)
    }
    fn get_category(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.category.to_string())
    }
    fn get_id(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.id.clone())
    }
    fn get_title(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.title.clone())
    }
    fn get_status(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.status.to_string())
    }
    fn get_window_id(&self) -> Result<i32, dbus::tree::MethodErr> {
        Ok(self.tray_properties.window_id.clone())
    }
    fn get_menu(&self) -> Result<dbus::Path<'static>, dbus::tree::MethodErr> {
        Ok(MENU_PATH.into())
    }
    fn get_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.icon_name.clone())
    }
    fn get_icon_theme_path(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.icon_theme_path.clone())
    }
    fn get_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        Ok(self
            .tray_properties
            .icon_pixmap
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }
    fn get_overlay_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.overlay_icon_name.clone())
    }
    fn get_overlay_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        Ok(self
            .tray_properties
            .overlay_icon_pixmap
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }
    fn get_attention_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.attention_icon_name.clone())
    }
    fn get_attention_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        Ok(self
            .tray_properties
            .attention_icon_pixmap
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }
    fn get_attention_movie_name(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.tray_properties.attention_movie_name.clone())
    }
    fn get_tool_tip(
        &self,
    ) -> Result<(String, Vec<(i32, i32, Vec<u8>)>, String, String), dbus::tree::MethodErr> {
        Ok(self.tray_properties.tool_tip.clone().into())
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
        Ok(self.menu_properties.text_direction.to_string())
    }
    fn get_status(&self) -> Result<String, dbus::tree::MethodErr> {
        Ok(self.menu_properties.status.to_string())
    }
    fn get_icon_theme_path(&self) -> Result<Vec<String>, dbus::tree::MethodErr> {
        Ok(vec![])
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

    let tray_service = Rc::new(TrayService {
        model: Model {
            inner: Arc::new(Mutex::new(tray)),
        },
        tray_properties: T::tray_properties(),
        menu_properties: T::menu_properties(),
        menu: RefCell::new(menu::menu_flatten(T::menu())),
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
