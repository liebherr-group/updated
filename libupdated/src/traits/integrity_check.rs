// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use crate::{progress_reporter::ProgressMessage, update_workflow::UpdateError};

/// A trait for checking the integrity of a system.
/// After an update has been installed, the system should be checked for integrity to ensure that the update was successful.
/// One or more integrity checks can be executed to check different aspects of the system.
/// ``` rust
/// use libupdated::update_workflow::UpdateError;
/// use libupdated::integrity_check::IntegrityCheck;
///
/// async fn example(checks: &mut impl IntegrityCheck) {
///   match checks.system_is_ok().await {
///     Ok(()) => {
///       println!("Everything ok!");
///     },
///     Err(UpdateError::RollbackNeeded(reason)) => {
///       println!("Rollback: {reason}");
///     },
///     Err(_) => {
///       println!("Checks crashed!");
///     }
///   };
/// }
/// ```
pub trait IntegrityCheck {
    /// Execute the integrity check.
    /// If successful, Ok(()) will be returned.
    /// Otherwise, a Rollback can be requested by returning Err(RollbackNeeded)
    fn system_is_ok(&mut self) -> impl std::future::Future<Output = Result<(), UpdateError>>;
}

/// A trait for checking the integrity of a system with progress reporting.
/// After an update has been installed, the system should be checked for integrity to ensure that the update was successful.
/// One or more integrity checks can be executed to check different aspects of the system.
/// ``` rust
/// use libupdated::update_workflow::UpdateError;
/// use libupdated::integrity_check::IntegrityCheckWithProgress;
///
/// async fn example(checks: &mut impl IntegrityCheckWithProgress) {
///   match checks.system_is_ok_with_progress().await {
///     Ok(progress) => {
///       for message in progress {
///         println!("Progress: {}", message.message);
///       }
///       println!("Everything ok!");
///     },
///     Err(UpdateError::RollbackNeeded(reason)) => {
///       println!("Rollback: {reason}");
///     },
///     Err(_) => {
///       println!("Checks crashed!");
///     }
///   };
/// }
/// ```
pub trait IntegrityCheckWithProgress {
    /// Execute the integrity check with progress reporting.
    /// If successful, Ok(Vec<ProgressMessage>) will be returned.
    /// Otherwise, a Rollback can be requested by returning Err(RollbackNeeded)
    fn system_is_ok_with_progress(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Vec<ProgressMessage>, UpdateError>>;
}

/// Blanket implementation of IntegrityCheckWithProgress for all IntegrityCheck types
/// to keep backward compatibility.
impl<T: IntegrityCheck> IntegrityCheckWithProgress for T {
    async fn system_is_ok_with_progress(&mut self) -> Result<Vec<ProgressMessage>, UpdateError> {
        self.system_is_ok().await?;
        Ok(vec![])
    }
}

// Implement IntegrityCheckWithProgress for tuples of IntegrityChecks with up to 5 elements

macro_rules! impl_integrity_check_for_tuple {
    ( $( $name:ident )+ ) => {
        impl<$($name: IntegrityCheckWithProgress),+> IntegrityCheckWithProgress for ($($name,)+)
        {
            async fn system_is_ok_with_progress(&mut self) -> Result<Vec<ProgressMessage>, UpdateError> {
                #[allow(non_snake_case)]
                let ($($name,)+) = self;
                let mut progress_messages = Vec::new();
                $(progress_messages.push($name.system_is_ok_with_progress().await?);)+
                Ok(progress_messages.into_iter().flatten().collect())
            }
        }
    };
}

impl_integrity_check_for_tuple! { A B }
impl_integrity_check_for_tuple! { A B C }
impl_integrity_check_for_tuple! { A B C D }
impl_integrity_check_for_tuple! { A B C D E }
