use ksni::TrayMethods;

/// This example demonstrates [`ksni::Tray::menu_about_to_show`].
///
/// The method is called before the root menu is displayed and acts as a
/// notification hook. It can be used to update the menu when it is shown.
#[derive(Debug)]
struct MyTray {
    refresh_on_show: bool,
    time: chrono::DateTime<chrono::Local>,
}

impl ksni::Tray for MyTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }
    fn icon_name(&self) -> String {
        "clock".into()
    }
    fn menu_about_to_show(&mut self) {
        if self.refresh_on_show {
            self.time = chrono::Local::now();
        }
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: self.time.format("%H:%M:%S%.3f").to_string(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Refresh time on show".into(),
                checked: self.refresh_on_show,
                activate: Box::new(|this: &mut Self| this.refresh_on_show = !this.refresh_on_show),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    MyTray {
        refresh_on_show: true,
        time: chrono::Local::now(),
    }
    .spawn()
    .await
    .unwrap();
    std::future::pending().await
}
