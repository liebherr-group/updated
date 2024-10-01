// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use hawkbit::ddi::{Execution, Finished};
use serde::Serialize;

use crate::{
    progress_reporter::{ProgressMessage, ProgressReporter, UpdateState},
    update_source::{Update, UpdateSourceError},
    update_workflow::UpdateError,
};

use super::HawkbitUpdate;

#[derive(Default, Clone, Copy, Debug)]
pub struct HawkbitProgressReporter {}

impl HawkbitProgressReporter {
    pub fn new() -> Self {
        Self {}
    }
}

impl ProgressReporter for HawkbitProgressReporter {
    async fn report(
        &mut self,
        update: &impl Update,
        message: &ProgressMessage,
    ) -> Result<(), UpdateError> {
        match update.as_any().downcast_ref::<HawkbitUpdate>() {
            Some(update) => {
                // reporting progress back to hawkbit is done through the Update object.
                update.report_progress(message).await
            }
            // if a non-hawkbit update is passed, we can therefore not report progress
            None => Err(UpdateError::UpdateSourceError(
                UpdateSourceError::InvalidState,
            )),
        }
    }
}

impl HawkbitUpdate {
    /// Report the progress of the update to the Hawkbit server.
    /// The progress is reported as a percentage of the total size of the update.
    ///
    /// arguments:
    /// - message: The progress message to report.
    pub(crate) async fn report_progress(
        &self,
        message: &ProgressMessage,
    ) -> Result<(), UpdateError> {
        let mut details = vec![message.message.as_str()];

        let execution_status = match &message.state {
            UpdateState::Pending => (Execution::Scheduled, Finished::None),
            UpdateState::Downloading => (Execution::Download, Finished::None),
            UpdateState::Installing => (Execution::Proceeding, Finished::None),
            UpdateState::Finished => (Execution::Closed, Finished::Success),
            UpdateState::Failed(reason) => {
                details.push(reason);
                (Execution::Closed, Finished::Failure)
            }
        };

        #[derive(Serialize)]
        struct Progress {
            cnt: u32,
            of: u32,
        }
        let progress = Some(Progress {
            cnt: message.cnt_of.0,
            of: message.cnt_of.1,
        });

        self.update
            .send_feedback_with_progress(execution_status.0, execution_status.1, progress, details)
            .await?;
        Ok(())
    }
}
