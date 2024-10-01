// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::path::{Path, PathBuf};

use crate::installer::{InstallProgress, InstallationFeedback, Installer, InstallerError};
use crate::update_source::Update;
use crate::update_workflow::UpdateError;

impl From<std::io::Error> for UpdateError {
    fn from(e: std::io::Error) -> Self {
        UpdateError::InstallerError(InstallerError::InstallationFailed(format!(
            "io error: {}",
            e
        )))
    }
}

/// An installer that installs updates to a specified directory on disk.
pub struct DirectoryInstaller {
    /// The target directory to install the downloaded files to
    dir: String,
}

impl DirectoryInstaller {
    pub fn new(dir: &str) -> Self {
        DirectoryInstaller {
            dir: dir.to_string(),
        }
    }
}

impl Installer for DirectoryInstaller {
    fn install(&self, update: &impl Update) -> Result<impl InstallProgress, UpdateError> {
        Ok(DirectoryInstallProgress {
            update: Box::new(update.clone()),
            installed: false,
            dir: self.dir.clone(),
        })
    }
}

/// Progress iterator for the directory installer.
pub struct DirectoryInstallProgress<T>
where
    T: Update,
{
    /// The update to install
    update: Box<T>,
    /// Whether the update has already been installed
    installed: bool,
    /// The directory to install the update to
    dir: String,
}

impl<T> InstallProgress for DirectoryInstallProgress<T>
where
    T: Update,
{
    async fn next(&mut self) -> Result<Option<InstallationFeedback>, UpdateError> {
        // on the first call, the update is installed and feedback is returned.
        // subsequent calls will return Ok(None) to signal completion.

        if self.installed {
            return Ok(None);
        }

        tokio::fs::create_dir_all(Path::new(&self.dir)).await?;
        self.update
            .save_to_disk(Some(PathBuf::from(&self.dir)))
            .await?;
        self.installed = true;
        Ok(Some(InstallationFeedback {
            status: "success".to_string(),
            message: format!("installed update to {}", self.dir),
            progress: Some((1, 1)),
        }))
    }
}
