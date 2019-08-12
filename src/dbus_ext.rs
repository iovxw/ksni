use std::cell::Cell;
use std::ptr::NonNull;

use dbus::blocking::Connection;

thread_local! {
    static CURRENT_DBUS_CONN: Cell<Option<NonNull<Connection>>> = Cell::new(None);
}

pub fn with_current<F: FnOnce(&Connection) -> R, R>(f: F) -> Option<R> {
    CURRENT_DBUS_CONN.with(|v| {
        v.get().map(|conn| {
            let conn = unsafe { conn.as_ref() };
            f(conn)
        })
    })
}

pub fn with_conn<F: FnOnce() -> R, R>(conn: &Connection, f: F) -> R {
    CURRENT_DBUS_CONN.with(|v| {
        let was = v.get();
        struct Reset(Option<NonNull<Connection>>);
        impl Drop for Reset {
            fn drop(&mut self) {
                CURRENT_DBUS_CONN.with(|v| v.set(self.0));
            }
        }
        let _reset = Reset(was);

        unsafe { v.set(Some(NonNull::new_unchecked(conn as *const _ as *mut _))) }
        f()
    })
}
