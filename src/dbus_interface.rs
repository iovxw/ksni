use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use crossbeam_queue::SegQueue;
use serde::Serialize;
use zbus::names::InterfaceName;
use zbus::zvariant::{self, ObjectPath, OwnedValue, Type, Value};
use zbus::{object_server::SignalEmitter, Connection};

use crate::compat::task_local;
use crate::service::{Service, State};
use crate::{Icon, ToolTip, Tray};

pub const SNI_PATH: ObjectPath = ObjectPath::from_static_str_unchecked("/StatusNotifierItem");
pub const MENU_PATH: ObjectPath = ObjectPath::from_static_str_unchecked("/MenuBar");
pub const SNI_INTERFACE: InterfaceName =
    InterfaceName::from_static_str_unchecked("org.kde.StatusNotifierItem");
pub const MENU_INTERFACE: InterfaceName =
    InterfaceName::from_static_str_unchecked("com.canonical.dbusmenu");
const TOKEN_GRACE_PERIOD: Duration = Duration::from_millis(20);

task_local! {
    static DBUS_CONTEXT: Context;
    static TOKEN_INIT_STATE: TokenState;
}

#[derive(Debug, PartialEq, Clone)]
pub enum TokenState {
    Ready(String),
    Waiting(Instant),
}

impl From<TokenState> for Option<String> {
    fn from(value: TokenState) -> Self {
        match value {
            TokenState::Ready(t) => Some(t),
            _ => None,
        }
    }
}

/// Shared state between zbus interfaces
#[derive(Clone)]
pub struct Context {
    current_event_token: Arc<ArcSwapOption<TokenState>>,
    pending_tokens: Arc<SegQueue<String>>,
}

impl Context {
    pub fn new() -> Self {
        let token_queue = Arc::new(SegQueue::new());
        Context {
            current_event_token: Arc::new(ArcSwapOption::empty()),
            pending_tokens: token_queue,
        }
    }

    pub fn from_env() -> Option<Self> {
        DBUS_CONTEXT.try_get().ok()
    }

    pub async fn retrieve_token(&self) -> TokenState {
        self.pending_tokens
            .pop()
            .map(|token| TokenState::Ready(token))
            .unwrap_or_else(|| TokenState::Waiting(Instant::now()))
    }

    pub async fn enter<F: AsyncFnOnce(&mut State<T>) -> R, R, T>(
        &self,
        init_token: TokenState,
        // require state lock to ensure we are the only running event
        // so it's safe to override current_event_token
        state: &mut State<T>,
        f: F,
    ) -> R {
        debug_assert!(self.current_event_token.load().is_none());
        self.current_event_token.store(Some(Arc::new(init_token)));
        struct TokenGurad<'a>(&'a Context);
        impl<'a> Drop for TokenGurad<'a> {
            fn drop(&mut self) {
                self.0.current_event_token.store(None);
            }
        }
        let _defer = TokenGurad(self);
        DBUS_CONTEXT.scope(self.clone(), f(state)).await
    }

    pub fn current_event_token(&self) -> Option<String> {
        self.current_event_token
            .load()
            .as_deref()
            .cloned()
            .map(Into::into)
            .flatten()
    }
}

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

pub struct StatusNotifierItem<T>(Service<T>);

impl<T> StatusNotifierItem<T> {
    pub fn new(service: Service<T>) -> Self {
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
            self.0
                .lock_state(true, async |state| state.call_activate(conn, x, y).await)
                .await;
            Ok(())
        }
    }

    async fn secondary_activate(
        &self,
        #[zbus(connection)] conn: &Connection,
        x: i32,
        y: i32,
    ) -> zbus::fdo::Result<()> {
        //let mut service = self.0.state.lock().await; // do NOT use any self methods after this
        //service.call_secondary_activate(conn, x, y).await;
        self.0
            .lock_state(true, async |state| {
                state.call_secondary_activate(conn, x, y).await
            })
            .await;
        Ok(())
    }

    async fn provide_xdg_activation_token(&self, token: String) -> zbus::fdo::Result<()> {
        let token_timestamp = Instant::now();
        let current_event_token = self.0.context.current_event_token.load();
        let token_is_for_next_event = match current_event_token.as_deref() {
            // event already got token, or no event yet. the token is for next event
            Some(TokenState::Ready(_)) | None => true,
            // event is waiting for a token
            Some(TokenState::Waiting(event_timestamp)) => {
                let within_grace = token_timestamp.saturating_duration_since(*event_timestamp)
                    < TOKEN_GRACE_PERIOD;
                // A token that is too late is for the next event. Otherwise try to
                // attach it to the current event; if another thread stored a token
                // first, this one is queued for the next event instead.
                !within_grace
                    || &*self.0.context.current_event_token.compare_and_swap(
                        &current_event_token,
                        Some(Arc::new(TokenState::Ready(token.clone()))),
                    ) != &*current_event_token
            }
        };
        if token_is_for_next_event {
            let _ = self.0.context.pending_tokens.push(token);
        }
        Ok(())
    }

    async fn scroll(
        &self,
        #[zbus(connection)] conn: &Connection,
        delta: i32,
        dir: crate::Orientation,
    ) -> zbus::fdo::Result<()> {
        self.0
            .lock_state(true, async |state| {
                state.call_scroll(conn, delta, dir).await
            })
            .await;
        Ok(())
    }

    // properties
    #[zbus(property)]
    async fn category(&self) -> zbus::fdo::Result<crate::Category> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_category())
            .await)
    }

    #[zbus(property)]
    async fn id(&self) -> zbus::fdo::Result<String> {
        Ok(self.0.lock_state(false, async |state| state.get_id()).await)
    }

    #[zbus(property)]
    async fn title(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_title())
            .await)
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<crate::Status> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_status())
            .await)
    }

    #[zbus(property)]
    async fn window_id(&self) -> zbus::fdo::Result<i32> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_window_id())
            .await)
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_icon_theme_path())
            .await)
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
        Ok(self
            .0
            .lock_state(false, async |state| state.get_icon_name())
            .await)
    }

    #[zbus(property)]
    async fn icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_icon_pixmap())
            .await)
    }

    #[zbus(property)]
    async fn overlay_icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_overlay_icon_name())
            .await)
    }

    #[zbus(property)]
    async fn overlay_icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_overlay_icon_pixmap())
            .await)
    }

    #[zbus(property)]
    async fn attention_icon_name(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_attention_icon_name())
            .await)
    }

    #[zbus(property)]
    async fn attention_icon_pixmap(&self) -> zbus::fdo::Result<Vec<Icon>> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_attention_icon_pixmap())
            .await)
    }

    #[zbus(property)]
    async fn attention_movie_name(&self) -> zbus::fdo::Result<String> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_attention_movie_name())
            .await)
    }

    #[zbus(property)]
    async fn tool_tip(&self) -> zbus::fdo::Result<ToolTip> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_tool_tip())
            .await)
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

pub struct DbusMenu<T>(Service<T>);

impl<T> DbusMenu<T> {
    pub fn new(service: Service<T>) -> Self {
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
        self.0
            .lock_state(false, async |state| {
                let tree = state.build_layout(
                    parent_id,
                    if recursion_depth < 0 {
                        None
                    } else {
                        Some(recursion_depth as usize)
                    },
                    property_names,
                );
                tree.map(|tree| (state.revision, tree))
                    .ok_or_else(|| zbus::fdo::Error::InvalidArgs("parentId not found".to_string()))
            })
            .await
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<Vec<(i32, HashMap<Cow<'static, str>, OwnedValue>)>> {
        self.0
            .lock_state(false, async |state| {
                if ids.is_empty() {
                    Ok(state.get_all_item(&property_names))
                } else {
                    Ok(ids
                        .into_iter()
                        .filter_map(|id| {
                            state
                                .get_menu_item(id, &property_names)
                                .map(|properties| (id, properties))
                        })
                        .collect())
                }
            })
            .await
    }

    async fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        self.0
            .lock_state(false, async |state| {
                state
                    .get_menu_item(id, &[name])
                    .ok_or_else(|| zbus::fdo::Error::InvalidArgs("id not found".into()))
                    .map(|map| map.into_iter().next().map(|entry| entry.1))
                    .transpose()
                    .unwrap_or_else(|| {
                        Err(zbus::fdo::Error::InvalidArgs("property not found".into()))
                    })
            })
            .await
    }

    async fn event(
        &self,
        #[zbus(connection)] conn: &Connection,
        id: i32,
        event_id: String,
        data: OwnedValue,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        self.0
            .lock_state(event_id == "clicked", async |state| {
                if !state
                    .event(conn, true, id, &event_id, data, timestamp)
                    .await?
                {
                    return Err(zbus::fdo::Error::InvalidArgs("id not found".into()));
                }
                Ok(())
            })
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
        let events_len = events.len();
        let last_id = events
            .last()
            .expect("`events.is_empty` should been checked")
            .0;
        let mut not_found_ids = Vec::with_capacity(events_len);
        for (id, event_id, data, timestamp) in events {
            if !self
                .0
                .lock_state(event_id == "clicked", async |state| {
                    state
                        .event(conn, id == last_id, id, &event_id, data, timestamp)
                        .await
                })
                .await?
            {
                not_found_ids.push(id);
            }
        }
        if not_found_ids.len() == events_len {
            Err(zbus::fdo::Error::InvalidArgs(
                "None of the id in the events can be found".into(),
            ))
        } else {
            Ok(not_found_ids)
        }
    }

    async fn about_to_show(
        &self,
        #[zbus(connection)] conn: &Connection,
        id: i32,
    ) -> zbus::fdo::Result<bool> {
        // TODO: run the hook in a separate task
        self.0
            .lock_state(false, async |state| {
                if state.run_about2show_hook(conn, id).await? {
                    // Always return false
                    // libdubusmenu does not respect the return value
                    // Qt’s solution is to always return false, and emit LayoutUpdated/PropertiesUpdated
                    // signals when the menu is updated. We follow the same approach
                    Ok(false)
                } else {
                    Err(zbus::fdo::Error::InvalidArgs("id not found".into()))
                }
            })
            .await
    }

    async fn about_to_show_group(
        &self,
        #[zbus(connection)] conn: &Connection,
        ids: Vec<i32>,
    ) -> zbus::fdo::Result<(Vec<i32>, Vec<i32>)> {
        let not_found_ids = self
            .0
            .lock_state(false, async |state| -> zbus::fdo::Result<Vec<i32>> {
                let mut not_found_ids = Vec::new();

                for &id in &ids {
                    if id == 0 {
                        state.run_about2show_hook(conn, id).await?;
                    } else if state.get_menu_item(id, &[]).is_none() {
                        // Only checks id exists, don't trigger update
                        not_found_ids.push(id);
                    }
                }

                Ok(not_found_ids)
            })
            .await?;

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
        Ok(self
            .0
            .lock_state(false, async |state| state.get_text_direction())
            .await)
    }

    #[zbus(property)]
    async fn status(&self) -> zbus::fdo::Result<crate::menu::Status> {
        Ok(self
            .0
            .lock_state(false, async |state| state.get_status().to_menu_status())
            .await)
    }

    #[zbus(property)]
    async fn icon_theme_path(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(self
            .0
            .lock_state(false, async |state| {
                let path = state.get_icon_theme_path();
                if path.is_empty() {
                    vec![]
                } else {
                    vec![path]
                }
            })
            .await)
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

/// Tests for the XDG activation token bookkeeping.
///
/// The token attribution rule is:
///
/// - A token received while no event is running is queued and belongs to the
///   next event that consumes a token, no matter how long before it arrives.
/// - If an event that needs a token did not receive one before it started, a
///   token arriving afterwards still belongs to it only when it arrives within
///   [`TOKEN_GRACE_PERIOD`] of the event start, otherwise the event gets no
///   token and the token is queued for the next one.
///
/// The grace period is exercised by directly manipulating the event timestamp,
/// because a real D-Bus round trip can not reliably land on either side of a
/// 20ms boundary.
#[cfg(all(test, any(feature = "tokio", feature = "async-io")))]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[cfg(feature = "async-io")]
    use macro_rules_attribute::apply;
    #[cfg(feature = "async-io")]
    use smol_macros::test;

    use super::{Context, StatusNotifierItem, TokenState};
    use crate::service::Service;
    use crate::Tray;

    #[derive(Clone, Default)]
    struct TokenTray;

    impl Tray for TokenTray {
        fn id(&self) -> String {
            "token-tray".into()
        }
    }

    fn service() -> Service<TokenTray> {
        Service::new(TokenTray)
    }

    #[test]
    fn from_env_is_none_outside_event() {
        assert!(Context::from_env().is_none());
        assert_eq!(crate::current_xdg_activation_token(), None);
    }

    #[test]
    fn token_state_converts_to_option() {
        assert_eq!(
            Option::<String>::from(TokenState::Waiting(Instant::now())),
            None
        );
        assert_eq!(
            Option::<String>::from(TokenState::Ready("token".into())),
            Some("token".to_owned())
        );
    }

    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn current_event_token_before_during_and_after_event() {
        let service = service();
        let ctx = service.context.clone();
        let mut state = service.state.lock().await;

        // before: no event is running, so there is no token to report
        assert_eq!(ctx.current_event_token(), None);
        assert_eq!(crate::current_xdg_activation_token(), None);

        // during: the token stored by `enter` is visible
        let (private, public) = ctx
            .enter(
                TokenState::Ready("token".into()),
                &mut state,
                async |_state| {
                    (
                        ctx.current_event_token(),
                        crate::current_xdg_activation_token(),
                    )
                },
            )
            .await;
        assert_eq!(private, Some("token".to_owned()));
        assert_eq!(public, Some("token".to_owned()));

        // after: the guard cleared it again
        assert_eq!(ctx.current_event_token(), None);
        assert_eq!(crate::current_xdg_activation_token(), None);
    }

    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn waiting_event_reports_no_token() {
        let service = service();
        let ctx = service.context.clone();
        let mut state = service.state.lock().await;

        let (private, public) = ctx
            .enter(
                TokenState::Waiting(Instant::now()),
                &mut state,
                async |_state| {
                    (
                        ctx.current_event_token(),
                        crate::current_xdg_activation_token(),
                    )
                },
            )
            .await;
        assert_eq!(private, None);
        assert_eq!(public, None);
    }

    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn retrieve_token_is_waiting_when_queue_is_empty() {
        let ctx = Context::new();
        let before = Instant::now();
        match ctx.retrieve_token().await {
            TokenState::Waiting(timestamp) => assert!(timestamp >= before),
            other => panic!("expected Waiting, got {other:?}"),
        }
    }

    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn retrieve_token_pops_fifo() {
        let ctx = Context::new();
        ctx.pending_tokens.push("first".into());
        ctx.pending_tokens.push("second".into());

        assert_eq!(
            ctx.retrieve_token().await,
            TokenState::Ready("first".into())
        );
        assert_eq!(
            ctx.retrieve_token().await,
            TokenState::Ready("second".into())
        );
        assert!(matches!(ctx.retrieve_token().await, TokenState::Waiting(_)));
    }

    /// A token sent before any event is delivered to the next event, and is not
    /// visible anymore once that event finished.
    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn advance_token_is_delivered_to_next_event() {
        let service = service();
        let sni = StatusNotifierItem::new(service.clone());
        sni.provide_xdg_activation_token("advance".into())
            .await
            .unwrap();

        let seen = service
            .lock_state(true, async |_state| crate::current_xdg_activation_token())
            .await;
        assert_eq!(seen.as_deref(), Some("advance"));
        assert_eq!(service.context.current_event_token(), None);
    }

    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn event_without_token_reports_none() {
        let service = service();
        let seen = service
            .lock_state(true, async |_state| crate::current_xdg_activation_token())
            .await;
        assert_eq!(seen, None);
    }

    /// Queued tokens are consumed in order and do not expire while waiting.
    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn queued_tokens_are_fifo_and_do_not_expire() {
        let service = service();
        let sni = StatusNotifierItem::new(service.clone());
        sni.provide_xdg_activation_token("first".into())
            .await
            .unwrap();
        std::thread::sleep(Duration::from_millis(30));
        sni.provide_xdg_activation_token("second".into())
            .await
            .unwrap();

        let first = service
            .lock_state(true, async |_state| crate::current_xdg_activation_token())
            .await;
        let second = service
            .lock_state(true, async |_state| crate::current_xdg_activation_token())
            .await;
        let third = service
            .lock_state(true, async |_state| crate::current_xdg_activation_token())
            .await;

        assert_eq!(first.as_deref(), Some("first"));
        assert_eq!(second.as_deref(), Some("second"));
        assert_eq!(third, None);
    }

    /// Events that can not open a window must not steal a queued token.
    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn event_without_window_does_not_consume_token() {
        let service = service();
        let sni = StatusNotifierItem::new(service.clone());
        sni.provide_xdg_activation_token("keep".into())
            .await
            .unwrap();

        service.lock_state(false, async |_state| ()).await;

        let seen = service
            .lock_state(true, async |_state| crate::current_xdg_activation_token())
            .await;
        assert_eq!(seen.as_deref(), Some("keep"));
    }

    /// A token arriving while the event is running belongs to it when it is
    /// delivered within the grace period.
    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn late_token_within_grace_belongs_to_running_event() {
        let service = service();
        let sni = StatusNotifierItem::new(service.clone());
        let ctx = service.context.clone();
        let mut state = service.state.lock().await;

        let observed = ctx
            .enter(
                TokenState::Waiting(Instant::now() - Duration::from_millis(5)),
                &mut state,
                async move |_state| {
                    sni.provide_xdg_activation_token("late".into())
                        .await
                        .unwrap();
                    crate::current_xdg_activation_token()
                },
            )
            .await;

        assert_eq!(observed.as_deref(), Some("late"));
        // consumed by the running event, so nothing is queued for the next one
        assert!(matches!(ctx.retrieve_token().await, TokenState::Waiting(_)));
    }

    /// A token arriving after the grace period belongs to the next event, and
    /// the running event is left without a token.
    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn late_token_beyond_grace_is_queued_for_next_event() {
        let service = service();
        let sni = StatusNotifierItem::new(service.clone());
        let ctx = service.context.clone();
        let mut state = service.state.lock().await;

        let observed = ctx
            .enter(
                TokenState::Waiting(Instant::now() - Duration::from_millis(25)),
                &mut state,
                async move |_state| {
                    sni.provide_xdg_activation_token("late".into())
                        .await
                        .unwrap();
                    crate::current_xdg_activation_token()
                },
            )
            .await;

        assert_eq!(observed, None);
        assert_eq!(ctx.retrieve_token().await, TokenState::Ready("late".into()));
    }

    /// A token must not overwrite the token of an event that already has one;
    /// it goes to the next event instead.
    #[cfg_attr(feature = "tokio", tokio::test)]
    #[cfg_attr(feature = "async-io", apply(test!))]
    async fn token_does_not_override_event_that_already_has_token() {
        let service = service();
        let sni = StatusNotifierItem::new(service.clone());
        let ctx = service.context.clone();
        ctx.current_event_token
            .store(Some(Arc::new(TokenState::Ready("first".into()))));

        sni.provide_xdg_activation_token("second".into())
            .await
            .unwrap();

        assert_eq!(ctx.current_event_token(), Some("first".to_owned()));
        assert_eq!(
            ctx.retrieve_token().await,
            TokenState::Ready("second".into())
        );
    }
}
