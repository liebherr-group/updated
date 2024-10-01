// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

/// PersistentStore represents a key-value store that is persistent across reboots and updates.
/// It serves as a way to store state across reboots and updates.
/// It can be implemented, e.g., using a file on a data partition, the bootloader environment, or a remote database.
/// It is important that the store is resilient to power loss and other failures and eagerly persists all changes.
///
/// ``` rust
/// use libupdated::traits::persistent_store::*;
/// use libupdated::update_workflow::UpdateError;
///
/// async fn example(store: &mut impl PersistentStore) -> Result<(), UpdateError> {
///     if !store.exists("initialized").await? {
///         // do initialization
///         // save initialization state
///         store.save("initialized", "true").await?;
///     } else {
///         let data = store.load("previous_data").await?.unwrap();
///     }
///     Ok(())
/// }
/// ```
pub trait PersistentStore {
    /// save a key-value pair to the store.
    fn save(
        &mut self,
        key: &str,
        value: &str,
    ) -> impl std::future::Future<Output = Result<(), StoreError>> + Send;
    /// load a value from the store by key. If the key does not exist, None is returned.
    fn load(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<Option<String>, StoreError>> + Send;
    /// delete a key from the store if it exists.
    fn delete(
        &mut self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<(), StoreError>> + Send;
    /// check if a key exists in the store.
    fn exists(
        &self,
        key: &str,
    ) -> impl std::future::Future<Output = Result<bool, StoreError>> + Send;
}
