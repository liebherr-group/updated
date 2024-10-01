// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::error;

use crate::{update_source::Update, update_workflow::UpdateError};

/// A *ProgressReporter* reports the current status of an *Update*, e.g. to a
/// server or a log file.
///
/// ``` rust
/// use libupdated::traits::installer::*;
/// use libupdated::traits::progress_reporter::*;
/// use libupdated::traits::update_source::*;
/// use libupdated::update_workflow::UpdateError;
///
/// async fn example(reporter: &mut impl ProgressReporter,
///                  update: &impl Update) -> Result<(), UpdateError> {
///     let progress_msg = ProgressMessage {
///         state: UpdateState::Finished,
///         cnt_of: (100, 100),
///         message: "Update installed successfully".to_string(),
///     };
///
///     reporter.report(update, &progress_msg).await?;
///
///     Ok(())
/// }
/// ```
pub trait ProgressReporter: Send + Sync {
    /// Send a progress message for an update.
    /// The destination and handling of the message is implementation-specific.
    fn report(
        &mut self,
        update: &impl Update,
        message: &ProgressMessage,
    ) -> impl std::future::Future<Output = Result<(), UpdateError>>;

    /// Repeatedly retry sending a progress message for an update.
    /// This should be used for important messages, e.g. the final
    /// message to close an update on the update server.
    /// If this is used with a tuple of progress reporters, the message
    /// might be duplicated and sent multiple times to some of the progress reporters.
    #[allow(async_fn_in_trait)]
    async fn report_retry(
        &mut self,
        update: &impl Update,
        message: &ProgressMessage,
        attempts: u32,
        wait_duration: Duration,
    ) -> Result<(), UpdateError> {
        for retries in (0..=attempts).rev() {
            match self.report(update, message).await {
                Ok(_) => break,
                Err(e) => {
                    if retries != 0 {
                        error!("Failed to report progress (Retries: {retries}): {e}");
                        sleep(wait_duration).await;
                    } else {
                        // after the attempt limit is reached, give up
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }
}

/// The current state of an update.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateState {
    /// The update is currently waiting to be processed, either because it is not yet
    /// scheduled or because it must be tested before reporting successful installation.
    Pending,
    /// The update is currently being downloaded.
    Downloading,
    /// The update is currently being installed.
    Installing,
    /// The update has been successfully installed.
    Finished,
    /// The update has failed to install.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ProgressMessage {
    pub state: UpdateState,
    pub cnt_of: (u32, u32),
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct VecReporter {
    pub messages: Vec<(String, ProgressMessage)>,
}

impl VecReporter {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

impl ProgressReporter for VecReporter {
    async fn report(
        &mut self,
        update: &impl Update,
        message: &ProgressMessage,
    ) -> Result<(), UpdateError> {
        self.messages
            .push((update.version().to_string(), message.clone()));
        Ok(())
    }
}

// Implement ProgressReporter for tuples of ProgressReporters with up to 5 elements

macro_rules! impl_progress_report_for_tuple {
    ( $( $name:ident )+ ) => {
        impl<$($name: ProgressReporter),+> ProgressReporter for ($($name,)+)
        {
            async fn report(&mut self, update: &impl Update, message: &ProgressMessage) -> Result<(), UpdateError> {
                #[allow(non_snake_case)]
                let ($($name,)+) = self;
                $($name.report(update, message).await?;)+
                Ok(())
            }
        }
    };
}

impl_progress_report_for_tuple! { A B }
impl_progress_report_for_tuple! { A B C }
impl_progress_report_for_tuple! { A B C D }
impl_progress_report_for_tuple! { A B C D E }
