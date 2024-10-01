// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use hex::ToHex;
use reqwest::{Client, Response, header::RANGE};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::{
    update_source::UpdateSourceError, update_workflow::UpdateError,
    util::hash_algorithm::HashAlgorithm,
};

/// Download client that wraps a `reqwest::Client` configured with optional TLS
/// settings and timeouts.
#[derive(Debug, Clone)]
pub struct DownloadClient {
    pub client: reqwest::Client,
}

impl DownloadClient {
    /// Creates a new `DownloadClient` instance with optional TLS settings and
    /// timeouts.
    pub async fn new(
        server_cert: Option<&str>,
        client_cert: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client = Self::client_builder(server_cert, client_cert, timeout)
            .await?
            .build()?;
        Ok(Self { client })
    }

    /// Creates a `DownloadClient` instance from an existing `reqwest::Client`.
    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Configures a client builder with optional TLS settings and timeouts, but
    /// returns the resulting `reqwest::ClientBuilder` instead of constructing a
    /// `DownloadClient`. This is useful if you want to further customize the
    /// client before constructing a `DownloadClient` (see `from_client`) or if
    /// the `reqwest::Client` should be used elsewhere, e.g. to construct a
    /// hawkbit client.
    pub async fn client_builder(
        server_cert: Option<&str>,
        client_cert: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<reqwest::ClientBuilder, Box<dyn std::error::Error>> {
        let mut builder = Client::builder();

        if let Some(timeout) = timeout {
            builder = builder.connect_timeout(timeout).read_timeout(timeout);
        }
        if let Some(server_cert) = server_cert {
            let mut buf = Vec::new();
            let mut file = File::open(server_cert).await?;
            file.read_to_end(&mut buf).await?;
            let certs = reqwest::Certificate::from_pem_bundle(&buf)?;

            builder = builder.tls_certs_only(certs).https_only(true);
        }
        if let Some(client_cert) = client_cert {
            let mut buf = Vec::new();
            let mut file = File::open(client_cert).await?;
            file.read_to_end(&mut buf).await?;
            let identity = reqwest::Identity::from_pem(&buf)?;

            builder = builder.identity(identity);
        }

        Ok(builder)
    }

    /// Downloads a file from the specified URL to the given file path.
    ///
    /// The download will be verified against the expected hash if provided.
    /// If the file is already downloaded in parts, the download will be resumed
    /// if possible.
    pub async fn download(
        &self,
        url: &str,
        file_name: &Path,
        expected_size: Option<usize>,
        hash_algorithm: HashAlgorithm,
        expected_hash: &str,
    ) -> Result<(), UpdateError> {
        let dir = file_name
            .parent()
            .ok_or(UpdateSourceError::FetchError(format!(
                "could not create download directory {file_name:?}"
            )))?;
        if !dir.exists() {
            tokio::fs::create_dir_all(dir).await?;
        }

        let size_matches = if tokio::fs::try_exists(&file_name).await?
            && let Some(expected_size) = expected_size
        {
            tokio::fs::metadata(&file_name).await?.len() as usize == expected_size
        } else {
            // the filesize is unknown, skip this check
            true
        };

        if tokio::fs::try_exists(&file_name).await?
            && size_matches
            && let Some(hash) = hash_algorithm.hash_file(file_name).await?
        {
            if hash == expected_hash.to_lowercase() {
                return Ok(());
            } else {
                // the existing file has a hash mismatch, so we remove it to force a
                // complete download
                tokio::fs::remove_file(&file_name).await?;
            }
        }

        // the file is first downloaded to a .part file in order to
        // be able to resume the download in case of a disconnection.
        // If a part file already exists, we try to resume the download (if supported by the server).
        let mut file_name_part = dir.to_path_buf();
        file_name_part.push(format!(
            "{}.part",
            file_name.file_name().unwrap().to_string_lossy()
        ));

        let response = if tokio::fs::try_exists(&file_name_part).await? {
            // try to resume the download
            let metadata = tokio::fs::metadata(&file_name_part).await?;
            self.client
                .get(url)
                .header(RANGE, format!("bytes={}-", metadata.len()))
                .send()
                .await
        } else {
            self.client.get(url).send().await
        }
        .and_then(Response::error_for_status)
        .map_err(|e| UpdateSourceError::ConnectionError(e.to_string()))?;

        let mut hasher = hash_algorithm.hasher();

        let mut dest = if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            // the server supports range requests, we can resume the download.
            // The already downloaded part has to be hashed as well, otherwise the
            // hash of the resumed download would only cover the appended bytes.
            if let Some(hasher) = &mut hasher {
                let mut downloaded = ReaderStream::new(File::open(&file_name_part).await?);
                while let Some(chunk) = downloaded.next().await {
                    hasher.update(&chunk?);
                }
            }

            OpenOptions::new()
                .append(true)
                .open(&file_name_part)
                .await?
        } else {
            File::create(&file_name_part).await?
        };

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| UpdateSourceError::ConnectionError(e.to_string()))?;
            if let Some(hasher) = &mut hasher {
                hasher.update(&chunk);
            }
            dest.write_all(&chunk).await?;
        }

        let hash = hasher.map(|h| h.finalize());
        // verify the hash before renaming the file
        if let Some(hash) = hash
            && hash.encode_hex::<String>() != expected_hash.to_lowercase()
        {
            // remove the file with the hash mismatch, so it is downloaded again on
            // a retry.
            tokio::fs::remove_file(&file_name_part).await?;
            return Err(UpdateSourceError::FetchError(format!(
                "Hash mismatch in downloaded file (expected: {expected_hash}, actual: {}",
                hash.encode_hex::<String>()
            ))
            .into());
        }

        // rename the file to remove the .part extension after the download is complete
        tokio::fs::rename(&file_name_part, &file_name).await?;

        Ok(())
    }
}
