use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zbus::connection;
use zbus::zvariant::OwnedValue;

use crate::common::{
    dbusmenu_assertions, decode_layout, find_layout_by_label, has_owner, menu_proxy, message_body,
    mutate_sni_properties, properties, registration_and_watcher_assertions, session_connection,
    snapshot_events, sni_property_and_method_assertions, sni_proxy, spawn_filtered_signal_waiter,
    spawn_signal_waiter, watcher_proxy, LayoutTuple, MockWatcher, RegisterItemError, TestTray,
    WatcherState, DEFAULT_TIMEOUT, MENU_INTERFACE, MENU_PATH, PROPERTIES_INTERFACE, SNI_INTERFACE,
    SNI_PATH, WATCHER_NAME, WATCHER_PATH,
};

#[cfg(feature = "tokio")]
async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(all(feature = "async-io", not(feature = "tokio")))]
async fn sleep(duration: Duration) {
    smol::Timer::after(duration).await;
}

#[cfg(feature = "tokio")]
async fn with_blocking<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("blocking helper should complete")
}

#[cfg(all(feature = "async-io", not(feature = "tokio")))]
async fn with_blocking<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    smol::unblock(f).await
}

async fn wait_until(timeout: Duration, condition: impl Fn() -> bool, description: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }

    assert!(condition(), "timed out waiting for {description}");
}

struct WatcherHandle {
    connection: zbus::Connection,
    state: Arc<Mutex<WatcherState>>,
}

impl WatcherHandle {
    async fn start_with_register_error(
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
            .build()
            .await?;
        connection.request_name(WATCHER_NAME).await?;
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

    async fn wait_for_registration_count_async(
        &self,
        count: usize,
        timeout: Duration,
    ) -> Vec<String> {
        wait_until(
            timeout,
            || self.registered_items().len() >= count,
            "tray registrations",
        )
        .await;
        self.registered_items()
    }

    async fn wait_for_item_registration(&self, timeout: Duration) -> String {
        self.wait_for_registration_count_async(1, timeout)
            .await
            .into_iter()
            .next()
            .expect("at least one registration should exist")
    }

    async fn close(self) {
        self.connection
            .close()
            .await
            .expect("watcher connection should close");
    }
}

async fn start_watcher(
    host_registered: bool,
    register_item_error: Option<RegisterItemError>,
) -> WatcherHandle {
    WatcherHandle::start_with_register_error(host_registered, register_item_error)
        .await
        .expect("mock watcher should start")
}

async fn close_watcher(watcher: WatcherHandle) {
    watcher.close().await;
}

pub async fn registration() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .spawn()
        .await
        .expect("tray should register with the mock watcher");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    assert!(service_name.starts_with("org.kde.StatusNotifierItem-"));
    with_blocking(move || {
        let connection = session_connection();
        registration_and_watcher_assertions(&connection, &service_name);
    })
    .await;
    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn registration_with_unique_name() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .disable_dbus_name(true)
        .spawn()
        .await
        .expect("tray should register with its unique name when dbus names are disabled");
    let unique_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    assert!(unique_name.starts_with(':'));
    assert!(
        with_blocking(move || {
            let connection = session_connection();
            has_owner(&connection, &unique_name)
        })
        .await
    );
    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn registration_fails_without_watcher() {
    use ksni::TrayMethods as _;

    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn().await {
        Ok(_) => panic!("missing watchers must fail without assume_sni_available"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        ksni::Error::Watcher(zbus::fdo::Error::ServiceUnknown(_))
    ));
}

pub async fn registration_assume_sni_available() {
    use ksni::TrayMethods as _;

    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .await
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
    )
    .await;
    handle.shutdown().await;
}

pub async fn registration_wont_show() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(false, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn().await {
        Ok(_) => panic!("watchers without hosts should report WontShow"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::WontShow));
    close_watcher(watcher).await;
}

pub async fn registration_fails_on_watcher_error() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(
        true,
        Some(RegisterItemError::InvalidArgs("mock rejection".into())),
    )
    .await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn().await {
        Ok(_) => panic!("watcher registration failures should surface as watcher errors"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        ksni::Error::Watcher(zbus::fdo::Error::InvalidArgs(_))
    ));
    close_watcher(watcher).await;
}

pub async fn watcher_lifecycle() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let first_registration = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    close_watcher(watcher).await;

    wait_until(
        DEFAULT_TIMEOUT,
        || {
            snapshot_events(&events)
                .offline
                .iter()
                .any(|entry| entry.contains("No"))
        },
        "watcher offline callback",
    )
    .await;

    let watcher = start_watcher(true, None).await;
    let second_registration = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).online_count == 1,
        "watcher online callback",
    )
    .await;
    assert_eq!(first_registration, second_registration);

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn status_notifier_item_protocol() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    let service_name_for_assertions = service_name.clone();
    let events_for_assertions = events.clone();
    with_blocking(move || {
        let connection = session_connection();
        sni_property_and_method_assertions(
            &connection,
            &service_name_for_assertions,
            &events_for_assertions,
        );
    })
    .await;

    let waiters_service_name = service_name.clone();
    let (
        new_title,
        new_icon,
        new_overlay,
        new_attention,
        new_tool_tip,
        new_status,
        sni_properties_changed,
        menu_properties_changed,
    ) = with_blocking(move || {
        (
            spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewTitle"),
            spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewIcon"),
            spawn_signal_waiter(
                &waiters_service_name,
                SNI_PATH,
                SNI_INTERFACE,
                "NewOverlayIcon",
            ),
            spawn_signal_waiter(
                &waiters_service_name,
                SNI_PATH,
                SNI_INTERFACE,
                "NewAttentionIcon",
            ),
            spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewToolTip"),
            spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewStatus"),
            spawn_filtered_signal_waiter(
                &waiters_service_name,
                SNI_PATH,
                PROPERTIES_INTERFACE,
                "PropertiesChanged",
                vec![(0, SNI_INTERFACE.to_string())],
            ),
            spawn_filtered_signal_waiter(
                &waiters_service_name,
                MENU_PATH,
                PROPERTIES_INTERFACE,
                "PropertiesChanged",
                vec![(0, MENU_INTERFACE.to_string())],
            ),
        )
    })
    .await;

    handle
        .update(|tray| mutate_sni_properties(tray))
        .await
        .expect("tray should still be alive for updates");

    let service_name_for_checks = service_name.clone();
    with_blocking(move || {
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
        assert_eq!(
            sni_changed,
            properties! {
                "Category" => "Communications",
                "WindowId" => 42_i32,
                "IconThemePath" => "/tmp/mock-icons-updated",
            }
        );

        let (menu_iface, menu_changed, menu_invalidated): (
            String,
            HashMap<String, OwnedValue>,
            Vec<String>,
        ) = message_body(menu_properties_changed.wait(DEFAULT_TIMEOUT));
        assert_eq!(menu_iface, MENU_INTERFACE);
        assert!(menu_invalidated.is_empty());
        assert_eq!(
            menu_changed,
            properties! {
                "TextDirection" => "rtl",
                "Status" => "notice",
                "IconThemePath" => vec!["/tmp/mock-icons-updated".to_string()],
            }
        );

        let connection = session_connection();
        let proxy = sni_proxy(&connection, &service_name_for_checks);
        assert_eq!(
            proxy.get_property::<String>("Title").unwrap(),
            "Updated Mock Tray"
        );
        assert_eq!(
            proxy.get_property::<String>("Status").unwrap(),
            "NeedsAttention"
        );
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn dbusmenu_protocol() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    let service_name_for_assertions = service_name.clone();
    let events_for_assertions = events.clone();
    with_blocking(move || {
        let connection = session_connection();
        dbusmenu_assertions(
            &connection,
            &service_name_for_assertions,
            &events_for_assertions,
        );
    })
    .await;

    let signal_service_name = service_name.clone();
    let items_properties_updated = with_blocking(move || {
        spawn_signal_waiter(
            &signal_service_name,
            MENU_PATH,
            MENU_INTERFACE,
            "ItemsPropertiesUpdated",
        )
    })
    .await;
    handle
        .update(|tray| {
            tray.standard_label = "Open updated".into();
            tray.checkmark_checked = false;
        })
        .await
        .expect("tray should still be alive for updates");
    with_blocking(move || {
        let (updated_props, removed_props): (
            Vec<(i32, HashMap<String, OwnedValue>)>,
            Vec<(i32, Vec<String>)>,
        ) = message_body(items_properties_updated.wait(DEFAULT_TIMEOUT));
        assert!(removed_props.is_empty());
        let updated_props: Vec<_> = updated_props.into_iter().collect();
        assert_eq!(
            updated_props,
            vec![
                (1, properties! { "label" => "Open updated" }),
                (2, properties! { "toggle-state" => 0_i32 }),
            ]
        );
    })
    .await;

    let layout_service_name = service_name.clone();
    let layout_updated = with_blocking(move || {
        spawn_signal_waiter(
            &layout_service_name,
            MENU_PATH,
            MENU_INTERFACE,
            "LayoutUpdated",
        )
    })
    .await;
    handle
        .update(|tray| {
            tray.include_extra_item = true;
        })
        .await
        .expect("tray should still be alive for layout updates");
    let layout_check_service_name = service_name.clone();
    with_blocking(move || {
        let (revision, parent): (u32, i32) = message_body(layout_updated.wait(DEFAULT_TIMEOUT));
        assert_eq!(parent, 0);
        assert!(revision > 0);

        let connection = session_connection();
        let proxy = menu_proxy(&connection, &layout_check_service_name);
        let (_, layout_all): (u32, LayoutTuple) = proxy
            .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
            .unwrap();
        let layout_all = decode_layout(layout_all);
        assert!(find_layout_by_label(&layout_all, "Extra").is_some());
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

/// Async variant of [`crate::blocking::menu_about_to_show_dynamic`].
///
/// When `Tray::menu_about_to_show` modifies the menu, `AboutToShow(0)` must
/// return `true` and the change must be reflected in `GetLayout`.
///
/// `AboutToShowGroup` must report the root ID in `updatesNeeded`.
pub async fn menu_about_to_show_dynamic() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (mut tray, _events) = TestTray::<false>::new("runtime-protocol-tray");
    tray.menu_about_to_show_extra = true;
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;

    let sn_for_blocking = service_name.clone();
    with_blocking(move || {
        let connection = session_connection();
        let proxy = menu_proxy(&connection, &sn_for_blocking);

        // First call: menu changes (include_extra_item toggles from false to true)
        let needs_update: bool = proxy.call("AboutToShow", &(0_i32,)).unwrap();
        assert!(needs_update, "AboutToShow(0) should return true when menu is modified");

        // Verify Extra item is now in the layout
        let (_, layout): (u32, LayoutTuple) = proxy
            .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
            .unwrap();
        let layout = decode_layout(layout);
        assert!(find_layout_by_label(&layout, "Extra").is_some(), "Extra item should appear after AboutToShow");

        // Second call: menu changes back (include_extra_item toggles from true to false)
        let needs_update: bool = proxy.call("AboutToShow", &(0_i32,)).unwrap();
        assert!(needs_update, "AboutToShow(0) should return true again when menu is modified back");

        // AboutToShowGroup with root - should detect change
        // Reset state first by calling once more so include_extra_item goes false→true
        let _: bool = proxy.call("AboutToShow", &(0_i32,)).unwrap();
        // Now call AboutToShowGroup
        let (updates_needed, id_errors): (Vec<i32>, Vec<i32>) = proxy
            .call("AboutToShowGroup", &(vec![0_i32],))
            .unwrap();
        assert_eq!(updates_needed, vec![0], "root should be in updatesNeeded when menu changes");
        assert!(id_errors.is_empty());
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

/// Async variant of [`crate::blocking::about_to_show_signal_silence`].
///
/// `AboutToShow(0) -> true` must not emit a `LayoutUpdated` D-Bus signal
pub async fn about_to_show_signal_silence() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (mut tray, _events) = TestTray::<false>::new("runtime-protocol-tray");
    tray.menu_about_to_show_extra = true;
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;

    let sn_for_blocking = service_name.clone();
    with_blocking(move || {
        let connection = session_connection();
        let proxy = menu_proxy(&connection, &sn_for_blocking);

        // Subscribe to LayoutUpdated before calling AboutToShow
        let waiter = spawn_signal_waiter(
            &sn_for_blocking,
            MENU_PATH,
            MENU_INTERFACE,
            "LayoutUpdated",
        );

        // Call AboutToShow on root menu (id=0) — should return true but NOT emit LayoutUpdated
        let needs_update: bool = proxy.call("AboutToShow", &(0_i32,)).unwrap();
        assert!(needs_update, "AboutToShow(0) should return true when menu is modified");

        // Verify no LayoutUpdated signal arrives within 500ms
        waiter.expect_no_signal(Duration::from_millis(500));
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn non_standard_compatibility() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<true>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    let compat_service_name = service_name.clone();
    with_blocking(move || {
        let connection = session_connection();
        let proxy = sni_proxy(&connection, &compat_service_name);

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
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn dynamic_watcher_properties() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;

    // Mutate watcher state while the tray is running; changes must be
    // immediately visible via D-Bus since the handle shares the same
    // Arc<Mutex<WatcherState>> as the MockWatcher interface.
    watcher.set_protocol_version(42);
    watcher.set_host_registered(false);

    let (protocol_version, host_registered) = with_blocking(|| {
        let connection = session_connection();
        let proxy = watcher_proxy(&connection);
        let version: i32 = proxy
            .get_property("ProtocolVersion")
            .expect("watcher should expose protocol version");
        let host: bool = proxy
            .get_property("IsStatusNotifierHostRegistered")
            .expect("watcher should expose host registration state");
        (version, host)
    })
    .await;
    assert_eq!(protocol_version, 42);
    assert!(!host_registered);

    handle.shutdown().await;
    watcher.close().await;
}

pub async fn watcher_offline_stops_tray() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (mut tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    tray.continue_on_offline = false;
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    close_watcher(watcher).await;

    wait_until(
        DEFAULT_TIMEOUT,
        || handle.is_closed(),
        "tray should stop after watcher_offline returns false",
    )
    .await;
}

pub async fn handle_is_closed() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    assert!(
        !handle.is_closed(),
        "is_closed should be false while the tray is running"
    );
    handle.shutdown().await;
    wait_until(
        DEFAULT_TIMEOUT,
        || handle.is_closed(),
        "is_closed should become true after shutdown",
    )
    .await;
    close_watcher(watcher).await;
}

pub async fn handle_clone() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    let handle2 = handle.clone();

    handle
        .update(|t| {
            t.title = "from-original".into();
        })
        .await
        .expect("update from original should work");
    handle2
        .update(|t| {
            t.title = "from-clone".into();
        })
        .await
        .expect("update from clone should work");
    assert!(
        !handle2.is_closed(),
        "cloned handle should not be closed while tray is running"
    );

    handle.shutdown().await;
    wait_until(
        DEFAULT_TIMEOUT,
        || handle2.is_closed(),
        "cloned handle should also see shutdown",
    )
    .await;
    assert!(
        handle.is_closed(),
        "original handle should also be closed after shutdown"
    );
    close_watcher(watcher).await;
}

pub async fn menu_item_optional_properties() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (mut tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    tray.standard_enabled = false;
    tray.standard_visible = false;
    tray.include_separator = true;
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    let service_name_clone = service_name.clone();
    with_blocking(move || {
        let connection = session_connection();
        let proxy = menu_proxy(&connection, &service_name_clone);
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
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn watcher_multiple_offline_online_cycles() {
    use ksni::TrayMethods as _;

    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let mut watcher = start_watcher(true, None).await;
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;

    for cycle in 1..=3_usize {
        close_watcher(watcher).await;
        wait_until(
            DEFAULT_TIMEOUT,
            || snapshot_events(&events).offline.len() >= cycle,
            "watcher offline callback",
        )
        .await;

        watcher = start_watcher(true, None).await;
        watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
        wait_until(
            DEFAULT_TIMEOUT,
            || snapshot_events(&events).online_count >= cycle,
            "watcher online callback",
        )
        .await;
    }

    let snapshot = snapshot_events(&events);
    assert_eq!(snapshot.offline.len(), 3);
    assert_eq!(snapshot.online_count, 3);

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn watcher_reregistration_failure() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;

    close_watcher(watcher).await;
    wait_until(
        DEFAULT_TIMEOUT,
        || {
            snapshot_events(&events)
                .offline
                .iter()
                .any(|e| e.contains("No"))
        },
        "watcher offline after close",
    )
    .await;

    let watcher = start_watcher(
        true,
        Some(RegisterItemError::Failed("registration rejected".into())),
    )
    .await;
    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).online_count >= 1,
        "watcher_online should be called when new watcher appears",
    )
    .await;
    wait_until(
        DEFAULT_TIMEOUT,
        || {
            let snap = snapshot_events(&events);
            snap.offline.len() >= 2 && snap.offline[1].contains("Error")
        },
        "watcher_offline with Error reason should be called when re-registration fails",
    )
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn update_after_shutdown_returns_none() {
    use ksni::TrayMethods as _;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    watcher.wait_for_item_registration(DEFAULT_TIMEOUT).await;
    handle.shutdown().await;
    wait_until(
        DEFAULT_TIMEOUT,
        || handle.is_closed(),
        "handle should be closed after shutdown",
    )
    .await;
    let result = handle.update(|_| ()).await;
    assert!(result.is_none(), "update after shutdown should return None");
    close_watcher(watcher).await;
}

macro_rules! async_protocol_tests {
    ($test_attr:meta) => {
        #[$test_attr]
        async fn registration() {
            async_tests::registration().await;
        }

        #[$test_attr]
        async fn registration_with_unique_name() {
            async_tests::registration_with_unique_name().await;
        }

        #[$test_attr]
        async fn registration_fails_without_watcher() {
            async_tests::registration_fails_without_watcher().await;
        }

        #[$test_attr]
        async fn registration_assume_sni_available() {
            async_tests::registration_assume_sni_available().await;
        }

        #[$test_attr]
        async fn registration_wont_show() {
            async_tests::registration_wont_show().await;
        }

        #[$test_attr]
        async fn registration_fails_on_watcher_error() {
            async_tests::registration_fails_on_watcher_error().await;
        }

        #[$test_attr]
        async fn watcher_lifecycle() {
            async_tests::watcher_lifecycle().await;
        }

        #[$test_attr]
        async fn status_notifier_item() {
            async_tests::status_notifier_item_protocol().await;
        }

        #[$test_attr]
        async fn dbusmenu() {
            async_tests::dbusmenu_protocol().await;
        }

        #[$test_attr]
        async fn non_standard_compatibility() {
            async_tests::non_standard_compatibility().await;
        }

        #[$test_attr]
        async fn dynamic_watcher_properties() {
            async_tests::dynamic_watcher_properties().await;
        }

        #[$test_attr]
        async fn watcher_offline_stops_tray() {
            async_tests::watcher_offline_stops_tray().await;
        }

        #[$test_attr]
        async fn update_after_shutdown_returns_none() {
            async_tests::update_after_shutdown_returns_none().await;
        }

        #[$test_attr]
        async fn handle_is_closed() {
            async_tests::handle_is_closed().await;
        }

        #[$test_attr]
        async fn handle_clone() {
            async_tests::handle_clone().await;
        }

        #[$test_attr]
        async fn menu_item_optional_properties() {
            async_tests::menu_item_optional_properties().await;
        }

        #[$test_attr]
        async fn watcher_multiple_offline_online_cycles() {
            async_tests::watcher_multiple_offline_online_cycles().await;
        }

        #[$test_attr]
        async fn watcher_reregistration_failure() {
            async_tests::watcher_reregistration_failure().await;
        }

        #[$test_attr]
        async fn menu_about_to_show_dynamic() {
            async_tests::menu_about_to_show_dynamic().await;
        }

        #[$test_attr]
        async fn about_to_show_signal_silence() {
            async_tests::about_to_show_signal_silence().await;
        }
    };
}

pub(crate) use async_protocol_tests;
