use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use zbus::zvariant::{ObjectPath, OwnedValue, Type, Value};
use zbus::SignalContext;

use crate::{Icon, ToolTip};

pub const SNI_PATH: &str = "/StatusNotifierItem";
pub const MENU_PATH: &str = "/MenuBar";

type ReplySender<T> = oneshot::Sender<zbus::fdo::Result<T>>;

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    // methods
    async fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;
    async fn register_status_notifier_host(&self, service: &str) -> zbus::Result<()>;

    // properties
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::Result<i32>;

    // signals
    #[zbus(signal)]
    fn status_notifier_item_registered(&self, name: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn status_notifier_item_unregistered(&self, name: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn status_notifier_host_registered(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn status_notifier_host_unregistered(&self) -> zbus::Result<()>;
}

#[derive(Debug)]
pub enum SniMessage {
    Activate(i32, i32),
    SecondaryActivate(i32, i32),
    Scroll(i32, String),
    GetDbusProperty(SniProperty),
}

#[derive(Debug)]
pub enum SniProperty {
    Category(ReplySender<String>),
    Id(ReplySender<String>),
    Title(ReplySender<String>),
    Status(ReplySender<String>),
    WindowId(ReplySender<i32>),
    IconThemePath(ReplySender<String>),
    IconName(ReplySender<String>),
    IconPixmap(ReplySender<Vec<Icon>>),
    OverlayIconName(ReplySender<String>),
    OverlayIconPixmap(ReplySender<Vec<Icon>>),
    AttentionIconName(ReplySender<String>),
    AttentionIconPixmap(ReplySender<Vec<Icon>>),
    AttentionMovieName(ReplySender<String>),
    ToolTip(ReplySender<ToolTip>),
}

pub struct StatusNotifierItem {
    sender: tokio::sync::mpsc::UnboundedSender<SniMessage>,
}

impl StatusNotifierItem {
    pub fn new() -> (Self, tokio::sync::mpsc::UnboundedReceiver<SniMessage>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (StatusNotifierItem { sender: tx }, rx)
    }

    fn send(&self, message: SniMessage) -> zbus::fdo::Result<()> {
        self.sender
            .send(message)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(())
    }

    async fn get<T>(
        &self,
        property: SniProperty,
        rx: oneshot::Receiver<zbus::fdo::Result<T>>,
    ) -> zbus::fdo::Result<T> {
        self.send(SniMessage::GetDbusProperty(property))?;
        rx.await
            .unwrap_or_else(|e| Err(zbus::fdo::Error::Failed(e.to_string())))
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    // methods
    fn context_menu(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        Ok(())
    }

    fn activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        self.send(SniMessage::Activate(x, y))
    }

    fn secondary_activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        self.send(SniMessage::SecondaryActivate(x, y))
    }

    fn scroll(&self, delta: i32, dir: &str) -> zbus::fdo::Result<()> {
        self.send(SniMessage::Scroll(delta, dir.to_string()))
    }

    // properties
    #[zbus(property)]
    async fn category(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::Category(tx), rx).await
    }

    #[zbus(property)]
    async fn id(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::Id(tx), rx).await
    }

    #[zbus(property)]
    async fn title(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::Title(tx), rx).await
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::Status(tx), rx).await
    }

    #[zbus(property)]
    async fn window_id(&self) -> zbus::fdo::Result<i32> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::WindowId(tx), rx).await
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::IconThemePath(tx), rx).await
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::fdo::Result<ObjectPath<'_>> {
        Ok(ObjectPath::from_static_str(MENU_PATH).expect("MENU_PATH valid"))
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    #[zbus(property)]
    async fn icon_name(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::IconName(tx), rx).await
    }

    #[zbus(property)]
    async fn icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::IconPixmap(tx), rx).await
    }

    #[zbus(property)]
    async fn overlay_icon_name(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::OverlayIconName(tx), rx).await
    }

    #[zbus(property)]
    async fn overlay_icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::OverlayIconPixmap(tx), rx).await
    }

    #[zbus(property)]
    async fn attention_icon_name(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::AttentionIconName(tx), rx).await
    }

    #[zbus(property)]
    async fn attention_icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::AttentionIconPixmap(tx), rx).await
    }

    #[zbus(property)]
    async fn attention_movie_name(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::AttentionMovieName(tx), rx).await
    }

    #[zbus(property)]
    async fn tool_tip(&self) -> zbus::fdo::Result<ToolTip> {
        let (tx, rx) = oneshot::channel();
        self.get(SniProperty::ToolTip(tx), rx).await
    }

    // signals
    #[zbus(signal)]
    pub async fn new_title(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_icon(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_attention_icon(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_overlay_icon(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_tool_tip(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_status(ctxt: &SignalContext<'_>, status: &str) -> zbus::Result<()>;
}

#[derive(Debug, Default, Type, Serialize, Deserialize, Value, OwnedValue)]
pub struct LayoutItem {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

#[derive(Debug)]
pub enum DbusMenuMessage {
    GetLayout(i32, i32, Vec<String>, ReplySender<(u32, LayoutItem)>),
    GetGroupProperties(
        Vec<i32>,
        Vec<String>,
        ReplySender<Vec<(i32, HashMap<String, OwnedValue>)>>,
    ),
    GetProperty(i32, String, ReplySender<OwnedValue>),
    Event(i32, String, OwnedValue, u32, ReplySender<()>),
    EventGroup(Vec<(i32, String, OwnedValue, u32)>, ReplySender<Vec<i32>>),
    GetDbusProperty(DbusMenuProperty),
}

#[derive(Debug)]
pub enum DbusMenuProperty {
    TextDirection(ReplySender<String>),
    Status(ReplySender<String>),
    IconThemePath(ReplySender<Vec<String>>),
}

pub struct DbusMenu {
    sender: tokio::sync::mpsc::UnboundedSender<DbusMenuMessage>,
}

impl DbusMenu {
    pub fn new() -> (Self, tokio::sync::mpsc::UnboundedReceiver<DbusMenuMessage>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (DbusMenu { sender: tx }, rx)
    }

    fn send(&self, message: DbusMenuMessage) -> zbus::fdo::Result<()> {
        self.sender
            .send(message)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(())
    }

    async fn send_recv<T>(
        &self,
        message: DbusMenuMessage,
        rx: oneshot::Receiver<zbus::fdo::Result<T>>,
    ) -> zbus::fdo::Result<T> {
        self.send(message)?;
        rx.await
            .unwrap_or_else(|e| Err(zbus::fdo::Error::Failed(e.to_string())))
    }

    async fn get<T>(
        &self,
        property: DbusMenuProperty,
        rx: oneshot::Receiver<zbus::fdo::Result<T>>,
    ) -> zbus::fdo::Result<T> {
        self.send_recv(DbusMenuMessage::GetDbusProperty(property), rx)
            .await
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    // methods
    async fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, LayoutItem)> {
        let (tx, rx) = oneshot::channel();
        self.send_recv(
            DbusMenuMessage::GetLayout(parent_id, recursion_depth, property_names, tx),
            rx,
        )
        .await
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<Vec<(i32, HashMap<String, OwnedValue>)>> {
        let (tx, rx) = oneshot::channel();
        self.send_recv(
            DbusMenuMessage::GetGroupProperties(ids, property_names, tx),
            rx,
        )
        .await
    }

    async fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let (tx, rx) = oneshot::channel();
        self.send_recv(DbusMenuMessage::GetProperty(id, name, tx), rx)
            .await
    }

    async fn event(
        &self,
        id: i32,
        event_id: String,
        data: OwnedValue,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_recv(
            DbusMenuMessage::Event(id, event_id, data, timestamp, tx),
            rx,
        )
        .await
    }

    async fn event_group(
        &self,
        events: Vec<(i32, String, OwnedValue, u32)>,
    ) -> zbus::fdo::Result<Vec<i32>> {
        let (tx, rx) = oneshot::channel();
        self.send_recv(DbusMenuMessage::EventGroup(events, tx), rx)
            .await
    }

    async fn about_to_show(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn about_to_show_group(&self) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        // FIXME: the DBus message should set the no reply flag
        Ok(Default::default())
    }

    // properties
    #[zbus(property)]
    fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(3)
    }

    #[zbus(property)]
    async fn text_direction(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(DbusMenuProperty::TextDirection(tx), rx).await
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.get(DbusMenuProperty::Status(tx), rx).await
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.get(DbusMenuProperty::IconThemePath(tx), rx).await
    }

    // signals
    #[zbus(signal)]
    pub async fn items_properties_updated(
        ctxt: &SignalContext<'_>,
        updated_props: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed_props: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn layout_updated(
        ctxt: &SignalContext<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}
