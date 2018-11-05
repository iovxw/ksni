use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

pub mod dbus_interface;
pub mod dbusmenu;

const SNI_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

pub trait Methods {
    type Err: std::fmt::Display;
    fn activate(&self, x: i32, y: i32) -> Result<(), Self::Err>;
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), Self::Err>;
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), Self::Err>;
    fn properties(&self) -> &Properties;
}

#[derive(Clone, Debug)]
pub struct Properties {
    pub category: Category,
    pub id: String,
    pub title: String,
    pub status: Status,
    pub window_id: i32, // u32 in org.freedesktop.StatusNotifierItem
    pub icon_name: String,
    pub icon_pixmap: Vec<Icon>,
    pub overlay_icon_name: String,
    pub overlay_icon_pixmap: Vec<Icon>,
    pub attention_icon_name: String,
    pub attention_icon_pixmap: Vec<Icon>,
    pub attention_moive_name: String,
    pub tool_tip: ToolTip,

    conn: Rc<dbus::Connection>,
}

impl Properties {
    fn new(conn: Rc<dbus::Connection>) -> Self {
        Properties {
            category: Category::ApplicationStatus,
            id: Default::default(),
            title: Default::default(),
            status: Status::Active,
            window_id: 0,
            icon_name: Default::default(),
            icon_pixmap: Default::default(),
            overlay_icon_name: Default::default(),
            overlay_icon_pixmap: Default::default(),
            attention_icon_name: Default::default(),
            attention_icon_pixmap: Default::default(),
            attention_moive_name: Default::default(),
            tool_tip: Default::default(),
            conn,
        }
    }
}

/// Describes the category of this item.
#[derive(Copy, Clone, Debug)]
pub enum Category {
    /// The item describes the status of a generic application, for instance
    /// the current state of a media player. In the case where the category of
    /// the item can not be known, such as when the item is being proxied from
    /// another incompatible or emulated system, ApplicationStatus can be used
    /// a sensible default fallback.
    ApplicationStatus,
    /// The item describes the status of communication oriented applications,
    /// like an instant messenger or an email client.
    Communications,
    /// The item describes services of the system not seen as a stand alone
    /// application by the user, such as an indicator for the activity of a disk
    /// indexing service.
    SystemServices,
    /// The item describes the state and control of a particular hardware,
    /// such as an indicator of the battery charge or sound card volume control.
    Hardware,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let r = match *self {
            Category::ApplicationStatus => "ApplicationStatus",
            Category::Communications => "Communications",
            Category::SystemServices => "SystemServices",
            Category::Hardware => "Hardware",
        };
        f.write_str(r)
    }
}

/// Describes the status of this item or of the associated application.
#[derive(Copy, Clone, Debug)]
pub enum Status {
    /// The item doesn't convey important information to the user, it can be
    /// considered an "idle" status and is likely that visualizations will chose
    /// to hide it.
    Passive,
    /// The item is active, is more important that the item will be shown in
    /// some way to the user.
    Active,
    /// The item carries really important information for the user, such as
    /// battery charge running out and is wants to incentive the direct user
    /// intervention. Visualizations should emphasize in some way the items with
    /// NeedsAttention status.
    NeedsAttention,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let r = match *self {
            Status::Passive => "Passive",
            Status::Active => "Active",
            Status::NeedsAttention => "NeedsAttention",
        };
        f.write_str(r)
    }
}

/// Data structure that describes extra information associated to this item,
/// that can be visualized for instance by a tooltip (or by any other mean the
/// visualization consider appropriate.
#[derive(Clone, Debug, Default)]
pub struct ToolTip {
    /// Freedesktop-compliant name for an icon.
    pub icon_name: String,
    /// Icon data
    pub icon_pixmap: Vec<Icon>,
    /// Title for this tooltip
    pub title: String,
    /// Descriptive text for this tooltip. It can contain also a subset of the
    /// HTML markup language, for a list of allowed tags see Section Markup.
    pub description: String,
}

impl From<ToolTip> for (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
    fn from(tooltip: ToolTip) -> Self {
        (
            tooltip.icon_name,
            tooltip.icon_pixmap.into_iter().map(Into::into).collect(),
            tooltip.title,
            tooltip.description,
        )
    }
}

#[derive(Clone, Debug)]
pub struct Icon {
    pub width: i32,
    pub height: i32,
    /// ARGB32 format, network byte order
    pub data: Vec<u8>,
}

impl From<Icon> for (i32, i32, Vec<u8>) {
    fn from(icon: Icon) -> Self {
        (icon.width, icon.height, icon.data)
    }
}

#[derive(Copy, Clone, Default)]
struct StatusNotifierItem<T: Methods> {
    inner: T,
}

impl<T: Methods> fmt::Debug for StatusNotifierItem<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct(&format!("StatusNotifierItem")).finish()
    }
}

impl<T: Methods> dbus_interface::StatusNotifierItem for StatusNotifierItem<T> {
    type Err = dbus::tree::MethodErr;
    fn activate(&self, x: i32, y: i32) -> Result<(), Self::Err> {
        self.inner
            .activate(x, y)
            .map_err(|e| dbus::tree::MethodErr::failed(&e))
    }
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), Self::Err> {
        self.inner
            .secondary_activate(x, y)
            .map_err(|e| dbus::tree::MethodErr::failed(&e))
    }
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), Self::Err> {
        self.inner
            .scroll(delta, dir)
            .map_err(|e| dbus::tree::MethodErr::failed(&e))
    }
    fn get_category(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().category.to_string())
    }
    fn get_id(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().id.clone())
    }
    fn get_title(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().title.clone())
    }
    fn get_status(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().status.to_string())
    }
    fn get_window_id(&self) -> Result<i32, Self::Err> {
        Ok(self.inner.properties().window_id.clone())
    }
    fn get_menu(&self) -> Result<dbus::Path<'static>, Self::Err> {
        Ok(MENU_PATH.into())
    }
    fn get_icon_name(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().icon_name.clone())
    }
    fn get_icon_theme_path(&self) -> Result<String, Self::Err> {
        Ok("".into())
    }
    fn get_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, Self::Err> {
        Ok(self
            .inner
            .properties()
            .icon_pixmap
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }
    fn get_overlay_icon_name(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().overlay_icon_name.clone())
    }
    fn get_overlay_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, Self::Err> {
        Ok(self
            .inner
            .properties()
            .overlay_icon_pixmap
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }
    fn get_attention_icon_name(&self) -> Result<String, Self::Err> {
        Ok(self.inner.properties().attention_icon_name.clone())
    }
    fn get_attention_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, Self::Err> {
        Ok(self
            .inner
            .properties()
            .attention_icon_pixmap
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }
    fn get_tool_tip(
        &self,
    ) -> Result<(String, Vec<(i32, i32, Vec<u8>)>, String, String), Self::Err> {
        Ok(self.inner.properties().tool_tip.clone().into())
    }
}

struct TData<T: Methods> {
    _marker: PhantomData<*const T>,
}
impl<T: Methods> Default for TData<T> {
    fn default() -> Self {
        TData {
            _marker: PhantomData,
        }
    }
}
impl<T: Methods> dbus::tree::DataType for TData<T> {
    type Tree = ();
    type ObjectPath = StatusNotifierItem<T>;
    type Property = ();
    type Interface = ();
    type Method = ();
    type Signal = ();
}

fn name_owner_changed(ci: &dbus::ConnectionItem) -> Option<(&str, Option<&str>, Option<&str>)> {
    let m = if let &dbus::ConnectionItem::Signal(ref s) = ci {
        s
    } else {
        return None;
    };
    if &*m.interface().unwrap() != "org.freedesktop.DBus" {
        return None;
    };
    if &*m.member().unwrap() != "NameOwnerChanged" {
        return None;
    };
    let (name, old_owner, new_owner) = m.get3::<&str, &str, &str>();
    Some((
        name.expect("NameOwnerChanged"),
        old_owner.filter(|s| !s.is_empty()),
        new_owner.filter(|s| !s.is_empty()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore]
    fn it_works() {
        use dbus::BusType;
        use dbus::Connection;
        use dbus::SignalArgs;
        let name = format!("org.kde.StatusNotifierItem-x-1");
        let conn = Connection::get_private(BusType::Session).unwrap();
        let conn = Rc::new(conn);

        struct Foo {
            p: Properties,
        }
        impl Methods for Foo {
            type Err = String;
            fn activate(&self, x: i32, y: i32) -> Result<(), Self::Err> {
                Ok(())
            }
            fn secondary_activate(&self, x: i32, y: i32) -> Result<(), Self::Err> {
                Ok(())
            }
            fn scroll(&self, delta: i32, dir: &str) -> Result<(), Self::Err> {
                Ok(())
            }
            fn properties(&self) -> &Properties {
                &self.p
            }
        }
        let mut p = Properties::new(conn.clone());
        p.icon_name = "desktop".to_owned();
        let foo = Foo { p };

        let menu: Rc<(dyn dbus_interface::Dbusmenu<Err = _>)> =
            Rc::new(dbusmenu::DBusMenu::from(vec![
                dbusmenu::MenuItem {
                    label: "a".into(),
                    submenu: vec![
                        dbusmenu::MenuItem {
                            label: "a1".into(),
                            submenu: vec![
                                dbusmenu::MenuItem {
                                    label: "a1.1".into(),
                                    ..Default::default()
                                },
                                dbusmenu::MenuItem {
                                    label: "a1.2".into(),
                                    ..Default::default()
                                },
                            ],
                            ..Default::default()
                        },
                        dbusmenu::MenuItem {
                            label: "a2".into(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                dbusmenu::MenuItem {
                    label: "b".into(),
                    ..Default::default()
                },
            ]));
        let sni: Rc<(dyn dbus_interface::StatusNotifierItem<Err = _>)> =
            Rc::new(StatusNotifierItem { inner: foo });

        let f = dbus::tree::Factory::new_fn::<()>();
        let sni_interface =
            dbus_interface::status_notifier_item_server(&f, (), move |_| sni.clone());
        let menu_interface = dbus_interface::dbusmenu_server(&f, (), move |_| menu.clone());
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
            );
        conn.register_name(&name, 0).unwrap();
        tree.set_registered(&conn, true).unwrap();
        conn.add_handler(tree);

        let status_notifier_watcher = conn.with_path(
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            1000,
        );
        use dbus_interface::StatusNotifierWatcher;
        status_notifier_watcher
            .register_status_notifier_item(&name)
            .unwrap_or_default();

        conn.add_match("interface='org.freedesktop.DBus',member='NameOwnerChanged'")
            .unwrap();

        for m in conn.iter(500) {
            //let msg =
            //    dbus_interface::StatusNotifierItemNewIcon {}.to_emit_message(&SNI_PATH.into());
            //conn.send(msg).unwrap();
            if let Some(("org.kde.StatusNotifierWatcher", _, Some(_new_owner))) =
                name_owner_changed(&m)
            {
                status_notifier_watcher
                    .register_status_notifier_item(&name)
                    .unwrap_or_default();
            }
            dbg!(m);
        }
    }
}
