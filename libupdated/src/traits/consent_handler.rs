// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use thiserror::Error;

use crate::{update_source::UpdateInfo, update_workflow::UpdateError};

#[derive(Debug, Error)]
pub enum ConsentHandlerError {
    #[error("Consent handler error: {0}")]
    Error(String),
}

/// A trait for asking the user for consent before performing an update.
///
/// ``` rust
/// use libupdated::traits::consent_handler::*;
/// use libupdated::traits::update_source::*;
/// use libupdated::update_workflow::UpdateError;
///
/// async fn example(consent_handler: &mut impl ConsentHandler,
///                  update_info: &impl UpdateInfo) -> Result<(), UpdateError> {
///     if consent_handler.ask_for_consent(update_info).await? {
///         // install update
///     } else {
///         // don't install update
///     }
///
///     Ok(())
/// }
/// ```
pub trait ConsentHandler {
    /// Ask the user for consent to perform an update.
    /// The update_info contains metadata about the update that is about to be installed, e.g., the version, changelog, etc.
    /// This metadata should be displayed to the user to allow them an informed decision.
    /// The user's consent is returned as a boolean. If true, the update is allowed to proceed.
    fn ask_for_consent(
        &mut self,
        update_info: &impl UpdateInfo,
    ) -> impl std::future::Future<Output = Result<bool, UpdateError>>;
}

/// A minimal consent handler that automatically either declines or consents to all updates.
pub struct AutoConsent {
    /// Result that should be returned for all consent requests
    pub consent: bool,
}

impl AutoConsent {
    pub fn new(consent: bool) -> Self {
        Self { consent }
    }
}

impl ConsentHandler for AutoConsent {
    async fn ask_for_consent(
        &mut self,
        _update_info: &impl UpdateInfo,
    ) -> Result<bool, UpdateError> {
        Ok(self.consent)
    }
}
