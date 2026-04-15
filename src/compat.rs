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
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_executor::Executor;
    use once_cell::sync::OnceCell;

    // Do NOT use async_lock::OnceCell instead
    // the spawn method may be called in async context
    // and async_lock::OnceCell::get_or_init_blocking may result in deadlocks
    struct ExecutorState {
        executor: Executor<'static>,
        driver_running: AtomicBool,
    }

    struct DriverGuard {
        state: &'static ExecutorState,
    }

    impl Drop for DriverGuard {
        fn drop(&mut self) {
            self.state.driver_running.store(false, Ordering::Release);
            if !self.state.executor.is_empty() {
                self.state.kick_driver();
            }
        }
    }

    impl ExecutorState {
        fn get() -> &'static Self {
            static STATE: OnceCell<ExecutorState> = OnceCell::new();
            STATE.get_or_init(|| Self {
                executor: Executor::new(),
                driver_running: AtomicBool::new(false),
            })
        }

        fn kick_driver(&'static self) {
            if self
                .driver_running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }

            std::thread::spawn(move || {
                let _guard = DriverGuard { state: self };
                block_on(async {
                    while !self.executor.is_empty() {
                        self.executor.tick().await;
                    }
                })
            });
        }
    }

    pub use async_io::block_on;
    pub use async_lock::Mutex;

    pub fn spawn<F>(future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let state = ExecutorState::get();
        // Queue the task first so a freshly spawned driver never observes an empty executor.
        state.executor.spawn(future).detach();
        state.kick_driver();
    }

    #[cfg(test)]
    mod tests {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        use super::*;

        fn wait_until(timeout: Duration, condition: impl Fn() -> bool) {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if condition() {
                    return;
                }
                std::thread::yield_now();
            }
            assert!(condition(), "timed out waiting for executor state change");
        }

        #[test]
        fn executor_driver_restarts_after_idle() {
            let state = ExecutorState::get();

            // Keep one task pending so the test can observe the driver start and stop cleanly.
            let (started_tx, started_rx) = mpsc::channel();
            let (finish_tx, finish_rx) = futures_channel::oneshot::channel();
            let (done_tx, done_rx) = mpsc::channel();

            spawn(async move {
                let _ = started_tx.send(());
                let _ = finish_rx.await;
                let _ = done_tx.send(());
            });

            started_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first task should start");
            wait_until(Duration::from_secs(1), || {
                state.driver_running.load(Ordering::Acquire)
            });

            finish_tx.send(()).expect("first task should still be pending");
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first task should complete");
            wait_until(Duration::from_secs(1), || {
                !state.driver_running.load(Ordering::Acquire) && state.executor.is_empty()
            });

            let (restart_started_tx, restart_started_rx) = mpsc::channel();
            let (restart_finish_tx, restart_finish_rx) = futures_channel::oneshot::channel();
            let (restart_done_tx, restart_done_rx) = mpsc::channel();

            spawn(async move {
                let _ = restart_started_tx.send(());
                let _ = restart_finish_rx.await;
                let _ = restart_done_tx.send(());
            });

            restart_started_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("second task should start");
            wait_until(Duration::from_secs(1), || {
                state.driver_running.load(Ordering::Acquire)
            });

            restart_finish_tx
                .send(())
                .expect("second task should still be pending");
            restart_done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("second task should complete");
            wait_until(Duration::from_secs(1), || {
                !state.driver_running.load(Ordering::Acquire) && state.executor.is_empty()
            });
        }

        #[test]
        fn executor_driver_recovers_after_task_panic() {
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));

            let state = ExecutorState::get();
            // A panicking task should not leave the driver marked as permanently running.
            spawn(async move {
                panic!("boom");
            });

            wait_until(Duration::from_secs(1), || {
                !state.driver_running.load(Ordering::Acquire) && state.executor.is_empty()
            });

            let (started_tx, started_rx) = mpsc::channel();
            let (finish_tx, finish_rx) = futures_channel::oneshot::channel();
            let (done_tx, done_rx) = mpsc::channel();

            spawn(async move {
                let _ = started_tx.send(());
                let _ = finish_rx.await;
                let _ = done_tx.send(());
            });

            started_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("task after panic should start");
            wait_until(Duration::from_secs(1), || {
                state.driver_running.load(Ordering::Acquire)
            });

            finish_tx.send(()).expect("task after panic should still be pending");
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("task after panic should complete");
            wait_until(Duration::from_secs(1), || {
                !state.driver_running.load(Ordering::Acquire) && state.executor.is_empty()
            });

            std::panic::set_hook(hook);
        }
    }

    #[doc(hidden)]
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
