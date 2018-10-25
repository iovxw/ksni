use std::collections::HashMap;
use std::fmt;

use dbus::arg::{RefArg, Variant};

#[derive(Debug)]
pub struct MenuItem {
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
    pub submenu: Vec<MenuItem>,
    pub vendor_properties: HashMap<VendorSpecific, Variant<Box<dyn RefArg + 'static>>>,
}

impl Clone for MenuItem {
    fn clone(&self) -> Self {
        let vendor_properties = self
            .vendor_properties
            .iter()
            .map(|(k, v)| (k.clone(), Variant(v.0.box_clone())))
            .collect();

        Self {
            r#type: self.r#type.clone(),
            label: self.label.clone(),
            enabled: self.enabled,
            visible: self.visible,
            icon_name: self.icon_name.clone(),
            icon_data: self.icon_data.clone(),
            shortcut: self.shortcut.clone(),
            toggle_type: self.toggle_type,
            toggle_state: self.toggle_state,
            submenu: self.submenu.clone(),
            vendor_properties,
        }
    }
}

macro_rules! if_not_default_then_insert {
    ($map: ident, $item: ident, $default: ident, $property: ident) => {
        if_not_default_then_insert!($map, $item, $default, $property, (|r| r));
    };
    ($map: ident, $item: ident, $default: ident, $property: ident, $to_refarg: tt) => {
        if $item.$property != $default.$property {
            $map.insert(
                stringify!($property).to_string(),
                Variant(Box::new($to_refarg($item.$property))),
            );
        }
    };
}

impl From<MenuItem> for DBusMenuItem {
    fn from(item: MenuItem) -> Self {
        let mut properties: HashMap<String, Variant<Box<dyn RefArg + 'static>>> =
            HashMap::with_capacity(11);

        let default = MenuItem::default();
        if item.r#type != default.r#type {
            properties.insert("type".into(), Variant(Box::new(item.r#type.to_string())));
        }
        if_not_default_then_insert!(properties, item, default, label);
        if_not_default_then_insert!(properties, item, default, enabled);
        if_not_default_then_insert!(properties, item, default, visible);
        if_not_default_then_insert!(properties, item, default, icon_name);
        if_not_default_then_insert!(properties, item, default, icon_data);
        if_not_default_then_insert!(properties, item, default, shortcut);
        if_not_default_then_insert!(
            properties,
            item,
            default,
            toggle_type,
            (|r: ToggleType| r.to_string())
        );
        if_not_default_then_insert!(properties, item, default, toggle_state, (|r| r as i32));

        if !item.submenu.is_empty() {
            properties.insert(
                "children_display".into(),
                Variant(Box::new("submenu".to_owned())),
            );
        }

        for (k, v) in item.vendor_properties {
            properties.insert(k.to_string(), v);
        }

        // FIXME
        let submenu = item.submenu.into_iter().map(Self::from).collect();

        DBusMenuItem {
            id: 0,
            properties,
            submenu,
        }
    }
}

impl Default for MenuItem {
    fn default() -> Self {
        Self {
            r#type: ItemType::Standard,
            label: String::default(),
            enabled: true,
            visible: true,
            icon_name: String::default(),
            icon_data: Vec::default(),
            shortcut: Vec::default(),
            toggle_type: ToggleType::Null,
            toggle_state: ToggleState::Indeterminate,
            submenu: Vec::default(),
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
            Sepatator => f.write_str("sepatator"),
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

#[derive(Debug)]
pub struct DBusMenu {
    pub inner: Vec<MenuItem>,
}

pub struct DBusMenuItem {
    id: i32,
    properties: HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
    submenu: Vec<DBusMenuItem>,
}

impl From<Vec<MenuItem>> for DBusMenuItem {
    fn from(items: Vec<MenuItem>) -> Self {
        let mut root: HashMap<String, Variant<Box<dyn RefArg + 'static>>> =
            HashMap::with_capacity(1);
        root.insert(
            "children_display".into(),
            Variant(Box::new("submenu".to_owned())),
        );
        Self {
            id: 0,
            properties: root,
            submenu: items.into_iter().map(Self::from).collect(),
        }
    }
}

impl From<DBusMenuItem>
    for (
        i32,
        HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
        Vec<Variant<Box<dyn RefArg + 'static>>>,
    )
{
    fn from(menu: DBusMenuItem) -> Self {
        (
            menu.id,
            menu.properties,
            menu.submenu
                .into_iter()
                // FIXME
                .map(|menu| Variant(Box::new(Self::from(menu)) as Box<dyn RefArg>))
                .collect(),
        )
    }
}

impl crate::dbus_interface::Dbusmenu for DBusMenu {
    type Err = dbus::tree::MethodErr;
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<&str>,
    ) -> Result<
        (
            u32,
            (
                i32,
                HashMap<String, Variant<Box<dyn RefArg + 'static>>>,
                Vec<Variant<Box<dyn RefArg + 'static>>>,
            ),
        ),
        Self::Err,
    > {
        Ok((0, DBusMenuItem::from(self.inner.clone()).into()))
    }
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<&str>,
    ) -> Result<Vec<(i32, HashMap<String, Variant<Box<dyn RefArg + 'static>>>)>, Self::Err> {
        unimplemented!()
    }
    fn get_property(
        &self,
        id: i32,
        name: &str,
    ) -> Result<Variant<Box<dyn RefArg + 'static>>, Self::Err> {
        unimplemented!()
    }
    fn event(
        &self,
        id: i32,
        event_id: &str,
        data: Variant<Box<dyn RefArg>>,
        timestamp: u32,
    ) -> Result<(), Self::Err> {
        unimplemented!()
    }
    fn event_group(
        &self,
        events: Vec<(i32, &str, Variant<Box<dyn RefArg>>, u32)>,
    ) -> Result<Vec<i32>, Self::Err> {
        unimplemented!()
    }
    fn about_to_show(&self, id: i32) -> Result<bool, Self::Err> {
        unimplemented!()
    }
    fn about_to_show_group(&self, ids: Vec<i32>) -> Result<(Vec<i32>, Vec<i32>), Self::Err> {
        unimplemented!()
    }
    fn get_version(&self) -> Result<u32, Self::Err> {
        unimplemented!()
    }
    fn get_text_direction(&self) -> Result<String, Self::Err> {
        unimplemented!()
    }
    fn get_status(&self) -> Result<String, Self::Err> {
        unimplemented!()
    }
    fn get_icon_theme_path(&self) -> Result<Vec<String>, Self::Err> {
        unimplemented!()
    }
}
