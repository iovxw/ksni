//! Demonstrates [`ksni::current_xdg_activation_token`] with GTK.
//!
//! On Wayland a window cannot be brought to the front without an XDG activation
//! token. GTK consumes the token while presenting a window, so the token is
//! read in the tray callback, forwarded to the GTK main thread and set with
//! `gtk_window_set_startup_id` right before `gtk_window_present`.

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use ksni::TrayMethods;

struct ActivationTray {
    window: glib::SendWeakRef<gtk::Window>,
    force_no_token: bool,
}

impl ActivationTray {
    fn show_window(&self, source: &str) {
        let token = if self.force_no_token {
            eprintln!("{source}: activation token forced to None");
            None
        } else {
            if let r @ Some(_) = ksni::current_xdg_activation_token() {
                r
            } else {
                eprintln!("{source}: activation token = None, retrying");
                // KDE may deliver the activation token slightly after the tray event
                // Waiting 1ms and retrying gets it in most cases.
                std::thread::sleep(std::time::Duration::from_millis(1));
                ksni::current_xdg_activation_token()
            }
        };
        eprintln!("{source}: activation token = {token:?}");

        let window = self.window.clone();
        // Wakes the GTK main loop and runs the closure on the main thread.
        glib::idle_add_once(move || {
            let window = window.upgrade().unwrap();
            match token.as_deref() {
                Some(token) => {
                    eprintln!("setting startup id and presenting the window");
                    window.set_startup_id(token);
                }
                None => eprintln!("no activation token, presenting the window"),
            }
            window.present();
        });
    }
}

impl ksni::Tray for ActivationTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }
    fn icon_name(&self) -> String {
        "window-new".into()
    }
    fn title(&self) -> String {
        "XDG Activation Token Example".into()
    }
    fn activate(&mut self, x: i32, y: i32) {
        eprintln!("Activate({x}, {y})");
        self.show_window("Activate");
    }
    fn secondary_activate(&mut self, x: i32, y: i32) {
        eprintln!("SecondaryActivate({x}, {y})");
        self.show_window("SecondaryActivate");
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Open Window".into(),
                icon_name: "window-new".into(),
                activate: Box::new(|this: &mut Self| this.show_window("menu item")),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Force token to None".into(),
                checked: self.force_no_token,
                activate: Box::new(|this: &mut Self| this.force_no_token = !this.force_no_token),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    gtk::init()?;

    let window = gtk::Window::builder()
        .title("XDG Activation Token Example")
        .build();

    let label = gtk::Label::builder()
        .label("Switch away from this window, then use the tray to bring it back.")
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .build();
    window.set_child(Some(&label));

    window.connect_close_request(|window| {
        eprintln!("window close requested, hiding it");
        window.set_visible(false);
        glib::Propagation::Stop
    });

    let weak: glib::WeakRef<gtk::Window> = glib::WeakRef::new();
    weak.set(Some(&window));
    let tray_window = glib::SendWeakRef::from(weak);

    window.present();
    eprintln!("window presented");

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    tokio_runtime.spawn(async move {
        (ActivationTray {
            window: tray_window,
            force_no_token: false,
        })
        .spawn()
        .await
        .unwrap()
    });

    // The GTK main loop must run on the main thread.
    glib::MainLoop::new(None, false).run();
    eprintln!("GTK main loop exited");

    Ok(())
}
