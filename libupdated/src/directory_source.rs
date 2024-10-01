// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::{fs::File, io::BufReader};

use crate::{
    update_source::{
        StreamSender, Update, UpdateFile, UpdateInfo, UpdateSource, UpdateSourceError,
    },
    update_workflow::UpdateError,
};

/// The expected file name of the manifest file that describes the update.
/// The source's check_for_updates will return an UpdateInfo if this file is
/// found in the configured update directory.
const UPDATE_MANIFEST: &str = "update.json";

pub struct DirectorySource {
    source_dir: PathBuf,
}

impl DirectorySource {
    pub fn new(source_dir: &Path) -> Self {
        DirectorySource {
            source_dir: source_dir.to_path_buf(),
        }
    }
}

impl UpdateSource for DirectorySource {
    #[allow(refining_impl_trait)]
    async fn check_for_updates(&mut self) -> Result<DirectoryUpdateInfo, UpdateError> {
        match tokio::fs::try_exists(self.source_dir.join(UPDATE_MANIFEST).as_path()).await {
            Ok(true) => {
                let filename = self.source_dir.join(UPDATE_MANIFEST);
                let content = &tokio::fs::read_to_string(filename.as_path()).await?;
                let mut info =
                    serde_json::from_str::<DirectoryUpdateInfo>(content).map_err(|e| {
                        UpdateSourceError::FetchError(format!("could not parse {filename:?}: {e}"))
                    })?;

                // prepend the source directory to all files in the update
                for file in &mut info.files {
                    let mut buf = PathBuf::new();
                    buf.push(&self.source_dir);
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    // clippy gives a false positive
                    buf.push(&file);
                    *file = buf.to_string_lossy().into_owned();
                }

                Ok(info)
            }
            Ok(false) => Err(UpdateError::NoPendingUpdates),
            Err(err) => Err(UpdateError::UpdateSourceError(
                UpdateSourceError::ConnectionError(format!("{err}")),
            )),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct DirectoryUpdateInfo {
    pub update_id: String,
    pub metadata: HashMap<String, String>,
    pub files: Vec<String>,
}

impl UpdateInfo for DirectoryUpdateInfo {
    fn needs_consent(&self) -> bool {
        false
    }

    fn metadata(&self) -> HashMap<String, String> {
        self.metadata.clone()
    }

    fn version(&self) -> String {
        self.update_id.clone()
    }

    async fn give_consent(&mut self) -> Result<(), UpdateError> {
        Ok(())
    }

    #[allow(refining_impl_trait)]
    fn update(&self) -> Result<DirectoryUpdate, UpdateError> {
        Ok(DirectoryUpdate::new(
            &self.update_id,
            self.files.iter().map(|s| s.into()).collect(),
        ))
    }
}

#[derive(Clone)]
pub struct DirectoryUpdate {
    update_id: String,
    files: Vec<PathBuf>,
    size: u64,
}

impl DirectoryUpdate {
    fn new(update_id: &str, files: Vec<PathBuf>) -> DirectoryUpdate {
        // calculate the size of the update by adding the file sizes
        let size = files.iter().fold(0u64, |v, path| {
            std::fs::metadata(path).map(|m| m.size()).unwrap_or(0) + v
        });

        DirectoryUpdate {
            update_id: update_id.to_string(),
            files,
            size,
        }
    }
}

impl Update for DirectoryUpdate {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn version(&self) -> &str {
        &self.update_id
    }

    fn size(&self) -> u64 {
        self.size
    }

    async fn save_to_disk_by_name(
        &self,
        artifact: String,
        save_dir: Option<PathBuf>,
    ) -> Result<UpdateFile<impl Update>, UpdateError> {
        // for a DirectoryUpdate, the artifact is the absolute path of the source file;
        // extract the target file name from it
        let filename = Path::new(&artifact).file_name();

        if let Some(filename) = filename {
            // create the target file name from the save_dir and the artifact name
            // if no save_dir is given, choose the directory in which the file
            // already exists to skip copying it.
            let target_file = if let Some(save_dir) = save_dir {
                let mut target_file = save_dir;
                target_file.push(filename);

                tokio::fs::copy(artifact, &target_file).await?;

                target_file
            } else {
                PathBuf::from(&artifact)
            };

            Ok(UpdateFile::new(&target_file, self).await?)
        } else {
            Err(UpdateSourceError::FetchError(format!("{artifact} is not a filename")).into())
        }
    }

    async fn stream_by_name(&self, artifact: String, tx: &StreamSender) {
        match File::open(artifact).await {
            Ok(file) => {
                let mut reader = BufReader::new(file);
                let mut buf = Vec::new();

                loop {
                    buf.clear();

                    match reader.read_buf(&mut buf).await {
                        Ok(bytes_read) => {
                            if bytes_read == 0 {
                                break;
                            }

                            tx.send(Ok(Bytes::from(buf.clone())))
                                .await
                                .map_err(|err| tx.try_send(Err(err.into())))
                                .ok();
                        }
                        Err(e) => {
                            tx.try_send(Err(e.into())).ok();
                            return;
                        }
                    }
                }
            }
            Err(err) => {
                tx.try_send(Err(err.into())).ok();
            }
        }
    }

    fn files(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }
}
