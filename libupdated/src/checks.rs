// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::time::Duration;

use crate::{integrity_check::IntegrityCheck, update_workflow::UpdateError};

#[cfg(feature = "uboot")]
use crate::{traits::persistent_store::PersistentStore, uboot::UBootEnv};

/// A simple integrity check that waits for a specified duration before returning Ok.
/// This tests general system stability and that the system is still running after the specified duration.
pub struct Timeout {
    /// The duration to wait before returning Ok
    duration: Duration,
}

impl Timeout {
    pub fn new(duration: Duration) -> Self {
        Timeout { duration }
    }
}

impl IntegrityCheck for Timeout {
    async fn system_is_ok(&mut self) -> Result<(), UpdateError> {
        // Sleep for the specified duration, if the system is still running after that, it must be ok
        tokio::time::sleep(self.duration).await;
        Ok(())
    }
}

/// Integrity check that runs `systemctl is-system-running --wait` to check if
/// the system is able to start all system services.
/// If the command fails or returns a non-zero exit code, a system rollback is requested.
pub struct SystemdIsSystemRunning;

impl IntegrityCheck for SystemdIsSystemRunning {
    async fn system_is_ok(&mut self) -> Result<(), UpdateError> {
        let output = tokio::process::Command::new("systemctl")
            .arg("is-system-running")
            .arg("--wait")
            .output()
            .await
            .map_err(|err| UpdateError::RollbackNeeded(err.to_string()))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(UpdateError::RollbackNeeded(format!(
                "systemctl is-system-running failed with '{}'",
                String::from_utf8(output.stdout).unwrap_or("<invalid ouput>".to_string())
            )))
        }
    }
}

#[cfg(feature = "uboot")]
pub struct UBootDidRollback {
    uboot: UBootEnv,
}

#[cfg(feature = "uboot")]
impl UBootDidRollback {
    pub fn new(uboot: UBootEnv) -> Self {
        Self { uboot }
    }
}

#[cfg(feature = "uboot")]
impl IntegrityCheck for UBootDidRollback {
    async fn system_is_ok(&mut self) -> Result<(), UpdateError> {
        if let Ok(Some(ustate)) = self.uboot.load("ustate").await {
            // if the ustate is 3, the bootloader did a rollback
            if ustate == "3" {
                // reset the ustate back to 0
                self.uboot.save("ustate", "0").await.ok();

                return Err(UpdateError::ExternalRollback);
            }
        }

        Ok(())
    }
}
