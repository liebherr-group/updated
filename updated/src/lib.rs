// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{collections::HashMap, path::Path, time::Duration};

use libupdated::{
    checks::SystemdIsSystemRunning,
    consent_handler::AutoConsent,
    directory_installer::DirectoryInstaller,
    directory_source::DirectorySource,
    file_store::FileStore,
    log_reporter::LogReporter,
    trigger::Rate,
    update_workflow::{
        UpdateOptions, WorkflowError, get_config, monitor_update, update_with_consent_flow,
    },
    workflow,
};
use tracing::{error, info};

// This is an example for a simple update workflow that periodically checks a
// directory for updates, always gives consent for the update, and copies the
// update files to a target directory.
// On the next start, it checks whether the system is still running correctly.
#[workflow]
pub async fn example_workflow(config: HashMap<String, String>) -> Result<(), WorkflowError> {
    // Create a Trigger that checks for updates every `update_interval_sec`
    // seconds.
    let update_interval = get_config(&config, "update_interval_sec")?
        .parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|e| WorkflowError::InvalidConfiguration(e.to_string()))?;
    let mut trigger = Rate::new(
        update_interval,
        UpdateOptions {
            ignore_previously_declined: true,
            consent_timeout: update_interval,
        },
    );

    // Create an UpdateSource for the directory `source_dir` in the config.
    let mut source = DirectorySource::new(Path::new(get_config(&config, "source_dir")?.as_str()));

    // If consent is required for the update, always accept it.
    let mut consent_handler = AutoConsent::new(true);

    // Create an Installer that copies files to the `target_dir` in the config.
    let installer = DirectoryInstaller::new(get_config(&config, "target_dir")?.as_str());

    // Create a store file to persist the state of an update across restarts.
    let mut store = FileStore::new(get_config(&config, "sota_state_file")?.as_str());

    // Report the progress of the update to the (tracing) log.
    let mut progress = LogReporter::new("example_workflow");

    // Configure checks to be run after an update.
    let mut check = SystemdIsSystemRunning {};

    info!(target: "example_workflow", "Workflow configuration finished, starting update loop. Configuration: {:?}", config);

    // Check whether a previous update left the system in a bad state
    if let Err(e) = monitor_update(&mut source, &mut store, &mut progress, &mut check).await {
        error!(target: "example_workflow", "Previous update failed: {}", e);
    }

    loop {
        // Run the update logic and print the result.
        if let Err(e) = update_with_consent_flow(
            &mut trigger,
            &mut source,
            &mut consent_handler,
            &installer,
            &mut store,
            &mut progress,
        )
        .await
        {
            error!(target: "example_workflow", "Update result: {}", e);
        } else {
            info!(target: "example_workflow", "Update completed successfully.");
        }
    }
}
