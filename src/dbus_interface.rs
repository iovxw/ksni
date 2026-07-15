use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use zbus::names::InterfaceName;
use zbus::zvariant::{self, ObjectPath, OwnedValue, Type, Value};
use zbus::{object_server::SignalEmitter, Connection};

use crate::compat::Mutex;
use crate::service::Service;
use crate::{Icon, ToolTip, Tray};

pub const SNI_PATH: ObjectPath = ObjectPath::from_static_str_unchecked("/StatusNotifierItem");
pub const MENU_PATH: ObjectPath = ObjectPath::from_static_str_unchecked("/MenuBar");
pub const SNI_INTERFACE: InterfaceName =
    InterfaceName::from_static_str_unchecked("org.kde.StatusNotifierItem");
pub const MENU_INTERFACE: InterfaceName =
    InterfaceName::from_static_str_unchecked("com.canonical.dbusmenu");

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
pub trait StatusNotifierWatcher {
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
impl<T: Tray> StatusNotifierItem<T> {
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
        if T::MENU_ON_ACTIVATE {
            // a UnknownMethod is required to make ItemIsMenu work on GNOME
            // https://github.com/ubuntu/gnome-shell-extension-appindicator/blob/557dbddc8d469d1aaa302e6cf70600855dd767d1/appIndicator.js#L803
            // KDE Plasma < 6.4 also relies on this behavior
            // https://github.com/KDE/plasma-workspace/blob/4a98130f76bcae4211d3f9b10e4a7b760613ffc6/applets/systemtray/package/contents/ui/items/StatusNotifierItem.qml#L44-L57
            // KDE Plasma >= 6.4 won't call activate if ItemIsMenu is true, so we can keep this workaround
            // https://invent.kde.org/plasma/plasma-workspace/-/merge_requests/5332
            Err(zbus::fdo::Error::UnknownMethod("ItemIsMenu".into()))
        } else {
            let mut service = self.0.lock().await; // do NOT use any self methods after this
            service.call_activate(conn, x, y).await;
            Ok(())
        }
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
        dir: crate::Orientation,
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
        Ok(T::MENU_ON_ACTIVATE)
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
    pub async fn new_title(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_attention_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_overlay_icon(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_tool_tip(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn new_status(ctxt: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

#[derive(Debug, Default, Type, Serialize, PartialEq)]
pub struct Layout {
    pub id: i32,
    pub properties: HashMap<Cow<'static, str>, OwnedValue>,
    // The construction of OwnedValue is recursive
    // which may overflow menu with very depth submenu
    // so we use Value<'static> here
    pub children: Vec<Value<'static>>,
}

impl TryFrom<Value<'static>> for Layout {
    type Error = zvariant::Error;
    fn try_from(value: Value<'static>) -> zvariant::Result<Self> {
        let mut fields = zvariant::Structure::try_from(value)?.into_fields();
        Ok(Self {
            id: fields.remove(0).downcast()?,
            properties: fields.remove(0).downcast::<ItemPropsValueHelper>()?.0,
            children: fields.remove(0).downcast()?,
        })
    }
}

impl TryFrom<OwnedValue> for Layout {
    type Error = zvariant::Error;
    fn try_from(value: OwnedValue) -> zvariant::Result<Self> {
        <Self as TryFrom<Value<'static>>>::try_from(value.into())
    }
}

impl<'a> From<Layout> for Value<'a> {
    fn from(s: Layout) -> Self {
        Value::from(
            zvariant::StructureBuilder::new()
                .add_field(s.id)
                .add_field(s.properties)
                .add_field(s.children)
                .build()
                .unwrap(),
        )
    }
}

impl From<Layout> for OwnedValue {
    fn from(s: Layout) -> Self {
        Value::from(s)
            .try_into_owned()
            .expect("Layout should not contains any fd")
    }
}

/// FIXME: remove this after `From<zbus::zvariant::Value<'_>>` is implemented for `Cow<'_, str>`
struct ItemPropsValueHelper(HashMap<Cow<'static, str>, OwnedValue>);

impl<'a> TryFrom<Value<'a>> for ItemPropsValueHelper {
    type Error = zvariant::Error;

    fn try_from(value: Value<'a>) -> Result<Self, Self::Error> {
        if let Value::Dict(v) = value {
            v.into_iter()
                .map(|(key, value)| {
                    let key = String::try_from(if let Value::Value(v) = key { *v } else { key })
                        .map(Into::into)?;

                    let value = OwnedValue::try_from(if let Value::Value(v) = value {
                        *v
                    } else {
                        value
                    })?;

                    Ok((key, value))
                })
                .collect::<Result<HashMap<Cow<'static, str>, OwnedValue>, _>>()
                .map(ItemPropsValueHelper)
        } else {
            Err(Self::Error::IncorrectType)
        }
    }
}

pub struct DbusMenu<T>(Arc<Mutex<Service<T>>>);

impl<T> DbusMenu<T> {
    pub fn new(service: Arc<Mutex<Service<T>>>) -> Self {
        Self(service)
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl<T: Tray> DbusMenu<T> {
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
    ) -> zbus::fdo::Result<Vec<(i32, HashMap<Cow<'static, str>, OwnedValue>)>> {
        let service = self.0.lock().await; // do NOT use any self methods after this
        if ids.is_empty() {
            Ok(service.get_all_item(&property_names))
        } else {
            Ok(ids
                .into_iter()
                .filter_map(|id| {
                    service
                        .get_menu_item(id, &property_names)
                        .map(|properties| (id, properties))
                })
                .collect())
        }
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

    async fn about_to_show(
        &self,
        #[zbus(connection)] conn: &Connection,
        id: i32,
    ) -> zbus::fdo::Result<bool> {
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        // TODO: run the hook in a separate task
        if service.run_about2show_hook(conn, id).await? {
            // Always return false
            // libdubusmenu does not respect the return value
            // Qt’s solution is to always return false, and emit LayoutUpdated/PropertiesUpdated
            // signals when the menu is updated. We follow the same approach
            Ok(false)
        } else {
            Err(zbus::fdo::Error::InvalidArgs("id not found".into()))
        }
    }

    async fn about_to_show_group(
        &self,
        #[zbus(connection)] conn: &Connection,
        ids: Vec<i32>,
    ) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        let mut service = self.0.lock().await; // do NOT use any self methods after this
        let mut not_found_ids = Vec::new();

        for &id in &ids {
            if id == 0 {
                service.run_about2show_hook(conn, id).await?;
            } else if service.get_menu_item(id, &[]).is_none() {
                // Only checks id exists, don't trigger update
                not_found_ids.push(id);
            }
        }

        if !ids.is_empty() && not_found_ids.len() == ids.len() {
            Err(zbus::fdo::Error::InvalidArgs(
                "None of the id in the group can be found".into(),
            ))
        } else {
            Ok((Vec::new(), not_found_ids))
        }
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
        Ok(service.get_status().to_menu_status())
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
        ctxt: &SignalEmitter<'_>,
        updated_props: Vec<(i32, HashMap<Cow<'static, str>, OwnedValue>)>,
        removed_props: Vec<(i32, Vec<Cow<'static, str>>)>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn layout_updated(
        ctxt: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}
