mod mock;

use macro_rules_attribute::apply;
use smol_macros::test;

mock::async_protocol_tests!(apply(test!));
