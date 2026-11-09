//! A blocking facade over the parts of zbus's async API that the synchronous
//! tests use.
//!
//! zbus 6 removed `zbus::blocking`. Each call here is polled to completion with
//! a plain executor, not `zbus::block_on`: with zbus's `tokio` feature on (the
//! default test run), `zbus::block_on` runs a single-threaded Tokio runtime that
//! drives its connections only while some thread is inside it, so a mock
//! watcher would stop answering between calls. Built outside any Tokio runtime,
//! a connection runs on zbus's default runtime instead, whose helper thread
//! keeps serving it in the background.

#![allow(dead_code)] // shared by every test binary; each uses a different subset

use std::time::Duration;

use futures_executor::block_on;
use futures_util::StreamExt;
use zbus::names::{BusName, InterfaceName, MemberName, WellKnownName};
use zbus::object_server::Interface;
use zbus::{Message, ObjectPath};

pub mod connection {
    use super::*;

    pub struct Builder<'a>(zbus::connection::Builder<'a>);

    impl<'a> Builder<'a> {
        pub fn session() -> Self {
            Self(zbus::connection::Builder::session())
        }

        pub fn method_timeout(self, timeout: Duration) -> Self {
            Self(self.0.method_timeout(timeout))
        }

        pub fn serve_at<P, I>(self, path: P, iface: I) -> Self
        where
            I: Interface,
            P: TryInto<ObjectPath<'a>>,
            P::Error: Into<zbus::Error>,
        {
            Self(self.0.serve_at(path, iface))
        }

        pub fn name<W>(self, well_known_name: W) -> Self
        where
            W: TryInto<WellKnownName<'a>>,
            W::Error: Into<zbus::Error>,
        {
            Self(self.0.name(well_known_name))
        }

        pub fn build(self) -> zbus::Result<Connection> {
            block_on(self.0.build()).map(Connection)
        }
    }
}

pub struct Connection(zbus::Connection);

impl Connection {
    pub fn close(self) -> zbus::Result<()> {
        block_on(self.0.close())
    }
}

pub struct Proxy<'a>(zbus::Proxy<'a>);

impl<'a> Proxy<'a> {
    pub fn new<D, P, I>(
        connection: &Connection,
        destination: D,
        path: P,
        interface: I,
    ) -> zbus::Result<Self>
    where
        D: TryInto<BusName<'a>>,
        P: TryInto<ObjectPath<'a>>,
        I: TryInto<InterfaceName<'a>>,
        D::Error: Into<zbus::Error>,
        P::Error: Into<zbus::Error>,
        I::Error: Into<zbus::Error>,
    {
        block_on(zbus::Proxy::new(
            &connection.0,
            destination,
            path,
            interface,
        ))
        .map(Self)
    }

    pub fn call<'m, M, B, R>(&self, method_name: M, body: &B) -> zbus::Result<R>
    where
        M: TryInto<MemberName<'m>>,
        M::Error: Into<zbus::Error>,
        B: serde::ser::Serialize + zbus::DynamicType,
        R: for<'d> zbus::wire::DynamicDeserialize<'d>,
    {
        block_on(self.0.call(method_name, body))
    }

    pub fn get_property<T>(&self, property_name: &str) -> zbus::Result<T>
    where
        T: serde::de::DeserializeOwned + zbus::Type,
    {
        block_on(self.0.get_property(property_name))
    }

    pub fn receive_signal_with_args<'m, M>(
        &self,
        signal_name: M,
        args: &[(u8, &str)],
    ) -> zbus::Result<SignalIterator<'m>>
    where
        M: TryInto<MemberName<'m>>,
        M::Error: Into<zbus::Error>,
    {
        block_on(self.0.receive_signal_with_args(signal_name, args)).map(SignalIterator)
    }
}

pub struct SignalIterator<'m>(zbus::proxy::SignalStream<'m>);

impl Iterator for SignalIterator<'_> {
    type Item = Message;

    fn next(&mut self) -> Option<Message> {
        block_on(self.0.next())
    }
}
