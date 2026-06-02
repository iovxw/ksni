mod async_tests;
mod common;

async_tests::async_protocol_tests!(tokio::test);
