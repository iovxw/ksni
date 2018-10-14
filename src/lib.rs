pub mod dbus_interface;

#[derive(Copy, Clone, Default, Debug)]
struct StatusNotifierItem {}

impl dbus_interface::StatusNotifierItem for StatusNotifierItem {
    type Err = dbus::tree::MethodErr;
    fn activate(&self, _x: i32, _y: i32) -> Result<(), Self::Err> {
        dbg!("activate");
        Ok(())
    }
    fn secondary_activate(&self, _x: i32, _y: i32) -> Result<(), Self::Err> {
        dbg!("seondary");
        Ok(())
    }
    fn scroll(&self, _delta: i32, _dir: &str) -> Result<(), Self::Err> {
        dbg!("scroll");
        Ok(())
    }
    fn get_category(&self) -> Result<String, Self::Err> {
        Ok("ApplicationStatus".into())
    }
    fn get_id(&self) -> Result<String, Self::Err> {
        Ok("AHHH".into())
    }
    fn get_title(&self) -> Result<String, Self::Err> {
        Ok("blahblah".into())
    }
    fn get_status(&self) -> Result<String, Self::Err> {
        Ok("Active".into())
    }
    fn get_window_id(&self) -> Result<i32, Self::Err> {
        Ok(0)
    }
    fn get_menu(&self) -> Result<dbus::Path<'static>, Self::Err> {
        Ok(Default::default())
    }
    fn get_icon_name(&self) -> Result<String, Self::Err> {
        dbg!("icon name");
        Ok("desktop".into())
    }
    fn get_icon_theme_path(&self) -> Result<String, Self::Err> {
        dbg!("icon theme path");
        Ok("".into())
    }
    fn get_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, Self::Err> {
        dbg!("pximap");
        Ok(vec![])
    }
    fn get_overlay_icon_name(&self) -> Result<String, Self::Err> {
        Ok("".into())
    }
    fn get_overlay_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, Self::Err> {
        Ok(vec![])
    }
    fn get_attention_icon_name(&self) -> Result<String, Self::Err> {
        Ok("".into())
    }
    fn get_attention_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, Self::Err> {
        Ok(vec![])
    }
    fn get_tool_tip(
        &self,
    ) -> Result<(String, Vec<(i32, i32, Vec<u8>)>, String, String), Self::Err> {
        Ok(("".into(), vec![], "".into(), "".into()))
    }
}

#[derive(Copy, Clone, Default, Debug)]
struct TData;
impl dbus::tree::DataType for TData {
    type Tree = ();
    type ObjectPath = StatusNotifierItem;
    type Property = ();
    type Interface = ();
    type Method = ();
    type Signal = ();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        use dbus::BusType;
        use dbus::Connection;
        use dbus::SignalArgs;
        let name = format!("org.kde.StatusNotifierItem-x-1");
        let sni_path = "/StatusNotifierItem";

        let f = dbus::tree::Factory::new_fn::<TData>();
        let i = dbus_interface::status_notifier_item_server(&f, (), |minfo| minfo.path.get_data());
        let tree = f.tree(()).add(
            f.object_path(sni_path, StatusNotifierItem::default())
                .add(i),
        );
        let conn = Connection::get_private(BusType::Session).unwrap();
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
            .unwrap();

        for m in conn.iter(1000) {
            let msg =
                dbus_interface::StatusNotifierItemNewIcon {}.to_emit_message(&sni_path.into());
            conn.send(msg).unwrap();
            dbg!(m);
        }
    }
}
