use std::sync::Arc;

use super::{Disposition, MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu};

/// A program-friendly [`MenuItem`]
///
/// This enum is the "flat" counterpart of [`MenuItem`].
/// Each variant carries its fields directly instead of wrapping a separate
/// struct, the latter approach (used by [`MenuItem`]) enables
/// Rust's struct update syntax (..Default::default()) for human ergonomics,
/// while this enum is designed for wrapper libraries that construct menus
/// programmatically from their own data model.
///
/// Unlike [`MenuItem`], the callback fields here use [`Arc`] and
/// require [`Sync`].  This is the price of making the enum
/// [`Clone`]-able, which wrapper libraries often need.
///
/// Use `.into()` to convert a `Vec<Node<T>>` into a `Vec<MenuItem<T>>` for use with
/// [`Tray::menu`](crate::Tray::menu).
///
/// # Variants
///
/// * [`Standard`](Node::Standard) — a clickable item
/// * [`Separator`](Node::Separator) — a visual separator
/// * [`Checkmark`](Node::Checkmark) — a checkable item
/// * [`Submenu`](Node::Submenu) — an item that opens a sub‑menu
/// * [`RadioGroup`](Node::RadioGroup) — a group of mutually‑exclusive options
pub enum Node<T> {
    /// A standard clickable menu item
    Standard {
        /// Text of the item, except that:
        /// -# two consecutive underscore characters "__" are displayed as a
        /// single underscore,
        /// -# any remaining underscore characters are not displayed at all,
        /// -# the first of those remaining underscore characters (unless it is
        /// the last character in the string) indicates that the following
        /// character is the access key.
        label: String,
        /// Whether the item can be activated or not.
        enabled: bool,
        /// True if the item is visible in the menu.
        visible: bool,
        /// Icon name of the item, following the freedesktop.org icon spec.
        icon_name: String,
        /// PNG data of the icon.
        icon_data: Vec<u8>,
        /// The shortcut of the item. Each array represents the key press
        /// in the list of keypresses. Each list of strings contains a list of
        /// modifiers and then the key that is used. The modifier strings
        /// allowed are: "Control", "Alt", "Shift" and "Super".
        /// - A simple shortcut like Ctrl+S is represented as:
        ///   `[["Control", "S"]]`
        /// - A complex shortcut like Ctrl+Q, Alt+X is represented as:
        ///   `[["Control", "Q"], ["Alt", "X"]]`
        shortcut: Vec<Vec<String>>,
        /// How the menuitem feels the information it's displaying to the
        /// user should be presented.
        disposition: Disposition,
        /// Callback invoked when the item is activated.
        ///
        /// The callback receives a mutable reference to the [`Tray`] that
        /// owns this menu.  Keep the handler lightweight — offload blocking
        /// work to a channel or a spawned task.
        ///
        /// [`Tray`]: crate::Tray
        activate: Arc<dyn Fn(&mut T) + Send + Sync>,
    },
    /// A visual separator
    Separator,
    /// A checkable menu item
    Checkmark {
        /// Text of the item, except that:
        /// -# two consecutive underscore characters "__" are displayed as a
        /// single underscore,
        /// -# any remaining underscore characters are not displayed at all,
        /// -# the first of those remaining underscore characters (unless it is
        /// the last character in the string) indicates that the following
        /// character is the access key.
        label: String,
        /// Whether the item can be activated or not.
        enabled: bool,
        /// True if the item is visible in the menu.
        visible: bool,
        /// Whether the item is currently checked.
        checked: bool,
        /// Icon name of the item, following the freedesktop.org icon spec.
        icon_name: String,
        /// PNG data of the icon.
        icon_data: Vec<u8>,
        /// The shortcut of the item. Each array represents the key press
        /// in the list of keypresses. Each list of strings contains a list of
        /// modifiers and then the key that is used. The modifier strings
        /// allowed are: "Control", "Alt", "Shift" and "Super".
        /// - A simple shortcut like Ctrl+S is represented as:
        ///   `[["Control", "S"]]`
        /// - A complex shortcut like Ctrl+Q, Alt+X is represented as:
        ///   `[["Control", "Q"], ["Alt", "X"]]`
        shortcut: Vec<Vec<String>>,
        /// How the menuitem feels the information it's displaying to the
        /// user should be presented.
        disposition: Disposition,
        /// Callback invoked when the item is activated.
        ///
        /// See [`Standard::activate`](Node::Standard::activate).
        activate: Arc<dyn Fn(&mut T) + Send + Sync>,
    },
    /// An item that opens a sub‑menu
    Submenu {
        /// Text of the item, except that:
        /// -# two consecutive underscore characters "__" are displayed as a
        /// single underscore,
        /// -# any remaining underscore characters are not displayed at all,
        /// -# the first of those remaining underscore characters (unless it is
        /// the last character in the string) indicates that the following
        /// character is the access key.
        label: String,
        /// Whether the item can be activated or not.
        enabled: bool,
        /// True if the item is visible in the menu.
        visible: bool,
        /// Icon name of the item, following the freedesktop.org icon spec.
        icon_name: String,
        /// PNG data of the icon.
        icon_data: Vec<u8>,
        /// The shortcut of the item. Each array represents the key press
        /// in the list of keypresses. Each list of strings contains a list of
        /// modifiers and then the key that is used. The modifier strings
        /// allowed are: "Control", "Alt", "Shift" and "Super".
        /// - A simple shortcut like Ctrl+S is represented as:
        ///   `[["Control", "S"]]`
        /// - A complex shortcut like Ctrl+Q, Alt+X is represented as:
        ///   `[["Control", "Q"], ["Alt", "X"]]`
        shortcut: Vec<Vec<String>>,
        /// How the menuitem feels the information it's displaying to the
        /// user should be presented.
        disposition: Disposition,
        /// List of child menu items.
        submenu: Vec<Node<T>>,
    },
    /// A group of mutually‑exclusive radio options
    RadioGroup {
        /// Index of the currently selected radio item.
        selected: usize,
        /// Callback invoked when an item is selected.
        ///
        /// The first argument is a mutable reference to the [`Tray`] that
        /// owns this menu.  The second argument is the index of the clicked
        /// radio item.
        ///
        /// [`Tray`]: crate::Tray
        select: Arc<dyn Fn(&mut T, usize) + Send + Sync>,
        /// List of radio items.
        options: Vec<RadioItem>,
    },
}

impl<T: 'static> From<Node<T>> for MenuItem<T> {
    fn from(node: Node<T>) -> Self {
        match node {
            Node::Standard {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate,
            } => StandardItem {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate: Box::new(move |this| activate(this)),
            }
            .into(),
            Node::Separator => MenuItem::Separator,
            Node::Checkmark {
                label,
                enabled,
                visible,
                checked,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate,
            } => super::CheckmarkItem {
                label,
                enabled,
                visible,
                checked,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate: Box::new(move |this| activate(this)),
            }
            .into(),
            Node::Submenu {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                submenu,
            } => {
                let submenu_items: Vec<MenuItem<T>> =
                    submenu.into_iter().map(MenuItem::from).collect();
                SubMenu {
                    label,
                    enabled,
                    visible,
                    icon_name,
                    icon_data,
                    shortcut,
                    disposition,
                    submenu: submenu_items,
                }
                .into()
            }
            Node::RadioGroup {
                selected,
                select,
                options,
            } => RadioGroup {
                selected,
                select: Box::new(move |this, index| select(this, index)),
                options,
            }
            .into(),
        }
    }
}

impl<T> Clone for Node<T> {
    fn clone(&self) -> Self {
        match self {
            Node::Standard {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate,
            } => Node::Standard {
                label: label.clone(),
                enabled: *enabled,
                visible: *visible,
                icon_name: icon_name.clone(),
                icon_data: icon_data.clone(),
                shortcut: shortcut.clone(),
                disposition: *disposition,
                activate: activate.clone(),
            },
            Node::Separator => Node::Separator,
            Node::Checkmark {
                label,
                enabled,
                visible,
                checked,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate,
            } => Node::Checkmark {
                label: label.clone(),
                enabled: *enabled,
                visible: *visible,
                checked: *checked,
                icon_name: icon_name.clone(),
                icon_data: icon_data.clone(),
                shortcut: shortcut.clone(),
                disposition: *disposition,
                activate: activate.clone(),
            },
            Node::Submenu {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                submenu,
            } => Node::Submenu {
                label: label.clone(),
                enabled: *enabled,
                visible: *visible,
                icon_name: icon_name.clone(),
                icon_data: icon_data.clone(),
                shortcut: shortcut.clone(),
                disposition: *disposition,
                submenu: submenu.clone(),
            },
            Node::RadioGroup {
                selected,
                select,
                options,
            } => Node::RadioGroup {
                selected: *selected,
                select: select.clone(),
                options: options.clone(),
            },
        }
    }
}

impl<T> std::fmt::Debug for Node<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Node::Standard {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate,
            } => f
                .debug_struct("Standard")
                .field("label", label)
                .field("enabled", enabled)
                .field("visible", visible)
                .field("icon_name", icon_name)
                .field("icon_data", &format!("<{} bytes>", icon_data.len()))
                .field("shortcut", shortcut)
                .field("disposition", disposition)
                .field(
                    "activate",
                    &format_args!("<fn @ {:p}>", Arc::as_ptr(activate)),
                )
                .finish(),
            Node::Separator => f.debug_struct("Separator").finish(),
            Node::Checkmark {
                label,
                enabled,
                visible,
                checked,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                activate,
            } => f
                .debug_struct("Checkmark")
                .field("label", label)
                .field("enabled", enabled)
                .field("visible", visible)
                .field("checked", checked)
                .field("icon_name", icon_name)
                .field("icon_data", &format!("<{} bytes>", icon_data.len()))
                .field("shortcut", shortcut)
                .field("disposition", disposition)
                .field(
                    "activate",
                    &format_args!("<fn @ {:p}>", Arc::as_ptr(activate)),
                )
                .finish(),
            Node::Submenu {
                label,
                enabled,
                visible,
                icon_name,
                icon_data,
                shortcut,
                disposition,
                submenu,
            } => f
                .debug_struct("Submenu")
                .field("label", label)
                .field("enabled", enabled)
                .field("visible", visible)
                .field("icon_name", icon_name)
                .field("icon_data", &format!("<{} bytes>", icon_data.len()))
                .field("shortcut", shortcut)
                .field("disposition", disposition)
                .field("submenu", submenu)
                .finish(),
            Node::RadioGroup {
                selected,
                select,
                options,
            } => f
                .debug_struct("RadioGroup")
                .field("selected", selected)
                .field("select", &format_args!("<fn @ {:p}>", Arc::as_ptr(select)))
                .field("options", options)
                .finish(),
        }
    }
}
