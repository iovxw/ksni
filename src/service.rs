use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use dbus;
use dbus::arg::{RefArg, Variant};
use dbus::blocking::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::SignalArgs;
use dbus::message::{MatchRule, MessageType};

use crate::dbus_ext;
use crate::dbus_interface;
use crate::dbus_interface::{
    DbusmenuItemsPropertiesUpdated, DbusmenuLayoutUpdated, StatusNotifierItemNewAttentionIcon,
    StatusNotifierItemNewIcon, StatusNotifierItemNewOverlayIcon, StatusNotifierItemNewStatus,
    StatusNotifierItemNewTitle, StatusNotifierItemNewToolTip, StatusNotifierWatcher,
};
use crate::freedesktop;
use crate::menu;
use crate::tray;
use crate::{State, Tray};

const SNI_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

static COUNTER: AtomicUsize = AtomicUsize::new(1);

pub struct TrayService<T> {
    state: State<T>,
    state_changed: Arc<AtomicBool>,
}

impl<T: Tray + 'static> TrayService<T> {
    pub fn new(tray: T) -> Self {
        let state_changed = Arc::new(AtomicBool::new(false));
        TrayService {
            state: State {
                state_changed: state_changed.clone(),
                inner: Arc::new(Mutex::new(tray)),
            },
            state_changed,
        }
    }
    pub fn state(&self) -> State<T> {
        self.state.clone()
    }
    pub fn run(self) -> Result<(), dbus::Error> {
        let name = format!(
            "org.kde.StatusNotifierItem-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::AcqRel)
        );
        let mut conn = Connection::new_session()?;
        conn.request_name(&name, true, true, false)?;

        let (menu_cache, prop_cache) = {
            let state = self.state.inner.lock().unwrap();
            (
                RefCell::new(menu::menu_flatten(T::menu(&*state))),
                RefCell::new(PropertiesCache::new(&*state)),
            )
        };
        let inner = Rc::new(Inner {
            state: self.state,
            state_changed: self.state_changed,
            menu_cache,
            revision: Cell::new(0),
            prop_cache,
        });

        let tray_service2 = inner.clone();
        let tray_service3 = inner.clone();
        let f = dbus::tree::Factory::new_fn::<()>();
        let sni_interface = dbus_interface::status_notifier_item_server(&f, (), move |_| {
            tray_service2.clone() as Rc<dyn dbus_interface::StatusNotifierItem>
        });
        let menu_interface = dbus_interface::dbusmenu_server(&f, (), move |_| {
            tray_service3.clone() as Rc<dyn dbus_interface::Dbusmenu>
        });
        let tree = f
            .tree(())
            .add(
                f.object_path(SNI_PATH, ())
                    .introspectable()
                    .add(sni_interface),
            )
            .add(
                f.object_path(MENU_PATH, ())
                    .introspectable()
                    .add(menu_interface),
            )
            // Add root path, to help introspection from debugging tools
            .add(f.object_path("/", ()).introspectable());
        let mut rule = MatchRule::new();
        rule.msg_type = Some(MessageType::MethodCall);
        conn.start_receive(
            rule,
            Box::new(move |msg, c| {
                dbus_ext::with_conn(c, || {
                    if let Some(replies) = tree.handle(&msg) {
                        for r in replies {
                            let _ = c.send(r);
                        }
                    }
                });
                true
            }),
        );

        register_to_watcher(&conn, name)?;

        loop {
            conn.process(Duration::from_millis(50))?;
            inner.update_state();
        }
    }

    pub fn spawn(self)
    where
        T: Send,
    {
        thread::spawn(|| self.run().unwrap());
    }
}

fn register_to_watcher(conn: &Connection, name: String) -> Result<(), dbus::Error> {
    let status_notifier_watcher = conn.with_proxy(
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        Duration::from_millis(1000),
    );
    status_notifier_watcher.register_status_notifier_item(&name)?;

    status_notifier_watcher.match_signal(
        move |h: freedesktop::NameOwnerChanged, c: &Connection| {
            if h.name == "org.kde.StatusNotifierWatcher" {
                c.with_proxy(
                    "org.kde.StatusNotifierWatcher",
                    "/StatusNotifierWatcher",
                    Duration::from_millis(1000),
                )
                .register_status_notifier_item(&name)
                .unwrap_or_default();
            }
            true
        },
    )?;
    Ok(())
}

struct Inner<T: Tray> {
    state: State<T>,
    state_changed: Arc<AtomicBool>,
    // A list of menu item and it's submenu
    menu_cache: RefCell<Vec<(menu::RawMenuItem<T>, Vec<usize>)>>,
    revision: Cell<u32>,
    prop_cache: RefCell<PropertiesCache>,
}

impl<T: Tray + 'static> Inner<T> {
    fn update_state(&self) {
        if self.state_changed.swap(false, Ordering::AcqRel) {
            self.update_properties();
            self.update_menu();
        }
    }
    // TODO: macro?
    fn update_properties(&self) {
        let sni_dbus_path: dbus::Path = SNI_PATH.into();
        let inner = self.state.inner.lock().unwrap();
        let mut prop_cache = self.prop_cache.borrow_mut();
        let mut dbusmenu_changed: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        let mut sni_changed: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();

        let mut dbus_msgs = Vec::new();

        if let Some(text_direction) = prop_cache.text_direction_changed(&*inner) {
            dbusmenu_changed.insert(
                "TextDirection".into(),
                Variant(Box::new(text_direction.to_string())),
            );
        }

        if let Some(tray_status) = prop_cache.status_changed(&*inner) {
            let msg = StatusNotifierItemNewStatus {
                status: tray_status.to_string(),
            }
            .to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
            let menu_status = match tray_status {
                tray::Status::Passive | tray::Status::Active => menu::Status::Normal,
                tray::Status::NeedsAttention => menu::Status::Notice,
            };
            dbusmenu_changed.insert("Status".into(), Variant(Box::new(menu_status.to_string())));
        }

        if let Some(icon_theme_path) = prop_cache.icon_theme_path_changed(&*inner) {
            dbusmenu_changed.insert(
                "IconThemePath".into(),
                Variant(Box::new(icon_theme_path.to_string())),
            );
            sni_changed.insert(
                "IconThemePath".into(),
                Variant(Box::new(vec![icon_theme_path.to_string()])),
            );
        }

        if !dbusmenu_changed.is_empty() {
            let msg = PropertiesPropertiesChanged {
                interface_name: "com.canonical.dbusmenu".to_owned(),
                changed_properties: dbusmenu_changed,
                invalidated_properties: Vec::new(),
            }
            .to_emit_message(&MENU_PATH.into());
            dbus_msgs.push(msg);
        }

        if let Some(category) = prop_cache.category_changed(&*inner) {
            sni_changed.insert("Category".into(), Variant(Box::new(category.to_string())));
        }

        if let Some(window_id) = prop_cache.window_id_changed(&*inner) {
            sni_changed.insert("WindowId".into(), Variant(Box::new(window_id.to_string())));
        }

        if !sni_changed.is_empty() {
            let msg = PropertiesPropertiesChanged {
                interface_name: "org.kde.StatusNotifierItem".to_owned(),
                changed_properties: sni_changed,
                invalidated_properties: Vec::new(),
            }
            .to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
        }

        // TODO: assert the id is consistent

        if prop_cache.title_changed(&*inner) {
            let msg = StatusNotifierItemNewTitle {}.to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
        }
        if prop_cache.icon_changed(&*inner) {
            let msg = StatusNotifierItemNewIcon {}.to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
        }
        if prop_cache.overlay_icon_changed(&*inner) {
            let msg = StatusNotifierItemNewOverlayIcon {}.to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
        }
        if prop_cache.attention_icon_changed(&*inner) {
            let msg = StatusNotifierItemNewAttentionIcon {}.to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
        }
        if prop_cache.tool_tip_changed(&*inner) {
            let msg = StatusNotifierItemNewToolTip {}.to_emit_message(&sni_dbus_path);
            dbus_msgs.push(msg);
        }

        dbus_ext::with_current(move |conn| {
            for msg in dbus_msgs {
                conn.send(msg).unwrap();
            }
        });
    }
    fn update_menu(&self) -> bool {
        let new_menu = menu::menu_flatten(T::menu(&*self.state.inner.lock().unwrap()));
        *self.menu_cache.borrow_mut() = new_menu;
        self.revision.set(self.revision.get() + 1);
        // TODO: check layout
        let msg = DbusmenuLayoutUpdated {
            parent: 0,
            revision: self.revision.get(),
        }
        .to_emit_message(&MENU_PATH.into());
        dbus_ext::with_current(move |conn| {
            conn.send(msg).unwrap();
        });
        // TODO: diff and send ItemsPropertiesUpdated
        true
    }
}

impl<T: Tray> fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct(&format!("StatusNotifierItem")).finish()
    }
}

impl<T: Tray + 'static> dbus_interface::StatusNotifierItem for Inner<T> {
    fn activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        self.state.update(|model| {
            Tray::activate(model, x, y);
        });
        self.update_state();
        Ok(())
    }
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        self.state.update(|model| {
            Tray::secondary_activate(&mut *model, x, y);
        });
        self.update_state();
        Ok(())
    }
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), dbus::tree::MethodErr> {
        self.state.update(|model| {
            Tray::scroll(&mut *model, delta, dir);
        });
        self.update_state();
        Ok(())
    }
    fn context_menu(&self, _x: i32, _y: i32) -> Result<(), dbus::tree::MethodErr> {
        Ok(())
    }
    fn get_item_is_menu(&self) -> Result<bool, dbus::tree::MethodErr> {
        Ok(false)
    }
    fn get_category(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::category(&*model).to_string())
    }
    fn get_id(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::id(&*model))
    }
    fn get_title(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::title(&*model))
    }
    fn get_status(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::status(&*model).to_string())
    }
    fn get_window_id(&self) -> Result<i32, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::window_id(&*model))
    }
    fn get_menu(&self) -> Result<dbus::Path<'static>, dbus::tree::MethodErr> {
        Ok(MENU_PATH.into())
    }
    fn get_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::icon_name(&*model))
    }
    fn get_icon_theme_path(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::icon_theme_path(&*model))
    }
    fn get_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::icon_pixmap(&*model)
            .into_iter()
            .map(Into::into)
            .collect())
    }
    fn get_overlay_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::overlay_icon_name(&*model))
    }
    fn get_overlay_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::overlay_icon_pixmap(&*model)
            .into_iter()
            .map(Into::into)
            .collect())
    }
    fn get_attention_icon_name(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::attention_icon_name(&*model))
    }
    fn get_attention_icon_pixmap(&self) -> Result<Vec<(i32, i32, Vec<u8>)>, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::attention_icon_pixmap(&*model)
            .into_iter()
            .map(Into::into)
            .collect())
    }
    fn get_attention_movie_name(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::attention_movie_name(&*model))
    }
    fn get_tool_tip(
        &self,
    ) -> Result<(String, Vec<(i32, i32, Vec<u8>)>, String, String), dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::tool_tip(&*model).into())
    }
}

impl<T: Tray + 'static> dbus_interface::Dbusmenu for Inner<T> {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<&str>,
    ) -> Result<
        (
            u32,
            (
                i32,
                HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
                Vec<Variant<Box<dyn RefArg + 'static>>>,
            ),
        ),
        dbus::tree::MethodErr,
    > {
        dbg!(parent_id, recursion_depth, &property_names);
        Ok((
            self.revision.get(),
            crate::menu::to_dbusmenu_variant(
                &self.menu_cache.borrow(),
                parent_id as usize,
                if recursion_depth < 0 {
                    None
                } else {
                    Some(recursion_depth as usize)
                },
                property_names,
            ),
        ))
    }
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<&str>,
    ) -> Result<
        Vec<(i32, HashMap<String, Variant<Box<dyn RefArg + 'static>>>)>,
        dbus::tree::MethodErr,
    > {
        let r = ids
            .into_iter()
            .map(|id| {
                (
                    id,
                    self.menu_cache.borrow()[id as usize]
                        .0
                        .to_dbus_map(&property_names),
                )
            })
            .collect();
        Ok(r)
    }
    fn get_property(
        &self,
        id: i32,
        name: &str,
    ) -> Result<Variant<Box<dyn RefArg + 'static>>, dbus::tree::MethodErr> {
        // FIXME
        Err(dbus::tree::MethodErr::failed(&"unimplemented"))
    }
    fn event(
        &self,
        id: i32,
        event_id: &str,
        _data: Variant<Box<dyn RefArg>>,
        _timestamp: u32,
    ) -> Result<(), dbus::tree::MethodErr> {
        match event_id {
            "clicked" => {
                let activate = self.menu_cache.borrow()[id as usize].0.on_clicked.clone();
                self.state.update(|state| {
                    (activate)(state, id as usize);
                });
                self.update_state();
            }
            _ => (),
        }
        Ok(())
    }
    fn event_group(
        &self,
        events: Vec<(i32, &str, Variant<Box<dyn RefArg>>, u32)>,
    ) -> Result<Vec<i32>, dbus::tree::MethodErr> {
        let (found, not_found) = events
            .into_iter()
            .partition::<Vec<_>, _>(|event| (event.0 as usize) < self.menu_cache.borrow().len());
        if found.is_empty() {
            return Err(dbus::tree::MethodErr::invalid_arg(
                &"None of the id in the events can be found",
            ));
        }
        for (id, event_id, data, timestamp) in found {
            self.event(id, event_id, data, timestamp)?;
        }
        Ok(not_found.into_iter().map(|event| event.0).collect())
    }
    fn about_to_show(&self, _id: i32) -> Result<bool, dbus::tree::MethodErr> {
        Ok(false)
    }
    fn about_to_show_group(
        &self,
        _ids: Vec<i32>,
    ) -> Result<(Vec<i32>, Vec<i32>), dbus::tree::MethodErr> {
        // FIXME: the DBus message should set the no reply flag
        Ok(Default::default())
    }
    fn get_version(&self) -> Result<u32, dbus::tree::MethodErr> {
        Ok(3)
    }
    fn get_text_direction(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(Tray::text_direction(&*model).to_string())
    }
    fn get_status(&self) -> Result<String, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        Ok(match Tray::status(&*model) {
            tray::Status::Active | tray::Status::Passive => menu::Status::Normal,
            tray::Status::NeedsAttention => menu::Status::Notice,
        }
        .to_string())
    }
    fn get_icon_theme_path(&self) -> Result<Vec<String>, dbus::tree::MethodErr> {
        let model = self.state.inner.lock().unwrap();
        let path = Tray::icon_theme_path(&*model);
        Ok(if path.is_empty() {
            Default::default()
        } else {
            vec![path]
        })
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
