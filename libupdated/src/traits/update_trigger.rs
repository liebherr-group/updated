// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateTriggerError {
    #[error("Trigger error: {0}")]
    TriggerError(String),
}

/// An *UpdateTrigger* encapsulates the condition to start an update workflow.
/// If the update is triggered, the trigger provides the update with user
/// defined information, e.g. additional configuration or the triggering event.
///
/// ``` rust
/// use libupdated::traits::update_trigger::*;
/// use libupdated::update_workflow::UpdateError;
/// use std::time::Instant;
///
/// enum TriggerSource {Manual(String), Timed(Instant)}
///
/// async fn example(trigger: &mut impl UpdateTrigger<TriggerSource>) -> Result<(), UpdateError> {
///     match trigger.update_triggered().await? {
///         TriggerSource::Manual(config) => {
///             println!("Manual update triggered. Config: {config}")
///         },
///         TriggerSource::Timed(time) => {
///             println!("Timed update triggered. Time: {time:?}")
///         },
///     }
///
///     Ok(())
/// }
/// ```
pub trait UpdateTrigger<T> {
    /// Trigger the update process.
    /// The trigger can define additional data (T) that is returned when the
    /// update is triggered.
    /// This user data can be used to discern different triggers if the update
    /// might be triggered in different ways or return, e.g., a configuration
    /// for the update process.
    fn update_triggered(
        &mut self,
    ) -> impl std::future::Future<Output = Result<T, UpdateTriggerError>>;
}

// Implement UpdateTrigger for tuples of UpdateTriggers with up to 5 elements

// all trigger conditions are awaited in parallel and the user data of the
// first trigger that fires is returned. All other triggers are cancelled.

macro_rules! impl_update_trigger_for_tuple {
    ( $( $name:ident )+ ) => {
        impl<T, $($name: UpdateTrigger<T>),+> UpdateTrigger<T> for ($($name,)+)
        {
            async fn update_triggered(&mut self) -> Result<T, UpdateTriggerError> {
                #[allow(non_snake_case)]
                let ($($name,)+) = self;
                let update_trigger_value = tokio::select! {
                    $(value = $name.update_triggered() => value,)+
                };
                update_trigger_value
            }
        }
    };
}

impl_update_trigger_for_tuple! { A B }
impl_update_trigger_for_tuple! { A B C }
impl_update_trigger_for_tuple! { A B C D }
impl_update_trigger_for_tuple! { A B C D E }
