use serde::{Deserialize, Serialize};
use std::fmt;
use zbus::zvariant::{Type, Value};

/// Represent the horizontal or vertical orientation of the scroll request
// In org.freedesktop.StatusNotifierItem it's "horizontal" and "vertical"
// In org.kde.StatusNotifierItem it's "Horizontal" and "Vertical"
// GNOME:
// https://github.com/ubuntu/gnome-shell-extension-appindicator/blob/557dbddc8d469d1aaa302e6cf70600855dd767d1/appIndicator.js#L840-L861
// KDE:
// https://github.com/KDE/plasma-workspace/blob/4a98130f76bcae4211d3f9b10e4a7b760613ffc6/applets/systemtray/package/contents/ui/items/StatusNotifierItem.qml#L99-L115
#[derive(Copy, Clone, Debug, Eq, PartialEq, Type, Deserialize)]
#[zvariant(signature = "s")]
pub enum Orientation {
    #[serde(alias = "horizontal")]
    Horizontal,
    #[serde(alias = "vertical")]
    Vertical,
}

/// Category of this item.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Type, Serialize)]
#[zvariant(signature = "s")]
pub enum Category {
    /// The item describes the status of a generic application, for instance
    /// the current state of a media player. In the case where the category of
    /// the item can not be known, such as when the item is being proxied from
    /// another incompatible or emulated system, ApplicationStatus can be used
    /// a sensible default fallback.
    ApplicationStatus,
    /// The item describes the status of communication oriented applications,
    /// like an instant messenger or an email client.
    Communications,
    /// The item describes services of the system not seen as a stand alone
    /// application by the user, such as an indicator for the activity of a disk
    /// indexing service.
    SystemServices,
    /// The item describes the state and control of a particular hardware,
    /// such as an indicator of the battery charge or sound card volume control.
    Hardware,
}

// The Value dervie macro can only handle `dict` or `a{sv}` values
// so we impl it manually
impl From<Category> for Value<'_> {
    fn from(value: Category) -> Self {
        value.to_string().into()
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        self.serialize(f)
    }
}

/// Status of this item or of the associated application.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Type, Serialize)]
#[zvariant(signature = "s")]
pub enum Status {
    /// The item doesn't convey important information to the user, it can be
    /// considered an "idle" status and is likely that visualizations will chose
    /// to hide it.
    Passive,
    /// The item is active, is more important that the item will be shown in
    /// some way to the user.
    Active,
    /// The item carries really important information for the user, such as
    /// battery charge running out and is wants to incentive the direct user
    /// intervention. Visualizations should emphasize in some way the items with
    /// NeedsAttention status.
    NeedsAttention,
}

// The Value dervie macro can only handle `dict` or `a{sv}` values
// so we impl it manually
impl From<Status> for Value<'_> {
    fn from(value: Status) -> Self {
        value.to_string().into()
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        self.serialize(f)
    }
}

/// Extra information associated to the item
///
/// That can be visualized for instance by a tooltip (or by any other mean the
/// visualization consider appropriate.
///
/// See [`Tray::tool_tip`]
///
/// [`Tray::tool_tip`]: crate::Tray::tool_tip
#[derive(Clone, Debug, Default, Hash, Type, Value, Serialize)]
pub struct ToolTip {
    /// Freedesktop-compliant name for an icon.
    pub icon_name: String,
    /// Icon data
    pub icon_pixmap: Vec<Icon>,
    /// Title for this tooltip
    pub title: String,
    /// Descriptive text for this tooltip. It can contain also a subset of the
    /// HTML markup language, for a list of allowed tags see Section Markup.
    pub description: String,
}

/// An ARGB32 image
///
/// # Example
///
/// A static inlined icon using [image crate]:
///
/// ```
/// # use std::sync::LazyLock;
/// # use image::GenericImageView;
/// #
/// static ICON: LazyLock<ksni::Icon> = LazyLock::new(|| {
///     let img = image::load_from_memory_with_format(
///         include_bytes!("../examples/custom_icon.png"),
///         image::ImageFormat::Png,
///     )
///     .expect("valid image");
///     let (width, height) = img.dimensions();
///     let mut data = img.into_rgba8().into_vec();
///     assert_eq!(data.len() % 4, 0);
///     for pixel in data.chunks_exact_mut(4) {
///         pixel.rotate_right(1) // rgba to argb
///     }
///     ksni::Icon {
///         width: width as i32,
///         height: height as i32,
///         data,
///     }
/// });
/// ```
///
/// [image crate]: https://crates.io/crates/image/
#[derive(Clone, Debug, Hash, Type, Value, Serialize)]
pub struct Icon {
    pub width: i32,
    pub height: i32,
    /// ARGB32 format, network byte order
    pub data: Vec<u8>,
}
