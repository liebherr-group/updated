// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{collections::HashMap, time::Duration};

mod common;

use common::{DummyInstall, DummySource, DummyUpdateInfo, HashMapStore};
use libupdated::integrity_check::IntegrityCheckWithProgress;
use libupdated::progress_reporter::ProgressMessage;
use libupdated::trigger::{Immediately, Rate};
use libupdated::update_workflow::{
    UpdateError, UpdateOptions, consent_flow_no_install, monitor_update, update_with_consent_flow,
};
use libupdated::{
    checks::Timeout,
    consent_handler::AutoConsent,
    directory_installer::DirectoryInstaller,
    file_store::FileStore,
    integrity_check::IntegrityCheck,
    log_reporter::LogReporter,
    persistent_store::PersistentStore,
    progress_reporter::{UpdateState, VecReporter},
};
use rand::random;

const LOG: &str = "libupdated_tests";

#[tokio::test]
async fn test_update_workflow() {
    let file = format!("/tmp/update_status_{}.json", random::<u32>());
    tokio::fs::remove_file(&file).await.ok();

    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: true,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "1.0.0".to_string(),
            can_give_consent: true,
        }),
    };
    let mut consent = AutoConsent::new(true);
    let mut store = FileStore::new(&file);
    let mut progress = LogReporter::new(LOG);

    let result = consent_flow_no_install(
        &mut source,
        &mut consent,
        &mut store,
        &mut progress,
        UpdateOptions {
            ignore_previously_declined: true,
            consent_timeout: Duration::from_secs(10),
        },
    )
    .await;

    if result.is_err() {
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok());
    tokio::fs::remove_file(&file).await.ok();
}

#[tokio::test]
async fn test_declined_update() {
    let file = format!("/tmp/update_status_{}.json", random::<u32>());
    tokio::fs::remove_file(&file).await.ok();

    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: true,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "1.0.0".to_string(),
            can_give_consent: true,
        }),
    };
    let mut consent = AutoConsent::new(false);
    let mut store = FileStore::new(&file);
    let mut progress = LogReporter::new(LOG);

    let result = consent_flow_no_install(
        &mut source,
        &mut consent,
        &mut store,
        &mut progress,
        UpdateOptions {
            ignore_previously_declined: true,
            consent_timeout: Duration::from_secs(10),
        },
    )
    .await;

    assert!(result.is_err_and(|e| match e {
        UpdateError::ConsentDenied => true,
        _ => false,
    }));

    let result = store.load("1.0.0").await.unwrap().unwrap();
    assert!(result == "declined");

    // Reschedule the update, it should be skipped this time
    source.pending_update = Some(DummyUpdateInfo {
        needs_consent: true,
        metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
        version: "1.0.0".to_string(),
        can_give_consent: true,
    });

    let mut progress = LogReporter::new(LOG);

    let result = consent_flow_no_install(
        &mut source,
        &mut consent,
        &mut store,
        &mut progress,
        UpdateOptions {
            ignore_previously_declined: true,
            consent_timeout: Duration::from_secs(10),
        },
    )
    .await;

    assert!(result.is_err_and(|e| match e {
        UpdateError::NoPendingUpdates => true,
        _ => false,
    }));
    tokio::fs::remove_file(&file).await.ok();
}

#[tokio::test]
async fn test_declined_update_not_ignored() {
    let file = format!("/tmp/update_status_{}.json", random::<u32>());
    tokio::fs::write(&file, r#"{"2.0.0":"declined"}"#)
        .await
        .ok();

    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: true,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "2.0.0".to_string(),
            can_give_consent: true,
        }),
    };
    let mut consent = AutoConsent::new(false);
    let mut store = FileStore::new(&file);
    let mut progress = LogReporter::new(LOG);

    let result = consent_flow_no_install(
        &mut source,
        &mut consent,
        &mut store,
        &mut progress,
        UpdateOptions {
            ignore_previously_declined: false,
            consent_timeout: Duration::from_secs(10),
        },
    )
    .await;

    assert!(result.is_err_and(|e| match e {
        UpdateError::ConsentDenied => true,
        _ => false,
    }));
    tokio::fs::remove_file(&file).await.ok();
}

#[tokio::test]
async fn test_install_workflow() {
    let file = format!("/tmp/update_status_{}.json", random::<u32>());
    tokio::fs::remove_file(&file).await.ok();
    tokio::fs::remove_file("/tmp/test_install_workflow/dummy.txt")
        .await
        .ok();

    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: true,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "1.0.0".to_string(),
            can_give_consent: true,
        }),
    };
    let mut consent = AutoConsent::new(true);
    let mut store = FileStore::new(&file);
    let mut installer = DirectoryInstaller::new("/tmp/test_install_workflow");
    let mut progress = VecReporter::new();

    let result = update_with_consent_flow(
        &mut Rate::new(
            Duration::from_secs(1),
            UpdateOptions {
                ignore_previously_declined: true,
                consent_timeout: Duration::from_secs(10),
            },
        ),
        &mut source,
        &mut consent,
        &mut installer,
        &mut store,
        &mut progress,
    )
    .await;

    if result.is_err() {
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok());
    assert_eq!(progress.messages.len(), 4);
    assert_eq!(progress.messages[0].0, "1.0.0");
    assert_eq!(
        progress.messages[0].1.message,
        "Update consent given, starting update"
    );
    assert_eq!(
        progress.messages[1].1.message,
        "Status: success Message: installed update to /tmp/test_install_workflow"
    );
    assert_eq!(
        progress.messages[2].1.message,
        "Installation finished, saving state to prepare for reboot"
    );
    assert_eq!(
        progress.messages[3].1.message,
        "Update installed, checking success after next reboot"
    );
    assert_eq!(progress.messages[2].1.state, UpdateState::Pending);
    assert_eq!(
        tokio::fs::read_to_string("/tmp/test_install_workflow/dummy.txt")
            .await
            .unwrap(),
        "dummy content"
    );
    tokio::fs::remove_file(&file).await.ok();
}

struct SucceedingCheckWithProgress;
impl IntegrityCheckWithProgress for SucceedingCheckWithProgress {
    async fn system_is_ok_with_progress(&mut self) -> Result<Vec<ProgressMessage>, UpdateError> {
        let msg = vec![
            ProgressMessage {
                state: UpdateState::Pending,
                cnt_of: (1, 2),
                message: "Halfway there".to_string(),
            },
            ProgressMessage {
                state: UpdateState::Pending,
                cnt_of: (2, 2),
                message: "All done".to_string(),
            },
        ];
        Ok(msg)
    }
}

#[tokio::test]
async fn test_monitor_update() {
    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: true,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "25".to_string(),
            can_give_consent: true,
        }),
    };
    let mut consent = AutoConsent::new(true);
    let mut store = HashMapStore::default();
    let mut progress = VecReporter::new();

    let result = update_with_consent_flow(
        &mut Immediately::new(UpdateOptions {
            ignore_previously_declined: true,
            consent_timeout: Duration::from_secs(10),
        }),
        &mut source,
        &mut consent,
        &mut DummyInstall {},
        &mut store,
        &mut progress,
    )
    .await;

    assert!(result.is_ok());
    assert!(
        store
            .load("25")
            .await
            .unwrap()
            .is_some_and(|status| status == "installed")
    );
    assert!(
        store
            .load("update_needs_testing")
            .await
            .unwrap()
            .is_some_and(|version| version == "25")
    );

    // Here the system is normally restarted

    // The update is still pending at the source but does not need consent anymore
    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: false,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "25".to_string(),
            can_give_consent: true,
        }),
    };

    // run an update check with timeout 0. This should always succeed
    let mut checks = (
        Timeout::new(Duration::from_secs(0)),
        SucceedingCheckWithProgress {},
    );
    let result = monitor_update(&mut source, &mut store, &mut progress, &mut checks).await;

    // now, no update should need testing anymore, and the update should be reported as installed
    assert!(result.is_ok());
    assert!(!store.exists("update_needs_testing").await.unwrap());
    assert!(
        progress
            .messages
            .iter()
            .find_map(|msg| if msg.0 == "25" && msg.1.message == "All done" {
                Some(())
            } else {
                None
            })
            .is_some()
    );
    assert!(
        progress
            .messages
            .iter()
            .find_map(|msg| if msg.0 == "25" && msg.1.message == "Halfway there" {
                Some(())
            } else {
                None
            })
            .is_some()
    );

    assert!(
        progress
            .messages
            .last()
            .is_some_and(|msg| msg.1.state == UpdateState::Finished)
    );
}

struct FailingCheck;
impl IntegrityCheck for FailingCheck {
    async fn system_is_ok(&mut self) -> Result<(), UpdateError> {
        Err(UpdateError::RollbackNeeded(
            "failing check failed".to_string(),
        ))
    }
}

#[tokio::test]
async fn test_monitor_fail() {
    let mut store = HashMapStore::default();
    let mut progress = VecReporter::new();

    store.save("25", "installed").await.unwrap();
    store.save("update_needs_testing", "25").await.unwrap();

    // The update is still pending at the source but does not need consent anymore
    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: false,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "25".to_string(),
            can_give_consent: true,
        }),
    };

    // run an update check with a failing check
    let mut checks = (Timeout::new(Duration::from_secs(0)), FailingCheck {});
    let result = monitor_update(&mut source, &mut store, &mut progress, &mut checks).await;

    // the error from the failing check should be returned and reported
    assert!(result.is_err_and(|err| match err {
        UpdateError::RollbackNeeded(msg) => msg == "failing check failed",
        _ => false,
    }));
    // after a rollback, the update does not need to be tested anymore
    assert!(!store.exists("update_needs_testing").await.unwrap());
    assert!(progress.messages.last().is_some_and(|msg| msg.1.state
        == UpdateState::Failed("Rollback needed: failing check failed".to_string())));
}
