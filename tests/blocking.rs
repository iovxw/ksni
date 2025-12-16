use std::sync::Mutex;

use ksni::blocking::TrayMethods;

fn system_has_sni() -> bool {
    let conn = zbus::blocking::Connection::session().unwrap();
    let dbus_object =
        zbus::blocking::fdo::DBusProxy::new(&conn).expect("built-in Proxy should be valid");

    dbus_object
        .name_has_owner(
            zbus::names::WellKnownName::from_static_str_unchecked("org.kde.StatusNotifierWatcher")
                .into(),
        )
        .unwrap()
}

#[test]
fn assume_sni_available() {
    static OFFLINE_REASON: Mutex<Option<ksni::OfflineReason>> = Mutex::new(None);
    struct MyTray;
    impl ksni::Tray for MyTray {
        fn id(&self) -> String {
            std::any::type_name::<Self>().into()
        }
        fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
            OFFLINE_REASON.lock().unwrap().replace(reason);
            false
        }
    }

    let handle = MyTray.assume_sni_available(true).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    handle.shutdown().wait();

    if system_has_sni() {
        assert!(OFFLINE_REASON.lock().unwrap().is_none());
    } else {
        assert!(matches!(
            *OFFLINE_REASON.lock().unwrap(),
            Some(ksni::OfflineReason::Error(ksni::Error::Watcher(
                zbus::fdo::Error::ServiceUnknown(_)
            )))
        ));
    }
}
