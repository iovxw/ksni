# ksni [![Build Status](https://travis-ci.com/iovxw/ksni.svg?branch=master)](https://travis-ci.com/iovxw/ksni)

A Rust implementation of the KDE/freedesktop StatusNotifierItem specification

## Example

```
use ksni::{self, menu, tray};

fn main() {
    struct MyTray;
    impl ksni::Tray for MyTray {
        type Err = std::convert::Infallible;
        fn tray_properties() -> tray::Properties {
            tray::Properties {
                icon_name: "music".to_owned(),
                ..Default::default()
            }
        }
        fn menu() -> Vec<menu::MenuItem> {
            use menu::*;
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
                MenuItem::Sepatator,
                CheckmarkItem {
                    label: "Checkable".into(),
                    checked: true,
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Exit".into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|| std::process::exit(0)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    ksni::run(MyTray);
}
```

Will create a system tray like this:

![screenshot_of_example_in_gnome.png](examples/screenshot_of_example_in_gnome.png)

(In GNOME with AppIndicator extension)

## Todo
 - [x] org.kde.StatusNotifierItem
 - [x] com.canonical.dbusmenu
 - [x] org.freedesktop.DBus.Introspectable
 - [x] org.freedesktop.DBus.Properties
 - [ ] radio item
 - [ ] documents
 - [ ] async [diwic/dbus-rs#166](https://github.com/diwic/dbus-rs/issues/166)
 - [ ] mutable menu items

## License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.
