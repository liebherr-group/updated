// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use tracing::info;

use crate::{
    progress_reporter::{ProgressMessage, ProgressReporter},
    update_source::Update,
    update_workflow::UpdateError,
};

pub struct LogReporter {
    log: String,
}

impl LogReporter {
    pub fn new(log: &str) -> Self {
        Self {
            log: log.to_string(),
        }
    }
}

impl ProgressReporter for LogReporter {
    async fn report(
        &mut self,
        update: &impl Update,
        message: &ProgressMessage,
    ) -> Result<(), UpdateError> {
        if message.cnt_of.1 == 0 {
            info!(
                self.log,
                "Update progress ({}): {:?} - {}",
                update.version(),
                message.state,
                message.message
            );
        } else {
            info!(
                self.log,
                "Update progress ({}): {:?} ({}/{}) - {}",
                update.version(),
                message.state,
                message.cnt_of.0,
                message.cnt_of.1,
                message.message
            );
        }
        Ok(())
    }
}
