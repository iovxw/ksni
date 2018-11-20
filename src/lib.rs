use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use dbus::arg::{RefArg, Variant};
use dbus::SignalArgs;

pub mod dbus_interface;
pub mod menu;
pub mod tray;

const SNI_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

pub trait Methods {
    type Err: std::fmt::Display;
    fn activate(&self, x: i32, y: i32) -> Result<(), Self::Err>;
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), Self::Err>;
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), Self::Err>;
    fn properties(&self) -> &tray::Properties;
}

struct TrayService<T: Methods> {
    inner: T,
    // A list of menu item and it's submenu
    list: RefCell<Vec<(menu::RawMenuItem, Vec<usize>)>>,
    conn: Rc<dbus::Connection>,
    menu_path: dbus::Path<'static>,
}

impl<T: Methods> fmt::Debug for TrayService<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct(&format!("StatusNotifierItem")).finish()
    }
}

impl<T: Methods> dbus_interface::StatusNotifierItem for TrayService<T> {
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
    fn context_menu(&self, x: i32, y: i32) -> Result<(), Self::Err> {
        Ok(())
    }
    fn get_item_is_menu(&self) -> Result<bool, Self::Err> {
        Ok(false)
    }
    fn get_attention_movie_name(&self) -> Result<String, Self::Err> {
        Ok("".into())
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

impl<T: Methods> dbus_interface::Dbusmenu for TrayService<T> {
    type Err = dbus::tree::MethodErr;
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
        Self::Err,
    > {
        Ok((
            0,
            crate::menu::to_dbusmenu_variant(
                &self.list.borrow(),
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
    ) -> Result<Vec<(i32, HashMap<String, Variant<Box<dyn RefArg + 'static>>>)>, Self::Err> {
        let r = ids
            .into_iter()
            .map(|id| {
                (
                    id,
                    self.list.borrow()[id as usize]
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
    ) -> Result<Variant<Box<dyn RefArg + 'static>>, Self::Err> {
        unimplemented!()
    }
    fn event(
        &self,
        id: i32,
        event_id: &str,
        data: Variant<Box<dyn RefArg>>,
        timestamp: u32,
    ) -> Result<(), Self::Err> {
        match event_id {
            "clicked" => {
                let activate = self.list.borrow()[id as usize].0.on_clicked.clone();
                let m = (activate)(&mut self.list.borrow_mut(), id as usize);
                self.conn.send(m.to_emit_message(&self.menu_path)).unwrap();
            }
            _ => (),
        }
        Ok(())
    }
    fn event_group(
        &self,
        events: Vec<(i32, &str, Variant<Box<dyn RefArg>>, u32)>,
    ) -> Result<Vec<i32>, Self::Err> {
        unimplemented!()
    }
    fn about_to_show(&self, id: i32) -> Result<bool, Self::Err> {
        Ok(false)
    }
    fn about_to_show_group(&self, ids: Vec<i32>) -> Result<(Vec<i32>, Vec<i32>), Self::Err> {
        unimplemented!()
    }
    fn get_version(&self) -> Result<u32, Self::Err> {
        Ok(3)
    }
    fn get_text_direction(&self) -> Result<String, Self::Err> {
        Ok("ltr".into())
    }
    fn get_status(&self) -> Result<String, Self::Err> {
        Ok("normal".into())
    }
    fn get_icon_theme_path(&self) -> Result<Vec<String>, Self::Err> {
        Ok(vec![])
    }
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
            p: tray::Properties,
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
            fn properties(&self) -> &tray::Properties {
                &self.p
            }
        }
        let mut p = tray::Properties::new();
        p.icon_name = "desktop".to_owned();
        let foo = Foo { p };

        use menu::*;
        let tray = Rc::new(TrayService {
            inner: foo,
            list: RefCell::new(menu::menu_flatten(vec![
                SubMenu {
                    label: "a".into(),
                    submenu: vec![
                        SubMenu {
                            label: "a1".into(),
                            submenu: vec![StandardItem {
                                label: "a1.1".into(),
                                activate: Box::new(|| println!("a")),
                                ..Default::default()
                            }
                            .into()],
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "a2".into(),
                            ..Default::default()
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
                MenuItem::Sepatator,
                CheckmarkItem {
                    label: "b".into(),
                    ..Default::default()
                }
                .into(),
            ])),
            conn: conn.clone(),
            menu_path: MENU_PATH.into(),
        });

        let tray1 = tray.clone();
        let f = dbus::tree::Factory::new_fn::<()>();
        let sni_interface = dbus_interface::status_notifier_item_server(&f, (), move |_| {
            tray1.clone() as Rc<dyn dbus_interface::StatusNotifierItem<Err = _>>
        });
        let menu_interface = dbus_interface::dbusmenu_server(&f, (), move |_| {
            tray.clone() as Rc<dbus_interface::Dbusmenu<Err = _>>
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
