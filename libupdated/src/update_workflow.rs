// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::collections::HashMap;
use std::fmt::Debug;
use std::process::ExitCode;
use std::time::Duration;

use futures::future::BoxFuture;
use thiserror::Error;
use tokio::time::{sleep, timeout};
use tracing::error;

use crate::consent_handler::{ConsentHandler, ConsentHandlerError};
use crate::installer::{InstallProgress, Installer, InstallerError};
use crate::integrity_check::IntegrityCheckWithProgress;
use crate::persistent_store::{PersistentStore, StoreError};
use crate::progress_reporter::{ProgressMessage, ProgressReporter, UpdateState};
use crate::update_source::{UpdateInfo, UpdateSource, UpdateSourceError};
use crate::update_trigger::{UpdateTrigger, UpdateTriggerError};

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("No pending updates.")]
    NoPendingUpdates,
    #[error("Consent denied.")]
    ConsentDenied,
    #[error("Consent request timed out.")]
    ConsentTimeout,
    /// Indicates that a rollback of the last update was performed by something else, e.g. the bootloader
    #[error("External rollback detected")]
    ExternalRollback,
    /// Indicates that a rollback is necessary and needs to be triggered by updated
    #[error("Rollback needed: {0}")]
    RollbackNeeded(String),
    #[error(transparent)]
    ConsentHandlerError(#[from] ConsentHandlerError),
    #[error(transparent)]
    StoreError(#[from] StoreError),
    #[error(transparent)]
    InstallerError(#[from] InstallerError),
    #[error(transparent)]
    UpdateSourceError(#[from] UpdateSourceError),
    #[error(transparent)]
    UpdateTriggerError(#[from] UpdateTriggerError),
}

pub trait WorkflowOptions: Debug + Clone {
    fn ignore_previously_declined(&self) -> bool;
    fn consent_timeout(&self) -> Duration;
}

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub ignore_previously_declined: bool,
    pub consent_timeout: Duration,
}

impl WorkflowOptions for UpdateOptions {
    fn ignore_previously_declined(&self) -> bool {
        self.ignore_previously_declined
    }

    fn consent_timeout(&self) -> Duration {
        self.consent_timeout
    }
}

/// monitor_update checks whether an update was installed correctly after a reboot.
/// It looks for a stored update_needs_testing key and starts the testing process if it is found.
/// The testing itself is done by the IntegrityCheck instance. If any check fails, monitor_update returns the error reported by the IntegrityCheck (typically UpdateError::RollbackNeeded).
/// Otherwise, it returns Ok(()).
pub async fn monitor_update(
    source: &mut impl UpdateSource,
    store: &mut impl PersistentStore,
    progress: &mut impl ProgressReporter,
    check: &mut impl IntegrityCheckWithProgress,
) -> Result<(), UpdateError> {
    // check whether an update was installed and needs testing
    if let Some(version) = store.load("update_needs_testing").await? {
        let mut attempt = 1;
        let update_info = loop {
            let update_info = source.check_for_updates().await;

            if let Err(UpdateError::NoPendingUpdates) = &update_info {
                // The update that should be tested according to the client state is not available on the server anymore.
                // This could happen if the update was accidently marked as failed on the server side.
                // We trust the server side, skip the testing, and log an error.
                error!(
                    "Pending update ({}) not found on the server. This is probably an error.",
                    version
                );
                store.delete("update_needs_testing").await?;
                return Ok(());
            }

            // we cannot assume to be connected to the update source immediately.
            // If the check_for_updates succeeds, we can start the self-check.
            // After 10 failed attempts, give up
            if update_info.is_ok() || attempt >= 10 {
                break update_info;
            } else {
                error!("Could not connect to the server (Attempt {attempt}).");
                attempt += 1;
                sleep(Duration::from_secs(60)).await;
            }
        }?; // Propagate error to the caller after 10 failed attempts

        if update_info.version() != version {
            error!(
                "Pending update ({}) does not match the installed update needing testing ({}). This is probably an error.",
                update_info.version(),
                version
            );

            // In order to solve this invalid state, we trust the update source and abort the testing.
            // Some other mechanism must have reported the state of the pending update already.
            store.delete("update_needs_testing").await?;
            return Ok(());
        }

        // get the update again to report progress
        let update = update_info.update()?;
        let message = ProgressMessage {
            state: UpdateState::Pending,
            cnt_of: (4, 5),
            message: "Update installed, testing in progress".to_string(),
        };
        progress
            .report_retry(&update, &message, 10, Duration::from_secs(60))
            .await?;

        match check.system_is_ok_with_progress().await {
            Ok(msgs) => {
                // report progress messages if any
                for msg in msgs {
                    progress
                        .report_retry(&update, &msg, 10, Duration::from_secs(60))
                        .await?;
                }

                // testing was successful, mark the update as finished
                let message = ProgressMessage {
                    state: UpdateState::Finished,
                    cnt_of: (5, 5),
                    message: "Update installed successfully".to_string(),
                };
                progress
                    .report_retry(&update, &message, 10, Duration::from_secs(60))
                    .await?;
                // after successfully reporting the update as finished,
                // delete the update_needs_testing key.
                if let Err(e) = store.delete("update_needs_testing").await {
                    // if the store file could not be updated and the update_needs_testing
                    // key could not be removed, we ignore the error for now as it could lead
                    // to an inconsistent state, where the update was successfully closed on
                    // the hawkbit server but the rollback mechanism is not disabled again on
                    // the device.
                    // Not writing the update_needs_testing key in this case is not critical,
                    // as the testing is skipped/the key removed after the next restart, if the
                    // update does not exist on the server anymore.
                    error!("Could not delete update_needs_testing key: {e}");
                }
            }
            Err(err) => {
                // testing failed, report the error
                let message = ProgressMessage {
                    state: UpdateState::Failed(err.to_string()),
                    cnt_of: (2, 2),
                    message: format!("Post-update check failed: {err}"),
                };

                progress
                    .report_retry(&update, &message, 10, Duration::from_secs(60))
                    .await
                    .ok();

                match err {
                    // in case of a rollback, do not monitor the update again after the next reboot
                    UpdateError::ExternalRollback | UpdateError::RollbackNeeded(_) => {
                        if let Err(e) = store.delete("update_needs_testing").await {
                            error!("Could not delete update_needs_testing key: {e}");
                        }
                    }
                    _ => {}
                }

                return Err(err);
            }
        }
    }

    Ok(())
}

/// update_with_consent_flow downloads and installs an update and asks for
/// consent if necessary.
/// Update progress is reported in 6 increments (the last 2 are in
/// monitor_update) to give a rough feedback on the current progress:
/// 0/5 consent given, update started
/// bytes/update_size while downloading
/// 2/5 installing
/// 3/5 awaiting reboot
/// 4/5 self-test
/// 5/5 finished
pub async fn update_with_consent_flow<A: WorkflowOptions>(
    trigger: &mut impl UpdateTrigger<A>,
    source: &mut impl UpdateSource,
    consent: &mut impl ConsentHandler,
    installer: &impl Installer,
    store: &mut impl PersistentStore,
    progress: &mut impl ProgressReporter,
) -> Result<(), UpdateError> {
    // wait for the update trigger to start the update. The trigger returns the
    // update options that configure the workflow.
    let options = trigger.update_triggered().await?;

    let mut update_info = source.check_for_updates().await?;

    // If this update is already installed, do not install it again
    if store
        .load(&update_info.version())
        .await?
        .is_some_and(|status| status == "installed")
    {
        return Err(UpdateError::NoPendingUpdates);
    }

    // If this update was previously declined, ignore it
    if options.ignore_previously_declined()
        && store
            .load(&update_info.version())
            .await?
            .is_some_and(|status| status == "declined")
    {
        return Err(UpdateError::NoPendingUpdates);
    }

    // If the update needs consent, ask for it
    if update_info.needs_consent() {
        match timeout(
            options.consent_timeout(),
            consent.ask_for_consent(&update_info),
        )
        .await
        {
            Ok(result) => {
                if result? {
                    // Report given consent to the update source to unlock update
                    update_info.give_consent().await?;
                } else {
                    store.save(&update_info.version(), "declined").await?;
                    return Err(UpdateError::ConsentDenied);
                }
            }
            Err(_) => {
                return Err(UpdateError::ConsentTimeout);
            }
        }
    }

    // We have or don't need consent -> install the update

    let update = update_info.update()?;
    let message = ProgressMessage {
        state: UpdateState::Pending,
        cnt_of: (0, 5),
        message: "Update consent given, starting update".to_string(),
    };
    progress.report(&update, &message).await?;

    // Fetch and install the update
    let mut install_progress = installer.install(&update)?;

    // Report installation progress until the update finished installing
    while let Some(feedback) = install_progress.next().await? {
        let message = ProgressMessage {
            state: UpdateState::Installing,
            cnt_of: feedback
                .progress
                .map(|(mut a, mut b)| {
                    // scale values to KiB, MiB, GiB… if necessary
                    while a > u32::MAX as u64 || b > u32::MAX as u64 {
                        a >>= 10;
                        b >>= 10;
                    }
                    (a as u32, b as u32)
                })
                .unwrap_or((2, 5)),
            message: format!("Status: {} Message: {}", feedback.status, feedback.message),
        };
        progress.report(&update, &message).await?;
    }

    // Installation finished successfully, do not yet report success as it needs to be tested
    // but mark it as installed to avoid re-installing it
    let message = ProgressMessage {
        state: UpdateState::Pending,
        cnt_of: (3, 5),
        message: "Installation finished, saving state to prepare for reboot".to_string(),
    };
    progress.report(&update, &message).await?;

    store.save(&update_info.version(), "installed").await?;
    store
        .save("update_needs_testing", &update_info.version())
        .await?;

    let message = ProgressMessage {
        state: UpdateState::Pending,
        cnt_of: (3, 5),
        message: "Update installed, checking success after next reboot".to_string(),
    };
    progress.report(&update, &message).await?;

    Ok(())
}

/// consent_flow_no_install describes the update workflow generically, independent of the actual update source, consent handler, etc.
/// The source and consent handler are configured in the main update_workflow function.
pub async fn consent_flow_no_install(
    source: &mut impl UpdateSource,
    consent: &mut impl ConsentHandler,
    store: &mut impl PersistentStore,
    progress: &mut impl ProgressReporter,
    options: UpdateOptions,
) -> Result<(), UpdateError> {
    let mut update_info = source.check_for_updates().await?;

    if options.ignore_previously_declined
        && store
            .load(&update_info.version())
            .await?
            .is_some_and(|status| status == "declined")
    {
        return Err(UpdateError::NoPendingUpdates);
    }

    if update_info.needs_consent() {
        match timeout(
            options.consent_timeout,
            consent.ask_for_consent(&update_info),
        )
        .await
        {
            Ok(result) => {
                if result? {
                    update_info.give_consent().await?;
                    let message = ProgressMessage {
                        state: UpdateState::Pending,
                        cnt_of: (0, 0),
                        message: "Update consent given, starting update".to_string(),
                    };
                    progress.report(&update_info.update()?, &message).await?;
                } else {
                    store.save(&update_info.version(), "declined").await?;
                    return Err(UpdateError::ConsentDenied);
                }
            }
            Err(_) => {
                return Err(UpdateError::ConsentTimeout);
            }
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("Invalid workflow configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Workflow execution failed: {0}")]
    ExecutionFailed(String),
}

type WorkflowFn =
    fn(HashMap<String, String>) -> BoxFuture<'static, Result<ExitCode, WorkflowError>>;

/// Do not use WorkflowPlugin directly, use the `workflow` macro instead:
///
/// ```
/// use libupdated::update_workflow::WorkflowError;
/// use std::collections::HashMap;
/// use libupdated::workflow;
///
/// #[workflow]
/// async fn my_workflow(config: HashMap<String, String>) -> Result<(), WorkflowError> {
///     // Your workflow logic here
///    Ok(())
/// }
/// ```
///
/// The `exiting` argument is used to indicate that the workflow is exiting and should return an `ExitCode`.
///
/// ```
/// use libupdated::update_workflow::WorkflowError;
/// use std::collections::HashMap;
/// use libupdated::workflow;
/// use std::process::ExitCode;
///
/// #[workflow(exiting)]
/// async fn my_workflow(config: HashMap<String, String>) -> Result<ExitCode, WorkflowError> {
///     // Your workflow logic here
///     Ok(ExitCode::SUCCESS)
/// }
/// ```
pub struct WorkflowPlugin {
    pub name: &'static str,
    pub func: WorkflowFn,
}

impl WorkflowPlugin {
    pub const fn new(name: &'static str, func: WorkflowFn) -> Self {
        Self { name, func }
    }
}

inventory::collect!(WorkflowPlugin);

/// Returns the configuration value corresponding to the configuration `key` as [`String`].
///
/// # Errors
///
/// Returns a [`WorkflowError`] if the `key` is not found.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use libupdated::update_workflow;
///
/// let config = HashMap::from([
///     ("sota_tenant".to_string(), "DEFAULT".to_string()),
///     ("sota_target_id".to_string(), "Target1".to_string()),
/// ]);
///
/// let tenant = update_workflow::get_config(&config, "sota_tenant").unwrap();
/// let controller = update_workflow::get_config(&config, "sota_target_id").unwrap();
///
/// assert_eq!(tenant, "DEFAULT".to_string());
/// assert_eq!(controller, "Target1".to_string());
/// ```
pub fn get_config(config: &HashMap<String, String>, key: &str) -> Result<String, WorkflowError> {
    config
        .get(key)
        .cloned()
        .ok_or(WorkflowError::InvalidConfiguration(format!(
            "{key} not found in config"
        )))
}

/// Returns the configuration value corresponding to the configuration `key` as [`bool`].
///
/// # Errors
///
/// Returns a [`WorkflowError`] if the `key` is not found or the value can't be parsed
/// (case insensitive) as a [`bool`].
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use libupdated::update_workflow;
///
/// let config = HashMap::from([
///     ("swupdate_use_streaming".to_string(), "true".to_string()),
///     ("swupdate_dry_run".to_string(), "false".to_string()),
/// ]);
///
/// let use_streaming = update_workflow::get_config_as_bool(&config, "swupdate_use_streaming").unwrap();
/// let dry_run = update_workflow::get_config_as_bool(&config, "swupdate_dry_run").unwrap();
///
/// assert!(use_streaming);
/// assert!(!dry_run);
/// ```
pub fn get_config_as_bool(
    config: &HashMap<String, String>,
    key: &str,
) -> Result<bool, WorkflowError> {
    get_config(config, key)?
        .to_lowercase()
        .parse()
        .map_err(|_| {
            WorkflowError::InvalidConfiguration(format!("failed to parse {} as bool", key))
        })
}

/// Returns the configuration value corresponding to the configuration `key` as [`u64`].
///
/// # Errors
///
/// Returns a [`WorkflowError`] if the `key` is not found or the value can't be parsed
/// as a [`u64`].
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use libupdated::update_workflow;
///
/// let config = HashMap::from([
///     ("timeout_secs".to_string(), "30".to_string()),
/// ]);
///
/// let timeout_secs = update_workflow::get_config_as_u64(&config, "timeout_secs").unwrap();
///
/// assert_eq!(timeout_secs, 30);
/// ```
pub fn get_config_as_u64(
    config: &HashMap<String, String>,
    key: &str,
) -> Result<u64, WorkflowError> {
    get_config(config, key)?
        .parse::<u64>()
        .map_err(|_| WorkflowError::InvalidConfiguration(format!("failed to parse {} as u64", key)))
}
