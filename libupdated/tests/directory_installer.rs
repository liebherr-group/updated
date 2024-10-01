// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{collections::HashMap, path::Path};

use common::DummyUpdate;
use libupdated::{
    directory_installer::*,
    directory_source::{DirectorySource, DirectoryUpdateInfo},
    installer::{InstallProgress, Installer},
    update_source::{Update, UpdateInfo, UpdateSource},
};

mod common;

#[tokio::test]
async fn test_install() {
    let dir: &str = "/tmp";
    let installer = DirectoryInstaller::new(dir);
    let update = DummyUpdate {
        version: "1.0".to_string(),
    };
    let mut progress = installer.install(&update).unwrap();
    let feedback = progress.next().await.unwrap().unwrap();
    assert_eq!(feedback.status, "success");
    assert_eq!(feedback.message, format!("installed update to {dir}"));

    let feedback = progress.next().await.unwrap();
    assert!(feedback.is_none());
    let content = tokio::fs::read_to_string("/tmp/dummy.txt").await.unwrap();
    assert_eq!(content, "dummy content");
}

#[tokio::test]
async fn test_copy() {
    let src_dir = "/tmp/src";
    let tgt_dir = "/tmp/tgt";

    tokio::fs::create_dir_all(src_dir).await.unwrap();
    tokio::fs::create_dir_all(tgt_dir).await.unwrap();

    let update_json = DirectoryUpdateInfo {
        update_id: "test_update".to_string(),
        metadata: HashMap::from([
            ("version".to_string(), "1".to_string()),
            ("changelog".to_string(), "excitinng changes!".to_string()),
        ]),
        files: vec!["f1.txt".to_string(), "f2.txt".to_string()],
    };

    tokio::fs::write(
        format!("{src_dir}/update.json"),
        serde_json::to_string(&update_json).unwrap(),
    )
    .await
    .unwrap();
    tokio::fs::write(format!("{src_dir}/f1.txt"), "some update stuff")
        .await
        .unwrap();
    tokio::fs::write(format!("{src_dir}/f2.txt"), "some more update stuff")
        .await
        .unwrap();

    let mut source = DirectorySource::new(&Path::new(src_dir));
    let installer = DirectoryInstaller::new(tgt_dir);

    let info = source.check_for_updates().await.unwrap();

    assert!(!info.needs_consent());

    let update = info.update().unwrap();
    let mut install_progress = installer.install(&update).unwrap();
    while let Some(feedback) = install_progress.next().await.unwrap() {
        assert_eq!(feedback.status, "success");
    }

    assert_eq!(update.size(), 39);
    let mut stream = update.stream().unwrap();
    let mut received = Vec::new();
    while let Some(msg) = stream.recv().await {
        match msg {
            Ok(bytes) => {
                received.append(&mut bytes.to_vec());
            }
            Err(_) => panic!(),
        }
    }

    assert_eq!(
        received,
        Vec::from("some update stuffsome more update stuff")
    );

    assert_eq!(
        tokio::fs::read_to_string(format!("{tgt_dir}/f1.txt"))
            .await
            .unwrap(),
        "some update stuff"
    );
    assert_eq!(
        tokio::fs::read_to_string(format!("{tgt_dir}/f2.txt"))
            .await
            .unwrap(),
        "some more update stuff"
    );

    tokio::fs::remove_dir_all(src_dir).await.unwrap();
    tokio::fs::remove_dir_all(tgt_dir).await.unwrap();
}
