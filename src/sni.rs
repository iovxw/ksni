use std::fmt;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Properties {
    pub category: Category,
    pub id: String,
    pub title: String,
    pub status: Status,
    pub window_id: i32, // u32 in org.freedesktop.StatusNotifierItem
    pub icon_name: String,
    pub icon_pixmap: Vec<Icon>,
    pub overlay_icon_name: String,
    pub overlay_icon_pixmap: Vec<Icon>,
    pub attention_icon_name: String,
    pub attention_icon_pixmap: Vec<Icon>,
    pub attention_moive_name: String,
    pub tool_tip: ToolTip,

    conn: Rc<dbus::Connection>,
}

impl Properties {
    pub fn new(conn: Rc<dbus::Connection>) -> Self {
        Properties {
            category: Category::ApplicationStatus,
            id: Default::default(),
            title: Default::default(),
            status: Status::Active,
            window_id: 0,
            icon_name: Default::default(),
            icon_pixmap: Default::default(),
            overlay_icon_name: Default::default(),
            overlay_icon_pixmap: Default::default(),
            attention_icon_name: Default::default(),
            attention_icon_pixmap: Default::default(),
            attention_moive_name: Default::default(),
            tool_tip: Default::default(),
            conn,
        }
    }
}

/// Describes the category of this item.
#[derive(Copy, Clone, Debug)]
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

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let r = match *self {
            Category::ApplicationStatus => "ApplicationStatus",
            Category::Communications => "Communications",
            Category::SystemServices => "SystemServices",
            Category::Hardware => "Hardware",
        };
        f.write_str(r)
    }
}

/// Describes the status of this item or of the associated application.
#[derive(Copy, Clone, Debug)]
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

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let r = match *self {
            Status::Passive => "Passive",
            Status::Active => "Active",
            Status::NeedsAttention => "NeedsAttention",
        };
        f.write_str(r)
    }
}

/// Data structure that describes extra information associated to this item,
/// that can be visualized for instance by a tooltip (or by any other mean the
/// visualization consider appropriate.
#[derive(Clone, Debug, Default)]
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

impl From<ToolTip> for (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
    fn from(tooltip: ToolTip) -> Self {
        (
            tooltip.icon_name,
            tooltip.icon_pixmap.into_iter().map(Into::into).collect(),
            tooltip.title,
            tooltip.description,
        )
    }
}

#[derive(Clone, Debug)]
pub struct Icon {
    pub width: i32,
    pub height: i32,
    /// ARGB32 format, network byte order
    pub data: Vec<u8>,
}

impl From<Icon> for (i32, i32, Vec<u8>) {
    fn from(icon: Icon) -> Self {
        (icon.width, icon.height, icon.data)
    }
}