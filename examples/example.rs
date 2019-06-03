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
                RadioGroup {
                    select: Box::new(|prev, current| {
                        dbg!(prev, current);
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
