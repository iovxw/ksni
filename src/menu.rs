use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use dbus::arg::{RefArg, Variant};

pub struct Properties {
    /// Represents the way the text direction of the application.  This
    /// allows the server to handle mismatches intelligently.
    pub text_direction: TextDirection,
    /// Tells if the menus are in a normal state or they believe that they
    /// could use some attention.  Cases for showing them would be if help
    /// were referring to them or they accessors were being highlighted.
    /// This property can have two values: "normal" in almost all cases and
    /// "notice" when they should have a higher priority to be shown.
    pub status: Status,
    /// A list of directories that should be used for finding icons using
    /// the icon naming spec.  Idealy there should only be one for the icon
    /// theme, but additional ones are often added by applications for
    /// app specific icons.
    pub icon_theme_path: Vec<String>,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            text_direction: TextDirection::LeftToRight,
            status: Status::Normal,
            icon_theme_path: Default::default(),
        }
    }
}

pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

impl fmt::Display for TextDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let r = match *self {
            TextDirection::LeftToRight => "ltr",
            TextDirection::RightToLeft => "rtl",
        };
        f.write_str(r)
    }
}

pub enum Status {
    Normal,
    Notice,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let r = match *self {
            Status::Normal => "normal",
            Status::Notice => "notice",
        };
        f.write_str(r)
    }
}

pub enum MenuItem {
    Standard(StandardItem),
    Sepatator,
    Checkmark(CheckmarkItem),
    SubMenu(SubMenu),
    RadioGroup(RadioGroup),
}

pub struct StandardItem {
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon_name: String,
    pub icon_data: Vec<u8>,
    pub shortcut: Vec<Vec<String>>,
    pub disposition: ItemDisposition,
    pub activate: Box<Fn()>,
}

impl Default for StandardItem {
    fn default() -> Self {
        StandardItem {
            label: String::default(),
            enabled: true,
            visible: true,
            icon_name: String::default(),
            icon_data: Vec::default(),
            shortcut: Vec::default(),
            disposition: ItemDisposition::Normal,
            activate: Box::new(|| {}),
        }
    }
}

impl From<StandardItem> for MenuItem {
    fn from(item: StandardItem) -> Self {
        MenuItem::Standard(item)
    }
}

impl From<StandardItem> for RawMenuItem {
    fn from(item: StandardItem) -> Self {
        let activate = item.activate;
        Self {
            r#type: ItemType::Standard,
            label: item.label,
            enabled: item.enabled,
            visible: item.visible,
            icon_name: item.icon_name,
            icon_data: item.icon_data,
            shortcut: item.shortcut,
            disposition: item.disposition,
            on_clicked: Rc::new(move |_, _| {
                (activate)();
                Default::default()
            }),
            ..Default::default()
        }
    }
}

pub struct SubMenu {
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon_name: String,
    pub icon_data: Vec<u8>,
    pub shortcut: Vec<Vec<String>>,
    pub disposition: ItemDisposition,
    pub submenu: Vec<MenuItem>,
}

impl Default for SubMenu {
    fn default() -> Self {
        Self {
            label: String::default(),
            enabled: true,
            visible: true,
            icon_name: String::default(),
            icon_data: Vec::default(),
            shortcut: Vec::default(),
            disposition: ItemDisposition::Normal,
            submenu: Vec::default(),
        }
    }
}

impl From<SubMenu> for MenuItem {
    fn from(item: SubMenu) -> Self {
        MenuItem::SubMenu(item)
    }
}

impl From<SubMenu> for RawMenuItem {
    fn from(item: SubMenu) -> Self {
        Self {
            r#type: ItemType::Standard,
            label: item.label,
            enabled: item.enabled,
            visible: item.visible,
            icon_name: item.icon_name,
            icon_data: item.icon_data,
            shortcut: item.shortcut,
            disposition: item.disposition,
            on_clicked: Rc::new(move |_, _| Default::default()),
            ..Default::default()
        }
    }
}

pub struct CheckmarkItem {
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub checked: bool,
    pub icon_name: String,
    pub icon_data: Vec<u8>,
    pub shortcut: Vec<Vec<String>>,
    pub disposition: ItemDisposition,
    pub activate: Box<Fn(bool)>,
}

impl Default for CheckmarkItem {
    fn default() -> Self {
        CheckmarkItem {
            label: String::default(),
            enabled: true,
            visible: true,
            checked: false,
            icon_name: String::default(),
            icon_data: Vec::default(),
            shortcut: Vec::default(),
            disposition: ItemDisposition::Normal,
            activate: Box::new(|_| {}),
        }
    }
}

impl From<CheckmarkItem> for MenuItem {
    fn from(item: CheckmarkItem) -> Self {
        MenuItem::Checkmark(item)
    }
}

impl From<CheckmarkItem> for RawMenuItem {
    fn from(item: CheckmarkItem) -> Self {
        let activate = item.activate;
        Self {
            r#type: ItemType::Standard,
            label: item.label,
            enabled: item.enabled,
            visible: item.visible,
            icon_name: item.icon_name,
            icon_data: item.icon_data,
            shortcut: item.shortcut,
            toggle_type: ToggleType::Checkmark,
            toggle_state: if item.checked {
                ToggleState::On
            } else {
                ToggleState::Off
            },
            disposition: item.disposition,
            on_clicked: Rc::new(move |tree, id| {
                let this = &mut tree[id].0;
                if let ToggleState::Off = this.toggle_state {
                    this.toggle_state = ToggleState::On;
                    activate(true);
                } else {
                    this.toggle_state = ToggleState::Off;
                    activate(false);
                }
                crate::dbus_interface::DbusmenuItemsPropertiesUpdated {
                    updated_props: vec![(
                        id as i32,
                        to_dbusmenu_variant(&tree, id, Some(0), ["toggle-state"].to_vec()).1,
                    )],
                    removed_props: vec![],
                }
            }),
            ..Default::default()
        }
    }
}

pub struct RadioGroup {}

pub struct RawMenuItem {
    pub r#type: ItemType,
    /// Text of the item, except that:
    /// -# two consecutive underscore characters "__" are displayed as a
    /// single underscore,
    /// -# any remaining underscore characters are not displayed at all,
    /// -# the first of those remaining underscore characters (unless it is
    /// the last character in the string) indicates that the following
    /// character is the access key.
    pub label: String,
    /// Whether the item can be activated or not.
    pub enabled: bool,
    /// True if the item is visible in the menu.
    pub visible: bool,
    /// Icon name of the item, following the freedesktop.org icon spec.
    pub icon_name: String,
    /// PNG data of the icon.
    pub icon_data: Vec<u8>,
    /// The shortcut of the item. Each array represents the key press
    /// in the list of keypresses. Each list of strings contains a list of
    /// modifiers and then the key that is used. The modifier strings
    /// allowed are: "Control", "Alt", "Shift" and "Super".
    /// - A simple shortcut like Ctrl+S is represented as:
    ///   [["Control", "S"]]
    /// - A complex shortcut like Ctrl+Q, Alt+X is represented as:
    ///   [["Control", "Q"], ["Alt", "X"]]
    pub shortcut: Vec<Vec<String>>,
    pub toggle_type: ToggleType,
    /// Describe the current state of a "togglable" item.
    /// Note:
    /// The implementation does not itself handle ensuring that only one
    /// item in a radio group is set to "on", or that a group does not have
    /// "on" and "indeterminate" items simultaneously; maintaining this
    /// policy is up to the toolkit wrappers.
    pub toggle_state: ToggleState,
    /// How the menuitem feels the information it's displaying to the
    /// user should be presented.
    pub disposition: ItemDisposition,
    pub on_clicked: Rc<
        Fn(
            &mut Vec<(RawMenuItem, Vec<usize>)>,
            usize,
        ) -> crate::dbus_interface::DbusmenuItemsPropertiesUpdated,
    >,
    pub vendor_properties: HashMap<VendorSpecific, Variant<Box<dyn RefArg + 'static>>>,
}

impl Clone for RawMenuItem {
    fn clone(&self) -> Self {
        let vendor_properties = self
            .vendor_properties
            .iter()
            .map(|(k, v)| (k.clone(), Variant(v.0.box_clone())))
            .collect();

        RawMenuItem {
            r#type: self.r#type.clone(),
            label: self.label.clone(),
            enabled: self.enabled,
            visible: self.visible,
            icon_name: self.icon_name.clone(),
            icon_data: self.icon_data.clone(),
            shortcut: self.shortcut.clone(),
            toggle_type: self.toggle_type,
            toggle_state: self.toggle_state,
            disposition: self.disposition,
            on_clicked: self.on_clicked.clone(),
            vendor_properties,
        }
    }
}

macro_rules! if_not_default_then_insert {
    ($map: ident, $item: ident, $default: ident, $filter: ident, $property: ident) => {
        if_not_default_then_insert!($map, $item, $default, $filter, $property, (|r| r));
    };
    ($map: ident, $item: ident, $default: ident, $filter: ident, $property: ident, $to_refarg: tt) => {{
        let name = stringify!($property).replace('_', "-");
        if_not_default_then_insert!($map, $item, $default, $filter, $property, name, $to_refarg);
    }};
    ($map: ident, $item: ident, $default: ident, $filter: ident, $property: ident, $property_name: tt, $to_refarg: tt) => {
        if ($filter.is_empty() || $filter.contains(&&*$property_name))
            && $item.$property != $default.$property
        {
            $map.insert(
                $property_name.to_string(),
                Variant(Box::new($to_refarg($item.$property))),
            );
        }
    };
}

impl fmt::Debug for RawMenuItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Item {}", self.label)
    }
}

impl RawMenuItem {
    pub fn to_dbus_map(
        &self,
        filter: &[&str],
    ) -> HashMap<String, Variant<Box<dyn RefArg + 'static>>> {
        let item = self.clone();
        let mut properties: HashMap<String, Variant<Box<dyn RefArg + 'static>>> =
            HashMap::with_capacity(11);

        let default = RawMenuItem::default();
        if_not_default_then_insert!(
            properties,
            item,
            default,
            filter,
            r#type,
            "type",
            (|r: ItemType| r.to_string())
        );
        if_not_default_then_insert!(properties, item, default, filter, label);
        if_not_default_then_insert!(properties, item, default, filter, enabled);
        if_not_default_then_insert!(properties, item, default, filter, visible);
        if_not_default_then_insert!(properties, item, default, filter, icon_name);
        if_not_default_then_insert!(properties, item, default, filter, icon_data);
        if_not_default_then_insert!(properties, item, default, filter, shortcut);
        if_not_default_then_insert!(
            properties,
            item,
            default,
            filter,
            toggle_type,
            (|r: ToggleType| r.to_string())
        );
        if_not_default_then_insert!(
            properties,
            item,
            default,
            filter,
            toggle_state,
            (|r| r as i32)
        );

        for (k, v) in item.vendor_properties {
            properties.insert(k.to_string(), v);
        }

        properties
    }
}

impl Default for RawMenuItem {
    fn default() -> Self {
        RawMenuItem {
            r#type: ItemType::Standard,
            label: String::default(),
            enabled: true,
            visible: true,
            icon_name: String::default(),
            icon_data: Vec::default(),
            shortcut: Vec::default(),
            toggle_type: ToggleType::Null,
            toggle_state: ToggleState::Indeterminate,
            disposition: ItemDisposition::Normal,
            //submenu: Vec::default(),
            on_clicked: Rc::new(|_, _| Default::default()),
            vendor_properties: HashMap::default(),
        }
    }
}

/// Vendor specific types/properties
/// will be formatted to "x-<vendor>-<name>""
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub struct VendorSpecific {
    pub vendor: String,
    pub name: String,
}

impl fmt::Display for VendorSpecific {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "x-{}-{}", self.vendor, self.name)
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ItemType {
    /// an item which can be clicked to trigger an action or
    Standard,
    /// a separator
    Sepatator,
    /// Vendor specific types
    Vendor(VendorSpecific),
}

impl fmt::Display for ItemType {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use ItemType::*;
        match self {
            Standard => f.write_str("standard"),
            Sepatator => f.write_str("separator"),
            Vendor(vendor) => vendor.fmt(f),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ToggleType {
    /// Item is an independent togglable item
    Checkmark,
    /// Item is part of a group where only one item can be toggled at a time
    Radio,
    /// Item cannot be toggled
    Null,
}

impl fmt::Display for ToggleType {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use ToggleType::*;
        let r = match self {
            Checkmark => "checkmark",
            Radio => "radio",
            Null => "",
        };
        f.write_str(r)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ToggleState {
    Off = 0,
    On = 1,
    Indeterminate = -1,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ItemDisposition {
    /// A standard menu item
    Normal,
    /// Providing additional information to the user
    Informative,
    /// Looking at potentially harmful results
    Warning,
    /// Something bad could potentially happen
    Alert,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MenuStatus {
    Normal,
    Notice,
}

impl fmt::Display for MenuStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use MenuStatus::*;
        let r = match self {
            Normal => "normal",
            Notice => "notice",
        };
        f.write_str(r)
    }
}

pub fn menu_flatten(items: Vec<MenuItem>) -> Vec<(RawMenuItem, Vec<usize>)> {
    let mut list: Vec<(RawMenuItem, Vec<usize>)> =
        vec![(RawMenuItem::default(), Vec::with_capacity(items.len()))];

    let mut stack = vec![(items, 0)]; // (menu, menu's parent)

    while let Some((mut current_menu, parent_index)) = stack.pop() {
        while !current_menu.is_empty() {
            match current_menu.remove(0) {
                MenuItem::Standard(item) => {
                    let index = list.len();
                    list.push((item.into(), Vec::new()));
                    // Add self to parent's submenu
                    list[parent_index].1.push(index);
                }
                MenuItem::Sepatator => {
                    let item = RawMenuItem {
                        r#type: ItemType::Sepatator,
                        ..Default::default()
                    };
                    let index = list.len();
                    list.push((item, Vec::new()));
                    list[parent_index].1.push(index);
                }
                MenuItem::Checkmark(item) => {
                    let index = list.len();
                    list.push((item.into(), Vec::new()));
                    list[parent_index].1.push(index);
                }
                MenuItem::SubMenu(mut item) => {
                    let submenu = std::mem::replace(&mut item.submenu, Default::default());
                    let index = list.len();
                    list.push((item.into(), Vec::with_capacity(submenu.len())));
                    list[parent_index].1.push(index);
                    if !submenu.is_empty() {
                        stack.push((current_menu, parent_index));
                        stack.push((submenu, index));
                        break;
                    }
                }
                MenuItem::RadioGroup(group) => (),
            }
        }
    }

    list
}

pub fn to_dbusmenu_variant(
    menu: &[(RawMenuItem, Vec<usize>)],
    parent_id: usize,
    recursion_depth: Option<usize>,
    property_names: Vec<&str>,
) -> (
    i32,
    HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
    Vec<Variant<Box<dyn RefArg + 'static>>>,
) {
    if menu.is_empty() {
        return Default::default();
    }

    let mut x: Vec<
        Option<(
            i32,
            HashMap<String, Variant<Box<dyn RefArg>>>,
            Vec<Variant<Box<dyn RefArg>>>,
            Vec<usize>,
        )>,
    > = menu
        .into_iter()
        .enumerate()
        .map(|(id, (item, submenu))| {
            (
                id as i32,
                item.to_dbus_map(&property_names),
                Vec::with_capacity(submenu.len()),
                submenu.clone(),
            )
        })
        .map(Some)
        .collect();
    let mut stack = vec![parent_id];

    while let Some(current) = stack.pop() {
        let submenu = &mut x[current].as_mut().unwrap().3;
        if submenu.is_empty() {
            let c = x[current].as_mut().unwrap();
            if !c.2.is_empty() {
                c.1.insert(
                    "children-display".into(),
                    Variant(Box::new("submenu".to_owned())),
                );
            }
            if let Some(parent) = stack.pop() {
                x.push(None);
                let item = x.swap_remove(current).unwrap();
                stack.push(parent);
                x[parent]
                    .as_mut()
                    .unwrap()
                    .2
                    .push(Variant(Box::new((item.0, item.1, item.2))));
            }
        } else {
            stack.push(current);
            let sub = submenu.remove(0);
            if recursion_depth.is_none() || recursion_depth.unwrap() + 1 >= stack.len() {
                stack.push(sub);
            }
        }
    }

    let r = x.remove(parent_id).unwrap();
    (r.0, r.1, r.2)
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_menu_flatten() {
        let x = vec![
            SubMenu {
                label: "a".into(),
                submenu: vec![
                    SubMenu {
                        label: "a1".into(),
                        submenu: vec![StandardItem {
                            label: "a1.1".into(),
                            ..Default::default()
                        }
                        .into()],
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
            StandardItem {
                label: "b".into(),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "c".into(),
                submenu: vec![
                    StandardItem {
                        label: "c1".into(),
                        ..Default::default()
                    }
                    .into(),
                    SubMenu {
                        label: "c2".into(),
                        submenu: vec![StandardItem {
                            label: "c2.1".into(),
                            ..Default::default()
                        }
                        .into()],
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
        ];

        let r = menu_flatten(x);
        let expect = vec![
            (
                RawMenuItem {
                    label: "".into(),
                    ..Default::default()
                },
                vec![1, 5, 6],
            ),
            (
                RawMenuItem {
                    label: "a".into(),
                    ..Default::default()
                },
                vec![2, 4],
            ),
            (
                RawMenuItem {
                    label: "a1".into(),
                    ..Default::default()
                },
                vec![3],
            ),
            (
                RawMenuItem {
                    label: "a1.1".into(),
                    ..Default::default()
                },
                vec![],
            ),
            (
                RawMenuItem {
                    label: "a2".into(),
                    ..Default::default()
                },
                vec![],
            ),
            (
                RawMenuItem {
                    label: "b".into(),
                    ..Default::default()
                },
                vec![],
            ),
            (
                RawMenuItem {
                    label: "c".into(),
                    ..Default::default()
                },
                vec![7, 8],
            ),
            (
                RawMenuItem {
                    label: "c1".into(),
                    ..Default::default()
                },
                vec![],
            ),
            (
                RawMenuItem {
                    label: "c2".into(),
                    ..Default::default()
                },
                vec![9],
            ),
            (
                RawMenuItem {
                    label: "c2.1".into(),
                    ..Default::default()
                },
                vec![],
            ),
        ];
        assert_eq!(r.len(), 10);
        assert_eq!(r[0].1, expect[0].1);
        assert_eq!(r[1].1, expect[1].1);
        assert_eq!(r[2].1, expect[2].1);
        assert_eq!(r[3].1, expect[3].1);
        assert_eq!(r[4].1, expect[4].1);
        assert_eq!(r[5].1, expect[5].1);
        assert_eq!(r[6].1, expect[6].1);
        assert_eq!(r[7].1, expect[7].1);
        assert_eq!(r[8].1, expect[8].1);
        assert_eq!(r[9].1, expect[9].1);
        assert_eq!(r[0].0.label, expect[0].0.label);
        assert_eq!(r[1].0.label, expect[1].0.label);
        assert_eq!(r[2].0.label, expect[2].0.label);
        assert_eq!(r[3].0.label, expect[3].0.label);
        assert_eq!(r[4].0.label, expect[4].0.label);
        assert_eq!(r[5].0.label, expect[5].0.label);
        assert_eq!(r[6].0.label, expect[6].0.label);
        assert_eq!(r[7].0.label, expect[7].0.label);
        assert_eq!(r[8].0.label, expect[8].0.label);
        assert_eq!(r[9].0.label, expect[9].0.label);
    }
}
