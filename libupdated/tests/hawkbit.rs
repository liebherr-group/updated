// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use core::panic;
use std::{path::PathBuf, time::Duration};

use hawkbit::ddi::{Execution, Finished, MaintenanceWindow, Type};
use hawkbit_mock::ddi::{
    ChunkProtocol, Deployment, DeploymentBuilder, Server, ServerBuilder, Target,
};
use libupdated::{
    hawkbit::{Attributes, Hawkbit, HawkbitOptions, HawkbitProgressReporter, HawkbitUpdate},
    progress_reporter::{ProgressMessage, ProgressReporter, UpdateState},
    update_source::{Update, UpdateInfo, UpdateSource, UpdateSourceError},
    update_workflow::UpdateError,
};
use serde_json::json;

async fn create_test_hawkbit() -> (Hawkbit, Server, Target) {
    let hawkbit_server = hawkbit_mock::ddi::ServerBuilder::default().build();
    let target = hawkbit_server.add_target("target1");

    let options = HawkbitOptions {
        handle_cancel: true,
        handle_config: true,
        default_polling_interval: Duration::from_secs(30),
        server_cert: None,
        client_cert: None,
        attributes_script: Attributes::Default,
        timeout: Duration::from_secs(5),
    };

    let hawkbit = Hawkbit::new(
        &hawkbit_server.base_url(),
        &hawkbit_server.tenant,
        &target.name,
        target.client_auth.clone(),
        options,
    )
    .await
    .unwrap();

    (hawkbit, hawkbit_server, target)
}

fn test_deployment(id: &str, needs_consent: bool) -> Deployment {
    let mut test_artifact = PathBuf::new();
    test_artifact.push("scripts");
    test_artifact.push("attributes_script.sh");

    DeploymentBuilder::new(id, Type::Forced, Type::Attempt)
        .maintenance_window(MaintenanceWindow::Available)
        .confirmation_required(needs_consent)
        .chunk_with_metadata(
            ChunkProtocol::HTTPS,
            "app-https",
            "1.0",
            "some-chunk",
            vec![(
                test_artifact,
                "10af3b3dfc80600cdf6db484058ed9f4",
                "7f0cfd5c24afc3fb9e3db952e54196ef462825b8",
                "c331355ebaed9615955af5053bde672957e51d567418ccd98f32e524d781bbf6",
            )],
            vec![
                ("version".to_string(), "1.0.0".to_string()),
                ("changelog".to_string(), "all the changes!".to_string()),
            ],
        )
        .build()
}

#[tokio::test]
async fn test_standard_attributes() {
    // mock hawkbit server
    let hawkbit_server = ServerBuilder::default().build();
    let hawkbit_url = hawkbit_server.base_url();
    let target = hawkbit_server.add_target("Target1");

    let options = HawkbitOptions {
        handle_cancel: true,
        handle_config: true,
        default_polling_interval: std::time::Duration::from_secs(1),
        server_cert: None,
        client_cert: None,
        attributes_script: Attributes::Default,
        timeout: Duration::from_secs(5),
    };
    let mut hawkbit = Hawkbit::new(
        &hawkbit_url,
        &hawkbit_server.tenant,
        "Target1",
        target.client_auth.clone(),
        options,
    )
    .await
    .unwrap();

    target.request_config(json!({
        "mode" : "replace",
        "data" : {
            "OSVersion": hawkbit.get_os_version().await.unwrap(),
            "Client": format!("libupdated v{}", env!("CARGO_PKG_VERSION")),
        },
        "status" : {
            "result" : {
            "finished" : "success"
            },
            "execution" : "closed",
            "details" : []
        }
    }));

    match hawkbit.check_for_updates().await {
        Ok(_) => panic!("There should be no updates"),
        Err(UpdateError::NoPendingUpdates) => (),
        Err(e) => panic!("Error: {:?}", e),
    };

    assert_eq!(target.config_data_hits(), 1);
}

#[tokio::test]
async fn test_attributes_script() {
    // mock hawkbit server
    let hawkbit_server = ServerBuilder::default().build();
    let hawkbit_url = hawkbit_server.base_url();
    let target = hawkbit_server.add_target("Target1");

    let options = HawkbitOptions {
        handle_cancel: true,
        handle_config: true,
        default_polling_interval: std::time::Duration::from_secs(1),
        server_cert: None,
        client_cert: None,
        attributes_script: Attributes::Script(format!(
            "{}/scripts/attributes_script.sh",
            env!("CARGO_MANIFEST_DIR")
        )),
        timeout: Duration::from_secs(5),
    };
    let mut hawkbit = Hawkbit::new(
        &hawkbit_url,
        &hawkbit_server.tenant,
        "Target1",
        target.client_auth.clone(),
        options,
    )
    .await
    .unwrap();

    target.request_config(json!({
        "mode" : "replace",
        "data" : {
            "OS_ID" : "liebherr os",
            "OS_VERSION_ID" : "1.0",
            "CURRENT_ROOT_PARTITION" : "2",
            "SWUPDATE_STATE" : "0",
        },
        "status" : {
            "result" : {
            "finished" : "success"
            },
            "execution" : "closed",
            "details" : []
        }
    }));

    match hawkbit.check_for_updates().await {
        Ok(_) => panic!("There should be no updates"),
        Err(UpdateError::NoPendingUpdates) => (),
        Err(e) => panic!("Error: {:?}", e),
    };

    assert_eq!(target.config_data_hits(), 1);
}

#[tokio::test]
async fn test_invalid_attributes_script() {
    // mock hawkbit server
    let hawkbit_server = ServerBuilder::default().build();
    let hawkbit_url = hawkbit_server.base_url();
    let target = hawkbit_server.add_target("Target1");

    let options = HawkbitOptions {
        handle_cancel: true,
        handle_config: true,
        default_polling_interval: std::time::Duration::from_secs(1),
        server_cert: None,
        client_cert: None,
        attributes_script: Attributes::Script("ls".to_string()),
        timeout: Duration::from_secs(5),
    };
    let mut hawkbit = Hawkbit::new(
        &hawkbit_url,
        &hawkbit_server.tenant,
        "Target1",
        target.client_auth.clone(),
        options,
    )
    .await
    .unwrap();

    target.request_config(json!({
        "mode" : "replace",
        "data" : {
            "OSVersion": hawkbit.get_os_version().await.unwrap(),
            "Client": format!("libupdated v{}", env!("CARGO_PKG_VERSION")),
        },
        "status" : {
            "result" : {
            "finished" : "success"
            },
            "execution" : "closed",
            "details" : []
        }
    }));

    match hawkbit.check_for_updates().await {
        Ok(_) => panic!("There should be no updates"),
        Err(UpdateError::NoPendingUpdates) => (),
        Err(e) => panic!("Error: {:?}", e),
    };

    assert_eq!(target.config_data_hits(), 1);
}

#[tokio::test]
async fn test_hawkbit_poll() {
    let (mut hawkbit, _hawkbit_server, target) = create_test_hawkbit().await;

    assert_eq!(
        hawkbit.preferred_polling_interval(),
        &Duration::from_secs(30)
    );

    match hawkbit.check_for_updates().await {
        Ok(_) => panic!("No updates should be available"),
        Err(UpdateError::NoPendingUpdates) => (),
        Err(err) => panic!("Unexpected error: {:?}", err),
    };
    assert_eq!(target.poll_hits(), 1);

    let deployment = DeploymentBuilder::new("123", Type::Forced, Type::Attempt).build();
    target.push_deployment(deployment);

    let _ = hawkbit.check_for_updates().await.unwrap();

    assert_eq!(target.poll_hits(), 1);

    // the server sets the polling interval to 60 seconds
    assert_eq!(
        hawkbit.preferred_polling_interval(),
        &Duration::from_secs(60)
    );
}

#[tokio::test]
async fn test_hawkbit_update_info() {
    let (mut hawkbit, _, target) = create_test_hawkbit().await;

    // update, that does not need user consent
    target.push_deployment(test_deployment("1234", false));

    let update_info = hawkbit.check_for_updates().await.unwrap();

    assert_eq!(update_info.version(), "1234");
    assert_eq!(
        update_info.metadata().get("changelog").unwrap(),
        "all the changes!"
    );

    assert_eq!(target.poll_hits(), 1);
    assert_eq!(target.deployment_hits(), 1);

    assert!(!update_info.needs_consent());

    let update = update_info.update().unwrap();
    let _hawkbit_update = update.as_any().downcast_ref::<HawkbitUpdate>().unwrap();

    // update, that needs user consent
    target.push_deployment(test_deployment("1235", true));

    let mut update_info = hawkbit.check_for_updates().await.unwrap();

    // we can access the metadata without giving consent
    assert_eq!(update_info.version(), "1235");
    assert_eq!(
        update_info.metadata().get("changelog").unwrap(),
        "all the changes!"
    );

    assert_eq!(target.poll_hits(), 1);
    assert_eq!(target.confirmation_hits(), 1);

    assert!(update_info.needs_consent());

    assert!(update_info.update().is_err_and(|e| {
        if let UpdateError::UpdateSourceError(UpdateSourceError::ConsentRequired) = e {
            true
        } else {
            false
        }
    }));

    // when giving consent, the client immediately polls and fetches the (now confirmed) deployment
    let confirmation_mock = target.expect_confirmation_feedback(
        "1235",
        Some(1),
        hawkbit::ddi::ConfirmationResponse::Confirmed,
        vec![],
    );
    target.push_deployment(test_deployment("1235", false));
    update_info.give_consent().await.unwrap();
    assert!(confirmation_mock.hits() == 1);
    assert!(target.poll_hits() == 1);
    assert_eq!(target.deployment_hits(), 1);

    // now fetching the update should work
    assert!(!update_info.needs_consent());

    let update = update_info.update().unwrap();
    let _hawkbit_update = update.as_any().downcast_ref::<HawkbitUpdate>().unwrap();
}

#[tokio::test]
async fn test_hawkbit_update() {
    let (mut hawkbit, _, target) = create_test_hawkbit().await;

    // update, that does not need user consent
    target.push_deployment(test_deployment("1234", false));

    let update_info = hawkbit.check_for_updates().await.unwrap();
    let update = update_info.update().unwrap();

    let filesize = update.size();
    assert_eq!(
        filesize,
        tokio::fs::metadata("scripts/attributes_script.sh")
            .await
            .unwrap()
            .len()
    );

    // download the file and assert that its content and filename match
    let filenames = update.save_to_disk(None).await.unwrap();
    let update_file = filenames.first().unwrap();
    assert_eq!(
        &update_file.filename,
        &PathBuf::from("/tmp/updated/update_1234/attributes_script.sh")
    );
    assert_eq!(
        tokio::fs::read_to_string(&update_file.filename)
            .await
            .unwrap(),
        tokio::fs::read_to_string("scripts/attributes_script.sh")
            .await
            .unwrap()
    );

    // stream the file to a vector and assert the content matches
    let mut stream = update.stream().unwrap();
    let mut buffer = Vec::<u8>::new();
    while let Some(data) = stream.recv().await {
        match data {
            Ok(bytes) => buffer.append(bytes.to_vec().as_mut()),
            Err(e) => panic!("Error: {:?}", e),
        }
    }
    assert_eq!(
        buffer,
        tokio::fs::read("scripts/attributes_script.sh")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_hawkbit_hashes() {
    let (mut hawkbit, _, target) = create_test_hawkbit().await;

    // update, that does not need user consent
    let mut test_artifact = PathBuf::new();
    test_artifact.push("scripts");
    test_artifact.push("attributes_script.sh");

    let deployment = DeploymentBuilder::new("1234", Type::Forced, Type::Attempt)
        .maintenance_window(MaintenanceWindow::Available)
        .confirmation_required(false)
        .chunk_with_metadata(
            ChunkProtocol::HTTPS,
            "app-https",
            "1.0",
            "some-chunk",
            vec![(test_artifact, "0", "0", "0")], // invalid hashes
            vec![
                ("version".to_string(), "1.0.0".to_string()),
                ("changelog".to_string(), "all the changes!".to_string()),
            ],
        )
        .build();

    target.push_deployment(deployment);

    let update_info = hawkbit.check_for_updates().await.unwrap();
    let update = update_info.update().unwrap();

    let filesize = update.size();
    assert_eq!(
        filesize,
        tokio::fs::metadata("scripts/attributes_script.sh")
            .await
            .unwrap()
            .len()
    );

    // download the file and assert that its content and filename match
    update
        .save_to_disk(None)
        .await
        .map(|_| ())
        .expect_err("Invalid hashes should cause an error");
    // stream the file to a vector and assert the content matches
    let mut stream = update.stream().unwrap();
    let mut buffer = Vec::<u8>::new();
    while let Some(data) = stream.recv().await {
        match data {
            Ok(bytes) => buffer.append(bytes.to_vec().as_mut()),
            Err(_) => {
                return; /* expect checksum error eventually */
            }
        }
    }
    panic!("Expected checksum error, but got: {:?}", buffer);
}

#[tokio::test]
async fn test_hawkbit_update_progress() {
    let (mut hawkbit, _, target) = create_test_hawkbit().await;

    // update, that does not need user consent
    target.push_deployment(test_deployment("1234", false));

    let update_info = hawkbit.check_for_updates().await.unwrap();
    let update = update_info.update().unwrap();

    let filesize = update.size();
    assert_eq!(
        filesize,
        tokio::fs::metadata("scripts/attributes_script.sh")
            .await
            .unwrap()
            .len()
    );

    let mut hawkbit_progress = HawkbitProgressReporter::new();

    // download the file and assert that its content and filename match
    let update_file = update
        .save_to_disk_by_name(
            "attributes_script.sh".to_string(),
            Some(PathBuf::from("/tmp/test_hawkbit_update_progress")),
        )
        .await
        .unwrap();

    target.expect_deployment_feedback(
        "1234",
        Execution::Download,
        Finished::None,
        Some(json!({"cnt": 1, "of": 2})),
        vec!["test1"],
    );
    let _ = hawkbit_progress
        .report_retry(
            &update,
            &ProgressMessage {
                state: UpdateState::Downloading,
                cnt_of: (1, 2),
                message: "test1".to_string(),
            },
            1,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    target.expect_deployment_feedback(
        "1234",
        Execution::Download,
        Finished::None,
        Some(json!({"cnt": 2, "of": 2})),
        vec!["test2"],
    );
    let _ = hawkbit_progress
        .report(
            &update_file,
            &ProgressMessage {
                state: UpdateState::Downloading,
                cnt_of: (2, 2),
                message: "test2".to_string(),
            },
        )
        .await
        .unwrap();
}
