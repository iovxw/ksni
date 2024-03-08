#[cfg(all(not(feature = "async-io"), not(feature = "tokio")))]
compile_error!(r#"Either "tokio" (default) or "async-io" must be enabled."#);

#[cfg(feature = "tokio")]
pub use tokio::select;

#[cfg(feature = "async-io")]
#[macro_export]
macro_rules! select {
    ($($patten:pat = $exp:expr => $blk:block)*) => {
         futures_util::select! {
             $( v = $exp => {
                 let $patten = v else { continue };
                 $blk
             } )*
         }
    };
}
#[cfg(feature = "async-io")]
pub use crate::select;

#[cfg(feature = "tokio")]
pub mod mpsc {
    pub use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
}

#[cfg(feature = "async-io")]
pub mod mpsc {
    use futures_util::StreamExt;

    pub use futures_channel::mpsc::TrySendError as SendError;

    pub fn unbounded_channel<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
        let (tx, rx) = futures_channel::mpsc::unbounded();
        (UnboundedSender(tx), UnboundedReceiver(rx))
    }

    pub struct UnboundedSender<T>(futures_channel::mpsc::UnboundedSender<T>);
    impl<T> UnboundedSender<T> {
        pub fn send(&self, value: T) -> Result<(), SendError<T>> {
            self.0.unbounded_send(value)
        }
    }
    impl<T> Clone for UnboundedSender<T> {
        fn clone(&self) -> Self {
            UnboundedSender(self.0.clone())
        }
    }

    pub struct UnboundedReceiver<T>(futures_channel::mpsc::UnboundedReceiver<T>);
    impl<T> UnboundedReceiver<T> {
        pub fn recv(
            &mut self,
        ) -> futures_util::stream::Next<'_, futures_channel::mpsc::UnboundedReceiver<T>> {
            self.0.next()
        }
    }
}

#[cfg(feature = "tokio")]
pub mod oneshot {
    pub use tokio::sync::oneshot::{channel, Receiver, Sender};
}

#[cfg(feature = "async-io")]
pub mod oneshot {
    pub use futures_channel::oneshot::{channel, Receiver, Sender};
    //    use std::future::Future;
    //
    //    pub use async_channel::Sender;
    //    pub fn channel<T>() -> (
    //        Sender<T>,
    //        Receiver<T>,
    //    ) {
    //        // The concurrent-queue that used by async-channel has
    //        // single-capacity optimization, performace is fine
    //        let (tx, rx) = async_channel::bounded(1);
    //        let rx = async move { rx.recv().await };
    //        (tx, rx)
    //    }
    //    pub type Receiver<T> = impl Future<Output = Result<T, async_channel::RecvError>>;
}
