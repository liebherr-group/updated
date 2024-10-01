// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use crate::persistent_store::{PersistentStore, StoreError};

#[derive(Debug, Clone)]
pub struct UBootConfig {
    /// Path to the binary to set U-Boot variables
    pub setenv_bin: String,
    /// Path to the binary to print U-Boot variables
    pub printenv_bin: String,
}

impl Default for UBootConfig {
    fn default() -> Self {
        UBootConfig {
            setenv_bin: "/usr/bin/fw_setenv".to_string(),
            printenv_bin: "/usr/bin/fw_printenv".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UBootEnv {
    /// Configuration for reading/writing the U-Boot environment
    config: UBootConfig,
}

impl UBootEnv {
    pub fn new(config: UBootConfig) -> Self {
        UBootEnv { config }
    }
}

impl PersistentStore for UBootEnv {
    async fn delete(&mut self, key: &str) -> Result<(), StoreError> {
        let output = tokio::process::Command::new(&self.config.setenv_bin)
            .arg(key)
            .output()
            .await?;

        if !output.status.success() {
            return Err(StoreError::IOError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to write U-Boot environment",
            )));
        }

        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        let output = tokio::process::Command::new(&self.config.printenv_bin)
            .arg(key)
            .output()
            .await?;

        if !output.status.success() {
            return Err(StoreError::IOError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to read U-Boot environment",
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let kv = stdout.trim_end().split('=').collect::<Vec<&str>>();

        if kv.len() == 2 && !kv[1].is_empty() {
            return Ok(true);
        };

        Ok(false)
    }

    async fn save(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        let output = tokio::process::Command::new(&self.config.setenv_bin)
            .arg(key)
            .arg(value)
            .output()
            .await?;

        if !output.status.success() {
            return Err(StoreError::IOError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to write U-Boot environment",
            )));
        }

        Ok(())
    }

    async fn load(&self, key: &str) -> Result<Option<String>, StoreError> {
        let output = tokio::process::Command::new(&self.config.printenv_bin)
            .arg(key)
            .output()
            .await?;

        if !output.status.success() {
            return Err(StoreError::IOError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to read U-Boot environment",
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let kv = stdout.trim_end().split('=').collect::<Vec<&str>>();

        if kv.len() == 2 && !kv[1].is_empty() {
            return Ok(Some(kv[1].to_string()));
        };

        Ok(None)
    }
}
