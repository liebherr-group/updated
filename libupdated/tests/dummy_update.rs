// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{any::Any, collections::HashMap, path::PathBuf};

use libupdated::{
    directory_installer::DirectoryInstaller,
    installer::{InstallProgress, Installer},
    update_source::*,
    update_workflow::UpdateError,
};

mod common;

use common::*;

#[tokio::test]
async fn dummy_update_no_updates() {
    let mut source = DummySource {
        pending_update: None,
    };

    let result = source.check_for_updates().await;

    assert!(result.is_err_and(|e| match e {
        UpdateError::NoPendingUpdates => true,
        _ => false,
    }));
}

#[tokio::test]
async fn dummy_update_needs_consent() {
    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: true,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "1.0.0".to_string(),
            can_give_consent: true,
        }),
    };

    let result = source.check_for_updates().await;

    assert!(result.as_ref().is_ok_and(|info| {
        info.needs_consent()
            && info.metadata().get("changelog").unwrap() == "Changelog"
            && info.version() == "1.0.0"
    }));

    let mut info = result.unwrap();
    let result = info.update();
    assert!(result.is_err_and(|e| match e {
        UpdateError::UpdateSourceError(UpdateSourceError::ConsentRequired) => true,
        _ => false,
    }));

    let result = info.give_consent().await;

    assert!(result.is_ok());
    assert!(!info.needs_consent());

    let result = info.update();
    assert!(result.is_ok());
}

#[tokio::test]
async fn dummy_update_through_update_file() {
    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: false,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "1.0.0".to_string(),
            can_give_consent: true,
        }),
    };

    let result = source.check_for_updates().await;

    assert!(result.as_ref().is_ok_and(|info| {
        !info.needs_consent()
            && info.metadata().get("changelog").unwrap() == "Changelog"
            && info.version() == "1.0.0"
    }));

    let info = result.unwrap();
    let update = info.update().unwrap();
    let update_files = update
        .save_to_disk(Some(PathBuf::from("/tmp/update_file_test")))
        .await
        .unwrap();
    // the local update file should contain only one file
    assert_eq!(update_files.len(), 1);
    let update_file = update_files.first().unwrap();
    // the parent of the update file should be the original update
    assert_eq!((*update_file.parent_update).type_id(), update.type_id());
    // it should be possible to downcast the update_file to get the original update
    let _: &DummyUpdate = update_file.as_any().downcast_ref().unwrap();
    // the update file should contain one file that has the same filename as the corresponding file in the original update
    assert_eq!(update_file.files().len(), 1);
    assert_eq!(
        update_file.files().first().unwrap(),
        "/tmp/update_file_test/dummy.txt"
    );
    assert_eq!(update_file.size(), update.size());
    assert_eq!(update_file.version(), update.version());

    // saving the update file to disk should just copy it
    let update_files_copy = update_file
        .save_to_disk(Some(PathBuf::from("/tmp/update_file_test/copy")))
        .await
        .unwrap();
    let update_file_copy = update_files_copy.first().unwrap();
    // it should still be possible to downcast the update_file to get the original update
    let _: &DummyUpdate = update_file_copy.as_any().downcast_ref().unwrap();

    let installer = DirectoryInstaller::new("/tmp/update_file_test/install");
    let mut progress = installer.install(update_file).unwrap();
    assert!(progress.next().await.unwrap().is_some());
    assert!(progress.next().await.unwrap().is_none());

    assert_eq!(
        tokio::fs::read_to_string("/tmp/update_file_test/install/dummy.txt")
            .await
            .unwrap(),
        tokio::fs::read_to_string("/tmp/update_file_test/copy/dummy.txt")
            .await
            .unwrap()
    );

    tokio::fs::remove_dir_all("/tmp/update_file_test")
        .await
        .unwrap();
}
