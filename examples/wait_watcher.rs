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
        match MyTray.launch(Duration::from_secs(5)).await {
            Err(e) => {
                eprintln!("System doesn't support SNI: {e}");
                // setup a fallback tray here
            }
            Ok(handle) => {
                let _ = my_handle_clone.set(handle);
            }
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
