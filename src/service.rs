use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::{future::Either, StreamExt};
use pastey::paste;
use zbus::fdo::DBusProxy;
use zbus::zvariant::{OwnedValue, Value};
use zbus::Connection;

use crate::compat::{self, mpsc, select, Mutex};
use crate::dbus_interface::{
    DbusMenu, Layout, StatusNotifierItem, StatusNotifierWatcherProxy, MENU_INTERFACE, MENU_PATH,
    SNI_INTERFACE, SNI_PATH,
};
use crate::menu;
use crate::{Error, HandleReuest, OfflineReason, Tray};

static INSTANCE_COUNTER: AtomicUsize = AtomicUsize::new(1);

pub(crate) async fn run<T: Tray>(
    service: Arc<Mutex<Service<T>>>,
    mut handle_rx: mpsc::UnboundedReceiver<HandleReuest>,
    own_name: bool,
    assume_sni_available: bool,
) -> Result<impl Future<Output = ()>, Error> {
    let sni_obj = StatusNotifierItem::new(service.clone());
    let menu_obj = DbusMenu::new(service.clone());

    // for those `expect`, see: https://github.com/dbus2/zbus/issues/403
    let conn = zbus::connection::Builder::session()
        .map_err(|e| Error::Dbus(e))?
        .internal_executor(false) // avoid extra thread when async-io enabled
        .serve_at(SNI_PATH, sni_obj)
        .expect("SNI_PATH should be valid")
        .serve_at(MENU_PATH, menu_obj)
        .expect("MENU_PATH should be valid")
        .build()
        .await
        .map_err(|e| Error::Dbus(e))?;

    if cfg!(feature = "async-io") {
        let executor = conn.executor().clone();
        // must start the executor before register_status_notifier_item
        compat::spawn(async move {
            // won't empty until conn stopped
            while !executor.is_empty() {
                executor.tick().await;
            }
        });
    }

    let name = if own_name {
        let name = format!(
            "org.kde.StatusNotifierItem-{}-{}",
            std::process::id(),
            INSTANCE_COUNTER.fetch_add(1, Ordering::AcqRel)
        );
        conn.request_name(&*name).await.map_err(|e| {
            assert_ne!(e, zbus::Error::NameTaken, "generated name should be unique");
            Error::Dbus(e)
        })?;
        name
    } else {
        conn.unique_name()
            .expect("unique name should be set after connected")
            .to_string()
    };

    let snw_object = StatusNotifierWatcherProxy::builder(&conn)
        // property caching internally calls DBus GetAll, which is not allowed under Snap strict confinement
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .expect("macro generated dbus Proxy should be valid");

    let register_result = snw_object.register_status_notifier_item(&name).await;
    if let Err(e) = register_result {
        let fdo_err: zbus::fdo::Error = e.into();

        if matches!(fdo_err, zbus::fdo::Error::ServiceUnknown(_)) && assume_sni_available {
            // Flag the watcher as offline, it may appear later.
            // Also ask the tray whether to continue or not
            if !service
                .lock()
                .await
                .tray
                .watcher_offline(OfflineReason::Error(Error::Watcher(fdo_err)))
            {
                // The error was handled by watcher_offline, just Ok()
                return Ok(Either::Left(async {}));
            }
        } else {
            return if let zbus::fdo::Error::ZBus(e) = fdo_err {
                Err(Error::Dbus(e))
            } else {
                Err(Error::Watcher(fdo_err))
            };
        }
    }

    // Note: both major SNI watcher implementations hardcode IsStatusNotifierHostRegistered = true
    // and never meaningfully call RegisterStatusNotifierHost, so WontShow cannot occur in practice.
    // - KDE Plasma: RegisterStatusNotifierHost is a no-op, IsStatusNotifierHostRegistered always
    //   returns true, and StatusNotifierHostRegistered is never emitted.
    //   https://github.com/KDE/plasma-workspace/blob/6112145c/statusnotifierwatcher/statusnotifierwatcher.cpp#L92-L100
    // - GNOME AppIndicator: RegisterStatusNotifierHost returns NOT_SUPPORTED, IsStatusNotifierHostRegistered
    //   always returns true, and StatusNotifierHostRegistered is emitted once in the constructor.
    //   https://github.com/ubuntu/gnome-shell-extension-appindicator/blob/f187dba/statusNotifierWatcher.js#L65
    //   https://github.com/ubuntu/gnome-shell-extension-appindicator/blob/f187dba/statusNotifierWatcher.js#L278-L280
    if !assume_sni_available
        && !snw_object
            .is_status_notifier_host_registered()
            .await
            .map_err(|e| Error::Dbus(e))?
    {
        return Err(Error::WontShow);
    }

    let dbus_object = DBusProxy::new(&conn)
        .await
        .expect("built-in Proxy should be valid");
    let mut name_changed_signal = dbus_object
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await
        .map_err(|e| Error::Dbus(e))?;

    let service_loop = async move {
        loop {
            select! {
                Some(event) = name_changed_signal.next() => {
                    let args = event.args().expect("dbus daemon should follow the specification");
                    let service = service.lock().await;
                    match args.new_owner.as_ref() {
                        Some(_new_owner) => {
                            if args.old_owner.is_none() {
                                // only call the watcher_online after the watcher really offline
                                service.tray.watcher_online();
                            }

                            if let Err(e) = snw_object.register_status_notifier_item(&name).await {
                                let fdo_err: zbus::fdo::Error = e.into();
                                let reason = if let zbus::fdo::Error::ZBus(e) = fdo_err {
                                    OfflineReason::Error(Error::Dbus(e))
                                } else {
                                    OfflineReason::Error(Error::Watcher(fdo_err))
                                };
                                if !service.tray.watcher_offline(reason) {
                                    let _ = conn.close().await;
                                    break;
                                }
                            }
                            // No need to check is_status_notifier_host_registered here:
                            // real watcher implementations always return true for it (see comment above).
                        }
                        None => {
                            if !service.tray.watcher_offline(OfflineReason::No) {
                                let _ = conn.close().await;
                                break;
                            }
                        }
                    }
                }
                Some(msg) = handle_rx.recv() => {
                    match msg {
                        HandleReuest::Update(singal) => {
                            let mut service = service.lock().await;
                            let _ = service.update(&conn).await;
                            let _ = singal.send(());
                        }
                        HandleReuest::Shutdown(singal) => {
                            let _ = conn.close().await;
                            let _ = singal.send(());
                            break;
                        }
                    }
                }
            }
        }
    };
    Ok(Either::Right(service_loop))
}

pub(crate) struct Service<T> {
    pub tray: T,
    flattened_menu: Vec<(menu::RawMenuItem<T>, Vec<usize>)>,
    prop_monitor: PropertiesMonitor,
    item_id_offset: i32,
    pub revision: u32,
}

impl<T: Tray> Service<T> {
    pub fn new(tray: T) -> Arc<Mutex<Self>> {
        let flattened_menu = menu::menu_flatten(T::menu(&tray));
        let prop_monitor = PropertiesMonitor::new(&tray);
        Arc::new(Mutex::new(Service {
            tray,
            flattened_menu,
            prop_monitor,
            item_id_offset: 0,
            revision: 0,
        }))
    }

    async fn update_properties(&mut self, conn: &Connection) -> zbus::Result<()> {
        let sni_obj = conn
            .object_server()
            .interface::<_, StatusNotifierItem<T>>(SNI_PATH)
            .await?;
        let menu_obj = conn
            .object_server()
            .interface::<_, DbusMenu<T>>(MENU_PATH)
            .await?;

        let mut sni_changed: HashMap<&str, Value> = HashMap::new();
        let mut menu_changed: HashMap<&str, Value> = HashMap::new();

        if self.text_direction_changed() {
            menu_changed.insert("TextDirection", self.get_text_direction().into());
        }

        if self.status_changed() {
            StatusNotifierItem::<T>::new_status(
                sni_obj.signal_emitter(),
                &self.get_status().to_string(),
            )
            .await?;
            menu_changed.insert("Status", self.get_status().to_menu_status().into());
        }

        if self.icon_theme_path_changed() {
            sni_changed.insert("IconThemePath", self.get_icon_theme_path().into());
            menu_changed.insert("IconThemePath", vec![self.get_icon_theme_path()].into());
        }

        if self.category_changed() {
            sni_changed.insert("Category", self.get_category().into());
        }

        if self.window_id_changed() {
            sni_changed.insert("WindowId", self.get_window_id().into());
        }

        // TODO: assert the id is consistent

        if self.title_changed() {
            StatusNotifierItem::<T>::new_title(sni_obj.signal_emitter()).await?;
        }
        if self.icon_name_changed() || self.icon_pixmap_changed() {
            StatusNotifierItem::<T>::new_icon(sni_obj.signal_emitter()).await?;
        }
        if self.overlay_icon_name_changed() || self.overlay_icon_pixmap_changed() {
            StatusNotifierItem::<T>::new_overlay_icon(sni_obj.signal_emitter()).await?;
        }
        if self.attention_icon_name_changed()
            || self.attention_icon_pixmap_changed()
            || self.attention_movie_name_changed()
        {
            StatusNotifierItem::<T>::new_attention_icon(sni_obj.signal_emitter()).await?;
        }
        if self.tool_tip_changed() {
            StatusNotifierItem::<T>::new_tool_tip(sni_obj.signal_emitter()).await?;
        }

        if !sni_changed.is_empty() {
            zbus::fdo::Properties::properties_changed(
                sni_obj.signal_emitter(),
                SNI_INTERFACE,
                sni_changed,
                Cow::Borrowed(&[]),
            )
            .await?;
        }
        if !menu_changed.is_empty() {
            zbus::fdo::Properties::properties_changed(
                menu_obj.signal_emitter(),
                MENU_INTERFACE,
                menu_changed,
                Cow::Borrowed(&[]),
            )
            .await?;
        }

        Ok(())
    }

    async fn update_menu(&mut self, conn: &Connection) -> zbus::Result<()> {
        let new_menu = menu::menu_flatten(self.tray.menu());
        let mut all_updated_props = Vec::new();
        let mut all_removed_props = Vec::new();
        let default = crate::menu::RawMenuItem::default();
        let mut layout_updated = false;
        for (index, (old, new)) in self
            .flattened_menu
            .iter()
            .chain(std::iter::repeat(&(default, vec![])))
            .zip(new_menu.iter())
            .enumerate()
        {
            let (old_item, old_childs) = old;
            let (new_item, new_childs) = new;

            if let Some((updated_props, removed_props)) = old_item.diff(new_item) {
                if !updated_props.is_empty() {
                    all_updated_props.push((self.index2id(index), updated_props));
                }
                if !removed_props.is_empty() {
                    all_removed_props.push((self.index2id(index), removed_props));
                }
            }
            if old_childs != new_childs {
                layout_updated = true;
                break;
            }
        }

        let menu_obj = conn
            .object_server()
            .interface::<_, DbusMenu<T>>(MENU_PATH)
            .await?;
        if layout_updated {
            // The layout has been changed, bump ID offset to invalidate all items,
            // which is required to avoid unexpected behaviors on some system tray
            self.revision += 1;
            self.item_id_offset += self.flattened_menu.len() as i32;
            DbusMenu::<T>::layout_updated(menu_obj.signal_emitter(), self.revision, 0).await?;
        } else if !all_updated_props.is_empty() || !all_removed_props.is_empty() {
            DbusMenu::<T>::items_properties_updated(
                menu_obj.signal_emitter(),
                all_updated_props,
                all_removed_props,
            )
            .await?;
        }
        // Always update menu_cache since `on_clicked` can be updated
        // and we can not detect that
        self.flattened_menu = new_menu;
        Ok(())
    }

    async fn update(&mut self, conn: &Connection) -> zbus::Result<()> {
        self.update_properties(&conn).await?;
        self.update_menu(&conn).await
    }

    // Return None if item not exists
    fn id2index(&self, id: i32) -> Option<usize> {
        let number_of_items = self.flattened_menu.len();
        let offset = self.item_id_offset;
        if id == 0 && number_of_items > 0 {
            // ID of the root item is always 0
            return Some(0);
        } else if id <= offset {
            // == illegal id, bug in index2id or dbus peer
            //  < expired id
            return None;
        }
        let index: usize = (id - offset).try_into().expect("unreachable!");
        if index < number_of_items {
            Some(index)
        } else {
            None
        }
    }

    fn index2id(&self, index: usize) -> i32 {
        // ID of the root item is always 0
        if index == 0 {
            0
        } else {
            <i32 as TryFrom<_>>::try_from(index)
                .expect("index overflow")
                .checked_add(self.item_id_offset)
                .expect("id overflow")
        }
    }
}

// dbus methods
impl<T: Tray> Service<T> {
    /// Build a menu tree from flattened menu
    /// Return None if parent_id not found
    pub fn build_layout(
        &self,
        parent_id: i32,
        recursion_depth: Option<usize>,
        property_filter: Vec<String>,
    ) -> Option<Layout> {
        let root = self.id2index(parent_id)?;

        let mut stack = vec![(root, 0, false)];
        let mut pending_children: Vec<OwnedValue> = Vec::new();

        while let Some((index, depth, all_childs_processed)) = stack.pop() {
            let (item, child_idxs) = &self.flattened_menu[index];

            let reach_limit = recursion_depth.is_some_and(|limit| depth >= limit);

            if all_childs_processed {
                let child_count = if reach_limit { 0 } else { child_idxs.len() };
                let children = pending_children.split_off(pending_children.len() - child_count);

                let layout = Layout {
                    id: self.index2id(index),
                    properties: item.to_dbus_map(&property_filter, !child_idxs.is_empty()),
                    children,
                };

                if index == root {
                    // DONE
                    return Some(layout);
                }

                pending_children.push(
                    layout
                        .try_into()
                        .expect("Layout should convert to OwnedValue"),
                );
            } else {
                stack.push((index, depth, true));

                if !reach_limit {
                    stack.extend(
                        child_idxs
                            .iter()
                            .rev() // because pop
                            .map(|&child_idx| (child_idx, depth + 1, false)),
                    );
                }
            }
        }

        unreachable!("the root item should be processed at the end of loop");
    }

    pub fn get_all_item(
        &self,
        property_filter: &[String],
    ) -> Vec<(i32, HashMap<Cow<'static, str>, OwnedValue>)> {
        self.flattened_menu
            .iter()
            .enumerate()
            .map(|(i, (item, children))| {
                (
                    i as i32,
                    item.to_dbus_map(property_filter, !children.is_empty()),
                )
            })
            .collect()
    }

    pub fn get_menu_item(
        &self,
        id: i32,
        property_filter: &[String],
    ) -> Option<HashMap<Cow<'static, str>, OwnedValue>> {
        self.id2index(id).map(|index| {
            let (item, children) = &self.flattened_menu[index];
            item.to_dbus_map(property_filter, !children.is_empty())
        })
    }

    pub async fn event(
        &mut self,
        conn: &Connection,
        do_update: bool,
        id: i32,
        event_id: &str,
        _data: OwnedValue,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        match event_id {
            "clicked" => {
                if id == 0 {
                    return Err(zbus::fdo::Error::InvalidArgs(
                        "root menu item is not clickable".to_string(),
                    ));
                }
                let index = self
                    .id2index(id)
                    .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".to_string()))?;
                (self.flattened_menu[index].0.on_clicked)(&mut self.tray, index);
                if do_update {
                    self.update(&conn).await?;
                }
            }
            _ => (),
        }
        Ok(())
    }

    pub async fn call_activate(&mut self, conn: &Connection, x: i32, y: i32) {
        self.tray.activate(x, y);
        let _ = self.update(conn).await;
    }

    pub async fn call_secondary_activate(&mut self, conn: &Connection, x: i32, y: i32) {
        self.tray.secondary_activate(x, y);
        let _ = self.update(conn).await;
    }

    pub async fn call_scroll(
        &mut self,
        conn: &Connection,
        delta: i32,
        orientation: crate::Orientation,
    ) {
        self.tray.scroll(delta, orientation);
        let _ = self.update(conn).await;
    }
}

macro_rules! def_properties_monitor {
    ($( $name:ident : $type:path ),+) => {
        struct PropertiesMonitor {
            $($name: AtomicU64),*
        }

        impl PropertiesMonitor {
            fn new<T: Tray>(tray: &T) -> Self {
                Self {
                    $($name: AtomicU64::new(hash_of(tray.$name()))),*
                }
            }
        }
        impl<T: Tray> Service<T> {
            paste! {
                $(
                    /// generated by def_properties_monitor
                    pub fn [<$name _changed>](&self) -> bool {
                        let new = hash_of(self.tray.$name());
                        // TODO: Relaxed should be fine
                        let old = self.prop_monitor.$name.swap(new, Ordering::AcqRel);
                        new != old
                    }
                    /// generated by def_properties_monitor
                    pub fn [<get_ $name>](&self) -> $type {
                        let r = self.tray.$name();
                        self.prop_monitor.$name.store(
                            hash_of(self.tray.$name()),
                            Ordering::Release,
                        );
                        r
                    }
                )*
            }
        }
    }
}

// dbus properties monitor, tracks hash of properties
def_properties_monitor! {
    category: crate::Category,
    title: String,
    status: crate::Status,
    window_id: i32,
    icon_theme_path: String,
    icon_name: String,
    icon_pixmap: Vec<crate::Icon>,
    overlay_icon_name: String,
    overlay_icon_pixmap: Vec<crate::Icon>,
    attention_icon_name: String,
    attention_icon_pixmap: Vec<crate::Icon>,
    attention_movie_name: String,
    tool_tip: crate::ToolTip,
    text_direction: crate::TextDirection
}

impl<T: Tray> Service<T> {
    // skip PropertiesMonitor,
    // id is a const property in Service lifetime
    pub fn get_id(&self) -> String {
        self.tray.id()
    }
}

fn hash_of<T: Hash>(v: T) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    v.hash(&mut hasher);
    hasher.finish()
}

#[cfg(all(test, any(feature = "tokio", feature = "async-io")))]
mod tests {
    use std::{borrow::Cow, collections::HashMap, sync::Arc};

    use super::Service;
    use crate::{menu::StandardItem, Tray};
    #[cfg(feature = "async-io")]
    use macro_rules_attribute::apply;
    #[cfg(feature = "async-io")]
    use smol_macros::test;
    use zbus::zvariant::OwnedValue;

    #[derive(Clone, Default)]
    struct TestTray;

    impl Tray for TestTray {
        fn id(&self) -> String {
            "test-tray".into()
        }

        fn menu(&self) -> Vec<crate::MenuItem<Self>> {
            vec![
                crate::menu::SubMenu {
                    label: "root-submenu".into(),
                    submenu: vec![
                        crate::menu::SubMenu {
                            label: "nested-submenu".into(),
                            submenu: vec![StandardItem {
                                label: "deep-item".into(),
                                ..Default::default()
                            }
                            .into()],
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "nested-item".into(),
                            ..Default::default()
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "item".into(),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    #[cfg(feature = "tokio")]
    fn blocking_lock_service<'a>(
        service: &'a Arc<crate::compat::Mutex<Service<TestTray>>>,
    ) -> tokio::sync::MutexGuard<'a, Service<TestTray>> {
        service.blocking_lock()
    }

    #[cfg(feature = "async-io")]
    fn blocking_lock_service<'a>(
        service: &'a Arc<crate::compat::Mutex<Service<TestTray>>>,
    ) -> async_lock::MutexGuard<'a, Service<TestTray>> {
        async_io::block_on(service.lock())
    }

    fn layout_children(
        layout: &crate::dbus_interface::Layout,
    ) -> Vec<crate::dbus_interface::Layout> {
        layout
            .children
            .iter()
            .cloned()
            .map(|value| value.try_into().unwrap())
            .collect()
    }

    fn get_string(properties: &HashMap<Cow<'static, str>, OwnedValue>, key: &str) -> String {
        properties
            .get(key)
            .unwrap_or_else(|| panic!("missing property: {key}"))
            .clone()
            .try_into()
            .unwrap()
    }

    fn find_layout_by_label(
        layout: &crate::dbus_interface::Layout,
        label: &str,
    ) -> Option<crate::dbus_interface::Layout> {
        if layout
            .properties
            .get("label")
            .and_then(|value| value.clone().try_into().ok())
            == Some(label.to_string())
        {
            return Some(crate::dbus_interface::Layout {
                id: layout.id,
                properties: layout.properties.clone(),
                children: layout.children.clone(),
            });
        }

        for child in layout_children(layout) {
            if let Some(found) = find_layout_by_label(&child, label) {
                return Some(found);
            }
        }

        None
    }

    #[test]
    fn build_layout_with_zero_recursion_keeps_children_hidden() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let layout = service
            .build_layout(0, Some(0), Vec::new())
            .expect("root layout should exist");

        assert_eq!(layout.id, 0);
        assert_eq!(
            get_string(&layout.properties, "children-display"),
            "submenu"
        );
        assert!(
            layout.children.is_empty(),
            "recursionDepth=0 must not include children"
        );
    }

    #[test]
    fn build_layout_respects_positive_recursion_depth() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let layout = service
            .build_layout(0, Some(1), Vec::new())
            .expect("root layout should exist");
        let root_children = layout_children(&layout);

        assert_eq!(root_children.len(), 2);
        assert_eq!(
            get_string(&root_children[0].properties, "label"),
            "root-submenu"
        );
        assert_eq!(get_string(&root_children[1].properties, "label"), "item");
        assert_eq!(
            get_string(&root_children[0].properties, "children-display"),
            "submenu"
        );
        assert!(
            root_children[0].children.is_empty(),
            "recursionDepth=1 should not include grandchildren"
        );
    }

    #[test]
    fn build_layout_without_recursion_limit_includes_full_subtree_in_order() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let layout = service
            .build_layout(0, None, vec!["label".into(), "children-display".into()])
            .expect("root layout should exist");
        let root_children = layout_children(&layout);

        assert_eq!(root_children.len(), 2);
        assert_eq!(
            get_string(&root_children[0].properties, "label"),
            "root-submenu"
        );
        assert_eq!(get_string(&root_children[1].properties, "label"), "item");

        let submenu_children = layout_children(&root_children[0]);
        assert_eq!(submenu_children.len(), 2);
        assert_eq!(
            get_string(&submenu_children[0].properties, "label"),
            "nested-submenu"
        );
        assert_eq!(
            get_string(&submenu_children[1].properties, "label"),
            "nested-item"
        );
        assert_eq!(
            get_string(&submenu_children[0].properties, "children-display"),
            "submenu"
        );

        let deep_children = layout_children(&submenu_children[0]);
        assert_eq!(deep_children.len(), 1);
        assert_eq!(
            get_string(&deep_children[0].properties, "label"),
            "deep-item"
        );
        assert!(
            !deep_children[0].properties.contains_key("children-display"),
            "leaf items must not expose children-display"
        );
    }

    #[test]
    fn build_layout_for_submenu_parent_returns_its_subtree() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let root_layout = service
            .build_layout(0, None, vec!["label".into(), "children-display".into()])
            .expect("root layout should exist");
        let root_submenu =
            find_layout_by_label(&root_layout, "root-submenu").expect("root submenu should exist");

        let layout = service
            .build_layout(
                root_submenu.id,
                None,
                vec!["label".into(), "children-display".into()],
            )
            .expect("submenu layout should exist");
        let subtree_children = layout_children(&layout);

        assert_eq!(layout.id, root_submenu.id);
        assert_eq!(get_string(&layout.properties, "label"), "root-submenu");
        assert_eq!(
            get_string(&layout.properties, "children-display"),
            "submenu"
        );
        assert_eq!(subtree_children.len(), 2);
        assert_eq!(
            get_string(&subtree_children[0].properties, "label"),
            "nested-submenu"
        );
        assert_eq!(
            get_string(&subtree_children[1].properties, "label"),
            "nested-item"
        );
    }

    #[test]
    fn build_layout_returns_none_for_unknown_parent_id() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        assert!(service.build_layout(999, None, Vec::new()).is_none());
    }

    #[test]
    fn build_layout_preserves_children_display_when_properties_are_filtered() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let layout = service
            .build_layout(0, None, vec!["children-display".into()])
            .expect("root layout should exist");
        let root_children = layout_children(&layout);

        assert_eq!(layout.properties.len(), 1);
        assert_eq!(
            get_string(&layout.properties, "children-display"),
            "submenu"
        );
        assert_eq!(root_children[0].properties.len(), 1);
        assert_eq!(
            get_string(&root_children[0].properties, "children-display"),
            "submenu"
        );
        assert!(root_children[1].properties.is_empty());

        let submenu_children = layout_children(&root_children[0]);
        assert_eq!(submenu_children[0].properties.len(), 1);
        assert_eq!(
            get_string(&submenu_children[0].properties, "children-display"),
            "submenu"
        );
        assert!(submenu_children[1].properties.is_empty());
        assert!(layout_children(&submenu_children[0])[0]
            .properties
            .is_empty());
    }

    #[test]
    fn build_layout_excludes_children_display_when_not_in_filter() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let layout = service
            .build_layout(0, None, vec!["label".into()])
            .expect("root layout should exist");
        let root_children = layout_children(&layout);

        assert!(
            !layout.properties.contains_key("children-display"),
            "root must not have children-display when not in filter"
        );
        assert!(
            !root_children[0].properties.contains_key("children-display"),
            "submenu must not have children-display when not in filter"
        );
        assert_eq!(
            get_string(&root_children[0].properties, "label"),
            "root-submenu"
        );
        assert_eq!(root_children[0].properties.len(), 1);
    }

    #[test]
    fn get_menu_item_returns_none_for_unknown_id() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        assert!(service.get_menu_item(999, &[]).is_none());
    }

    #[test]
    fn get_menu_item_with_empty_filter_returns_all_properties() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let root_layout = service
            .build_layout(0, None, vec!["label".into(), "children-display".into()])
            .expect("root layout should exist");
        let root_submenu =
            find_layout_by_label(&root_layout, "root-submenu").expect("root-submenu should exist");

        let properties = service
            .get_menu_item(root_submenu.id, &[])
            .expect("root-submenu should exist");

        assert_eq!(get_string(&properties, "label"), "root-submenu");
        assert_eq!(get_string(&properties, "children-display"), "submenu");
    }

    #[test]
    fn get_menu_item_filter_excludes_unspecified_properties() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let root_layout = service
            .build_layout(0, None, vec!["label".into(), "children-display".into()])
            .expect("root layout should exist");
        let root_submenu =
            find_layout_by_label(&root_layout, "root-submenu").expect("root-submenu should exist");

        let properties = service
            .get_menu_item(root_submenu.id, &["label".to_string()])
            .expect("root-submenu should exist");

        assert_eq!(properties.len(), 1);
        assert_eq!(get_string(&properties, "label"), "root-submenu");
        assert!(
            !properties.contains_key("children-display"),
            "children-display must not appear when not in filter"
        );
    }

    #[test]
    fn get_menu_item_includes_children_display_when_in_filter() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let root_layout = service
            .build_layout(0, None, vec!["label".into(), "children-display".into()])
            .expect("root layout should exist");
        let root_submenu =
            find_layout_by_label(&root_layout, "root-submenu").expect("root-submenu should exist");

        let properties = service
            .get_menu_item(root_submenu.id, &["children-display".to_string()])
            .expect("root-submenu should exist");

        assert_eq!(properties.len(), 1);
        assert_eq!(get_string(&properties, "children-display"), "submenu");
    }

    #[test]
    fn get_menu_item_leaf_item_never_has_children_display() {
        let service = Service::new(TestTray);
        let service = blocking_lock_service(&service);

        let root_layout = service
            .build_layout(0, None, vec!["label".into(), "children-display".into()])
            .expect("root layout should exist");
        let item = find_layout_by_label(&root_layout, "item").expect("item should exist");

        let properties = service
            .get_menu_item(item.id, &[])
            .expect("item should exist");

        assert!(
            !properties.contains_key("children-display"),
            "leaf items must never have children-display"
        );
    }

    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn assert_root_menu_clicked_returns_invalid_args() {
        let service = Service::new(TestTray);
        let conn = zbus::Connection::session().await.unwrap();
        let mut service = service.lock().await;

        let err = service
            .event(&conn, false, 0, "clicked", OwnedValue::from(0_u8), 0)
            .await
            .expect_err("root menu clicked should return InvalidArgs");

        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
    }
}
