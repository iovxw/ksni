use std::sync::LazyLock;

use image::GenericImageView;
use ksni::TrayMethods; // provides the spawn method

#[derive(Debug)]
struct MyTray;

impl ksni::Tray for MyTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        static ICON: LazyLock<ksni::Icon> = LazyLock::new(|| {
            let img = image::load_from_memory_with_format(
                include_bytes!("screenshot_of_example_in_gnome.png"),
                image::ImageFormat::Png,
            )
            .expect("valid image");
            let (width, height) = img.dimensions();
            let mut data = img.into_rgba8().into_vec();
            assert_eq!(data.len() % 4, 0);
            for pixel in data.chunks_mut(4) {
                pixel.rotate_right(1) // rgba to argb
            }
            ksni::Icon {
                width: width as i32,
                height: height as i32,
                data,
            }
        });

        // A clone is a waste for static icon, but the API have to accommodate dynamically generated
        // icons, and keep simplicity
        vec![ICON.clone()]
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    MyTray.spawn().await.unwrap();

    // Run forever
    std::future::pending().await
}
