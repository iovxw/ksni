use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::blocking::{Connection, connection};
use zbus::zvariant::OwnedValue;

use super::{
    MockWatcher, RegisterItemError, TestTray, WatcherState, DEFAULT_TIMEOUT,
    MENU_INTERFACE, MENU_PATH, PROPERTIES_INTERFACE, SNI_INTERFACE, SNI_PATH, WATCHER_NAME,
    WATCHER_PATH, dbusmenu_assertions, has_owner, message_body, mutate_sni_properties,
    property_i32, property_string, registration_and_watcher_assertions,
    session_connection, sni_property_and_method_assertions, snapshot_events,
    spawn_filtered_signal_waiter, spawn_signal_waiter, sni_proxy, wait_until,
    watcher_proxy,
};

pub struct WatcherHandle {
    connection: Connection,
    state: Arc<Mutex<WatcherState>>,
}

impl WatcherHandle {
    pub fn start(host_registered: bool) -> zbus::Result<Self> {
        Self::start_with_register_error(host_registered, None)
    }

    pub fn start_with_register_error(
        host_registered: bool,
        register_item_error: Option<RegisterItemError>,
    ) -> zbus::Result<Self> {
        let state = Arc::new(Mutex::new(WatcherState {
            registered_items: Vec::new(),
            host_registered,
            protocol_version: 0,
            register_item_error,
        }));
        let connection = connection::Builder::session()?
            .method_timeout(DEFAULT_TIMEOUT)
            .serve_at(
                WATCHER_PATH,
                MockWatcher {
                    state: state.clone(),
                },
            )?
            .name(WATCHER_NAME)?
            .build()?;
        Ok(Self { connection, state })
    }

    pub fn registered_items(&self) -> Vec<String> {
        self.state.lock().unwrap().registered_items.clone()
    }

    pub fn set_host_registered(&self, value: bool) {
        self.state.lock().unwrap().host_registered = value;
    }

    pub fn set_protocol_version(&self, value: i32) {
        self.state.lock().unwrap().protocol_version = value;
    }

    pub fn set_register_item_error(&self, value: Option<RegisterItemError>) {
        self.state.lock().unwrap().register_item_error = value;
    }

    pub fn wait_for_registration_count(&self, count: usize, timeout: Duration) -> Vec<String> {
        wait_until(timeout, || self.registered_items().len() >= count, "tray registrations");
        self.registered_items()
    }

    pub fn wait_for_item_registration(&self, timeout: Duration) -> String {
        self.wait_for_registration_count(1, timeout)
            .into_iter()
            .next()
            .expect("at least one registration should exist")
    }

    pub fn close(self) {
        self.connection.close().expect("watcher connection should close");
    }
}

pub fn blocking_registration_and_watchers() {
    use ksni::blocking::TrayMethods as _;

    let connection = session_connection();

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should register with the mock watcher");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    assert!(service_name.starts_with("org.kde.StatusNotifierItem-"));
    registration_and_watcher_assertions(&connection, &service_name);
    handle.shutdown().wait();
    watcher.close();

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .disable_dbus_name(true)
        .spawn()
        .expect("tray should register with its unique name when dbus names are disabled");
    let unique_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    assert!(unique_name.starts_with(':'));
    assert!(has_owner(&connection, &unique_name));
    handle.shutdown().wait();
    watcher.close();

    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn() {
        Ok(_) => panic!("missing watchers must fail without assume_sni_available"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::Watcher(zbus::fdo::Error::ServiceUnknown(_))));

    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .expect("assume_sni_available should turn missing watchers into a soft offline state");
    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).offline.iter().any(|entry| entry.contains("ServiceUnknown")),
        "ServiceUnknown watcher_offline callback",
    );
    handle.shutdown().wait();

    let watcher = WatcherHandle::start(false).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn() {
        Ok(_) => panic!("watchers without hosts should report WontShow"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::WontShow));
    watcher.close();

    let watcher = WatcherHandle::start_with_register_error(
        true,
        Some(RegisterItemError::InvalidArgs("mock rejection".into())),
    )
    .unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn() {
        Ok(_) => panic!("watcher registration failures should surface as watcher errors"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::Watcher(zbus::fdo::Error::InvalidArgs(_))));
    watcher.close();
}

pub fn blocking_watcher_lifecycle() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let first_registration = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    watcher.close();

    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).offline.iter().any(|entry| entry.contains("No")),
        "watcher offline callback",
    );

    let watcher = WatcherHandle::start(true).unwrap();
    let second_registration = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).online_count == 1,
        "watcher online callback",
    );
    assert_eq!(first_registration, second_registration);

    handle.shutdown().wait();
    watcher.close();
}

pub fn blocking_status_notifier_item_protocol() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let connection = session_connection();

    sni_property_and_method_assertions(&connection, &service_name, &events);

    let new_title = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewTitle");
    let new_icon = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewIcon");
    let new_overlay =
        spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewOverlayIcon");
    let new_attention =
        spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewAttentionIcon");
    let new_tool_tip =
        spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewToolTip");
    let new_status = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewStatus");
    let sni_properties_changed = spawn_filtered_signal_waiter(
        &service_name,
        SNI_PATH,
        PROPERTIES_INTERFACE,
        "PropertiesChanged",
        vec![(0, SNI_INTERFACE.to_string())],
    );
    let menu_properties_changed = spawn_filtered_signal_waiter(
        &service_name,
        MENU_PATH,
        PROPERTIES_INTERFACE,
        "PropertiesChanged",
        vec![(0, MENU_INTERFACE.to_string())],
    );

    handle
        .update(|tray| mutate_sni_properties(tray))
        .expect("tray should still be alive for updates");

    let _: () = message_body(new_title.wait(DEFAULT_TIMEOUT));
    let _: () = message_body(new_icon.wait(DEFAULT_TIMEOUT));
    let _: () = message_body(new_overlay.wait(DEFAULT_TIMEOUT));
    let _: () = message_body(new_attention.wait(DEFAULT_TIMEOUT));
    let _: () = message_body(new_tool_tip.wait(DEFAULT_TIMEOUT));
    let (status,): (String,) = message_body(new_status.wait(DEFAULT_TIMEOUT));
    assert_eq!(status, "NeedsAttention");

    let (sni_iface, sni_changed, sni_invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) =
        message_body(sni_properties_changed.wait(DEFAULT_TIMEOUT));
    assert_eq!(sni_iface, SNI_INTERFACE);
    assert!(sni_invalidated.is_empty());
    assert_eq!(property_string(&sni_changed, "Category"), "Communications");
    assert_eq!(property_i32(&sni_changed, "WindowId"), 42);
    assert_eq!(property_string(&sni_changed, "IconThemePath"), "/tmp/mock-icons-updated");

    let (menu_iface, menu_changed, menu_invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) =
        message_body(menu_properties_changed.wait(DEFAULT_TIMEOUT));
    assert_eq!(menu_iface, MENU_INTERFACE);
    assert!(menu_invalidated.is_empty());
    assert_eq!(property_string(&menu_changed, "TextDirection"), "rtl");
    assert_eq!(property_string(&menu_changed, "Status"), "notice");

    handle.shutdown().wait();
    watcher.close();
}

pub fn blocking_dbusmenu_protocol() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let connection = session_connection();

    dbusmenu_assertions(&connection, &service_name, &events);

    let items_properties_updated =
        spawn_signal_waiter(&service_name, MENU_PATH, MENU_INTERFACE, "ItemsPropertiesUpdated");
    handle
        .update(|tray| {
            tray.standard_label = "Open updated".into();
            tray.checkmark_checked = false;
        })
        .expect("tray should still be alive for updates");
    let (updated_props, removed_props): (
        Vec<(i32, HashMap<String, OwnedValue>)>,
        Vec<(i32, Vec<String>)>,
    ) = message_body(items_properties_updated.wait(DEFAULT_TIMEOUT));
    assert!(removed_props.is_empty());
    assert!(updated_props
        .iter()
        .any(|(_, properties)| properties.get("label").is_some()));
    assert!(updated_props
        .iter()
        .any(|(_, properties)| properties.get("toggle-state").is_some()));

    let layout_updated = spawn_signal_waiter(&service_name, MENU_PATH, MENU_INTERFACE, "LayoutUpdated");
    handle
        .update(|tray| {
            tray.include_extra_item = true;
        })
        .expect("tray should still be alive for layout updates");
    let (revision, parent): (u32, i32) = message_body(layout_updated.wait(DEFAULT_TIMEOUT));
    assert_eq!(parent, 0);
    assert!(revision > 0);

    handle.shutdown().wait();
    watcher.close();
}

pub fn blocking_non_standard_compatibility() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<true>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let connection = session_connection();
    let proxy = sni_proxy(&connection, &service_name);

    let item_is_menu: bool = proxy.get_property("ItemIsMenu").unwrap();
    assert!(item_is_menu);

    // non-standard compatibility behavior: ksni reports Activate as UnknownMethod(ItemIsMenu)
    // when MENU_ON_ACTIVATE is enabled to match existing desktop-environment behavior.
    let activate_err = proxy
        .call::<_, _, ()>("Activate", &(1_i32, 2_i32))
        .expect_err("MENU_ON_ACTIVATE trays should reject Activate calls");
    let activate_err = format!("{activate_err:?}");
    assert!(activate_err.contains("UnknownMethod"));
    assert!(activate_err.contains("ItemIsMenu"));

    // non-standard compatibility behavior: ksni does not implement ContextMenu and reports
    // UnknownMethod instead of trying to render a menu itself.
    let context_err = proxy
        .call::<_, _, ()>("ContextMenu", &(0_i32, 0_i32))
        .expect_err("ContextMenu should not be implemented");
    let context_err = format!("{context_err:?}");
    assert!(context_err.contains("UnknownMethod"));

    handle.shutdown().wait();
    watcher.close();
}

pub fn blocking_dynamic_watcher_properties() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);

    // Mutate watcher state while the tray is running; changes must be
    // immediately visible via D-Bus since the handle shares the same
    // Arc<Mutex<WatcherState>> as the MockWatcher interface.
    watcher.set_protocol_version(42);
    watcher.set_host_registered(false);

    let connection = session_connection();
    let proxy = watcher_proxy(&connection);
    let version: i32 = proxy
        .get_property("ProtocolVersion")
        .expect("watcher should expose protocol version");
    let host: bool = proxy
        .get_property("IsStatusNotifierHostRegistered")
        .expect("watcher should expose host registration state");
    assert_eq!(version, 42);
    assert!(!host);

    handle.shutdown().wait();
    watcher.close();
}

macro_rules! blocking_protocol_tests {
    () => {
        #[test]
        fn protocol_registration_and_watchers() {
            crate::mock::blocking::blocking_registration_and_watchers();
        }

        #[test]
        fn protocol_watcher_lifecycle() {
            crate::mock::blocking::blocking_watcher_lifecycle();
        }

        #[test]
        fn protocol_status_notifier_item() {
            crate::mock::blocking::blocking_status_notifier_item_protocol();
        }

        #[test]
        fn protocol_dbusmenu() {
            crate::mock::blocking::blocking_dbusmenu_protocol();
        }

        #[test]
        fn protocol_non_standard_compatibility() {
            crate::mock::blocking::blocking_non_standard_compatibility();
        }

        #[test]
        fn protocol_dynamic_watcher_properties() {
            crate::mock::blocking::blocking_dynamic_watcher_properties();
        }
    };
}

pub(crate) use blocking_protocol_tests;
