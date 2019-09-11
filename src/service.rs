use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use dbus;
use dbus::arg::{RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, MessageType, SignalArgs};

use crate::dbus_ext;
use crate::dbus_interface;
use crate::dbus_interface::StatusNotifierWatcher;
use crate::freedesktop;
use crate::menu;
use crate::tray;
use crate::{State, Tray};

pub(crate) const SNI_PATH: &str = "/StatusNotifierItem";
pub(crate) const MENU_PATH: &str = "/MenuBar";

static COUNTER: AtomicUsize = AtomicUsize::new(1);

pub struct TrayService<T> {
    state: State<T>,
    rx: mpsc::Receiver<dbus::Message>,
}

impl<T: Tray + 'static> TrayService<T> {
    pub fn new(tray: T) -> Self {
        let (tx, rx) = mpsc::channel();
        let prop_cache = super::PropertiesCache::new(&tray);
        TrayService {
            state: State {
                tx: tx,
                inner: Arc::new(Mutex::new(tray)),
                prop_cache: Arc::new(Mutex::new(prop_cache)),
            },
            rx,
        }
    }
    pub fn run(self) -> Result<(), dbus::Error> {
        let name = format!(
            "org.kde.StatusNotifierItem-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::AcqRel)
        );
        let mut conn = Connection::new_session()?;
        conn.request_name(&name, true, true, false)?;

        let menu = {
            let state = self.state.inner.lock().unwrap();
            RefCell::new(menu::menu_flatten(T::menu(&*state)))
        };
        let inner = Rc::new(Inner {
            state: self.state,
            msgs: self.rx,
            menu: menu,
            menu_path: MENU_PATH.into(),
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
            inner.flush_msgs();
        }
    }

    pub fn spwan(self)
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
    msgs: mpsc::Receiver<dbus::Message>,
    // A list of menu item and it's submenu
    menu: RefCell<Vec<(menu::RawMenuItem, Vec<usize>)>>,
    menu_path: dbus::Path<'static>,
}

impl<T: Tray> Inner<T> {
    fn flush_msgs(&self) {
        dbus_ext::with_current(|conn| {
            while let Ok(msg) = self.msgs.try_recv() {
                conn.send(msg).expect("send dbus message");
            }
        });
    }
}

impl<T: Tray> fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct(&format!("StatusNotifierItem")).finish()
    }
}

impl<T: Tray> dbus_interface::StatusNotifierItem for Inner<T> {
    fn activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        let mut model = self.state.inner.lock().unwrap();
        Tray::activate(&mut *model, x, y);
        self.flush_msgs();
        Ok(())
    }
    fn secondary_activate(&self, x: i32, y: i32) -> Result<(), dbus::tree::MethodErr> {
        let mut model = self.state.inner.lock().unwrap();
        Tray::secondary_activate(&mut *model, x, y);
        self.flush_msgs();
        Ok(())
    }
    fn scroll(&self, delta: i32, dir: &str) -> Result<(), dbus::tree::MethodErr> {
        let mut model = self.state.inner.lock().unwrap();
        Tray::scroll(&mut *model, delta, dir);
        self.flush_msgs();
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

impl<T: Tray> dbus_interface::Dbusmenu for Inner<T> {
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
        Ok((
            0,
            crate::menu::to_dbusmenu_variant(
                &self.menu.borrow(),
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
                    self.menu.borrow()[id as usize]
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
                let activate = self.menu.borrow()[id as usize].0.on_clicked.clone();
                let m = (activate)(&mut self.menu.borrow_mut(), id as usize);
                if let Some(msg) = m {
                    dbus_ext::with_current(|conn| {
                        conn.send(msg.to_emit_message(&self.menu_path))
                            .expect("send dbus message");
                    })
                    .unwrap()
                };
                self.flush_msgs();
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
            .partition::<Vec<_>, _>(|event| (event.0 as usize) < self.menu.borrow().len());
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
