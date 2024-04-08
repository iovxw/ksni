use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use paste::paste;
use zbus::fdo::DBusProxy;
use zbus::zvariant::{OwnedValue, Str};
use zbus::Connection;

use crate::compat::{mpsc, Mutex};
use crate::dbus_interface::{
    DbusMenu, Layout, StatusNotifierItem, StatusNotifierWatcherProxy, MENU_PATH, SNI_PATH,
};

use crate::compat::select;
use crate::menu;
use crate::{ClientRequest, Handle, Tray};

static COUNTER: AtomicUsize = AtomicUsize::new(1);

// TODO: don't use zbus result publicly(?)
pub fn spawn<T: Tray + Send + 'static>(tray: T) -> zbus::Result<Handle<T>> {
    let (client_tx, client_rx) = mpsc::unbounded_channel::<ClientRequest<T>>();
    std::thread::Builder::new()
        .name("ksni-tokio".into())
        .spawn(move || {
            #[cfg(feature = "tokio")]
            {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio::new_current_thread()");
                rt.block_on(async move {
                    let _ = run_async(tray, client_rx).await;
                });
            }
            #[cfg(feature = "async-io")]
            {
                let ex = async_executor::LocalExecutor::new();
                futures_lite::future::block_on(ex.run(async move {
                    let _ = run_async(tray, client_rx).await;
                }));
            }
        })
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;

    Ok(Handle { sender: client_tx })
}

pub async fn run_async<T: Tray + Send + 'static>(
    tray: T,
    mut client_rx: mpsc::UnboundedReceiver<ClientRequest<T>>,
) -> zbus::Result<()> {
    let flattened_menu = menu::menu_flatten(T::menu(&tray));
    let prop_monitor = PropertiesMonitor::new(&tray);
    let service = Arc::new(Mutex::new(Service {
        tray,
        flattened_menu,
        prop_monitor,
        item_id_offset: 0,
        revision: 0,
    }));
    let name = format!(
        "org.kde.StatusNotifierItem-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::AcqRel)
    );

    let sni_obj = StatusNotifierItem::new(service.clone());
    let menu_obj = DbusMenu::new(service.clone());

    // for those `expect`, see: https://github.com/dbus2/zbus/issues/403
    let conn = zbus::connection::Builder::session()?
        .name(&*name)
        .expect("validity of name should be ensured by developer")
        .serve_at(SNI_PATH, sni_obj)
        .expect("SNI_PATH should be valid")
        .serve_at(MENU_PATH, menu_obj)
        .expect("MENU_PATH should be valid")
        .build()
        .await?;

    let snw_object = StatusNotifierWatcherProxy::new(&conn)
        .await
        .expect("macro generated dbus Proxy should be valid");

    match snw_object
        .register_status_notifier_item(&name)
        .await
        .map_err(|e| e.into())
    {
        Ok(()) => service.lock().await.tray.watcher_online(),
        Err(zbus::fdo::Error::ServiceUnknown(_)) => {
            if !service.lock().await.tray.watcher_offline() {
                return Ok(());
            }
        }
        Err(e) => return Err(e.into()),
    }

    let dbus_object = DBusProxy::new(&conn)
        .await
        .expect("built-in Proxy should be valid");
    let mut name_changed_signal = dbus_object
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await?;

    loop {
        select! {
            Some(event) = name_changed_signal.next() => {
                let service = service.lock().await;
                match event
                    .args()
                    .expect("dbus daemon should follow the specification")
                    .new_owner()
                    .as_ref()
                {
                    Some(_new_owner) => {
                        snw_object.register_status_notifier_item(&name).await?;
                        service.tray.watcher_online();
                    }
                    None => {
                        if !service.tray.watcher_offline() {
                            break Ok(());
                        }
                    }
                }
            }
            Some(msg) = client_rx.recv() => {
                match msg {
                    ClientRequest::Update(cb) => {
                        let mut service = service.lock().await;
                        cb(&mut service.tray);
                        let _ = service.update(&conn).await;
                    }
                    ClientRequest::Shutdown => {
                        break Ok(());
                    }
                }
            }
        }
    }
}

pub(crate) struct Service<T> {
    tray: T,
    flattened_menu: Vec<(menu::RawMenuItem<T>, Vec<usize>)>,
    prop_monitor: PropertiesMonitor,
    item_id_offset: i32,
    pub revision: u32,
}

impl<T: Tray + Send + 'static> Service<T> {
    async fn update_properties(&mut self, conn: &Connection) -> zbus::Result<()> {
        let sni_obj = conn
            .object_server()
            .interface::<_, StatusNotifierItem<T>>(SNI_PATH)
            .await?;
        let menu_obj = conn
            .object_server()
            .interface::<_, DbusMenu<T>>(MENU_PATH)
            .await?;

        if self.text_direction_changed() {
            menu_obj
                .get_mut()
                .await
                .text_direction_changed(menu_obj.signal_context())
                .await?;
        }

        if self.status_changed() {
            StatusNotifierItem::<T>::new_status(
                sni_obj.signal_context(),
                &self.get_status().to_string(),
            )
            .await?;
            menu_obj
                .get_mut()
                .await
                .status_changed(menu_obj.signal_context())
                .await?;
        }

        if self.icon_theme_path_changed() {
            sni_obj
                .get_mut()
                .await
                .icon_theme_path_changed(sni_obj.signal_context())
                .await?;
            menu_obj
                .get_mut()
                .await
                .icon_theme_path_changed(menu_obj.signal_context())
                .await?;
        }

        if self.category_changed() {
            sni_obj
                .get_mut()
                .await
                .category_changed(sni_obj.signal_context())
                .await?;
        }

        if self.window_id_changed() {
            sni_obj
                .get_mut()
                .await
                .window_id_changed(sni_obj.signal_context())
                .await?;
        }

        // TODO: assert the id is consistent

        if self.title_changed() {
            StatusNotifierItem::<T>::new_title(sni_obj.signal_context()).await?;
        }
        if self.icon_name_changed() || self.icon_pixmap_changed() {
            StatusNotifierItem::<T>::new_icon(sni_obj.signal_context()).await?;
        }
        if self.overlay_icon_name_changed() || self.overlay_icon_pixmap_changed() {
            StatusNotifierItem::<T>::new_overlay_icon(sni_obj.signal_context()).await?;
        }
        if self.attention_icon_name_changed()
            || self.attention_icon_pixmap_changed()
            || self.attention_movie_name_changed()
        {
            StatusNotifierItem::<T>::new_attention_icon(sni_obj.signal_context()).await?;
        }
        if self.tool_tip_changed() {
            StatusNotifierItem::<T>::new_tool_tip(sni_obj.signal_context()).await?;
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
            DbusMenu::<T>::layout_updated(menu_obj.signal_context(), self.revision, 0).await?;
        } else if !all_updated_props.is_empty() || !all_removed_props.is_empty() {
            DbusMenu::<T>::items_properties_updated(
                menu_obj.signal_context(),
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
impl<T: Tray + Send + 'static> Service<T> {
    /// Build a menu tree from flattened menu
    /// Return None if parent_id not found
    pub fn build_layout(
        &self,
        parent_id: i32,
        recursion_depth: Option<usize>,
        property_names: Vec<String>,
    ) -> Option<Layout> {
        let root = self.id2index(parent_id)?;

        let mut items: Vec<Option<(Layout, Vec<usize>)>> = self
            .flattened_menu
            .iter()
            .enumerate()
            .map(|(index, (item, submenu))| {
                (
                    Layout {
                        id: self.index2id(index),
                        properties: item.to_dbus_map(&property_names),
                        children: Vec::with_capacity(submenu.len()),
                    },
                    submenu.clone(),
                )
            })
            .map(Some)
            .collect();
        let mut stack = vec![root];

        // depth first
        while let Some(current) = stack.pop() {
            let (layout, pending_children) = &mut items[current]
                .as_mut()
                .expect("stack pointer should always point to a valid item");
            if pending_children.is_empty() {
                if !layout.children.is_empty() {
                    layout.properties.insert(
                        "children-display".into(),
                        Str::from_static("submenu").into(),
                    );
                }
                // if there's a parent, move current to parent's children
                if let Some(parent) = stack.pop() {
                    let current = std::mem::replace(&mut items[current], None);
                    let layout = current.expect("should have been unwrapped once already").0;
                    stack.push(parent);
                    items[parent]
                        .as_mut()
                        .unwrap()
                        .0
                        .children
                        .push(layout.try_into().expect(
                            "Layout should not contain anything that can not be formatted as Value",
                        ));
                }
            } else {
                stack.push(current);
                let child = pending_children.remove(0);
                if recursion_depth.map_or(true, |depth| depth + 1 >= stack.len()) {
                    stack.push(child);
                }
            }
        }
        let root_item = items.remove(root)?;
        Some(root_item.0)
    }

    pub fn get_menu_item(
        &self,
        id: i32,
        property_filter: &[String],
    ) -> Option<HashMap<String, OwnedValue>> {
        self.id2index(id)
            .map(|index| self.flattened_menu[index].0.to_dbus_map(property_filter))
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
                assert_ne!(id, 0, "ROOT MENU ITEM CLICKED");
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

    pub async fn call_scroll(&mut self, conn: &Connection, delta: i32, dir: &str) {
        self.tray.scroll(delta, dir);
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
        impl<T: Tray + Send + 'static> Service<T> {
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

impl<T: Tray + Send + 'static> Service<T> {
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
