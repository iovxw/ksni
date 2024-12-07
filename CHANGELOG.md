# 0.3.1 (2024-12-07)

- Fixed compatibility of `Orientation` with org.kde.StatusNotifierItem, previously only with org.freedesktop.StatusNotifierItem
- Documentation updates

# 0.3.0 (2024-12-05)

Replaced dbus-rs with zbus, got async

All methods of `TrayService` have been moved into `TrayMethods`. `TrayMethods` is a trait that is
implemented by default for all `T where T: Tray` ([RFC #445]), so you no longer need to wrap a
`Tray` with `TrayService` to call the spawn method.

The new `spawn` method returns a `Result<Handle, Error>`. Any error during the tray creation is
returned directly. If the spawn succeeds, tray is created. No longer need to impl `watcher_online`
and `watcher_offline` to handle the result of a spawned tray.

The `run` method has been removed, no one's actually using it. With this change, we don't have to
provide a separate method to return the `Handle`, it can be returned directly by the spawn method.

Big thanks to [@lunixbochs](https://github.com/lunixbochs)

[RFC #445]: https://rust-lang.github.io/rfcs/0445-extension-trait-conventions.html

## Added

- `TrayMethods`
- `OfflineReason`, see below #Changed
- `Orientation`
- `blocking::*` for blocking API
- `Tray::MENU_ON_ACTIVATE` for the org.freedesktop.StatusNotifierItem.ItemIsMenu

## Removed

- `TrayService`, see the new `TrayMethods`
- Deprecated methods in 0.2

## Changed

- All methods that should be async are now async
- `Tray` now requires `Send`. If you are using `.spawn`, this won't affect you.
- `Tray::id` is a required method now, default impl removed
- `Tray::scroll(&mut self, i32, &str)` -> `Tray::scroll(&mut self, i32, Orientation)`
- `Tray::watcher_offline` have a new `OfflineReason` argument
- `Tray::watcher_online` or `Tray::watcher_offline` won't be called immediately after tray started,
now only be called after the state of watcher changed

# 0.2.2 (2024-04-27)

## New methods

- `TrayService::run_without_dbus_name`
- `TrayService::spwan_without_dbus_name`

See https://github.com/iovxw/ksni/pull/25 for details
