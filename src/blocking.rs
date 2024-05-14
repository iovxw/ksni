//! The blocking API

use std::sync::{Arc, Weak};
use std::thread;

use crate::{
    compat::{self, mpsc, oneshot, Mutex},
    private, service, Error, HandleReuest, Tray,
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
        Ok(Handle {
            service: Arc::downgrade(&service),
            sender: handle_tx,
        })
    }
}
impl<T: Tray> TrayMethods for T {}

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
    pub fn update<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> Option<R> {
        compat::block_on(async {
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
        })
    }

    /// Shutdown the tray service
    pub fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.sender.send(HandleReuest::Shutdown(tx)).is_ok() {
            let _ = compat::block_on(rx);
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
