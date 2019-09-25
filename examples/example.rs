use ksni;

fn main() {
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
            if self.checked {
                "CHECKED!"
            } else {
                "MyTray"
            }.into()
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
                MenuItem::Sepatator,
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

    let service = ksni::TrayService::new(MyTray {
        selected_option: 0,
        checked: false,
    });
    let state = service.state();
    service.spawn();

    std::thread::sleep(std::time::Duration::from_secs(5));
    state.update(|state: &mut MyTray| {
        state.checked = true;
    });
    // Run forever
    loop {
        std::thread::park();
    }
}
