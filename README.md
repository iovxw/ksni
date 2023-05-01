# ksni 

[![Build Status](https://github.com/iovxw/ksni/workflows/Rust/badge.svg)](https://github.com/iovxw/ksni/actions?query=workflow%3ARust)
[![Crates](https://img.shields.io/crates/v/ksni.svg)](https://crates.io/crates/ksni)
[![Documentation](https://docs.rs/ksni/badge.svg)](https://docs.rs/ksni)

A Rust implementation of the KDE/freedesktop StatusNotifierItem specification

## Example

```rust
use ksni;

#[derive(Debug)]
struct MyTray {
    selected_option: usize,
    checked: bool,
}

impl ksni::Tray for MyTray {
    fn icon_name(&self) -> String {
        "help-about".into()
    }
    fn title(&self) -> String {
        if self.checked { "CHECKED!" } else { "MyTray" }.into()
    }
    fn id(&self) -> String {
        "com.example.MyApplicationId".into()
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

fn main() {
    let service = ksni::TrayService::new(MyTray {
        selected_option: 0,
        checked: false,
    });
    let handle = service.handle();
    service.spawn();

    std::thread::sleep(std::time::Duration::from_secs(5));
    // We can modify the tray
    handle.update(|tray: &mut MyTray| {
        tray.checked = true;
    });
    // Run forever
    loop {
        std::thread::park();
    }
}
```

Will create a system tray like this:

![screenshot_of_example_in_gnome.png](examples/screenshot_of_example_in_gnome.png)

(In GNOME with AppIndicator extension)

## Todo
 - [X] org.kde.StatusNotifierItem
 - [X] com.canonical.dbusmenu
 - [X] org.freedesktop.DBus.Introspectable
 - [X] org.freedesktop.DBus.Properties
 - [X] radio item
 - [ ] documents
 - [ ] async [diwic/dbus-rs#166](https://github.com/diwic/dbus-rs/issues/166)
 - [X] mutable menu items

## License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.
