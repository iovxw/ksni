#[cfg(all(not(feature = "async-io"), not(feature = "tokio")))]
compile_error!(r#"Either "tokio" (default) or "async-io" must be enabled."#);

#[cfg(feature = "tokio")]
mod tokio {
    use std::future::Future;

    pub use tokio::select;
    pub use tokio::sync::Mutex;

    // remove the return value to compat with async-io
    pub fn spawn<F>(future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(future);
    }

    #[cfg(feature = "blocking")]
    pub fn block_on<T>(future: impl Future<Output = T>) -> T {
        use once_cell::sync::OnceCell;
        use tokio::runtime::Runtime;
        static RUNTIME: OnceCell<Runtime> = OnceCell::new();

        RUNTIME
            .get_or_init(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
            })
            .block_on(future)
    }

    pub mod mpsc {
        pub use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
    }
    pub mod oneshot {
        pub use tokio::sync::oneshot::{channel, Receiver, Sender};
    }
}
#[cfg(feature = "tokio")]
pub use tokio::*;

#[cfg(feature = "async-io")]
mod async_io {
    use std::future::Future;

    use async_executor::Executor;
    use once_cell::sync::OnceCell;

    // Do NOT use async_lock::OnceCell instead
    // the spawn method may be called in async context
    // and async_lock::OnceCell::get_or_init_blocking may result in deadlocks
    static EXECUTOR: OnceCell<Executor> = OnceCell::new();

    pub use async_io::block_on;
    pub use async_lock::Mutex;

    pub fn spawn<F>(future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        EXECUTOR
            .get_or_init(|| {
                let executor = Executor::new();
                std::thread::spawn(move || {
                    let executor = EXECUTOR.wait();
                    block_on(async {
                        // TODO: exit when tray stopped
                        loop {
                            executor.tick().await;
                        }
                    })
                });
                executor
            })
            .spawn(future)
            .detach()
    }

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
    pub use crate::select;

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
            pub fn is_closed(&self) -> bool {
                self.0.is_closed()
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
            ) -> futures_util::stream::Next<'_, futures_channel::mpsc::UnboundedReceiver<T>>
            {
                self.0.next()
            }
        }
    }

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
}
#[cfg(feature = "async-io")]
pub use async_io::*;
