// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::path::PathBuf;

use common::DummyUpdate;
use libupdated::installer::*;
use libupdated::swupdate::*;

mod common;

#[tokio::test]
async fn test_install() {
    let tmpdir = "/tmp/swupdate_test_install";
    let swupdate_script = "swupdate_test_install_mock";

    tokio::fs::create_dir_all(tmpdir).await.unwrap();
    tokio::fs::copy(
        format!(
            "{}/scripts/swupdate_client_mock.sh",
            env!("CARGO_MANIFEST_DIR")
        ),
        format!("{tmpdir}/{swupdate_script}"),
    )
    .await
    .unwrap();

    let installer = SWUpdate::new(SWUpdateOptions {
        install_mode: SWUpdateInstallMode::TemporaryCopy {
            tmp_dir: PathBuf::from(tmpdir),
            bufsize: 1024 * 1024,
        },
        dry_run: true,
        swupdate_client_bin: format!("{tmpdir}/{swupdate_script}"),
        swupdate_sw_mode: Some(("stable".to_string(), "root_a".to_string())),
        ..Default::default()
    });
    let update = DummyUpdate {
        version: "1.0".to_string(),
    };
    let mut progress = installer.install(&update).unwrap();
    while let Some(feedback) = progress.next().await.unwrap() {
        println!("feedback: {}", feedback.message);
        if feedback.status == "1" {
            assert_eq!(feedback.message, "installing update");
        } else {
            assert_eq!(feedback.status, "0");
            assert_eq!(feedback.message, "update installed");
        }
    }
    let report = tokio::fs::read_to_string(format!("/tmp/{swupdate_script}.txt"))
        .await
        .unwrap();
    assert!(
        report.contains("stdin: \"dummy content\""),
        "report: {report}"
    );
    assert!(
        report.contains("cmdline: \"-s /run/swupdate/sockinstctrl -v -p -d -e stable,root_a\""),
        "report: {report}"
    );

    tokio::fs::remove_dir_all(tmpdir).await.unwrap();
    tokio::fs::remove_file(format!("/tmp/{swupdate_script}.txt"))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_stream_error() {
    let installer = SWUpdate::new(SWUpdateOptions {
        install_mode: SWUpdateInstallMode::Stream,
        dry_run: true,
        swupdate_client_bin: format!(
            "{}/scripts/swupdate_client_mock.sh",
            env!("CARGO_MANIFEST_DIR")
        ),
        ..Default::default()
    });
    let update = DummyUpdate {
        version: "1.0".to_string(),
    };
    let mut progress = installer.install(&update).unwrap();
    loop {
        match progress.next().await {
            Ok(Some(feedback)) => {
                println!("feedback: {}", feedback.message);
                if feedback.status == "1" {
                    assert_eq!(feedback.message, "installing update");
                } else {
                    assert_eq!(feedback.status, "0");
                    assert_eq!(feedback.message, "update installed");
                    break;
                }
            }
            Ok(None) => panic!("the update should not install cleanly"),
            Err(err) => {
                assert_eq!(
                    err.to_string(),
                    "fetch error: stream error: streaming unsupported"
                );
                break;
            }
        }
    }
}
