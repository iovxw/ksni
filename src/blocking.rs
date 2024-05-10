//! The blocking API

use std::thread;

use crate::{
    compat::{self, mpsc, oneshot},
    private, service, Error, HandleReuest, Tray,
};

// TODO: doc
pub trait TrayMethods: Tray + private::Sealed {
    // TODO: doc
    fn spawn(self) -> Result<Handle<Self>, Error> {
        let (handle_tx, handle_rx) = mpsc::unbounded_channel();
        let service_loop = compat::block_on(service::run(self, handle_rx))?;
        thread::spawn(move || {
            compat::block_on(service_loop);
        });
        Ok(Handle { sender: handle_tx })
    }
}
impl<T: Tray> TrayMethods for T {}

/// Handle to the tray
pub struct Handle<T> {
    sender: mpsc::UnboundedSender<HandleReuest<T>>,
}

impl<T> Handle<T> {
    /// Update the tray
    ///
    /// Returns the result of `f`, returns `None` if the tray service
    /// has been shutdown.
    pub fn update<R: Send + 'static, F: FnOnce(&mut T) -> R + Send + 'static>(
        &self,
        f: F,
    ) -> Option<R> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(HandleReuest::Update(Box::new(move |t: &mut T| {
                let _ = tx.send((f)(t));
            })))
            .ok()?;
        compat::block_on(rx).ok()
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
            sender: self.sender.clone(),
        }
    }
}
