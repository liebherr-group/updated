// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use libupdated::workflow_runner::Updated;
use std::process::ExitCode;
use tracing::info;

// The library part of the updated crate must be explicitly linked,
// otherwise it is removed by the linker.
// It can be enabled or disabled via the `example_workflow` feature flag.
#[cfg(feature = "example_workflow")]
extern crate updated_example;

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize the tracing logger
    tracing_subscriber::fmt::init();

    // Create an instance of the standard updated workflow runner and read the
    // workflow and configuration file from the command line arguments.
    // (available through the `cli` feature of libupdated)
    let mut updated = Updated::from_cli();

    // Spawn a signal handler task to gracefully stop on SIGINT and SIGTERM signals
    updated.spawn_signal_handler();

    // Spawn a watchdog task to feed the systemd watchdog if enabled
    // (available through the `systemd` feature of libupdated)
    updated.spawn_watchdog_task();

    // List all available workflows. If your workflow is not listed here,
    // make sure to use the `workflow` macro and ensure that the crate
    // containing the workflow is linked correctly.
    for workflow in Updated::available_workflow_names() {
        info!(target: "updated", "Available workflow: {}", workflow);
    }

    // Run the workflow until it completes. If the workflow fails or is not
    // found, it will return an error code.
    updated.run().await
}
