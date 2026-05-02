use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use ksni::menu::{CheckmarkItem, Disposition, RadioGroup, RadioItem, StandardItem, SubMenu, TextDirection};
use ksni::{Category, Icon, MenuItem, Status, ToolTip};
use zbus::Message;
use zbus::connection as async_connection;
use zbus::blocking::{Connection, Proxy, connection};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
pub const SNI_PATH: &str = "/StatusNotifierItem";
pub const SNI_INTERFACE: &str = "org.kde.StatusNotifierItem";
pub const MENU_PATH: &str = "/MenuBar";
pub const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub enum RegisterItemError {
    InvalidArgs(String),
    Failed(String),
}

impl RegisterItemError {
    fn into_fdo_error(self) -> zbus::fdo::Error {
        match self {
            Self::InvalidArgs(message) => zbus::fdo::Error::InvalidArgs(message),
            Self::Failed(message) => zbus::fdo::Error::Failed(message),
        }
    }
}

#[derive(Clone)]
struct WatcherState {
    registered_items: Arc<Mutex<Vec<String>>>,
    host_registered: bool,
    protocol_version: i32,
    register_item_error: Arc<Mutex<Option<RegisterItemError>>>,
}

struct MockWatcher {
    state: WatcherState,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl MockWatcher {
    async fn register_status_notifier_item(&self, service: &str) -> zbus::fdo::Result<()> {
        if let Some(error) = self.state.register_item_error.lock().unwrap().clone() {
            return Err(error.into_fdo_error());
        }

        self.state
            .registered_items
            .lock()
            .unwrap()
            .push(service.to_string());
        Ok(())
    }

    async fn register_status_notifier_host(&self, _service: &str) -> zbus::fdo::Result<()> {
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(self.state.registered_items.lock().unwrap().clone())
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::fdo::Result<bool> {
        Ok(self.state.host_registered)
    }

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::fdo::Result<i32> {
        Ok(self.state.protocol_version)
    }
}

pub struct WatcherHandle {
    connection: Connection,
    state: WatcherState,
}

pub struct AsyncWatcherHandle {
    connection: zbus::Connection,
    state: WatcherState,
}

impl WatcherHandle {
    pub fn start(host_registered: bool) -> zbus::Result<Self> {
        Self::start_with_register_error(host_registered, None)
    }

    pub fn start_with_register_error(
        host_registered: bool,
        register_item_error: Option<RegisterItemError>,
    ) -> zbus::Result<Self> {
        let state = WatcherState {
            registered_items: Arc::new(Mutex::new(Vec::new())),
            host_registered,
            protocol_version: 0,
            register_item_error: Arc::new(Mutex::new(register_item_error)),
        };
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
        self.state.registered_items.lock().unwrap().clone()
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

impl AsyncWatcherHandle {
    pub async fn start_with_register_error(
        host_registered: bool,
        register_item_error: Option<RegisterItemError>,
    ) -> zbus::Result<Self> {
        let state = WatcherState {
            registered_items: Arc::new(Mutex::new(Vec::new())),
            host_registered,
            protocol_version: 0,
            register_item_error: Arc::new(Mutex::new(register_item_error)),
        };
        let connection = async_connection::Builder::session()?
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

    pub fn registered_items(&self) -> Vec<String> {
        self.state.registered_items.lock().unwrap().clone()
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

    pub async fn wait_for_registration_count_async(
        &self,
        count: usize,
        timeout: Duration,
    ) -> Vec<String> {
        wait_until_async(timeout, || self.registered_items().len() >= count, "tray registrations").await;
        self.registered_items()
    }

    pub async fn wait_for_item_registration_async(&self, timeout: Duration) -> String {
        self.wait_for_registration_count_async(1, timeout)
            .await
            .into_iter()
            .next()
            .expect("at least one registration should exist")
    }

    pub async fn close(self) {
        self.connection
            .close()
            .await
            .expect("watcher connection should close");
    }
}

#[derive(Clone, Debug, Default)]
pub struct CallbackLog {
    pub activations: Vec<(i32, i32)>,
    pub secondary_activations: Vec<(i32, i32)>,
    pub scrolls: Vec<(i32, String)>,
    pub menu_clicks: Vec<String>,
    pub offline: Vec<String>,
    pub online_count: usize,
}

pub type CallbackProbe = Arc<Mutex<CallbackLog>>;

pub fn snapshot_events(probe: &CallbackProbe) -> CallbackLog {
    probe.lock().unwrap().clone()
}

pub struct TestTray<const MENU_ON_ACTIVATE: bool> {
    pub id: String,
    pub category: Category,
    pub title: String,
    pub status: Status,
    pub window_id: i32,
    pub icon_theme_path: String,
    pub icon_name: String,
    pub icon_pixmap: Vec<Icon>,
    pub overlay_icon_name: String,
    pub overlay_icon_pixmap: Vec<Icon>,
    pub attention_icon_name: String,
    pub attention_icon_pixmap: Vec<Icon>,
    pub attention_movie_name: String,
    pub tool_tip: ToolTip,
    pub text_direction: TextDirection,
    pub standard_label: String,
    pub standard_enabled: bool,
    pub standard_visible: bool,
    pub standard_icon_name: String,
    pub standard_icon_data: Vec<u8>,
    pub standard_shortcut: Vec<Vec<String>>,
    pub standard_disposition: Disposition,
    pub checkmark_label: String,
    pub checkmark_checked: bool,
    pub submenu_label: String,
    pub submenu_child_label: String,
    pub radio_selected: usize,
    pub include_extra_item: bool,
    pub continue_on_offline: bool,
    pub events: CallbackProbe,
}

impl<const MENU_ON_ACTIVATE: bool> TestTray<MENU_ON_ACTIVATE> {
    pub fn new(id: &str) -> (Self, CallbackProbe) {
        let events = Arc::new(Mutex::new(CallbackLog::default()));
        (
            Self {
                id: id.to_string(),
                category: Category::Hardware,
                title: "Mock Tray".into(),
                status: Status::Active,
                window_id: 7,
                icon_theme_path: "/tmp/mock-icons".into(),
                icon_name: "main-icon".into(),
                icon_pixmap: vec![icon(0x11)],
                overlay_icon_name: "overlay-icon".into(),
                overlay_icon_pixmap: vec![icon(0x22)],
                attention_icon_name: "attention-icon".into(),
                attention_icon_pixmap: vec![icon(0x33)],
                attention_movie_name: "attention.gif".into(),
                tool_tip: ToolTip {
                    icon_name: "tooltip-icon".into(),
                    icon_pixmap: vec![icon(0x44)],
                    title: "Mock tooltip".into(),
                    description: "Tooltip description".into(),
                },
                text_direction: TextDirection::LeftToRight,
                standard_label: "Open".into(),
                standard_enabled: true,
                standard_visible: true,
                standard_icon_name: "open-icon".into(),
                standard_icon_data: vec![1, 2, 3, 4],
                standard_shortcut: vec![vec!["Control".into(), "O".into()]],
                standard_disposition: Disposition::Informative,
                checkmark_label: "Pinned".into(),
                checkmark_checked: true,
                submenu_label: "More".into(),
                submenu_child_label: "Nested".into(),
                radio_selected: 1,
                include_extra_item: false,
                continue_on_offline: true,
                events: events.clone(),
            },
            events,
        )
    }
}

impl<const MENU_ON_ACTIVATE: bool> ksni::Tray for TestTray<MENU_ON_ACTIVATE> {
    const MENU_ON_ACTIVATE: bool = MENU_ON_ACTIVATE;

    fn id(&self) -> String {
        self.id.clone()
    }

    fn activate(&mut self, x: i32, y: i32) {
        self.events.lock().unwrap().activations.push((x, y));
    }

    fn secondary_activate(&mut self, x: i32, y: i32) {
        self.events.lock().unwrap().secondary_activations.push((x, y));
    }

    fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
        self.events
            .lock()
            .unwrap()
            .scrolls
            .push((delta, format!("{orientation:?}")));
    }

    fn category(&self) -> Category {
        self.category
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn status(&self) -> Status {
        self.status
    }

    fn window_id(&self) -> i32 {
        self.window_id
    }

    fn icon_theme_path(&self) -> String {
        self.icon_theme_path.clone()
    }

    fn icon_name(&self) -> String {
        self.icon_name.clone()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon_pixmap.clone()
    }

    fn overlay_icon_name(&self) -> String {
        self.overlay_icon_name.clone()
    }

    fn overlay_icon_pixmap(&self) -> Vec<Icon> {
        self.overlay_icon_pixmap.clone()
    }

    fn attention_icon_name(&self) -> String {
        self.attention_icon_name.clone()
    }

    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        self.attention_icon_pixmap.clone()
    }

    fn attention_movie_name(&self) -> String {
        self.attention_movie_name.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        self.tool_tip.clone()
    }

    fn text_direction(&self) -> TextDirection {
        self.text_direction
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = vec![
            StandardItem {
                label: self.standard_label.clone(),
                enabled: self.standard_enabled,
                visible: self.standard_visible,
                icon_name: self.standard_icon_name.clone(),
                icon_data: self.standard_icon_data.clone(),
                shortcut: self.standard_shortcut.clone(),
                disposition: self.standard_disposition,
                activate: Box::new(|tray: &mut Self| {
                    tray.events.lock().unwrap().menu_clicks.push("standard".into());
                }),
            }
            .into(),
            CheckmarkItem {
                label: self.checkmark_label.clone(),
                checked: self.checkmark_checked,
                activate: Box::new(|tray: &mut Self| {
                    tray.checkmark_checked = !tray.checkmark_checked;
                    tray.events.lock().unwrap().menu_clicks.push("checkmark".into());
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: self.submenu_label.clone(),
                submenu: vec![
                    StandardItem {
                        label: self.submenu_child_label.clone(),
                        activate: Box::new(|tray: &mut Self| {
                            tray.events
                                .lock()
                                .unwrap()
                                .menu_clicks
                                .push("submenu-child".into());
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            RadioGroup {
                selected: self.radio_selected,
                select: Box::new(|tray: &mut Self, index| {
                    tray.radio_selected = index;
                    tray.events
                        .lock()
                        .unwrap()
                        .menu_clicks
                        .push(format!("radio:{index}"));
                }),
                options: vec![
                    RadioItem {
                        label: "Mode A".into(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "Mode B".into(),
                        disposition: Disposition::Warning,
                        ..Default::default()
                    },
                ],
            }
            .into(),
        ];

        if self.include_extra_item {
            items.push(
                StandardItem {
                    label: "Extra".into(),
                    activate: Box::new(|tray: &mut Self| {
                        tray.events.lock().unwrap().menu_clicks.push("extra".into());
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        items
    }

    fn watcher_online(&self) {
        self.events.lock().unwrap().online_count += 1;
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        self.events
            .lock()
            .unwrap()
            .offline
            .push(format!("{reason:?}"));
        self.continue_on_offline
    }
}

fn icon(seed: u8) -> Icon {
    Icon {
        width: 1,
        height: 1,
        data: vec![seed, 0, 0, 0xff],
    }
}

type LayoutTuple = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[derive(Clone, Debug)]
struct LayoutNode {
    id: i32,
    properties: HashMap<String, OwnedValue>,
    children: Vec<LayoutNode>,
}

fn decode_layout(layout: LayoutTuple) -> LayoutNode {
    LayoutNode {
        id: layout.0,
        properties: layout.1,
        children: layout.2.into_iter().map(layout_from_value).collect(),
    }
}

fn layout_from_value(value: OwnedValue) -> LayoutNode {
    let layout: LayoutTuple = value.try_into().expect("value should decode into a layout tuple");
    decode_layout(layout)
}

fn find_layout_by_label<'a>(layout: &'a LayoutNode, label: &str) -> Option<&'a LayoutNode> {
    if layout
        .properties
        .get("label")
        .and_then(|value| value.clone().try_into().ok())
        == Some(label.to_string())
    {
        return Some(layout);
    }

    layout
        .children
        .iter()
        .find_map(|child| find_layout_by_label(child, label))
}

fn property_string(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    properties
        .get(key)
        .unwrap_or_else(|| panic!("missing property: {key}"))
        .clone()
        .try_into()
        .expect("property should decode into a string")
}

fn property_bytes(properties: &HashMap<String, OwnedValue>, key: &str) -> Vec<u8> {
    properties
        .get(key)
        .unwrap_or_else(|| panic!("missing property: {key}"))
        .clone()
        .try_into()
        .expect("property should decode into bytes")
}

fn property_shortcut(properties: &HashMap<String, OwnedValue>, key: &str) -> Vec<Vec<String>> {
    properties
        .get(key)
        .unwrap_or_else(|| panic!("missing property: {key}"))
        .clone()
        .try_into()
        .expect("property should decode into a shortcut list")
}

fn property_i32(properties: &HashMap<String, OwnedValue>, key: &str) -> i32 {
    properties
        .get(key)
        .unwrap_or_else(|| panic!("missing property: {key}"))
        .clone()
        .try_into()
        .expect("property should decode into an i32")
}

fn session_connection() -> Connection {
    connection::Builder::session()
        .expect("session bus builder should be available")
        .method_timeout(DEFAULT_TIMEOUT)
        .build()
        .expect("session bus connection should be available")
}

fn watcher_proxy<'a>(connection: &'a Connection) -> Proxy<'a> {
    Proxy::new(connection, WATCHER_NAME, WATCHER_PATH, WATCHER_INTERFACE)
        .expect("watcher proxy should be valid")
}

fn sni_proxy<'a>(connection: &'a Connection, destination: &'a str) -> Proxy<'a> {
    Proxy::new(connection, destination, SNI_PATH, SNI_INTERFACE).expect("SNI proxy should be valid")
}

fn menu_proxy<'a>(connection: &'a Connection, destination: &'a str) -> Proxy<'a> {
    Proxy::new(connection, destination, MENU_PATH, MENU_INTERFACE)
        .expect("dbusmenu proxy should be valid")
}

fn has_owner(connection: &Connection, name: &str) -> bool {
    Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .expect("DBus proxy should be valid")
    .call("NameHasOwner", &(name.to_string(),))
    .expect("NameHasOwner should succeed")
}

fn wait_until(timeout: Duration, condition: impl Fn() -> bool, description: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(condition(), "timed out waiting for {description}");
}

#[cfg(feature = "tokio")]
async fn async_sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(all(feature = "async-io", not(feature = "tokio")))]
async fn async_sleep(duration: Duration) {
    smol::Timer::after(duration).await;
}

async fn wait_until_async(
    timeout: Duration,
    condition: impl Fn() -> bool,
    description: &str,
) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        async_sleep(Duration::from_millis(10)).await;
    }

    assert!(condition(), "timed out waiting for {description}");
}

struct SignalWaiter {
    rx: mpsc::Receiver<Option<Message>>,
    context: String,
}

impl SignalWaiter {
    fn wait(self, timeout: Duration) -> Message {
        self.rx
            .recv_timeout(timeout)
            .unwrap_or_else(|_| panic!("timed out waiting for {}", self.context))
            .unwrap_or_else(|| panic!("signal stream ended for {}", self.context))
    }
}

fn spawn_signal_waiter(
    destination: &str,
    path: &'static str,
    interface: &'static str,
    signal_name: &'static str,
) -> SignalWaiter {
    spawn_filtered_signal_waiter(destination, path, interface, signal_name, Vec::new())
}

fn spawn_filtered_signal_waiter(
    destination: &str,
    path: &'static str,
    interface: &'static str,
    signal_name: &'static str,
    args: Vec<(u8, String)>,
) -> SignalWaiter {
    let destination = destination.to_string();
    let context = format!("{interface}.{signal_name}");
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (message_tx, message_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let connection = session_connection();
        let proxy = Proxy::new(&connection, destination.as_str(), path, interface)
            .expect("signal proxy should be valid");
        let refs = args
            .iter()
            .map(|(index, value)| (*index, value.as_str()))
            .collect::<Vec<_>>();
        let mut signals = proxy
            .receive_signal_with_args(signal_name, &refs)
            .expect("signal subscription should succeed");
        ready_tx
            .send(())
            .expect("signal waiter should notify readiness");
        let _ = message_tx.send(signals.next());
    });
    ready_rx
        .recv_timeout(DEFAULT_TIMEOUT)
        .unwrap_or_else(|_| panic!("timed out arming {context}"));
    SignalWaiter {
        rx: message_rx,
        context,
    }
}

fn message_body<T>(message: Message) -> T
where
    T: serde::de::DeserializeOwned + zbus::zvariant::Type,
{
    message.body().deserialize().expect("message body should deserialize")
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

async fn start_watcher(
    host_registered: bool,
    register_item_error: Option<RegisterItemError>,
) -> AsyncWatcherHandle {
    AsyncWatcherHandle::start_with_register_error(host_registered, register_item_error)
        .await
        .expect("mock watcher should start")
}

async fn close_watcher(watcher: AsyncWatcherHandle) {
    watcher.close().await;
}

#[cfg(feature = "tokio")]
fn async_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[cfg(all(feature = "async-io", not(feature = "tokio")))]
fn async_test_lock() -> &'static async_lock::Mutex<()> {
    static LOCK: OnceLock<async_lock::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| async_lock::Mutex::new(()))
}

#[cfg(feature = "blocking")]
fn blocking_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn mutate_sni_properties(tray: &mut TestTray<false>) {
    tray.category = Category::Communications;
    tray.title = "Updated Mock Tray".into();
    tray.status = Status::NeedsAttention;
    tray.window_id = 42;
    tray.icon_theme_path = "/tmp/mock-icons-updated".into();
    tray.icon_name = "main-icon-updated".into();
    tray.overlay_icon_name = "overlay-icon-updated".into();
    tray.attention_icon_name = "attention-icon-updated".into();
    tray.attention_movie_name = "attention-updated.gif".into();
    tray.tool_tip.title = "Updated tooltip".into();
    tray.tool_tip.description = "Updated tooltip description".into();
    tray.text_direction = TextDirection::RightToLeft;
}

fn registration_and_watcher_assertions(connection: &Connection, service_name: &str) {
    let watcher = watcher_proxy(connection);
    let registered_items: Vec<String> = watcher
        .get_property("RegisteredStatusNotifierItems")
        .expect("watcher should expose registered items");
    assert_eq!(registered_items, vec![service_name.to_string()]);

    let host_registered: bool = watcher
        .get_property("IsStatusNotifierHostRegistered")
        .expect("watcher should expose host registration state");
    assert!(host_registered);

    let protocol_version: i32 = watcher
        .get_property("ProtocolVersion")
        .expect("watcher should expose protocol version");
    assert_eq!(protocol_version, 0);

    assert!(has_owner(connection, service_name));
}

fn sni_property_and_method_assertions(connection: &Connection, service_name: &str, events: &CallbackProbe) {
    let proxy = sni_proxy(connection, service_name);

    let category: String = proxy.get_property("Category").unwrap();
    let id: String = proxy.get_property("Id").unwrap();
    let title: String = proxy.get_property("Title").unwrap();
    let status: String = proxy.get_property("Status").unwrap();
    let window_id: i32 = proxy.get_property("WindowId").unwrap();
    let icon_theme_path: String = proxy.get_property("IconThemePath").unwrap();
    let menu_path: OwnedObjectPath = proxy.get_property("Menu").unwrap();
    let item_is_menu: bool = proxy.get_property("ItemIsMenu").unwrap();
    let icon_name: String = proxy.get_property("IconName").unwrap();
    let icon_pixmap: Vec<(i32, i32, Vec<u8>)> = proxy.get_property("IconPixmap").unwrap();
    let overlay_icon_name: String = proxy.get_property("OverlayIconName").unwrap();
    let overlay_icon_pixmap: Vec<(i32, i32, Vec<u8>)> = proxy.get_property("OverlayIconPixmap").unwrap();
    let attention_icon_name: String = proxy.get_property("AttentionIconName").unwrap();
    let attention_icon_pixmap: Vec<(i32, i32, Vec<u8>)> =
        proxy.get_property("AttentionIconPixmap").unwrap();
    let attention_movie_name: String = proxy.get_property("AttentionMovieName").unwrap();
    let tool_tip: (String, Vec<(i32, i32, Vec<u8>)>, String, String) =
        proxy.get_property("ToolTip").unwrap();

    assert_eq!(category, "Hardware");
    assert_eq!(id, "runtime-protocol-tray");
    assert_eq!(title, "Mock Tray");
    assert_eq!(status, "Active");
    assert_eq!(window_id, 7);
    assert_eq!(icon_theme_path, "/tmp/mock-icons");
    assert_eq!(menu_path.as_str(), MENU_PATH);
    assert!(!item_is_menu);
    assert_eq!(icon_name, "main-icon");
    assert_eq!(icon_pixmap, vec![(1, 1, vec![0x11, 0, 0, 0xff])]);
    assert_eq!(overlay_icon_name, "overlay-icon");
    assert_eq!(overlay_icon_pixmap, vec![(1, 1, vec![0x22, 0, 0, 0xff])]);
    assert_eq!(attention_icon_name, "attention-icon");
    assert_eq!(attention_icon_pixmap, vec![(1, 1, vec![0x33, 0, 0, 0xff])]);
    assert_eq!(attention_movie_name, "attention.gif");
    assert_eq!(tool_tip.0, "tooltip-icon");
    assert_eq!(tool_tip.1, vec![(1, 1, vec![0x44, 0, 0, 0xff])]);
    assert_eq!(tool_tip.2, "Mock tooltip");
    assert_eq!(tool_tip.3, "Tooltip description");

    proxy.call::<_, _, ()>("Activate", &(10_i32, 20_i32)).unwrap();
    proxy
        .call::<_, _, ()>("SecondaryActivate", &(30_i32, 40_i32))
        .unwrap();
    proxy
        .call::<_, _, ()>("Scroll", &(7_i32, "horizontal"))
        .unwrap();

    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(events).scrolls.len() == 1,
        "tray method callbacks",
    );
    let snapshot = snapshot_events(events);
    assert_eq!(snapshot.activations, vec![(10, 20)]);
    assert_eq!(snapshot.secondary_activations, vec![(30, 40)]);
    assert_eq!(snapshot.scrolls, vec![(7, "Horizontal".into())]);
}

fn dbusmenu_assertions(connection: &Connection, service_name: &str, events: &CallbackProbe) {
    let proxy = menu_proxy(connection, service_name);

    let version: u32 = proxy.get_property("Version").unwrap();
    let text_direction: String = proxy.get_property("TextDirection").unwrap();
    let status: String = proxy.get_property("Status").unwrap();
    let icon_theme_path: Vec<String> = proxy.get_property("IconThemePath").unwrap();

    assert_eq!(version, 3);
    assert_eq!(text_direction, "ltr");
    assert_eq!(status, "normal");
    assert_eq!(icon_theme_path, vec!["/tmp/mock-icons".to_string()]);

    let (_, layout_all): (u32, LayoutTuple) = proxy
        .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
        .unwrap();
    let layout_all = decode_layout(layout_all);
    assert_eq!(layout_all.id, 0);
    assert_eq!(property_string(&layout_all.properties, "children-display"), "submenu");
    assert_eq!(layout_all.children.len(), 5);

    let standard = find_layout_by_label(&layout_all, "Open").expect("standard item should exist");
    assert_eq!(property_string(&standard.properties, "label"), "Open");
    assert_eq!(property_string(&standard.properties, "icon-name"), "open-icon");
    assert_eq!(property_bytes(&standard.properties, "icon-data"), vec![1, 2, 3, 4]);
    assert_eq!(
        property_shortcut(&standard.properties, "shortcut"),
        vec![vec!["Control".to_string(), "O".to_string()]],
    );
    assert_eq!(property_string(&standard.properties, "disposition"), "informative");
    assert!(!standard.properties.contains_key("enabled"));

    let checkmark = find_layout_by_label(&layout_all, "Pinned").expect("checkmark item should exist");
    assert_eq!(property_string(&checkmark.properties, "toggle-type"), "checkmark");
    assert_eq!(property_i32(&checkmark.properties, "toggle-state"), 1);

    let submenu = find_layout_by_label(&layout_all, "More").expect("submenu should exist");
    assert_eq!(property_string(&submenu.properties, "children-display"), "submenu");
    assert_eq!(submenu.children.len(), 1);
    assert_eq!(property_string(&submenu.children[0].properties, "label"), "Nested");

    let radio_a = find_layout_by_label(&layout_all, "Mode A").expect("radio item A should exist");
    let radio_b = find_layout_by_label(&layout_all, "Mode B").expect("radio item B should exist");
    assert_eq!(property_string(&radio_a.properties, "toggle-type"), "radio");
    assert_eq!(property_i32(&radio_a.properties, "toggle-state"), 0);
    assert_eq!(property_i32(&radio_b.properties, "toggle-state"), 1);
    assert_eq!(property_string(&radio_b.properties, "disposition"), "warning");

    let (_, layout_zero): (u32, LayoutTuple) = proxy
        .call("GetLayout", &(0_i32, 0_i32, Vec::<String>::new()))
        .unwrap();
    assert!(decode_layout(layout_zero).children.is_empty());

    let (_, layout_one): (u32, LayoutTuple) = proxy
        .call("GetLayout", &(0_i32, 1_i32, Vec::<String>::new()))
        .unwrap();
    let layout_one = decode_layout(layout_one);
    let one_submenu = find_layout_by_label(&layout_one, "More").expect("submenu should exist");
    assert!(one_submenu.children.is_empty());

    let group_all: Vec<(i32, HashMap<String, OwnedValue>)> = proxy
        .call("GetGroupProperties", &(Vec::<i32>::new(), Vec::<String>::new()))
        .unwrap();
    assert_eq!(group_all.len(), 7);
    assert_eq!(group_all[0].0, 0);
    assert_eq!(property_string(&group_all[0].1, "children-display"), "submenu");

    let filtered: Vec<(i32, HashMap<String, OwnedValue>)> = proxy
        .call(
            "GetGroupProperties",
            &(vec![standard.id], vec!["label".to_string()]),
        )
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].1.len(), 1);
    assert_eq!(property_string(&filtered[0].1, "label"), "Open");

    let label_value: OwnedValue = proxy
        .call("GetProperty", &(standard.id, "label".to_string()))
        .unwrap();
    let label_value: String = label_value.try_into().unwrap();
    assert_eq!(label_value, "Open".to_string());
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, OwnedValue>("GetProperty", &(999_i32, "label".to_string()))
            .expect_err("invalid item ids must fail")
    )
    .contains("InvalidArgs"));
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, OwnedValue>("GetProperty", &(standard.id, "missing".to_string()))
            .expect_err("invalid property names must fail")
    )
    .contains("InvalidArgs"));

    proxy
        .call::<_, _, ()>("Event", &(checkmark.id, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32))
        .unwrap();
    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(events).menu_clicks.iter().any(|entry| entry == "checkmark"),
        "checkmark click",
    );
    let toggle_state: OwnedValue = proxy
        .call("GetProperty", &(checkmark.id, "toggle-state".to_string()))
        .unwrap();
    let toggle_state: i32 = toggle_state.try_into().unwrap();
    assert_eq!(toggle_state, 0);

    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, ()>("Event", &(0_i32, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32))
            .expect_err("root menu clicks must fail")
    )
    .contains("InvalidArgs"));
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, ()>("Event", &(999_i32, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32))
            .expect_err("invalid menu ids must fail")
    )
    .contains("InvalidArgs"));

    let before_hover = snapshot_events(events).menu_clicks.len();
    for event_name in ["hovered", "opened", "closed", "x-test-custom"] {
        proxy
            .call::<_, _, ()>(
                "Event",
                &(standard.id, event_name.to_string(), OwnedValue::from(0_u8), 0_u32),
            )
            .unwrap();
    }
    assert_eq!(snapshot_events(events).menu_clicks.len(), before_hover);

    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, Vec<i32>>("EventGroup", &(Vec::<(i32, String, OwnedValue, u32)>::new(),))
            .expect_err("empty EventGroup calls must fail")
    )
    .contains("InvalidArgs"));

    let not_found: Vec<i32> = proxy
        .call(
            "EventGroup",
            &(
                vec![
                    (999_i32, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32),
                    (checkmark.id, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32),
                ],
            ),
        )
        .unwrap();
    assert_eq!(not_found, vec![999]);
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, Vec<i32>>(
                "EventGroup",
                &(vec![(999_i32, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32)],),
            )
            .expect_err("all-invalid EventGroup calls must fail")
    )
    .contains("InvalidArgs"));

    let about_to_show: bool = proxy.call("AboutToShow", &(standard.id,)).unwrap();
    assert!(!about_to_show);
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, bool>("AboutToShow", &(999_i32,))
            .expect_err("invalid AboutToShow ids must fail")
    )
    .contains("InvalidArgs"));

    let (updates_needed, id_errors): (Vec<i32>, Vec<i32>) = proxy
        .call("AboutToShowGroup", &(vec![standard.id, 999_i32],))
        .unwrap();
    assert!(updates_needed.is_empty());
    assert_eq!(id_errors, vec![999]);
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, (Vec<i32>, Vec<i32>)>("AboutToShowGroup", &(vec![999_i32],))
            .expect_err("all-invalid AboutToShowGroup ids must fail")
    )
    .contains("InvalidArgs"));
}

pub async fn async_registration_and_watchers() {
    use ksni::TrayMethods as _;

    let _guard = async_test_lock().lock().await;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should register with the mock watcher");
    let service_name = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
    assert!(service_name.starts_with("org.kde.StatusNotifierItem-"));
    let default_service_name = service_name.clone();
    with_blocking(move || {
        let connection = session_connection();
        registration_and_watcher_assertions(&connection, &default_service_name);
    })
    .await;
    handle.shutdown().await;
    close_watcher(watcher).await;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .disable_dbus_name(true)
        .spawn()
        .await
        .expect("tray should register with its unique name when dbus names are disabled");
    let unique_name = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
    assert!(unique_name.starts_with(':'));
    let unique_name_for_owner = unique_name.clone();
    assert!(
        with_blocking(move || {
            let connection = session_connection();
            has_owner(&connection, &unique_name_for_owner)
        })
        .await
    );
    handle.shutdown().await;
    close_watcher(watcher).await;

    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn().await {
        Ok(_) => panic!("missing watchers must fail without assume_sni_available"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::Watcher(zbus::fdo::Error::ServiceUnknown(_))));

    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .await
        .expect("assume_sni_available should turn missing watchers into a soft offline state");
    wait_until_async(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).offline.iter().any(|entry| entry.contains("ServiceUnknown")),
        "ServiceUnknown watcher_offline callback",
    )
    .await;
    handle.shutdown().await;

    let watcher = start_watcher(false, None).await;
    let (tray, _) = TestTray::<false>::new("runtime-protocol-tray");
    let err = match tray.spawn().await {
        Ok(_) => panic!("watchers without hosts should report WontShow"),
        Err(err) => err,
    };
    assert!(matches!(err, ksni::Error::WontShow));
    close_watcher(watcher).await;

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
    assert!(matches!(err, ksni::Error::Watcher(zbus::fdo::Error::InvalidArgs(_))));
    close_watcher(watcher).await;
}

pub async fn async_watcher_lifecycle() {
    use ksni::TrayMethods as _;

    let _guard = async_test_lock().lock().await;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let first_registration = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
    close_watcher(watcher).await;

    wait_until_async(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).offline.iter().any(|entry| entry.contains("No")),
        "watcher offline callback",
    )
    .await;

    let watcher = start_watcher(true, None).await;
    let second_registration = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
    wait_until_async(
        DEFAULT_TIMEOUT,
        || snapshot_events(&events).online_count == 1,
        "watcher online callback",
    )
    .await;
    assert_eq!(first_registration, second_registration);

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn async_status_notifier_item_protocol() {
    use ksni::TrayMethods as _;

    let _guard = async_test_lock().lock().await;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
    let service_name_for_assertions = service_name.clone();
    let events_for_assertions = events.clone();
    with_blocking(move || {
        let connection = session_connection();
        sni_property_and_method_assertions(&connection, &service_name_for_assertions, &events_for_assertions);
    })
    .await;

    let waiters_service_name = service_name.clone();
    let (new_title, new_icon, new_overlay, new_attention, new_tool_tip, new_status, sni_properties_changed, menu_properties_changed) =
        with_blocking(move || {
            (
                spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewTitle"),
                spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewIcon"),
                spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewOverlayIcon"),
                spawn_signal_waiter(&waiters_service_name, SNI_PATH, SNI_INTERFACE, "NewAttentionIcon"),
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
        let icon_paths: Vec<String> = menu_changed
            .get("IconThemePath")
            .expect("IconThemePath should be present")
            .clone()
            .try_into()
            .unwrap();
        assert_eq!(icon_paths, vec!["/tmp/mock-icons-updated".to_string()]);

        let connection = session_connection();
        let proxy = sni_proxy(&connection, &service_name_for_checks);
        assert_eq!(proxy.get_property::<String>("Title").unwrap(), "Updated Mock Tray");
        assert_eq!(proxy.get_property::<String>("Status").unwrap(), "NeedsAttention");
    })
    .await;

    handle.shutdown().await;
    close_watcher(watcher).await;
}

pub async fn async_dbusmenu_protocol() {
    use ksni::TrayMethods as _;

    let _guard = async_test_lock().lock().await;

    let watcher = start_watcher(true, None).await;
    let (tray, events) = TestTray::<false>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
    let service_name_for_assertions = service_name.clone();
    let events_for_assertions = events.clone();
    with_blocking(move || {
        let connection = session_connection();
        dbusmenu_assertions(&connection, &service_name_for_assertions, &events_for_assertions);
    })
    .await;

    let signal_service_name = service_name.clone();
    let items_properties_updated = with_blocking(move || {
        spawn_signal_waiter(&signal_service_name, MENU_PATH, MENU_INTERFACE, "ItemsPropertiesUpdated")
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
        assert!(updated_props
            .iter()
            .any(|(_, properties)| properties.get("label").is_some()));
        assert!(updated_props
            .iter()
            .any(|(_, properties)| properties.get("toggle-state").is_some()));
    })
    .await;

    let layout_service_name = service_name.clone();
    let layout_updated = with_blocking(move || {
        spawn_signal_waiter(&layout_service_name, MENU_PATH, MENU_INTERFACE, "LayoutUpdated")
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

pub async fn async_non_standard_compatibility() {
    use ksni::TrayMethods as _;

    let _guard = async_test_lock().lock().await;

    let watcher = start_watcher(true, None).await;
    let (tray, _) = TestTray::<true>::new("runtime-protocol-tray");
    let handle = tray.spawn().await.expect("tray should start");
    let service_name = watcher.wait_for_item_registration_async(DEFAULT_TIMEOUT).await;
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

#[cfg(feature = "blocking")]
pub fn blocking_registration_and_watchers() {
    use ksni::blocking::TrayMethods as _;

    let _guard = blocking_test_lock().lock().unwrap();

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

#[cfg(feature = "blocking")]
pub fn blocking_watcher_lifecycle() {
    use ksni::blocking::TrayMethods as _;

    let _guard = blocking_test_lock().lock().unwrap();

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

#[cfg(feature = "blocking")]
pub fn blocking_status_notifier_item_protocol() {
    use ksni::blocking::TrayMethods as _;

    let _guard = blocking_test_lock().lock().unwrap();

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

#[cfg(feature = "blocking")]
pub fn blocking_dbusmenu_protocol() {
    use ksni::blocking::TrayMethods as _;

    let _guard = blocking_test_lock().lock().unwrap();

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

#[cfg(feature = "blocking")]
pub fn blocking_non_standard_compatibility() {
    use ksni::blocking::TrayMethods as _;

    let _guard = blocking_test_lock().lock().unwrap();

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

macro_rules! async_protocol_tests {
    ($test_attr:meta) => {
        #[ $test_attr ]
        async fn protocol_registration_and_watchers() {
            crate::mock::async_registration_and_watchers().await;
        }

        #[ $test_attr ]
        async fn protocol_watcher_lifecycle() {
            crate::mock::async_watcher_lifecycle().await;
        }

        #[ $test_attr ]
        async fn protocol_status_notifier_item() {
            crate::mock::async_status_notifier_item_protocol().await;
        }

        #[ $test_attr ]
        async fn protocol_dbusmenu() {
            crate::mock::async_dbusmenu_protocol().await;
        }

        #[ $test_attr ]
        async fn protocol_non_standard_compatibility() {
            crate::mock::async_non_standard_compatibility().await;
        }
    };
}

pub(crate) use async_protocol_tests;

#[cfg(feature = "blocking")]
macro_rules! blocking_protocol_tests {
    () => {
        #[test]
        fn protocol_registration_and_watchers() {
            crate::mock::blocking_registration_and_watchers();
        }

        #[test]
        fn protocol_watcher_lifecycle() {
            crate::mock::blocking_watcher_lifecycle();
        }

        #[test]
        fn protocol_status_notifier_item() {
            crate::mock::blocking_status_notifier_item_protocol();
        }

        #[test]
        fn protocol_dbusmenu() {
            crate::mock::blocking_dbusmenu_protocol();
        }

        #[test]
        fn protocol_non_standard_compatibility() {
            crate::mock::blocking_non_standard_compatibility();
        }
    };
}

#[cfg(feature = "blocking")]
pub(crate) use blocking_protocol_tests;