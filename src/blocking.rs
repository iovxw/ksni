//! The blocking API

use std::sync::Arc;
use std::thread;

use crate::{
    compat::{self, mpsc},
    private, service, Error, Tray,
};

// TODO: doc
pub trait TrayMethods: Tray + private::Sealed {
    // TODO: doc
    fn spawn(self) -> Result<Handle<Self>, Error> {
        let (handle_tx, handle_rx) = mpsc::unbounded_channel();
        let service = service::Service::new(self);
        let service_loop = compat::block_on(service::run(service.clone(), handle_rx))?;
        thread::spawn(move || {
            compat::block_on(service_loop);
        });
        Ok(Handle(crate::Handle {
            service: Arc::downgrade(&service),
            sender: handle_tx,
        }))
    }
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
    pub fn shutdown(&self) {
        compat::block_on(self.0.shutdown())
    }

    /// Returns `true` if the tray service has been shutdown
    pub fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle(self.0.clone())
    }
}
