// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::collections::HashMap;

use common::DummyUpdateInfo;
use libupdated::traits::consent_handler::{AutoConsent, ConsentHandler};

mod common;

#[tokio::test]
async fn auto_consent() {
    let mut consent = AutoConsent::new(true);
    let update_info = DummyUpdateInfo {
        can_give_consent: true,
        metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
        version: "version".to_string(),
        needs_consent: true,
    };
    let result = consent.ask_for_consent(&update_info).await;
    assert_eq!(result.unwrap(), true);
}

#[tokio::test]
async fn auto_decline() {
    let mut consent = AutoConsent::new(false);
    let update_info = DummyUpdateInfo {
        can_give_consent: true,
        metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
        version: "version".to_string(),
        needs_consent: true,
    };
    let result = consent.ask_for_consent(&update_info).await;
    assert_eq!(result.unwrap(), false);
}
