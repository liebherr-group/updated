// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use futures::StreamExt;
use hex::ToHex;
use sha2::digest::DynDigest;
use sha2::{Digest, Sha256, Sha512};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

/// Hash algorithm used to verify file integrity.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    #[default]
    Sha256,
    Sha512,
    /// Other unsupported algorithm; store its name for reference.
    Other(String),
}

pub type Hasher = Box<dyn DynDigest + Send>;

impl HashAlgorithm {
    /// Creates a `HashAlgorithm` instance from an algorithm name.
    pub fn from_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "sha256" | "sha-256" => HashAlgorithm::Sha256,
            "sha512" | "sha-512" => HashAlgorithm::Sha512,
            other => HashAlgorithm::Other(other.to_string()),
        }
    }

    /// Returns the name of the hash algorithm as a string.
    pub fn name(&self) -> String {
        match self {
            HashAlgorithm::Sha256 => "sha256".to_string(),
            HashAlgorithm::Sha512 => "sha512".to_string(),
            HashAlgorithm::Other(name) => name.clone(),
        }
    }

    /// Returns a hasher instance corresponding to the hash algorithm, if supported.
    pub fn hasher(&self) -> Option<Hasher> {
        match self {
            HashAlgorithm::Sha256 => Some(Box::new(Sha256::new())),
            HashAlgorithm::Sha512 => Some(Box::new(Sha512::new())),
            HashAlgorithm::Other(_) => None,
        }
    }

    /// Computes the hash of a file at the given path using the hash algorithm.
    /// Returns `None` if the algorithm is unsupported.
    pub async fn hash_file(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<String>, std::io::Error> {
        if let Some(mut hasher) = self.hasher() {
            let mut file = ReaderStream::new(File::open(path).await?);

            while let Some(chunk) = file.next().await {
                hasher.update(&chunk?);
            }

            Ok(Some(hasher.finalize().encode_hex::<String>()))
        } else {
            Ok(None)
        }
    }
}
