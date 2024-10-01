// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use bytes::Bytes;
use std::{
    any::Any,
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::update_workflow::UpdateError;

#[derive(Debug, Error)]
pub enum UpdateSourceError {
    #[error("configuration error: {0}")]
    ConfigurationError(String),
    #[error("connection error: {0}")]
    ConnectionError(String),
    #[error("fetch error: {0}")]
    FetchError(String),
    #[error("Consent required")]
    ConsentRequired,
    #[error("Invalid state")]
    InvalidState,
}

impl From<StreamError> for UpdateError {
    fn from(e: StreamError) -> Self {
        UpdateError::UpdateSourceError(UpdateSourceError::FetchError(format!(
            "stream error: {}",
            e
        )))
    }
}

/// The UpdateInfo trait represents information about an available update.
/// This information includes whether the update needs user consent to be installed,
/// arbitrary metadata about the update, and a unique "version" to identify the update.
/// Note that the version is not necessarily a semantic version of the files to be installed,
/// but rather a unique identifier for the update, e.g. a hawkbit action_id.
/// The update itself is not yet fetched or downloaded.
pub trait UpdateInfo {
    /// True if the update needs user consent to be installed.
    fn needs_consent(&self) -> bool;
    /// Arbitrary key-value metadata about the update.
    fn metadata(&self) -> HashMap<String, String>;
    /// The version identifier of the update.
    fn version(&self) -> String;
    /// Give consent to install the update. An implementation might log this consent or
    /// e.g., forward it to an update server to get the download link.
    fn give_consent(&mut self)
    -> impl std::future::Future<Output = Result<(), UpdateError>> + Send;
    /// Get the update handle that can be used to download the update.
    /// This fails with Err(UpdateSourceError::ConsentRequired) if the update needs consent and the user has not given it yet.
    fn update(&self) -> Result<impl Update, UpdateError>;
}

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("streaming unsupported")]
    Unsupported,
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    ReceiveError(String),
    #[error("{0}")]
    SendError(String),
}

impl From<tokio::sync::mpsc::error::SendError<Result<Bytes, StreamError>>> for StreamError {
    fn from(e: tokio::sync::mpsc::error::SendError<Result<Bytes, StreamError>>) -> Self {
        StreamError::SendError(format!("send error: {}", e))
    }
}

pub type StreamSender = Sender<Result<Bytes, StreamError>>;
pub type StreamReceiver = Receiver<Result<Bytes, StreamError>>;

/// The Update trait represents an update that is ready to be downloaded and installed on the device.
pub trait Update: Send + Sync + Clone + 'static {
    /// Support function to downcast the trait object to the actual implementation.
    fn as_any(&self) -> &dyn Any;
    /// The version identifier of the update.
    fn version(&self) -> &str;
    /// The download size of the update
    fn size(&self) -> u64;
    /// Save all files of the update to disk in the given directory. All saved file paths are returned.
    fn save_to_disk(
        &self,
        path_hint: Option<PathBuf>,
    ) -> impl std::future::Future<Output = Result<Vec<UpdateFile<impl Update>>, UpdateError>>
    + std::marker::Send {
        async move {
            if let Some(path_hint) = &path_hint {
                tokio::fs::create_dir_all(path_hint).await?
            }

            let mut target_files = Vec::new();
            for file in self.files() {
                target_files.push(self.save_to_disk_by_name(file, path_hint.clone()).await?);
            }
            Ok(target_files)
        }
    }
    /// Get a download stream (mpsc::Receiver) of the update file to process it without saving it to disk.
    /// A separate task to fill the stream is spawned, so this method does not block.
    fn stream(&self) -> Result<StreamReceiver, UpdateError> {
        let (tx, rx) = mpsc::channel::<Result<Bytes, StreamError>>(1);
        let update = self.clone();
        tokio::spawn(async move {
            let tx = tx;
            for file in update.files() {
                update.stream_by_name(file, &tx).await;
            }
        });
        Ok(rx)
    }
    /// save a specific *artifact* from the update to disk in the directory *save_dir*.
    fn save_to_disk_by_name(
        &self,
        artifact: String,
        save_dir: Option<PathBuf>,
    ) -> impl std::future::Future<Output = Result<UpdateFile<impl Update>, UpdateError>>
    + std::marker::Send;
    /// receive a specific *artifact* from the update through a *stream*. Any errors that occur are sent to the stream.
    /// No separate task is spawned, i.e. without awaiting this method, nothing is pushed to the stream.
    fn stream_by_name(
        &self,
        artifact: String,
        stream: &StreamSender,
    ) -> impl std::future::Future<Output = ()> + std::marker::Send;
    /// get a list of artifacts in the update that can then be fetched separately.
    fn files(&self) -> Vec<String>;
}

#[derive(Clone)]
pub struct UpdateFile<U: Update> {
    pub filename: PathBuf,
    pub version: String,
    pub size: u64,
    pub parent_update: Arc<U>,
}

impl<U: Update> UpdateFile<U> {
    pub async fn new(filename: &Path, parent: &U) -> Result<UpdateFile<U>, UpdateError> {
        let size = tokio::fs::metadata(filename).await?.size();
        Ok(UpdateFile {
            filename: PathBuf::from(filename),
            version: parent.version().to_string(),
            size,
            parent_update: Arc::new(parent.clone()),
        })
    }
}

impl<U: Update> Update for UpdateFile<U> {
    fn as_any(&self) -> &dyn Any {
        self.parent_update.as_any()
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn size(&self) -> u64 {
        self.size
    }

    async fn save_to_disk_by_name(
        &self,
        artifact: String,
        save_dir: Option<PathBuf>,
    ) -> Result<UpdateFile<impl Update>, UpdateError> {
        if let Some(save_dir) = save_dir {
            // the the target directory is given, copy the file
            let filename = Path::new(&artifact).file_name();
            if let Some(filename) = filename {
                let mut target_file = save_dir;
                target_file.push(filename);

                // if the target file and the local file are the same, skip copying it
                if self.filename != target_file {
                    tokio::fs::copy(artifact, &target_file).await?;
                }
                Ok(UpdateFile::new(&target_file, &*self.parent_update).await?)
            } else {
                Err(UpdateSourceError::FetchError(format!("{artifact} is not a filename")).into())
            }
        } else {
            // otherwise, skip the copy and return self
            Ok(self.clone())
        }
    }

    async fn stream_by_name(&self, _artifact: String, stream: &StreamSender) {
        stream.send(Err(StreamError::Unsupported)).await.ok();
    }

    fn files(&self) -> Vec<String> {
        vec![self.filename.to_string_lossy().to_string()]
    }
}

/// The UpdateSource trait represents a source of updates that can be checked for new updates.
///
/// ``` rust
/// use libupdated::traits::update_source::*;
/// use libupdated::update_workflow::UpdateError;
/// use std::path::PathBuf;
///
/// async fn example(source: &mut impl UpdateSource) -> Result<(), UpdateError> {
///     let mut update_info = source.check_for_updates().await?;
///
///     if update_info.needs_consent() {
///         // TODO: actually ask the user
///         update_info.give_consent().await?;
///     }
///
///     // now we can get the update
///     let update = update_info.update()?;
///
///     // download the update to the /tmp folder
///     let update_file = update.save_to_disk(Some(PathBuf::from("/tmp"))).await?;
///
///     Ok(())
/// }
/// ```
pub trait UpdateSource {
    /// Check for updates from the source.
    /// This function returns an UpdateInfo object if an update is available.
    /// Otherwise, it returns Err(UpdateError::NoPendingUpdates).
    fn check_for_updates(
        &mut self,
    ) -> impl std::future::Future<Output = Result<impl UpdateInfo, UpdateError>>;
}
