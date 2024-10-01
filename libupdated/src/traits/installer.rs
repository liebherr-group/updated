// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use thiserror::Error;

use crate::{update_source::Update, update_workflow::UpdateError};

/// An error that can occur during installation.
#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("installer error: {0}")]
    InstallationFailed(String),
    #[error("rollback failed: {0}")]
    RollbackFailed(String),
}

/// Feedback message to report the status of an ongoing installation.
/// An install progress iterator will return InstallationFeedback messages until the installation is finished.
/// Typically, these status messages will be reported (to the user, log, update server, …) using a progress reporter.
#[derive(Debug)]
pub struct InstallationFeedback {
    /// A status indicator for the installation
    pub status: String,
    /// A message to describe the current installation status
    pub message: String,
    /// An optional progress indicator. This is typically used to render the progress using a progress bar with the
    /// first value representing the current progress and the second value the complete progress.
    /// For example, (22, 100) could represent 22% progress.
    pub progress: Option<(u64, u64)>,
}

/// An iterator on the progress of an installation.
pub trait InstallProgress {
    /// Poll the progress of the installation.
    /// This function should be called repeatedly until it returns Ok(None), indicating that the installation is finished.
    fn next(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<InstallationFeedback>, UpdateError>> + Send;
}

/// An installer that can install updates from an update source.
///
/// ``` rust
/// use libupdated::traits::installer::*;
/// use libupdated::traits::update_source::*;
/// use libupdated::update_workflow::UpdateError;
///
/// async fn example(installer: &(impl Installer + Rollback),
///                  update: &impl Update) -> Result<(), UpdateError> {
///     let mut progress = installer.install(update)?;
///
///     while let Some(feedback) = progress.next().await? {
///         println!("Status: {}, Message: {}", feedback.status, feedback.message);
///     }
///
///     // installation finished, uninstall it again
///
///     if installer.rollback().await? == RollbackStatus::RebootRequired {
///         // reboot the system
///     }
///
///     Ok(())
/// }
/// ```
pub trait Installer {
    /// Install an update.
    /// Install returns a progress iterator, that can be polled for progress. As soon as it returns Ok(None), the installation is finished.
    fn install(&self, update: &impl Update) -> Result<impl InstallProgress, UpdateError>;
}

#[derive(PartialEq, Eq)]
pub enum RollbackStatus {
    /// The rollback was successful.
    Success,
    /// The rollback needs a reboot to take effect.
    RebootRequired,
}

/// A trait for rolling back an installation.
/// This trait is separate from Installer, as not every installation can be rolled back.
pub trait Rollback {
    /// Roll back the last installation.
    /// As the rollback operation can be triggered long after the installation, information about the update might not be available anymore.
    /// Therefore, no update argument is passed to the rollback function.
    /// The rollback implementation is expected to know how to undo the last installation or cooperate with the installer to persist the
    /// necessary information, e.g., by writing a list of installed files to disk.
    fn rollback(&self)
    -> impl std::future::Future<Output = Result<RollbackStatus, InstallerError>>;
}
