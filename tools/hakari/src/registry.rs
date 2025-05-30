// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use iddqd::{BiHashItem, bi_upcast};

#[derive(Clone, Debug)]
pub(crate) struct Registry {
    pub(crate) name: String,
    pub(crate) url: String,
}

impl BiHashItem for Registry {
    type K1<'a> = &'a str;
    type K2<'a> = &'a str;

    fn key1(&self) -> Self::K1<'_> {
        &self.name
    }

    fn key2(&self) -> Self::K2<'_> {
        &self.url
    }

    bi_upcast!();
}
