//! A Rust implementation of the KDE/freedesktop StatusNotifierItem specification
//!
//! See the [README.md](https://github.com/iovxw/ksni) for an example

use std::sync::{Arc, Weak};

#[cfg(feature = "blocking")]
pub mod blocking;
mod compat;
mod dbus_interface;
pub mod menu;
mod service;
mod tray;

#[doc(inline)]
pub use menu::{MenuItem, TextDirection};
pub use tray::{Category, Icon, Orientation, Status, ToolTip};

use crate::compat::{mpsc, oneshot, Mutex};

/// A system tray, implement this to create your tray
///
/// **NOTE**: On some system trays, [`Tray::id`] is a required property to avoid unexpected behaviors
pub trait Tray: Sized + Send + 'static {
    /// Asks the status notifier item for activation, this is typically a
    /// consequence of user input, such as mouse left click over the graphical
    /// representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn activate(&mut self, _x: i32, _y: i32) {}

    /// Is to be considered a secondary and less important form of activation
    /// compared to Activate.
    /// This is typically a consequence of user input, such as mouse middle
    /// click over the graphical representation of the item.
    /// The application will perform any task is considered appropriate as an
    /// activation request.
    ///
    /// the x and y parameters are in screen coordinates and is to be considered
    /// an hint to the item where to show eventual windows (if any).
    fn secondary_activate(&mut self, _x: i32, _y: i32) {}

    /// The user asked for a scroll action. This is caused from input such as
    /// mouse wheel over the graphical representation of the item.
    ///
    /// The delta parameter represent the amount of scroll, the orientation
    /// parameter represent the horizontal or vertical orientation of the scroll
    /// request.
    fn scroll(&mut self, _delta: i32, _orientation: Orientation) {}

    /// Describes the category of this item.
    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    /// It's a name that should be unique for this application and consistent
    /// between sessions, such as the application name itself.
    fn id(&self) -> String {
        Default::default()
    }

    /// It's a name that describes the application, it can be more descriptive
    /// than Id.
    fn title(&self) -> String {
        Default::default()
    }

    /// Describes the status of this item or of the associated application.
    fn status(&self) -> Status {
        Status::Active
    }

    // NOTE: u32 in org.freedesktop.StatusNotifierItem
    /// It's the windowing-system dependent identifier for a window, the
    /// application can chose one of its windows to be available through this
    /// property or just set 0 if it's not interested.
    fn window_id(&self) -> i32 {
        0
    }

    /// An additional path to add to the theme search path to find the icons.
    fn icon_theme_path(&self) -> String {
        Default::default()
    }

    /// The item only support the context menu, the visualization
    /// should prefer showing the menu or sending ContextMenu()
    /// instead of Activate()
    // fn item_is_menu() -> bool { false }

    /// The StatusNotifierItem can carry an icon that can be used by the
    /// visualization to identify the item.
    fn icon_name(&self) -> String {
        Default::default()
    }

    /// Carries an ARGB32 binary representation of the icon
    fn icon_pixmap(&self) -> Vec<Icon> {
        Default::default()
    }

    /// The Freedesktop-compliant name of an icon. This can be used by the
    /// visualization to indicate extra state information, for instance as an
    /// overlay for the main icon.
    fn overlay_icon_name(&self) -> String {
        Default::default()
    }

    /// ARGB32 binary representation of the overlay icon described in the
    /// previous paragraph.
    fn overlay_icon_pixmap(&self) -> Vec<Icon> {
        Default::default()
    }

    /// The Freedesktop-compliant name of an icon. this can be used by the
    /// visualization to indicate that the item is in RequestingAttention state.
    fn attention_icon_name(&self) -> String {
        Default::default()
    }

    /// ARGB32 binary representation of the requesting attention icon describe in
    /// the previous paragraph.
    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        Default::default()
    }

    /// An item can also specify an animation associated to the
    /// RequestingAttention state.
    /// This should be either a Freedesktop-compliant icon name or a full path.
    /// The visualization can chose between the movie or AttentionIconPixmap (or
    /// using neither of those) at its discretion.
    fn attention_movie_name(&self) -> String {
        Default::default()
    }

    /// Data structure that describes extra information associated to this item,
    /// that can be visualized for instance by a tooltip (or by any other mean
    /// the visualization consider appropriate.
    fn tool_tip(&self) -> ToolTip {
        Default::default()
    }

    /// Represents the way the text direction of the application.  This
    /// allows the server to handle mismatches intelligently.
    fn text_direction(&self) -> TextDirection {
        TextDirection::LeftToRight
    }

    /// The menu that you want to display
    fn menu(&self) -> Vec<MenuItem<Self>> {
        Default::default()
    }

    /// The `org.kde.StatusNotifierWatcher` is online
    fn watcher_online(&self) {}

    /// The `org.kde.StatusNotifierWatcher` is offline
    ///
    /// You can setup a fallback tray here
    ///
    /// Return `false` to shutdown the tray service
    #[allow(
        unused_variables,
        reason = "the default impl don't use this parameter, but it should be used by user, so keep the name without _ for autocomplete"
    )]
    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        true
    }
}

/// Why is the tray offline
#[derive(Debug)]
#[non_exhaustive]
pub enum OfflineReason {
    /// The [StatusNotifierWatcher] just go offline with no reason nor any error
    ///
    /// # What could cause this?
    /// - User restarted the shell in GNOME on Xorg
    ///   - In this case, the watcher will back online quickly
    /// - User disabled the tray plugin in their desktop environment
    ///   - The watcher will back, or never
    ///   - Consider setting a fallback tray
    ///
    /// [StatusNotifierWatcher]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierWatcher/
    No,
    /// An error occurred while the tray was running
    Error(Error),
}

/// An error while connecting to the [StatusNotifierWatcher]
///
/// [StatusNotifierWatcher]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierWatcher/
#[derive(Debug)]
// FIXME: impl Error
#[non_exhaustive]
pub enum Error {
    /// D-Bus connection error
    ///
    /// Can not connect to the system D-Bus daemon, or encounter an error during the connection.
    /// The system may not have a D-Bus daemon (which is extremely rare in Linux desktop) or you
    /// are in a sandbox environment which didn't configured correctly.
    Dbus(zbus::Error),
    /// Failed to register to the [StatusNotifierWatcher]
    ///
    /// Current desktop environment does not support the [StatusNotifierItem] specification or the
    /// plugin that adds support is not running.
    ///
    /// [StatusNotifierWatcher]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierWatcher/
    /// [StatusNotifierItem]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/
    Watcher(zbus::fdo::Error),
    /// The tray was successfully created but can not be displayed due to no [StatusNotifierHost]
    /// exists
    ///
    /// The specification recommend you "should fall back using the Freedesktop System tray specification"
    ///
    /// This error can be caused by the program starting earlier than the desktop environment
    ///
    /// [StatusNotifierHost]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierHost/
    WontShow,
}

// TODO: doc
// the returned `Future` of all methods is always `Send`, because `Tray: Send` and `Self: Tray`
// verified by `_assert_tray_methods_returned_future_is_send`
#[allow(async_fn_in_trait)]
pub trait TrayMethods: Tray + private::Sealed {
    // Get [`Handle`] of a running [`Tray`]
    //
    // # Panics
    //
    // Will panic if the tray is not running, should only be used in [Tray::menu]
    // callbacks
    //fn handle(&self) -> Handle<Self> {
    //    todo!()
    //}

    // TODO: doc
    async fn spawn(self) -> Result<Handle<Self>, Error> {
        self.spawn_with_name(true).await
    }

    /// Run the tray service in background, but without a dbus well-known name
    ///
    /// This violates the [StatusNotifierItem] specification, but is required in some sandboxed
    /// environments (e.g., flatpak).
    ///
    /// See <https://chromium-review.googlesource.com/c/chromium/src/+/4179380>
    ///
    /// [StatusNotifierItem]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierItem/
    async fn spawn_without_dbus_name(self) -> Result<Handle<Self>, Error> {
        self.spawn_with_name(false).await
    }

    // sealed trait, safe to add private methods
    #[doc(hidden)]
    async fn spawn_with_name(self, own_name: bool) -> Result<Handle<Self>, Error> {
        let (handle_tx, handle_rx) = mpsc::unbounded_channel();
        let service = service::Service::new(self);
        let service_loop = service::run(service.clone(), handle_rx, own_name).await?;
        compat::spawn(service_loop);
        Ok(Handle {
            service: Arc::downgrade(&service),
            sender: handle_tx,
        })
    }
}
impl<T: Tray> TrayMethods for T {}

fn _assert_tray_methods_returned_future_is_send<T: Tray + Clone>(x: T) {
    fn assert_send<T: Send>(_: T) {}
    assert_send(x.clone().spawn());
    assert_send(x.clone().spawn_without_dbus_name());
}

mod private {
    pub trait Sealed {}
    impl<T: crate::Tray> Sealed for T {}
}

pub(crate) enum HandleReuest {
    Update(oneshot::Sender<()>),
    Shutdown(oneshot::Sender<()>),
}

/// Handle to the tray
pub struct Handle<T> {
    service: Weak<Mutex<service::Service<T>>>,
    sender: mpsc::UnboundedSender<HandleReuest>,
}

impl<T> Handle<T> {
    /// Update the tray
    ///
    /// Returns the result of `f`, returns `None` if the tray service
    /// has been shutdown.
    pub async fn update<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> Option<R> {
        if let Some(service) = self.service.upgrade() {
            // NOTE: free the lock before send any message
            let r = f(&mut service.lock().await.tray);
            let (tx, rx) = oneshot::channel();
            if self.sender.send(HandleReuest::Update(tx)).is_ok() {
                let _ = rx.await;
                return Some(r);
            }
        }
        None
    }

    /// Shutdown the tray service
    ///
    /// The shutdown process will be start immediately,
    /// even if you don't await the result
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.sender.send(HandleReuest::Shutdown(tx)).is_ok() {
            let _ = rx.await;
        }
    }

    /// Returns `true` if the tray service has been shutdown
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle {
            service: self.service.clone(),
            sender: self.sender.clone(),
        }
    }
}
