use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::stream::StreamExt;
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::zvariant::{Value, OwnedValue, Str};

use crate::dbus_interface::{
    SNI_PATH, MENU_PATH,
    StatusNotifierWatcherProxy,
    StatusNotifierItem, SniMessage, SniProperty,
    DbusMenu, DbusMenuMessage, DbusMenuProperty,
    LayoutItem,
};

use crate::menu;
use crate::tray;
use crate::{Handle, Tray, ClientRequest};

static COUNTER: AtomicUsize = AtomicUsize::new(1);

// TODO: don't use zbus result publicly(?)
pub fn spawn<T: Tray + Send + 'static>(tray: T) -> zbus::Result<Handle<T>> {
    let (client_tx, client_rx) = tokio::sync::mpsc::unbounded_channel::<ClientRequest<T>>();
    std::thread::Builder::new()
        .name("ksni-tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio::new_current_thread()");
            rt.block_on(async move { let _ = run_async(tray, client_rx).await; });
        })
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;

    Ok(Handle { sender: client_tx })
}

pub async fn run_async<T: Tray + Send + 'static>(tray: T, mut client_rx: tokio::sync::mpsc::UnboundedReceiver<ClientRequest<T>>) -> zbus::Result<()> {
    let conn = Connection::session().await.unwrap();
    let name = format!(
        "org.kde.StatusNotifierItem-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::AcqRel)
    );

    let (sni_obj, mut sni_rx) = StatusNotifierItem::new();
    let (menu_obj, mut menu_rx) = DbusMenu::new();

    conn.object_server()
        .at(SNI_PATH, sni_obj)
        .await?;
    conn.object_server()
        .at(MENU_PATH, menu_obj)
        .await?;
    conn.request_name(name.as_str()).await?;

    let snw_object = StatusNotifierWatcherProxy::new(&conn).await?;
    let register_result = snw_object.register_status_notifier_item(&name).await;
    if let Err(zbus::Error::FDO(err)) = &register_result {
        if let zbus::fdo::Error::ServiceUnknown(_) = **err {
            if !tray.watcher_offline() {
                return Ok(());
            }
        } else {
            register_result?;
        }
    } else {
        tray.watcher_online();
    }

    let dbus_object = DBusProxy::new(&conn).await?;
    let mut name_changed_signal = dbus_object
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await?;

    let menu_cache = menu::menu_flatten(T::menu(&tray));
    let prop_cache = PropertiesCache::new(&tray);
    let mut service = Service {
        conn,
        tray,
        menu_cache,
        prop_cache,
        item_id_offset: 0,
        revision: 0,
    };
    loop {
        tokio::select! {
            Some(event) = name_changed_signal.next() => {
                if let Ok(args) = event.args() {
                    match args.new_owner().as_ref() {
                        Some(_new_owner) => {
                            service.tray.watcher_online();
                            let _ = snw_object.register_status_notifier_item(&name).await;
                        }
                        None => {
                            if !service.tray.watcher_offline() {
                                break Ok(());
                            }
                        }
                    }
                }
            }
            Some(msg) = client_rx.recv() => {
                match msg {
                    ClientRequest::Update(cb) => {
                        cb(&mut service.tray);
                        let _ = service.update().await;
                    }
                    ClientRequest::Shutdown => {
                        break Ok(());
                    }
                }
            }
            Some(msg) = sni_rx.recv() => {
                match msg {
                    SniMessage::Activate(x, y) => {
                        service.tray.activate(x, y);
                        let _ = service.update().await;
                    }
                    SniMessage::SecondaryActivate(x, y) => {
                        service.tray.secondary_activate(x, y);
                        let _ = service.update().await;
                    }
                    SniMessage::Scroll(delta, dir) => {
                        service.tray.scroll(delta, &dir);
                        let _ = service.update().await;
                    }
                    SniMessage::GetDbusProperty(prop) => match prop {
                        SniProperty::Category(r) => {
                            let _ = r.send(Ok(service.tray.category().to_string()));
                        }
                        SniProperty::Id(r) => {
                            let _ = r.send(Ok(service.tray.id()));
                        }
                        SniProperty::Title(r) => {
                            let _ = r.send(Ok(service.tray.title()));
                        }
                        SniProperty::Status(r) => {
                            let _ = r.send(Ok(service.tray.status().to_string()));
                        }
                        SniProperty::WindowId(r) => {
                            let _ = r.send(Ok(service.tray.window_id()));
                        }
                        SniProperty::IconThemePath(r) => {
                            let _ = r.send(Ok(service.tray.icon_theme_path()));
                        }
                        SniProperty::IconName(r) => {
                            let _ = r.send(Ok(service.tray.icon_name()));
                        }
                        SniProperty::IconPixmap(r) => {
                            let _ = r.send(Ok(service.tray.icon_pixmap()));
                        }
                        SniProperty::OverlayIconName(r) => {
                            let _ = r.send(Ok(service.tray.overlay_icon_name()));
                        }
                        SniProperty::OverlayIconPixmap(r) => {
                            let _ = r.send(Ok(service.tray.overlay_icon_pixmap()));
                        }
                        SniProperty::AttentionIconName(r) => {
                            let _ = r.send(Ok(service.tray.attention_icon_name()));
                        }
                        SniProperty::AttentionIconPixmap(r) => {
                            let _ = r.send(Ok(service.tray.attention_icon_pixmap()));
                        }
                        SniProperty::AttentionMovieName(r) => {
                            let _ = r.send(Ok(service.tray.attention_movie_name()));
                        }
                        SniProperty::ToolTip(r) => {
                            let _ = r.send(Ok(service.tray.tool_tip()));
                        }
                    }
                }
            }
            Some(msg) = menu_rx.recv() => {
                match msg {
                    DbusMenuMessage::GetLayout(parent_id, recursion_depth, property_names, r) => {
                        let tree = service.gen_dbusmenu_tree(
                            parent_id,
                            if recursion_depth < 0 {
                                None
                            } else {
                                Some(recursion_depth as usize)
                            },
                            property_names,
                        );
                        let result = tree
                            .map(|tree| (service.revision, tree))
                            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("parentId not found".to_string()));
                        let _ = r.send(result);
                    }
                    DbusMenuMessage::GetGroupProperties(ids, property_names, r) => {
                        let result = ids
                            .into_iter()
                            .filter_map(|id| service.id2index(id).map(|idx| (id, idx)))
                            .map(|(id, index)| {
                                (
                                    id,
                                    service.menu_cache[index]
                                        .0
                                        .to_dbus_map(&property_names),
                                )
                            })
                            .collect();
                        let _ = r.send(Ok(result));
                    }
                    DbusMenuMessage::GetProperty(id, name, r) => {
                        let result = service.id2index(id)
                            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".to_string()))
                            .map(|index| service.menu_cache[index].0.to_dbus_map(&vec![name]))
                            .map(|value| Value::from(value).to_owned());
                        let _ = r.send(result);
                    }
                    DbusMenuMessage::Event(id, event_id, data, timestamp, r) => {
                        let _ = r.send(service.event(id, &event_id, data, timestamp).await);
                    }
                    DbusMenuMessage::EventGroup(events, r) => {
                        let _ = r.send(service.event_group(events).await);
                    }
                    DbusMenuMessage::GetDbusProperty(prop) => match prop {
                        DbusMenuProperty::TextDirection(r) => {
                            let _ = r.send(Ok(service.tray.text_direction().to_string()));
                        }
                        DbusMenuProperty::Status(r) => {
                            let status = match service.tray.status() {
                                tray::Status::Active | tray::Status::Passive => menu::Status::Normal,
                                tray::Status::NeedsAttention => menu::Status::Notice,
                            };
                            let _ = r.send(Ok(status.to_string()));
                        }
                        DbusMenuProperty::IconThemePath(r) => {
                            let path = service.tray.icon_theme_path();
                            let path = if path.is_empty() { vec![] } else { vec![path] };
                            let _ = r.send(Ok(path));
                        }
                    }
                }
            }
        }
    }
}

struct Service<T> {
    conn: Connection,
    tray: T,
    menu_cache: Vec<(menu::RawMenuItem<T>, Vec<usize>)>,
    prop_cache: PropertiesCache,
    item_id_offset: i32,
    revision: u32,
}

impl<T: Tray + Send + 'static> Service<T> {
    async fn update_properties(&mut self) -> zbus::Result<()> {
        let sni_obj  = self.conn.object_server().interface::<_, StatusNotifierItem>(SNI_PATH).await?;
        let menu_obj = self.conn.object_server().interface::<_, DbusMenu>(MENU_PATH).await?;

        let text_direction = self.prop_cache.text_direction_changed(&self.tray);
        if text_direction.is_some() {
            menu_obj.get_mut().await.text_direction_changed(menu_obj.signal_context()).await?;
        }

        let tray_status = self.prop_cache.status_changed(&self.tray);
        if let Some(tray_status) = tray_status {
            StatusNotifierItem::new_status(sni_obj.signal_context(), &tray_status.to_string()).await?;
            menu_obj.get_mut().await.status_changed(menu_obj.signal_context()).await?;
        }

        let icon_theme_path = self.prop_cache.icon_theme_path_changed(&self.tray);
        if icon_theme_path.is_some() {
            sni_obj.get_mut().await.icon_theme_path_changed(sni_obj.signal_context()).await?;
            menu_obj.get_mut().await.icon_theme_path_changed(menu_obj.signal_context()).await?;
        }

        let category = self.prop_cache.category_changed(&self.tray);
        if category.is_some() {
            sni_obj.get_mut().await.category_changed(sni_obj.signal_context()).await?;
        }

        let window_id = self.prop_cache.window_id_changed(&self.tray);
        if window_id.is_some() {
            sni_obj.get_mut().await.window_id_changed(sni_obj.signal_context()).await?;
        }

        // TODO: assert the id is consistent

        if self.prop_cache.title_changed(&self.tray) {
            StatusNotifierItem::new_title(sni_obj.signal_context()).await?;
        }
        if self.prop_cache.icon_changed(&self.tray) {
            StatusNotifierItem::new_icon(sni_obj.signal_context()).await?;
        }
        if self.prop_cache.overlay_icon_changed(&self.tray) {
            StatusNotifierItem::new_overlay_icon(sni_obj.signal_context()).await?;
        }
        if self.prop_cache.attention_icon_changed(&self.tray) {
            StatusNotifierItem::new_attention_icon(sni_obj.signal_context()).await?;
        }
        if self.prop_cache.tool_tip_changed(&self.tray) {
            StatusNotifierItem::new_tool_tip(sni_obj.signal_context()).await?;
        }
        Ok(())
    }

    async fn update_menu(&mut self) -> zbus::Result<()> {
        let new_menu = menu::menu_flatten(self.tray.menu());
        let old_menu = &self.menu_cache;
        let mut all_updated_props = Vec::new();
        let mut all_removed_props = Vec::new();
        let default = crate::menu::RawMenuItem::default();
        let mut layout_updated = false;
        for (index, (old, new)) in old_menu
            .iter()
            .chain(std::iter::repeat(&(default, vec![])))
            .zip(new_menu.clone().into_iter())
            .enumerate()
        {
            let (old_item, old_childs) = old;
            let (new_item, new_childs) = new;

            if let Some((updated_props, removed_props)) = old_item.diff(new_item) {
                if !updated_props.is_empty() {
                    all_updated_props
                        .push((self.index2id(index), updated_props));
                }
                if !removed_props.is_empty() {
                    all_removed_props
                        .push((self.index2id(index), removed_props));
                }
            }
            if *old_childs != new_childs {
                layout_updated = true;
                break;
            }
        }

        let menu_obj = self.conn.object_server().interface::<_, DbusMenu>(MENU_PATH).await?;
        if layout_updated {
            // The layout has been changed, bump ID offset to invalidate all items,
            // which is required to avoid unexpected behaviors on some system tray
            self.revision += 1;
            self.item_id_offset += old_menu.len() as i32;
            DbusMenu::layout_updated(menu_obj.signal_context(), self.revision, 0).await?;
        } else if !all_updated_props.is_empty() || !all_removed_props.is_empty() {
            DbusMenu::items_properties_updated(menu_obj.signal_context(), all_updated_props, all_removed_props).await?;
        }
        self.menu_cache = new_menu;
        Ok(())
    }

    async fn update(&mut self) -> zbus::Result<()> {
        self.update_properties().await?;
        self.update_menu().await
    }

    // Return None if item not exists
    fn id2index(&self, id: i32) -> Option<usize> {
        let number_of_items = self.menu_cache.len();
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

    fn gen_dbusmenu_tree(
        &self,
        parent_id: i32,
        recursion_depth: Option<usize>,
        property_names: Vec<String>,
    ) -> Option<LayoutItem> {
        let parent_index = self.id2index(parent_id)?;

        // The type is Vec<Option<id, properties, Vec<submenu>, Vec<submenu_index>>>
        let mut x: Vec<
            Option<(
                i32,
                HashMap<String, OwnedValue>,
                Vec<OwnedValue>,
                Vec<usize>,
            )>,
        > = self
            .menu_cache
            .iter()
            .enumerate()
            .map(|(index, (item, submenu))| {
                (
                    self.index2id(index),
                    item.to_dbus_map(&property_names),
                    Vec::with_capacity(submenu.len()),
                    submenu.clone(),
                )
            })
            .map(Some)
            .collect();
        let mut stack = vec![parent_index];

        while let Some(current) = stack.pop() {
            let submenu_indexes = &mut x[current].as_mut().unwrap().3;
            if submenu_indexes.is_empty() {
                let c = x[current].as_mut().unwrap();
                if !c.2.is_empty() {
                    c.1.insert(
                        "children-display".into(),
                        Str::from_static("submenu").into(),
                    );
                }
                if let Some(parent) = stack.pop() {
                    x.push(None);
                    let item = x.swap_remove(current).unwrap();
                    stack.push(parent);
                    x[parent]
                        .as_mut()
                        .unwrap()
                        .2
                        .push(LayoutItem {
                            id: item.0,
                            properties: item.1,
                            children: item.2
                        }.into());
                }
            } else {
                stack.push(current);
                let sub = submenu_indexes.remove(0);
                if recursion_depth.map_or(true, |depth| depth + 1 >= stack.len()) {
                    stack.push(sub);
                }
            }
        }
        let resp = x.remove(parent_index)?;
        Some(LayoutItem {
            id: resp.0,
            properties: resp.1,
            children: resp.2,
        })
    }

    async fn event(&mut self, id: i32, event_id: &str, _data: OwnedValue, _timestamp: u32) -> zbus::fdo::Result<()> {
        match event_id {
            "clicked" => {
                assert_ne!(id, 0, "ROOT MENU ITEM CLICKED");
                let index = self.id2index(id)
                    .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".to_string()))?;
                if let Ok(activate) = self.menu_cache[index].0.on_clicked.clone().lock() {
                    (activate)(&mut self.tray, index);
                }
                self.update().await?;
            }
            _ => (),
        }
        Ok(())
    }

    async fn event_group(&mut self, events: Vec<(i32, String, OwnedValue, u32)>) -> zbus::fdo::Result<Vec<i32>> {
        let (found, not_found) = events
            .into_iter()
            .partition::<Vec<_>, _>(|event| self.id2index(event.0).is_some());
        if found.is_empty() {
            return Err(zbus::fdo::Error::InvalidArgs("None of the id in the events can be found".to_string()));
        } else {
            for (id, event_id, data, timestamp) in found {
                self.event(id, &event_id, data, timestamp).await?;
            }
            Ok(not_found.into_iter().map(|event| event.0).collect())
        }
    }
}

struct PropertiesCache {
    category: crate::Category,
    title: u64,
    status: crate::Status,
    window_id: i32,
    icon_theme_path: u64,
    icon: u64,
    overlay_icon: u64,
    attention_icon: u64,
    tool_tip: u64,
    text_direction: crate::TextDirection,
}

impl PropertiesCache {
    fn new<T: Tray>(tray: &T) -> Self {
        PropertiesCache {
            category: tray.category(),
            title: hash_of(tray.title()),
            status: tray.status(),
            window_id: tray.window_id(),
            icon_theme_path: hash_of(tray.icon_theme_path()),
            icon: hash_of((tray.icon_name(), tray.icon_pixmap())),
            overlay_icon: hash_of((tray.overlay_icon_name(), tray.overlay_icon_pixmap())),
            attention_icon: hash_of((
                tray.attention_icon_name(),
                tray.attention_icon_pixmap(),
                tray.attention_movie_name(),
            )),
            tool_tip: hash_of(tray.tool_tip()),
            text_direction: tray.text_direction(),
        }
    }

    fn category_changed<T: Tray>(&mut self, t: &T) -> Option<crate::Category> {
        let v = t.category();
        if self.category != v {
            self.category = v;
            Some(v)
        } else {
            None
        }
    }

    fn title_changed<T: Tray>(&mut self, t: &T) -> bool {
        let hash = hash_of(t.title());
        self.title != hash && {
            self.title = hash;
            true
        }
    }

    fn status_changed<T: Tray>(&mut self, t: &T) -> Option<crate::Status> {
        let v = t.status();
        if self.status != v {
            self.status = v;
            Some(v)
        } else {
            None
        }
    }

    fn window_id_changed<T: Tray>(&mut self, t: &T) -> Option<i32> {
        let v = t.window_id();
        if self.window_id != v {
            self.window_id = v;
            Some(v)
        } else {
            None
        }
    }

    fn icon_theme_path_changed<T: Tray>(&mut self, t: &T) -> Option<String> {
        let v = t.icon_theme_path();
        let hash = hash_of(&v);
        if self.icon_theme_path != hash {
            self.icon_theme_path = hash;
            Some(v)
        } else {
            None
        }
    }

    fn icon_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of((tray.icon_name(), tray.icon_pixmap()));
        self.icon != hash && {
            self.icon = hash;
            true
        }
    }

    fn overlay_icon_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of((tray.overlay_icon_name(), tray.overlay_icon_pixmap()));
        self.overlay_icon != hash && {
            self.overlay_icon = hash;
            true
        }
    }

    fn attention_icon_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of((
            tray.attention_icon_name(),
            tray.attention_icon_pixmap(),
            tray.attention_movie_name(),
        ));
        self.attention_icon != hash && {
            self.attention_icon = hash;
            true
        }
    }

    fn tool_tip_changed<T: Tray>(&mut self, tray: &T) -> bool {
        let hash = hash_of(tray.tool_tip());
        self.tool_tip != hash && {
            self.tool_tip = hash;
            true
        }
    }

    fn text_direction_changed<T: Tray>(&mut self, t: &T) -> Option<crate::TextDirection> {
        let v = t.text_direction();
        if self.text_direction != v {
            self.text_direction = v;
            Some(v)
        } else {
            None
        }
    }
}

fn hash_of<T: Hash>(v: T) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    v.hash(&mut hasher);
    hasher.finish()
}
