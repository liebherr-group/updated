// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

const LOG: &str = "libupdated";

pub mod traits;
pub use traits::*;

pub mod util;

/// This module implements a Hawkbit update source that fetches updates from a Hawkbit server.
/// It uses hawkbit-rs as the Hawkbit client library.
#[cfg(feature = "hawkbit")]
pub mod hawkbit;

/// This module implements a consent handler that relays the consent decision via MQTT to an external application.
/// It uses rumqttc as the MQTT client and fully encapsulates the MQTT communication and event loop.
#[cfg(feature = "mqtt")]
pub mod mqtt_consent;

/// This module implements an installer using the swupdate tool. It uses the swupdate-rs crate to interact with swupdate via its IPC interface.
#[cfg(feature = "swupdate")]
pub mod swupdate;

#[cfg(feature = "uboot")]
pub mod uboot;

/// This module contains generic building blocks for update workflows.
pub mod update_workflow;

/// This module contains building blocks to search, configure and run update workflows.
pub mod workflow_runner;

/// Simple installer that copys the update files to a target directory.
pub mod directory_installer;

/// Simple update source that scans a directory for an update.
pub mod directory_source;

/// Simple persistent store that saves key-value pairs to a file.
pub mod file_store;

/// Simple progress reporter that logs progress messages to the console.
pub mod log_reporter;

/// Implementations of IntegriryChecks for updates, e.g. simple timeout, systemd is_system_running.
pub mod checks;

/// Implementations of UpdateTriggers, e.g. Sleep.
pub mod trigger;

pub use libupdated_macros::workflow;

#[doc(hidden)]
pub use ::inventory;

#[doc(hidden)]
pub use ::futures::future::BoxFuture;
