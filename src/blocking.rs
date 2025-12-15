//! The blocking API

use std::sync::Arc;
use std::thread;

use crate::{
    compat::{self, mpsc},
    private, service, Error, Tray,
};

/// Provides blocking methods for [`Tray`]
pub trait TrayMethods: Tray + private::Sealed {
    /// Run the tray service in background
    ///
    /// If your application will be running in a sandbox, set [`disable_dbus_name`] first
    ///
    /// [`disable_dbus_name`]: Self::disable_dbus_name
    fn spawn(self) -> Result<Handle<Self>, Error> {
        TrayServiceBuilder::new(self).spawn()
    }

    #[doc(hidden)]
    #[deprecated(
        note = "use `disable_dbus_name(true).spawn()` instead",
        since = "0.3.4"
    )]
    /// Run the tray service in background, but without a dbus well-known name
    ///
    /// This violates the [StatusNotifierItem] specification, but is required in some sandboxed
    /// environments (e.g., flatpak).
    ///
    /// See <https://chromium-review.googlesource.com/c/chromium/src/+/4179380>
    ///
    /// [StatusNotifierItem]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierItem/
    fn spawn_without_dbus_name(self) -> Result<Handle<Self>, Error> {
        self.disable_dbus_name(true).spawn()
    }

    /// Disable owning a D-Bus well-known name (`StatusNotifierItem-PID-ID`) for the tray service
    ///
    /// This violates the [StatusNotifierItem] specification, but is required in some sandboxed
    /// environments (e.g., flatpak).
    ///
    /// See <https://chromium-review.googlesource.com/c/chromium/src/+/4179380>
    ///
    /// # Examples
    /// ```no_run
    /// # use ksni::blocking::TrayMethods;
    /// # struct MyTray;
    /// # impl ksni::Tray for MyTray {
    /// # fn id(&self) -> String { "my_tray".into() }
    /// # }
    /// # fn test() {
    /// let handle = MyTray
    ///     .disable_dbus_name(true)
    ///     .spawn()
    ///     .expect("system should have a working SNI implementation");
    /// # }
    /// ```
    ///
    /// [StatusNotifierItem]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierItem/
    fn disable_dbus_name(self, disable: bool) -> TrayServiceBuilder<Self> {
        TrayServiceBuilder::new(self).disable_dbus_name(disable)
    }

    /// Assume the system has a working StatusNotifierItem implementation
    ///
    /// When `true`, `Error::Watcher(ServiceUnknown("The name org.kde.StatusNotifierWatcher was not provided by any .service files"))`
    /// (message may vary by D-Bus implementation) and [`Error::WontShow`] are treated as "soft
    /// errors": they are routed to [`Tray::watcher_offline`] instead of causing [`spawn()`] to return
    /// immediately.
    ///
    /// Useful when your application may start before the desktop environment is fully initialized,
    /// but it also means the tray may never appear if SNI support is truly absent.
    ///
    /// Use with caution.
    ///
    /// [`spawn()`]: Self::spawn
    fn assume_sni_available(self, assume_available: bool) -> TrayServiceBuilder<Self> {
        TrayServiceBuilder::new(self).assume_sni_available(assume_available)
    }
}

/// Builder to customize tray service
///
/// All methods are equivalent to those in [`TrayMethods`]
///
/// Should not be constructed directly, use [`TrayMethods`] instead.
pub struct TrayServiceBuilder<T: Tray> {
    tray: T,
    own_name: bool,
    assume_sni_available: bool,
}

impl<T: Tray> TrayServiceBuilder<T> {
    /// Create a new builder with default options
    /// DO NOT PUBLIC
    fn new(tray: T) -> Self {
        Self {
            tray,
            own_name: true,
            assume_sni_available: false,
        }
    }

    /// Run the tray service in background
    ///
    /// If your application will be running in a sandbox, set [`disable_dbus_name`] first
    ///
    /// [`disable_dbus_name`]: Self::disable_dbus_name
    pub fn spawn(self) -> Result<Handle<T>, Error> {
        spawn_with_options(self.tray, self.own_name, self.assume_sni_available)
    }

    /// Disable owning a D-Bus well-known name (`StatusNotifierItem-PID-ID`) for the tray service
    ///
    /// This violates the [StatusNotifierItem] specification, but is required in some sandboxed
    /// environments (e.g., flatpak).
    ///
    /// See <https://chromium-review.googlesource.com/c/chromium/src/+/4179380>
    ///
    /// # Examples
    /// ```no_run
    /// # use ksni::blocking::TrayMethods;
    /// # struct MyTray;
    /// # impl ksni::Tray for MyTray {
    /// # fn id(&self) -> String { "my_tray".into() }
    /// # }
    /// # fn test() {
    /// let handle = MyTray
    ///     .disable_dbus_name(true)
    ///     .spawn()
    ///     .expect("system should have a working SNI implementation");
    /// # }
    /// ```
    ///
    /// [StatusNotifierItem]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/StatusNotifierItem/
    pub fn disable_dbus_name(self, disable: bool) -> Self {
        Self {
            own_name: !disable,
            ..self
        }
    }

    /// Assume the system has a working StatusNotifierItem implementation
    ///
    /// When `true`, `Error::Watcher(ServiceUnknown("The name org.kde.StatusNotifierWatcher was not provided by any .service files"))`
    /// (message may vary by D-Bus implementation) and [`Error::WontShow`] are treated as "soft
    /// errors": they are routed to [`Tray::watcher_offline`] instead of causing [`spawn()`] to return
    /// immediately.
    ///
    /// Useful when your application may start before the desktop environment is fully initialized,
    /// but it also means the tray may never appear if SNI support is truly absent.
    ///
    /// Use with caution.
    ///
    /// [`spawn()`]: Self::spawn
    pub fn assume_sni_available(self, assume_available: bool) -> Self {
        Self {
            assume_sni_available: assume_available,
            ..self
        }
    }
}

fn spawn_with_options<T: Tray>(
    tray: T,
    own_name: bool,
    assume_sni_available: bool,
) -> Result<Handle<T>, Error> {
    let (handle_tx, handle_rx) = mpsc::unbounded_channel();
    let service = service::Service::new(tray);
    let service_loop = compat::block_on(service::run(
        service.clone(),
        handle_rx,
        own_name,
        assume_sni_available,
    ))?;
    thread::spawn(move || {
        compat::block_on(service_loop);
    });
    Ok(Handle(crate::Handle {
        service: Arc::downgrade(&service),
        sender: handle_tx,
    }))
}

impl<T: Tray> TrayMethods for T {}

/// Handle to the tray
pub struct Handle<T>(crate::Handle<T>);

impl<T> Handle<T> {
    /// Update the tray
    ///
    /// Returns the result of `f`, returns `None` if the tray service
    /// has been shutdown.
    pub fn update<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> Option<R> {
        compat::block_on(self.0.update(f))
    }

    /// Shutdown the tray service
    pub fn shutdown(&self) -> ShutdownAwaiter {
        ShutdownAwaiter(self.0.shutdown())
    }

    /// Returns `true` if the tray service has been shutdown
    pub fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
}

/// Returned by [`Handle::shutdown`]
pub struct ShutdownAwaiter(crate::ShutdownAwaiter);

impl ShutdownAwaiter {
    /// Wait the shutdown to complete
    pub fn wait(self) {
        compat::block_on(self.0)
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle(self.0.clone())
    }
}
