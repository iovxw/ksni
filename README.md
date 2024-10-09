# ksni 

[![Build Status](https://github.com/iovxw/ksni/workflows/Rust/badge.svg)](https://github.com/iovxw/ksni/actions?query=workflow%3ARust)
[![Crates](https://img.shields.io/crates/v/ksni.svg)](https://crates.io/crates/ksni)
[![Documentation](https://docs.rs/ksni/badge.svg)](https://docs.rs/ksni)
[![MSRV](https://img.shields.io/badge/msrv-1.58.0-blue)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field)

A Rust implementation of the KDE/freedesktop StatusNotifierItem specification

## Example

```rust
use ksni::TrayMethods; // provides the spawn method

#[derive(Debug)]
struct MyTray {
    selected_option: usize,
    checked: bool,
}

impl ksni::Tray for MyTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }
    fn icon_name(&self) -> String {
        "help-about".into()
    }
    fn title(&self) -> String {
        if self.checked { "CHECKED!" } else { "MyTray" }.into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            SubMenu {
                label: "a".into(),
                submenu: vec![
                    SubMenu {
                        label: "a1".into(),
                        submenu: vec![
                            StandardItem {
                                label: "a1.1".into(),
                                ..Default::default()
                            }
                            .into(),
                            StandardItem {
                                label: "a1.2".into(),
                                ..Default::default()
                            }
                            .into(),
                        ],
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "a2".into(),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            RadioGroup {
                selected: self.selected_option,
                select: Box::new(|this: &mut Self, current| {
                    this.selected_option = current;
                }),
                options: vec![
                    RadioItem {
                        label: "Option 0".into(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "Option 1".into(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "Option 2".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Checkable".into(),
                checked: self.checked,
                activate: Box::new(|this: &mut Self| this.checked = !this.checked),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let tray = MyTray {
        selected_option: 0,
        checked: false,
    };
    let handle = tray.spawn().await.unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    // We can modify the tray
    handle.update(|tray: &mut MyTray| tray.checked = true).await;
    // Run forever
    std::future::pending().await
}
```

Will create a system tray like this:

![screenshot_of_example_in_gnome.png](examples/screenshot_of_example_in_gnome.png)

(In GNOME with AppIndicator extension)

## Async Runtime

ksni uses [Tokio] by default, but can be runtime-agnostic by disabling the "tokio" feature and
enabling the "async-io" feature

```toml
[dependencies]
ksni = { version = "0.3", default-features = false, features = ["async-io"] }
```

## Blocking API

Enable the "blocking" feature in Cargo.toml to get a non-async API

```toml
[dependencies]
ksni = { version = "0.3", features = ["blocking"] }
```

[Tokio]: https://tokio.rs

## Todo
 - [X] org.kde.StatusNotifierItem
 - [X] com.canonical.dbusmenu
 - [X] org.freedesktop.DBus.Introspectable
 - [X] org.freedesktop.DBus.Properties
 - [X] radio item
 - [ ] documents
 - [X] async ~~[diwic/dbus-rs#166](https://github.com/diwic/dbus-rs/issues/166)~~ edit: zbus now
 - [X] mutable menu items

## License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.
