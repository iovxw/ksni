use std::{sync::Arc, time::Duration};

use ksni::TrayMethods;
use tokio::sync::OnceCell;

#[derive(Debug)]
struct MyTray;

impl ksni::Tray for MyTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }
    fn icon_name(&self) -> String {
        "help-about".into()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let my_handle = Arc::new(OnceCell::new());
    let my_handle_clone = my_handle.clone();
    tokio::spawn(async move {
        if ksni::wait_watcher_online(Duration::from_secs(5))
            .await
            .unwrap_or(false)
            && let Ok(handle) = MyTray.spawn().await
        {
            let _ = my_handle_clone.set(handle);
        } else {
            eprintln!("System doesn't support SNI");
            // setup a fallback tray here
        }
    });

    // do something
    tokio::time::sleep(Duration::from_secs(10)).await;

    // if we need the handle
    if let Some(_handle) = my_handle.get() {
        // finally
    } else {
        // check the fallback tray
    }

    // Run forever
    std::future::pending().await
}
