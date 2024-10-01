// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use libupdated::{
    persistent_store::PersistentStore,
    uboot::{UBootConfig, UBootEnv},
};

#[tokio::test]
async fn test_uboot_env() {
    let fw_setenv_mock = format!("{}/scripts/fw_setenv_mock.sh", env!("CARGO_MANIFEST_DIR"));
    let fw_printenv_mock = format!("{}/scripts/fw_printenv_mock.sh", env!("CARGO_MANIFEST_DIR"));

    // clear the environment
    tokio::process::Command::new(&fw_setenv_mock)
        .arg("clear")
        .output()
        .await
        .unwrap();

    // create UBootEnv PersistentStore
    let mut uboot_env = UBootEnv::new(UBootConfig {
        setenv_bin: fw_setenv_mock,
        printenv_bin: fw_printenv_mock,
    });

    // set an environment variable
    uboot_env.save("test", "value").await.unwrap();

    // get the environment variable
    let value = uboot_env.load("test").await.unwrap().unwrap();
    assert_eq!(value, "value");

    // check the value in the file
    let content = tokio::fs::read_to_string("/tmp/uboot_env.txt")
        .await
        .unwrap();
    assert_eq!(content.trim(), "test=value");

    // check existing key exists
    assert!(uboot_env.exists("test").await.unwrap());

    // check non-existing key does not exist
    assert!(!uboot_env.exists("non-existing").await.unwrap());

    // change the key
    uboot_env.save("test", "new_value").await.unwrap();
    let value = uboot_env.load("test").await.unwrap().unwrap();
    assert_eq!(value, "new_value");

    // add another key
    uboot_env.save("test2", "value2").await.unwrap();

    // delete the first key
    uboot_env.delete("test").await.unwrap();

    // check the first key does not exist anymore
    assert!(!uboot_env.exists("test").await.unwrap());

    // check the file content
    let content = tokio::fs::read_to_string("/tmp/uboot_env.txt")
        .await
        .unwrap();
    assert_eq!(content.trim(), "test2=value2");
}
