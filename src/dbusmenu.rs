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
    ($map: ident, $item: ident, $default: ident, $filter: ident, $property: ident) => {
        if_not_default_then_insert!($map, $item, $default, $filter, $property, (|r| r));
    };
    ($map: ident, $item: ident, $default: ident, $filter: ident, $property: ident, $to_refarg: tt) => {{
        let name = stringify!($property);
        if_not_default_then_insert!($map, $item, $default, $filter, $property, name, $to_refarg);
    }};
    ($map: ident, $item: ident, $default: ident, $filter: ident, $property: ident, $property_name: tt, $to_refarg: tt) => {
        if ($filter.is_empty() || $filter.contains(&$property_name))
            && $item.$property != $default.$property
        {
            $map.insert(
                $property_name.to_string(),
                Variant(Box::new($to_refarg($item.$property))),
            );
        }
    };
}

impl MenuItem {
    fn to_dbus_map(&self, filter: &[&str]) -> HashMap<String, Variant<Box<dyn RefArg + 'static>>> {
        let item = self.clone();
        let mut properties: HashMap<String, Variant<Box<dyn RefArg + 'static>>> =
            HashMap::with_capacity(11);

        let default = MenuItem::default();
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

#[derive(Debug, Clone)]
pub struct DBusMenu {
    // A list of menu item and it's submenu
    pub list: Vec<(MenuItem, Vec<usize>)>,
}

impl From<Vec<MenuItem>> for DBusMenu {
    fn from(items: Vec<MenuItem>) -> Self {
        let mut list: Vec<(MenuItem, Vec<usize>)> =
            vec![(MenuItem::default(), Vec::with_capacity(items.len()))];

        let mut stack = vec![(items, 0)]; // (menu, menu's parent)

        while let Some((mut current_menu, parent_index)) = stack.pop() {
            while !current_menu.is_empty() {
                let mut item = current_menu.remove(0);
                let mut submenu = Vec::new();
                std::mem::swap(&mut item.submenu, &mut submenu);
                let index = list.len();
                list.push((item, Vec::with_capacity(submenu.len())));
                // Add self to parent's submenu
                list[parent_index].1.push(index);
                if !submenu.is_empty() {
                    stack.push((current_menu, parent_index));
                    stack.push((submenu, index));
                    break;
                }
            }
        }

        DBusMenu { list }
    }
}

fn to_dbusmenu_variant(
    menu: &[(MenuItem, Vec<usize>)],
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
        dbg!((parent_id, recursion_depth, &property_names));
        Ok((
            0,
            dbg!(to_dbusmenu_variant(
                &self.list,
                parent_id as usize,
                if recursion_depth < 0 {
                    None
                } else {
                    Some(recursion_depth as usize)
                },
                property_names,
            )),
        ))
    }
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<&str>,
    ) -> Result<Vec<(i32, HashMap<String, Variant<Box<dyn RefArg + 'static>>>)>, Self::Err> {
        dbg!(("get_group_properties", &ids, &property_names));
        let r = ids
            .into_iter()
            .map(|id| (id, self.list[id as usize].0.to_dbus_map(&property_names)))
            .collect();
        Ok(dbg!(r))
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
        dbg!((id, event_id, data, timestamp));
        Ok(())
    }
    fn event_group(
        &self,
        events: Vec<(i32, &str, Variant<Box<dyn RefArg>>, u32)>,
    ) -> Result<Vec<i32>, Self::Err> {
        unimplemented!()
    }
    fn about_to_show(&self, id: i32) -> Result<bool, Self::Err> {
        dbg!(("about to show", id));
        Ok(false)
    }
    fn about_to_show_group(&self, ids: Vec<i32>) -> Result<(Vec<i32>, Vec<i32>), Self::Err> {
        unimplemented!()
    }
    fn get_version(&self) -> Result<u32, Self::Err> {
        Ok(0)
    }
    fn get_text_direction(&self) -> Result<String, Self::Err> {
        Ok("ltr".into())
    }
    fn get_status(&self) -> Result<String, Self::Err> {
        Ok("normal".into())
    }
    fn get_icon_theme_path(&self) -> Result<Vec<String>, Self::Err> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_menuitem_list_to_dbusmenu() {
        let x = vec![
            MenuItem {
                label: "a".into(),
                submenu: vec![
                    MenuItem {
                        label: "a1".into(),
                        submenu: vec![MenuItem {
                            label: "a1.1".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    MenuItem {
                        label: "a2".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            MenuItem {
                label: "b".into(),
                ..Default::default()
            },
            MenuItem {
                label: "c".into(),
                submenu: vec![
                    MenuItem {
                        label: "c1".into(),
                        ..Default::default()
                    },
                    MenuItem {
                        label: "c2".into(),
                        submenu: vec![MenuItem {
                            label: "c2.1".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ];

        let r: DBusMenu = x.into();
        let expect = DBusMenu {
            list: vec![
                (
                    MenuItem {
                        label: "".into(),
                        ..Default::default()
                    },
                    vec![1, 5, 6],
                ),
                (
                    MenuItem {
                        label: "a".into(),
                        ..Default::default()
                    },
                    vec![2, 4],
                ),
                (
                    MenuItem {
                        label: "a1".into(),
                        ..Default::default()
                    },
                    vec![3],
                ),
                (
                    MenuItem {
                        label: "a1.1".into(),
                        ..Default::default()
                    },
                    vec![],
                ),
                (
                    MenuItem {
                        label: "a2".into(),
                        ..Default::default()
                    },
                    vec![],
                ),
                (
                    MenuItem {
                        label: "b".into(),
                        ..Default::default()
                    },
                    vec![],
                ),
                (
                    MenuItem {
                        label: "c".into(),
                        ..Default::default()
                    },
                    vec![7, 8],
                ),
                (
                    MenuItem {
                        label: "c1".into(),
                        ..Default::default()
                    },
                    vec![],
                ),
                (
                    MenuItem {
                        label: "c2".into(),
                        ..Default::default()
                    },
                    vec![9],
                ),
                (
                    MenuItem {
                        label: "c2.1".into(),
                        ..Default::default()
                    },
                    vec![],
                ),
            ],
        };
        assert_eq!(r.list.len(), 10);
        assert_eq!(r.list[0].1, expect.list[0].1);
        assert_eq!(r.list[1].1, expect.list[1].1);
        assert_eq!(r.list[2].1, expect.list[2].1);
        assert_eq!(r.list[3].1, expect.list[3].1);
        assert_eq!(r.list[4].1, expect.list[4].1);
        assert_eq!(r.list[5].1, expect.list[5].1);
        assert_eq!(r.list[6].1, expect.list[6].1);
        assert_eq!(r.list[7].1, expect.list[7].1);
        assert_eq!(r.list[8].1, expect.list[8].1);
        assert_eq!(r.list[9].1, expect.list[9].1);
        assert_eq!(r.list[0].0.label, expect.list[0].0.label);
        assert_eq!(r.list[1].0.label, expect.list[1].0.label);
        assert_eq!(r.list[2].0.label, expect.list[2].0.label);
        assert_eq!(r.list[3].0.label, expect.list[3].0.label);
        assert_eq!(r.list[4].0.label, expect.list[4].0.label);
        assert_eq!(r.list[5].0.label, expect.list[5].0.label);
        assert_eq!(r.list[6].0.label, expect.list[6].0.label);
        assert_eq!(r.list[7].0.label, expect.list[7].0.label);
        assert_eq!(r.list[8].0.label, expect.list[8].0.label);
        assert_eq!(r.list[9].0.label, expect.list[9].0.label);
    }
}
