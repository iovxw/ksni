mod async_tests;
mod common;

use macro_rules_attribute::apply;
use smol_macros::test;

async_tests::async_protocol_tests!(apply(test!));
