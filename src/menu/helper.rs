use std::sync::Arc;

use super::{Disposition, MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu};

/// A program-friendly menu item
///
/// This enum is the "flat" counterpart of [`super::MenuItem`].
/// Each variant carries its fields directly instead of wrapping a separate
/// struct, the latter approach (used by [`super::MenuItem`]) enables
/// Rust's struct update syntax (..Default::default()) for human ergonomics,
/// while this enum is designed for wrapper libraries that construct menus
/// programmatically from their own data model.
///
/// Unlike [`super::MenuItem`], the callback fields here use [`Arc`] and
/// require [`Sync`].  This is the price of making the enum
/// [`Clone`]-able, which wrapper libraries often need.
///
/// Use `From<helper::Node<T>>` to convert a node tree into the
/// MenuItem tree that ksni expects.
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
