use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zbus::zvariant::{ObjectPath, OwnedValue, Type, Value};
use zbus::{Connection, SignalContext};

use crate::compat::Mutex;
use crate::service::Service;
use crate::{Icon, ToolTip, Tray};

pub const SNI_PATH: ObjectPath = ObjectPath::from_static_str_unchecked("/StatusNotifierItem");
pub const MENU_PATH: ObjectPath = ObjectPath::from_static_str_unchecked("/MenuBar");

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

pub struct StatusNotifierItem<T>(Arc<Mutex<Service<T>>>);

impl<T> StatusNotifierItem<T> {
    pub fn new(service: Arc<Mutex<Service<T>>>) -> Self {
        Self(service)
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl<T: Tray + Send + 'static> StatusNotifierItem<T> {
    // show a self rendered menu, not supported by ksni
    fn context_menu(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::UnknownMethod(
            "Not supported, please use `menu`".into(),
        ))
    }

    async fn activate(
        &self,
        #[zbus(connection)] conn: &Connection,
        x: i32,
        y: i32,
    ) -> zbus::fdo::Result<()> {
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        service.call_activate(conn, x, y).await;
        Ok(())
    }

    async fn secondary_activate(
        &self,
        #[zbus(connection)] conn: &Connection,
        x: i32,
        y: i32,
    ) -> zbus::fdo::Result<()> {
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        service.call_secondary_activate(conn, x, y).await;
        Ok(())
    }

    async fn scroll(
        &self,
        #[zbus(connection)] conn: &Connection,
        delta: i32,
        dir: &str,
    ) -> zbus::fdo::Result<()> {
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        service.call_scroll(conn, delta, dir).await;
        Ok(())
    }

    // properties
    #[zbus(property)]
    async fn category(&self) -> zbus::fdo::Result<crate::Category> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_category())
    }

    #[zbus(property)]
    async fn id(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_id())
    }

    #[zbus(property)]
    async fn title(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_title())
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<crate::Status> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_status())
    }

    #[zbus(property)]
    async fn window_id(&self) -> zbus::fdo::Result<i32> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_window_id())
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_icon_theme_path())
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::fdo::Result<ObjectPath<'_>> {
        Ok(MENU_PATH)
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    #[zbus(property)]
    async fn icon_name(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_icon_name())
    }

    #[zbus(property)]
    async fn icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_icon_pixmap())
    }

    #[zbus(property)]
    async fn overlay_icon_name(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_overlay_icon_name())
    }

    #[zbus(property)]
    async fn overlay_icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_overlay_icon_pixmap())
    }

    #[zbus(property)]
    async fn attention_icon_name(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_attention_icon_name())
    }

    #[zbus(property)]
    async fn attention_icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_attention_icon_pixmap())
    }

    #[zbus(property)]
    async fn attention_movie_name(&self) -> zbus::fdo::Result<String> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_attention_movie_name())
    }

    #[zbus(property)]
    async fn tool_tip(&self) -> zbus::fdo::Result<ToolTip> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_tool_tip())
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
pub struct Layout {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

pub struct DbusMenu<T>(Arc<Mutex<Service<T>>>);

impl<T> DbusMenu<T> {
    pub fn new(service: Arc<Mutex<Service<T>>>) -> Self {
        Self(service)
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl<T: Tray + Send + 'static> DbusMenu<T> {
    // methods
    async fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        let tree = service.build_layout(
            parent_id,
            if recursion_depth < 0 {
                None
            } else {
                Some(recursion_depth as usize)
            },
            property_names,
        );
        tree.map(|tree| (service.revision, tree))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("parentId not found".to_string()))
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<Vec<(i32, HashMap<String, OwnedValue>)>> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        let items = ids
            .into_iter()
            .filter_map(|id| service.get_menu_item(id, &property_names).map(|r| (id, r)))
            .filter(|r| !r.1.is_empty())
            .collect();
        // TODO: return an error if items is empty
        Ok(items)
    }

    async fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        service
            .get_menu_item(id, &[name])
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))
            .map(|map| map.into_iter().next().map(|entry| entry.1))
            .transpose()
            .unwrap_or_else(|| Err(zbus::fdo::Error::InvalidArgs("property not found".into())))
    }

    async fn event(
        &self,
        #[zbus(connection)] conn: &Connection,
        id: i32,
        event_id: String,
        data: OwnedValue,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        service
            .event(conn, true, id, &event_id, data, timestamp)
            .await
    }

    async fn event_group(
        &self,
        #[zbus(connection)] conn: &Connection,
        events: Vec<(i32, String, OwnedValue, u32)>,
    ) -> zbus::fdo::Result<Vec<i32>> {
        if events.is_empty() {
            return Err(zbus::fdo::Error::InvalidArgs("Empty events".into()));
        }
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        let events_len = events.len();
        let last_id = events
            .last()
            .expect("`events.is_empty` should been checked")
            .0;
        let mut not_found = Vec::with_capacity(events_len);
        for (id, event_id, data, timestamp) in events {
            if service
                .event(conn, id == last_id, id, &event_id, data, timestamp)
                .await
                .is_err()
            {
                not_found.push(id);
            }
        }
        if not_found.len() == events_len {
            Err(zbus::fdo::Error::InvalidArgs(
                "None of the id in the events can be found".into(),
            ))
        } else {
            Ok(not_found)
        }
    }

    async fn about_to_show(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn about_to_show_group(&self) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        Ok(Default::default())
    }

    // properties
    #[zbus(property)]
    fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(3)
    }

    #[zbus(property)]
    async fn text_direction(&self) -> zbus::fdo::Result<crate::TextDirection> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        Ok(service.get_text_direction())
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<crate::menu::Status> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        let status = match service.get_status() {
            crate::tray::Status::Active | crate::tray::Status::Passive => {
                crate::menu::Status::Normal
            }
            crate::tray::Status::NeedsAttention => crate::menu::Status::Notice,
        };
        Ok(status)
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<Vec<String>> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        let path = service.get_icon_theme_path();
        let path = if path.is_empty() { vec![] } else { vec![path] };
        Ok(path)
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
