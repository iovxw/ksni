use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::blocking::{connection, Connection};
use zbus::zvariant::OwnedValue;

mod common;

use common::{
    dbusmenu_assertions, decode_layout, find_layout_by_label, has_owner, menu_proxy, message_body,
    mutate_sni_properties, properties, property_i32, property_string,
    registration_and_watcher_assertions, session_connection, snapshot_events,
    sni_property_and_method_assertions, sni_proxy, spawn_filtered_signal_waiter,
    spawn_signal_waiter, wait_until, watcher_proxy, LayoutTuple, MockWatcher, RegisterItemError,
    TestTray, WatcherState, DEFAULT_TIMEOUT, MENU_INTERFACE, MENU_PATH, PROPERTIES_INTERFACE,
    SNI_INTERFACE, SNI_PATH, WATCHER_NAME, WATCHER_PATH,
};

struct WatcherHandle {
    connection: Connection,
    state: Arc<Mutex<WatcherState>>,
}

impl WatcherHandle {
    fn start(host_registered: bool) -> zbus::Result<Self> {
        Self::start_with_register_error(host_registered, None)
    }

    fn start_with_register_error(
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

    fn registered_items(&self) -> Vec<String> {
        self.state.lock().unwrap().registered_items.clone()
    }

    fn set_host_registered(&self, value: bool) {
        self.state.lock().unwrap().host_registered = value;
    }

    fn set_protocol_version(&self, value: i32) {
        self.state.lock().unwrap().protocol_version = value;
    }

    fn set_register_item_error(&self, value: Option<RegisterItemError>) {
        self.state.lock().unwrap().register_item_error = value;
    }

    fn wait_for_registration_count(&self, count: usize, timeout: Duration) -> Vec<String> {
        wait_until(
            timeout,
            || self.registered_items().len() >= count,
            "tray registrations",
        );
        self.registered_items()
    }

    fn wait_for_item_registration(&self, timeout: Duration) -> String {
        self.wait_for_registration_count(1, timeout)
            .into_iter()
            .next()
            .expect("at least one registration should exist")
    }

    fn close(self) {
        self.connection
            .close()
            .expect("watcher connection should close");
    }
}

#[test]
fn registration() {
    use ksni::blocking::TrayMethods as _;

    let connection = session_connection();
    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .spawn()
        .expect("tray should register with the mock watcher");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    assert!(service_name.starts_with("org.kde.StatusNotifierItem-"));
    registration_and_watcher_assertions(&connection, &service_name);
    handle.shutdown().wait();
    watcher.close();
}

#[test]
fn registration_with_unique_name() {
    use ksni::blocking::TrayMethods as _;

    let connection = session_connection();
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
}

#[test]
fn registration_fails_without_watcher() {
    use ksni::blocking::TrayMethods as _;

    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn() {
        Ok(_) => panic!("missing watchers must fail without assume_sni_available"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        ksni::Error::Watcher(zbus::fdo::Error::ServiceUnknown(_))
    ));
}

#[test]
fn registration_assume_sni_available() {
    use ksni::blocking::TrayMethods as _;

    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .expect("assume_sni_available should turn missing watchers into a soft offline state");
    wait_until(
        DEFAULT_TIMEOUT,
        || {
            snapshot_events(&events)
                .offline
                .iter()
                .any(|entry| entry.contains("ServiceUnknown"))
        },
        "ServiceUnknown watcher_offline callback",
    );
    handle.shutdown().wait();
}

#[test]
fn registration_wont_show() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(false).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn() {
        Ok(_) => panic!("watchers without hosts should report WontShow"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::WontShow));
    watcher.close();
}

#[test]
fn registration_fails_on_watcher_error() {
    use ksni::blocking::TrayMethods as _;

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
    assert!(matches!(
        err,
        ksni::Error::Watcher(zbus::fdo::Error::InvalidArgs(_))
    ));
    watcher.close();
}

#[test]
fn watcher_lifecycle() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let first_registration = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    watcher.close();

    wait_until(
        DEFAULT_TIMEOUT,
        || {
            snapshot_events(&events)
                .offline
                .iter()
                .any(|entry| entry.contains("No"))
        },
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

#[test]
fn status_notifier_item_protocol() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let connection = session_connection();

    sni_property_and_method_assertions(&connection, &service_name, &events);

    let new_title = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewTitle");
    let new_icon = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewIcon");
    let new_overlay = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewOverlayIcon");
    let new_attention =
        spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewAttentionIcon");
    let new_tool_tip = spawn_signal_waiter(&service_name, SNI_PATH, SNI_INTERFACE, "NewToolTip");
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

    let (sni_iface, sni_changed, sni_invalidated): (
        String,
        HashMap<String, OwnedValue>,
        Vec<String>,
    ) = message_body(sni_properties_changed.wait(DEFAULT_TIMEOUT));
    assert_eq!(sni_iface, SNI_INTERFACE);
    assert!(sni_invalidated.is_empty());
    assert_eq!(property_string(&sni_changed, "Category"), "Communications");
    assert_eq!(property_i32(&sni_changed, "WindowId"), 42);
    assert_eq!(
        property_string(&sni_changed, "IconThemePath"),
        "/tmp/mock-icons-updated"
    );

    let (menu_iface, menu_changed, menu_invalidated): (
        String,
        HashMap<String, OwnedValue>,
        Vec<String>,
    ) = message_body(menu_properties_changed.wait(DEFAULT_TIMEOUT));
    assert_eq!(menu_iface, MENU_INTERFACE);
    assert!(menu_invalidated.is_empty());
    assert_eq!(property_string(&menu_changed, "TextDirection"), "rtl");
    assert_eq!(property_string(&menu_changed, "Status"), "notice");
    let icon_paths: Vec<String> = menu_changed
        .get("IconThemePath")
        .expect("IconThemePath should be present")
        .clone()
        .try_into()
        .unwrap();
    assert_eq!(icon_paths, vec!["/tmp/mock-icons-updated".to_string()]);

    let proxy = sni_proxy(&connection, &service_name);
    assert_eq!(
        proxy.get_property::<String>("Title").unwrap(),
        "Updated Mock Tray"
    );
    assert_eq!(
        proxy.get_property::<String>("Status").unwrap(),
        "NeedsAttention"
    );

    handle.shutdown().wait();
    watcher.close();
}

#[test]
fn dbusmenu_protocol() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let connection = session_connection();

    dbusmenu_assertions(&connection, &service_name, &events);

    let items_properties_updated = spawn_signal_waiter(
        &service_name,
        MENU_PATH,
        MENU_INTERFACE,
        "ItemsPropertiesUpdated",
    );
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
    assert_eq!(
        updated_props,
        vec![
            (1, properties! { "label" => "Open updated" }),
            (2, properties! { "toggle-state" => 0_i32 }),
        ]
    );

    let layout_updated =
        spawn_signal_waiter(&service_name, MENU_PATH, MENU_INTERFACE, "LayoutUpdated");
    handle
        .update(|tray| {
            tray.include_extra_item = true;
        })
        .expect("tray should still be alive for layout updates");
    let (revision, parent): (u32, i32) = message_body(layout_updated.wait(DEFAULT_TIMEOUT));
    assert_eq!(parent, 0);
    assert!(revision > 0);

    let proxy = menu_proxy(&connection, &service_name);
    let (_, layout_all): (u32, LayoutTuple) = proxy
        .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
        .unwrap();
    let layout_all = decode_layout(layout_all);
    assert!(find_layout_by_label(&layout_all, "Extra").is_some());

    handle.shutdown().wait();
    watcher.close();
}

#[test]
fn non_standard_compatibility() {
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

#[test]
fn dynamic_watcher_properties() {
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

#[test]
fn watcher_offline_stops_tray() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (mut tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    tray.continue_on_offline = false;
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    watcher.close();

    wait_until(
        DEFAULT_TIMEOUT,
        || handle.is_closed(),
        "tray should stop after watcher_offline returns false",
    );
}

#[test]
fn update_after_shutdown_returns_none() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    handle.shutdown().wait();
    wait_until(
        DEFAULT_TIMEOUT,
        || handle.is_closed(),
        "handle should be closed after shutdown",
    );
    let result = handle.update(|_| ());
    assert!(result.is_none(), "update after shutdown should return None");
    watcher.close();
}

#[test]
fn handle_is_closed() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    assert!(
        !handle.is_closed(),
        "is_closed should be false while the tray is running"
    );
    handle.shutdown().wait();
    wait_until(
        DEFAULT_TIMEOUT,
        || handle.is_closed(),
        "is_closed should become true after shutdown",
    );
    watcher.close();
}

#[test]
fn handle_clone() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let handle2 = handle.clone();

    handle
        .update(|t| {
            t.title = "from-original".into();
        })
        .expect("update from original should work");
    handle2
        .update(|t| {
            t.title = "from-clone".into();
        })
        .expect("update from clone should work");
    assert!(
        !handle2.is_closed(),
        "cloned handle should not be closed while tray is running"
    );

    handle.shutdown().wait();
    wait_until(
        DEFAULT_TIMEOUT,
        || handle2.is_closed(),
        "cloned handle should also see shutdown",
    );
    assert!(
        handle.is_closed(),
        "original handle should also be closed after shutdown"
    );
    watcher.close();
}

#[test]
fn menu_item_optional_properties() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (mut tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    tray.standard_enabled = false;
    tray.standard_visible = false;
    tray.include_separator = true;
    let handle = tray.spawn().expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
    let connection = session_connection();
    let proxy = menu_proxy(&connection, &service_name);
    let (_, layout): (u32, LayoutTuple) = proxy
        .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
        .unwrap();
    let layout = decode_layout(layout);

    let standard = find_layout_by_label(&layout, "Open").expect("standard item should exist");
    assert_eq!(
        standard.properties,
        properties! {
            "label" => "Open",
            "enabled" => false,
            "visible" => false,
            "icon-name" => "open-icon",
            "icon-data" => vec![1_u8, 2, 3, 4],
            "shortcut" => vec![vec!["Control".to_string(), "O".to_string()]],
            "disposition" => "informative",
        }
    );

    let separator = layout
        .children
        .iter()
        .find(|node| {
            node.properties
                .get("type")
                .and_then(|v| v.clone().try_into().ok())
                == Some("separator".to_string())
        })
        .expect("separator item should exist in the menu");
    assert_eq!(separator.properties, properties! { "type" => "separator" });

    handle.shutdown().wait();
    watcher.close();
}

#[test]
fn watcher_multiple_offline_online_cycles() {
    use ksni::blocking::TrayMethods as _;

    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let mut watcher = WatcherHandle::start(true).unwrap();
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);

    for cycle in 1..=3_usize {
        watcher.close();
        wait_until(
            DEFAULT_TIMEOUT,
            || snapshot_events(&events).offline.len() >= cycle,
            "watcher offline callback",
        );

        watcher = WatcherHandle::start(true).unwrap();
        watcher.wait_for_item_registration(DEFAULT_TIMEOUT);
        wait_until(
            DEFAULT_TIMEOUT,
            || snapshot_events(&events).online_count >= cycle,
            "watcher online callback",
        );
    }

    let snapshot = snapshot_events(&events);
    assert_eq!(snapshot.offline.len(), 3);
    assert_eq!(snapshot.online_count, 3);

    handle.shutdown().wait();
    watcher.close();
}

#[test]
fn watcher_reregistration_failure() {
    use ksni::blocking::TrayMethods as _;

    let watcher = WatcherHandle::start(true).unwrap();
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT);

    watcher.close();
    wait_until(
        DEFAULT_TIMEOUT,
        || {
            snapshot_events(&events)
                .offline
                .iter()
                .any(|e| e.contains("No"))
        },
        "watcher offline after close",
    );

    let watcher = WatcherHandle::start_with_register_error(
        true,
        Some(RegisterItemError::Failed("registration rejected".into())),
    )
    .unwrap();
    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).online_count >= 1,
        "watcher_online should be called when new watcher appears",
    );
    wait_until(
        DEFAULT_TIMEOUT,
        || {
            let snap = snapshot_events(&events);
            snap.offline.len() >= 2 && snap.offline[1].contains("Error")
        },
        "watcher_offline with Error reason should be called when re-registration fails",
    );

    handle.shutdown().wait();
    watcher.close();
}
