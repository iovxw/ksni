use std::sync::Mutex;

use ksni::TrayMethods;

async fn system_has_sni() -> bool {
    let conn = zbus::Connection::session().await.unwrap();
    let dbus_object = zbus::fdo::DBusProxy::new(&conn)
        .await
        .expect("built-in Proxy should be valid");

    dbus_object
        .name_has_owner(
            zbus::names::WellKnownName::from_static_str_unchecked("org.kde.StatusNotifierWatcher")
                .into(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn assume_sni_available() {
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

    let handle = MyTray.assume_sni_available(true).spawn().await.unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    handle.shutdown().await;

    if system_has_sni().await {
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
