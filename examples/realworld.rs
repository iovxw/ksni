use std::{io::Write, thread};

use ksni::TrayMethods;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Debug)]
struct ImeTray {
    selected_im: usize,
    available_ims: Vec<InputMethod>,
    switching_im: bool,
    notifier: UnboundedSender<TrayMessage>,
}

enum TrayMessage {
    SelectIm { im: InputMethod, index: usize },
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMethod {
    English,
    Chinese,
    Japanese,
}

impl ToString for InputMethod {
    fn to_string(&self) -> String {
        match self {
            InputMethod::English => "English".into(),
            InputMethod::Chinese => "Chinese".into(),
            InputMethod::Japanese => "Japanese".into(),
        }
    }
}

impl ksni::Tray for ImeTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }
    fn icon_name(&self) -> String {
        "keyboard".into()
    }
    fn title(&self) -> String {
        "Fake IME".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            SubMenu {
                label: "Input Method".into(),
                submenu: vec![RadioGroup {
                    selected: self.selected_im,
                    select: Box::new(|this: &mut Self, selected| {
                        this.switching_im = true;
                        this.notifier
                            .send(TrayMessage::SelectIm {
                                im: this.available_ims[selected],
                                index: selected,
                            })
                            .expect("main thread alive");
                    }),
                    options: self
                        .available_ims
                        .iter()
                        .map(|im| RadioItem {
                            label: im.to_string(),
                            enabled: !self.switching_im,
                            ..Default::default()
                        })
                        .collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Reload config".into(),
                icon_name: "reload".into(),
                activate: Box::new(|_| {
                    // run blocking operation in a new task
                    tokio::spawn(async {
                        tokio::process::Command::new("echo")
                            .arg("Reloading config...")
                            .status()
                            .await
                            .expect("command executed");
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| {
                    this.notifier
                        .send(TrayMessage::Exit)
                        .expect("main thread alive");
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let (notifier, mut tray_msgs) = tokio::sync::mpsc::unbounded_channel();
    let tray = ImeTray {
        selected_im: 0,
        available_ims: vec![InputMethod::English, InputMethod::Chinese],
        switching_im: false,
        notifier,
    };
    let handle = tray
        .disable_dbus_name(ashpd::is_sandboxed().await)
        .spawn()
        .await
        .unwrap();

    let mut stdin = stdin_lines();
    loop {
        println!("0. Exit, 1. Add a Japanese Input Method");
        print!("Please choose:\n> ");
        std::io::stdout().flush().unwrap();
        tokio::select! {
            Some(line) = stdin.recv() => {
                match &*line {
                    "0" => break Ok(()),
                    "1" => {
                        if handle
                            .update(|tray: &mut ImeTray| {
                                if tray.available_ims.contains(&InputMethod::Japanese) {
                                    false
                                } else {
                                    tray.available_ims.push(InputMethod::Japanese);
                                    true
                                }
                            })
                            .await.expect("tray running") {
                                println!("Done")
                            } else {
                                println!("Already added")
                            }
                    }
                    _ => continue,
                }
            }
            Some(msg) = tray_msgs.recv() => {
                match msg {
                    TrayMessage::SelectIm { im, index } => {
                        println!("Switching Input Method to {:?}...", im);
                        // do something to switch input method
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        // done, update the tray state
                        handle
                            .update(|tray: &mut ImeTray| {
                                tray.switching_im = false;
                                tray.selected_im = index;
                            })
                            .await;
                        println!("Selected Input Method: {:?}", im);
                    }
                    TrayMessage::Exit => {
                        break Ok(());
                    }
                }
            }
        }
    }
}

// tokio::io::stdin are not for interactive uses
// so we make our own here
// https://docs.rs/tokio/latest/tokio/io/fn.stdin.html
fn stdin_lines() -> UnboundedReceiver<String> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    thread::spawn(move || {
        for line in std::io::stdin().lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    rx
}
