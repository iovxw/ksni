use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use ksni::menu::{
    CheckmarkItem, Disposition, RadioGroup, RadioItem, StandardItem, SubMenu, TextDirection,
};
use ksni::{Category, Icon, MenuItem, Status, ToolTip};
use zbus::blocking::{connection, Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::Message;

pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
pub const SNI_PATH: &str = "/StatusNotifierItem";
pub const SNI_INTERFACE: &str = "org.kde.StatusNotifierItem";
pub const MENU_PATH: &str = "/MenuBar";
pub const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

macro_rules! repetition_utils {
    (@count $($tokens:tt),*) => {{
        [$($crate::common::repetition_utils!(@replace $tokens => ())),*].len()
    }};

    (@replace $x:tt => $y:tt) => { $y }
}

macro_rules! properties {
    () => {{ std::collections::HashMap::new() }};

    ( $( $key:expr => $value:expr ),* $(,)? ) => {{
        let mut map = std::collections::HashMap::with_capacity(
            const { $crate::common::repetition_utils!(@count $($key),*) }
        );
        $(
            let value = zbus::zvariant::Value::from($value).try_into_owned().unwrap();
            map.insert($key.into(), value);
        )*
        map
    }}
}

pub(crate) use {properties, repetition_utils};

#[derive(Clone, Debug)]
pub enum RegisterItemError {
    InvalidArgs(String),
    Failed(String),
}

impl RegisterItemError {
    pub fn into_fdo_error(self) -> zbus::fdo::Error {
        match self {
            Self::InvalidArgs(message) => zbus::fdo::Error::InvalidArgs(message),
            Self::Failed(message) => zbus::fdo::Error::Failed(message),
        }
    }
}

pub struct WatcherState {
    pub registered_items: Vec<String>,
    pub host_registered: bool,
    pub protocol_version: i32,
    pub register_item_error: Option<RegisterItemError>,
}

pub struct MockWatcher {
    pub state: Arc<Mutex<WatcherState>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl MockWatcher {
    async fn register_status_notifier_item(&self, service: &str) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(error) = state.register_item_error.clone() {
            return Err(error.into_fdo_error());
        }
        state.registered_items.push(service.to_string());
        Ok(())
    }

    async fn register_status_notifier_host(&self, _service: &str) -> zbus::fdo::Result<()> {
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(self.state.lock().unwrap().registered_items.clone())
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::fdo::Result<bool> {
        Ok(self.state.lock().unwrap().host_registered)
    }

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::fdo::Result<i32> {
        Ok(self.state.lock().unwrap().protocol_version)
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
    pub include_separator: bool,
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
                include_separator: false,
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
        self.events
            .lock()
            .unwrap()
            .secondary_activations
            .push((x, y));
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
                    tray.events
                        .lock()
                        .unwrap()
                        .menu_clicks
                        .push("standard".into());
                }),
            }
            .into(),
            CheckmarkItem {
                label: self.checkmark_label.clone(),
                checked: self.checkmark_checked,
                activate: Box::new(|tray: &mut Self| {
                    tray.checkmark_checked = !tray.checkmark_checked;
                    tray.events
                        .lock()
                        .unwrap()
                        .menu_clicks
                        .push("checkmark".into());
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: self.submenu_label.clone(),
                submenu: vec![StandardItem {
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
                .into()],
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

        if self.include_separator {
            items.push(MenuItem::Separator);
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

pub type LayoutTuple = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<LayoutNode>,
}

pub fn decode_layout(layout: LayoutTuple) -> LayoutNode {
    LayoutNode {
        id: layout.0,
        properties: layout.1,
        children: layout.2.into_iter().map(layout_from_value).collect(),
    }
}

pub fn layout_from_value(value: OwnedValue) -> LayoutNode {
    let layout: LayoutTuple = value
        .try_into()
        .expect("value should decode into a layout tuple");
    decode_layout(layout)
}

pub fn find_layout_by_label<'a>(layout: &'a LayoutNode, label: &str) -> Option<&'a LayoutNode> {
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

pub fn session_connection() -> Connection {
    connection::Builder::session()
        .expect("session bus builder should be available")
        .method_timeout(DEFAULT_TIMEOUT)
        .build()
        .expect("session bus connection should be available")
}

pub fn watcher_proxy<'a>(connection: &'a Connection) -> Proxy<'a> {
    Proxy::new(connection, WATCHER_NAME, WATCHER_PATH, WATCHER_INTERFACE)
        .expect("watcher proxy should be valid")
}

pub fn sni_proxy<'a>(connection: &'a Connection, destination: &'a str) -> Proxy<'a> {
    Proxy::new(connection, destination, SNI_PATH, SNI_INTERFACE).expect("SNI proxy should be valid")
}

pub fn menu_proxy<'a>(connection: &'a Connection, destination: &'a str) -> Proxy<'a> {
    Proxy::new(connection, destination, MENU_PATH, MENU_INTERFACE)
        .expect("dbusmenu proxy should be valid")
}

pub fn has_owner(connection: &Connection, name: &str) -> bool {
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

pub fn wait_until(timeout: Duration, condition: impl Fn() -> bool, description: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(condition(), "timed out waiting for {description}");
}

pub struct SignalWaiter {
    rx: mpsc::Receiver<Option<Message>>,
    context: String,
}

impl SignalWaiter {
    pub fn wait(self, timeout: Duration) -> Message {
        self.rx
            .recv_timeout(timeout)
            .unwrap_or_else(|_| panic!("timed out waiting for {}", self.context))
            .unwrap_or_else(|| panic!("signal stream ended for {}", self.context))
    }
}

pub fn spawn_signal_waiter(
    destination: &str,
    path: &'static str,
    interface: &'static str,
    signal_name: &'static str,
) -> SignalWaiter {
    spawn_filtered_signal_waiter(destination, path, interface, signal_name, Vec::new())
}

pub fn spawn_filtered_signal_waiter(
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

pub fn message_body<T>(message: Message) -> T
where
    T: serde::de::DeserializeOwned + zbus::zvariant::Type,
{
    message
        .body()
        .deserialize()
        .expect("message body should deserialize")
}
pub fn mutate_sni_properties(tray: &mut TestTray<false>) {
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

pub fn registration_and_watcher_assertions(connection: &Connection, service_name: &str) {
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

pub fn sni_property_and_method_assertions(
    connection: &Connection,
    service_name: &str,
    events: &CallbackProbe,
) {
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
    let overlay_icon_pixmap: Vec<(i32, i32, Vec<u8>)> =
        proxy.get_property("OverlayIconPixmap").unwrap();
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

    proxy
        .call::<_, _, ()>("Activate", &(10_i32, 20_i32))
        .unwrap();
    proxy
        .call::<_, _, ()>("SecondaryActivate", &(30_i32, 40_i32))
        .unwrap();
    proxy
        .call::<_, _, ()>("Scroll", &(7_i32, "horizontal"))
        .unwrap();
    proxy
        .call::<_, _, ()>("Scroll", &(3_i32, "vertical"))
        .unwrap();

    wait_until(
        DEFAULT_TIMEOUT,
        || snapshot_events(events).scrolls.len() == 2,
        "tray method callbacks",
    );
    let snapshot = snapshot_events(events);
    assert_eq!(snapshot.activations, vec![(10, 20)]);
    assert_eq!(snapshot.secondary_activations, vec![(30, 40)]);
    assert_eq!(
        snapshot.scrolls,
        vec![(7, "Horizontal".into()), (3, "Vertical".into())]
    );
}

pub fn dbusmenu_assertions(connection: &Connection, service_name: &str, events: &CallbackProbe) {
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
    assert_eq!(
        layout_all.properties,
        properties! { "children-display" => "submenu" }
    );
    assert_eq!(layout_all.children.len(), 5);

    let standard = find_layout_by_label(&layout_all, "Open").expect("standard item should exist");
    assert_eq!(
        standard.properties,
        properties! {
            "label" => "Open",
            "icon-name" => "open-icon",
            "icon-data" => vec![1_u8, 2, 3, 4],
            "shortcut" => vec![vec!["Control".to_string(), "O".to_string()]],
            "disposition" => "informative",
        }
    );

    let checkmark =
        find_layout_by_label(&layout_all, "Pinned").expect("checkmark item should exist");
    assert_eq!(
        checkmark.properties,
        properties! {
            "label" => "Pinned",
            "toggle-type" => "checkmark",
            "toggle-state" => 1_i32,
        }
    );

    let submenu = find_layout_by_label(&layout_all, "More").expect("submenu should exist");
    assert_eq!(
        submenu.properties,
        properties! {
            "label" => "More",
            "children-display" => "submenu",
        }
    );
    assert_eq!(submenu.children.len(), 1);
    assert_eq!(
        submenu.children[0].properties,
        properties! { "label" => "Nested" }
    );

    let radio_a = find_layout_by_label(&layout_all, "Mode A").expect("radio item A should exist");
    let radio_b = find_layout_by_label(&layout_all, "Mode B").expect("radio item B should exist");
    assert_eq!(
        radio_a.properties,
        properties! {
            "label" => "Mode A",
            "toggle-type" => "radio",
            "toggle-state" => 0_i32,
        }
    );
    assert_eq!(
        radio_b.properties,
        properties! {
            "label" => "Mode B",
            "toggle-type" => "radio",
            "toggle-state" => 1_i32,
            "disposition" => "warning",
        }
    );

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
        .call(
            "GetGroupProperties",
            &(Vec::<i32>::new(), Vec::<String>::new()),
        )
        .unwrap();
    assert_eq!(group_all.len(), 7);
    assert_eq!(group_all[0].0, 0);
    assert_eq!(
        group_all[0].1,
        properties! { "children-display" => "submenu" }
    );

    let filtered: Vec<(i32, HashMap<String, OwnedValue>)> = proxy
        .call(
            "GetGroupProperties",
            &(vec![standard.id], vec!["label".to_string()]),
        )
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].1, properties! { "label" => "Open" });

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
        .call::<_, _, ()>(
            "Event",
            &(
                checkmark.id,
                "clicked".to_string(),
                OwnedValue::from(0_u8),
                0_u32,
            ),
        )
        .unwrap();
    wait_until(
        DEFAULT_TIMEOUT,
        || {
            snapshot_events(events)
                .menu_clicks
                .iter()
                .any(|entry| entry == "checkmark")
        },
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
            .call::<_, _, ()>(
                "Event",
                &(0_i32, "clicked".to_string(), OwnedValue::from(0_u8), 0_u32)
            )
            .expect_err("root menu clicks must fail")
    )
    .contains("InvalidArgs"));
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, ()>(
                "Event",
                &(
                    999_i32,
                    "clicked".to_string(),
                    OwnedValue::from(0_u8),
                    0_u32
                )
            )
            .expect_err("invalid menu ids must fail")
    )
    .contains("InvalidArgs"));

    let before_hover = snapshot_events(events).menu_clicks.len();
    for event_name in ["hovered", "opened", "closed", "x-test-custom"] {
        proxy
            .call::<_, _, ()>(
                "Event",
                &(
                    standard.id,
                    event_name.to_string(),
                    OwnedValue::from(0_u8),
                    0_u32,
                ),
            )
            .unwrap();
    }
    assert_eq!(snapshot_events(events).menu_clicks.len(), before_hover);

    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, Vec<i32>>(
                "EventGroup",
                &(Vec::<(i32, String, OwnedValue, u32)>::new(),)
            )
            .expect_err("empty EventGroup calls must fail")
    )
    .contains("InvalidArgs"));

    let not_found: Vec<i32> = proxy
        .call(
            "EventGroup",
            &(vec![
                (
                    999_i32,
                    "clicked".to_string(),
                    OwnedValue::from(0_u8),
                    0_u32,
                ),
                (
                    checkmark.id,
                    "clicked".to_string(),
                    OwnedValue::from(0_u8),
                    0_u32,
                ),
            ],),
        )
        .unwrap();
    assert_eq!(not_found, vec![999]);
    assert!(format!(
        "{:?}",
        proxy
            .call::<_, _, Vec<i32>>(
                "EventGroup",
                &(vec![(
                    999_i32,
                    "clicked".to_string(),
                    OwnedValue::from(0_u8),
                    0_u32
                )],),
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
