// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

//! Integration tests for [`DownloadClient::download`].
//!
//! The tests run against a minimal HTTPS server: `openssl s_server` is spawned
//! as a raw TLS terminator (mutual TLS, no `-WWW`) and the HTTP request/response
//! exchange is driven through its stdin/stdout by [`TestServer`]. This gives the
//! tests full control over the HTTP semantics (range requests, stalled
//! responses, ...) which the built in `-WWW` file server of openssl does not
//! provide.

use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use libupdated::update_source::UpdateSourceError;
use libupdated::update_workflow::UpdateError;
use libupdated::util::download::DownloadClient;
use libupdated::util::hash_algorithm::HashAlgorithm;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

/// Root directory for the certificates and download directories of all tests.
const TEST_ROOT: &str = "/tmp/updated_download_tests";

// ---------------------------------------------------------------------------
// certificates
// ---------------------------------------------------------------------------

/// Certificate authority, server and client certificates used by the tests.
/// They are generated once per test binary invocation.
struct Certs {
    dir: PathBuf,
}

impl Certs {
    fn path(&self, name: &str) -> String {
        self.dir.join(name).to_string_lossy().into_owned()
    }

    fn generate() -> Self {
        let dir = PathBuf::from(TEST_ROOT).join("certs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("could not create certificate directory");
        let certs = Certs { dir };

        std::fs::write(
            certs.path("server.ext"),
            "basicConstraints=critical,CA:FALSE\n\
             keyUsage=critical,digitalSignature,keyEncipherment\n\
             extendedKeyUsage=serverAuth\n\
             subjectAltName=IP:127.0.0.1,DNS:localhost\n",
        )
        .unwrap();
        std::fs::write(
            certs.path("client.ext"),
            "basicConstraints=critical,CA:FALSE\n\
             keyUsage=critical,digitalSignature\n\
             extendedKeyUsage=clientAuth\n\
             subjectAltName=DNS:updated-test-client\n",
        )
        .unwrap();

        certs.create_ca("ca");
        certs.create_ca("rogue_ca");
        certs.sign_leaf("ca", "server", "server.ext", "/CN=localhost");
        certs.sign_leaf("rogue_ca", "rogue", "server.ext", "/CN=localhost");
        certs.sign_leaf("ca", "client", "client.ext", "/CN=updated-test-client");

        // reqwest expects the key and the certificate in a single PEM file
        let key = std::fs::read_to_string(certs.path("client.key")).unwrap();
        let cert = std::fs::read_to_string(certs.path("client.pem")).unwrap();
        std::fs::write(certs.path("client_identity.pem"), format!("{key}{cert}")).unwrap();

        certs
    }

    fn create_ca(&self, name: &str) {
        openssl(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "3650",
            "-subj",
            &format!("/CN=updated-test-{name}"),
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-keyout",
            &self.path(&format!("{name}.key")),
            "-out",
            &self.path(&format!("{name}.pem")),
        ]);
    }

    fn sign_leaf(&self, ca: &str, name: &str, extensions: &str, subject: &str) {
        openssl(&[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            subject,
            "-keyout",
            &self.path(&format!("{name}.key")),
            "-out",
            &self.path(&format!("{name}.csr")),
        ]);
        openssl(&[
            "x509",
            "-req",
            "-days",
            "3650",
            "-in",
            &self.path(&format!("{name}.csr")),
            "-CA",
            &self.path(&format!("{ca}.pem")),
            "-CAkey",
            &self.path(&format!("{ca}.key")),
            "-CAcreateserial",
            "-extfile",
            &self.path(extensions),
            "-out",
            &self.path(&format!("{name}.pem")),
        ]);
    }
}

fn openssl(args: &[&str]) {
    let output = std::process::Command::new("openssl")
        .args(args)
        .output()
        .expect("openssl is required to run these tests");
    assert!(
        output.status.success(),
        "openssl {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn certs() -> &'static Certs {
    static CERTS: OnceLock<Certs> = OnceLock::new();
    CERTS.get_or_init(Certs::generate)
}

// ---------------------------------------------------------------------------
// test server
// ---------------------------------------------------------------------------

/// Which server certificate `openssl s_server` presents to the client.
enum ServerCert {
    /// Signed by the CA the client trusts.
    Trusted,
    /// Signed by a CA unknown to the client.
    Untrusted,
}

/// How the test server responds to the single request it serves.
#[derive(Clone)]
enum Behavior {
    /// Serve `body`, answering `Range` requests with `206 Partial Content`.
    Serve(Vec<u8>),
    /// Announce `announced_len` bytes, send `prefix` and then go silent without
    /// closing the connection.
    Stall {
        announced_len: usize,
        prefix: Vec<u8>,
    },
}

/// A single-connection HTTPS server backed by `openssl s_server`.
struct TestServer {
    port: u16,
    _child: Child,
    task: JoinHandle<()>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    async fn start(server_cert: ServerCert, behavior: Behavior) -> Self {
        let certs = certs();
        let (cert, key) = match server_cert {
            ServerCert::Trusted => ("server.pem", "server.key"),
            ServerCert::Untrusted => ("rogue.pem", "rogue.key"),
        };
        let port = free_port();

        let mut child = Command::new("openssl")
            .args([
                "s_server",
                "-accept",
                &format!("127.0.0.1:{port}"),
                // terminate after the single connection the test needs
                "-naccept",
                "1",
                // do not mix server chatter into the application data on stdout
                "-quiet",
                // close the TLS connection once the response has been written
                "-no_ign_eof",
                "-cert",
                &certs.path(cert),
                "-key",
                &certs.path(key),
                // require a client certificate signed by the test CA
                "-CAfile",
                &certs.path("ca.pem"),
                "-Verify",
                "1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("openssl is required to run these tests");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn(serve(stdout, stdin, behavior, requests.clone()));

        wait_until_listening(port).await;

        Self {
            port,
            _child: child,
            task,
            requests,
        }
    }

    fn url(&self, file: &str) -> String {
        format!("https://127.0.0.1:{}/{file}", self.port)
    }

    /// The requests received so far, as raw HTTP request headers.
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Drives the HTTP exchange of a single connection over the stdio pipes of
/// `openssl s_server`.
async fn serve(
    mut stdout: ChildStdout,
    mut stdin: ChildStdin,
    behavior: Behavior,
    requests: Arc<Mutex<Vec<String>>>,
) {
    let Some(request) = read_request(&mut stdout).await else {
        return;
    };
    requests.lock().unwrap().push(request.clone());

    let response = match &behavior {
        Behavior::Serve(body) => build_response(&request, body),
        Behavior::Stall {
            announced_len,
            prefix,
        } => {
            let mut response = headers("200 OK", *announced_len, None);
            response.extend_from_slice(prefix);
            response
        }
    };

    if stdin.write_all(&response).await.is_err() {
        return;
    }
    let _ = stdin.flush().await;

    if let Behavior::Stall { .. } = behavior {
        // keep the connection open, but never send the announced remainder
        std::future::pending::<()>().await;
    }

    // closing stdin makes s_server shut the TLS connection down
    drop(stdin);
}

/// Reads HTTP request headers from the server process until the terminating
/// empty line is received.
async fn read_request(stdout: &mut ChildStdout) -> Option<String> {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];

    loop {
        let read = stdout.read(&mut buf).await.ok()?;
        if read == 0 {
            return None;
        }
        request.extend_from_slice(&buf[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Some(String::from_utf8_lossy(&request).into_owned());
        }
    }
}

fn headers(status: &str, content_length: usize, content_range: Option<String>) -> Vec<u8> {
    let mut headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n"
    );
    if let Some(content_range) = content_range {
        headers.push_str(&format!("Content-Range: {content_range}\r\n"));
    }
    headers.push_str(&format!("Content-Length: {content_length}\r\n\r\n"));
    headers.into_bytes()
}

fn build_response(request: &str, body: &[u8]) -> Vec<u8> {
    match range_start(request) {
        Some(start) if start >= body.len() => headers(
            "416 Range Not Satisfiable",
            0,
            Some(format!("bytes */{}", body.len())),
        ),
        Some(start) => {
            let part = &body[start..];
            let mut response = headers(
                "206 Partial Content",
                part.len(),
                Some(format!("bytes {}-{}/{}", start, body.len() - 1, body.len())),
            );
            response.extend_from_slice(part);
            response
        }
        None => {
            let mut response = headers("200 OK", body.len(), None);
            response.extend_from_slice(body);
            response
        }
    }
}

/// Parses the first byte offset of a `Range: bytes=<start>-` request header.
fn range_start(request: &str) -> Option<usize> {
    request
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("range:"))
        .and_then(|line| line.split('=').nth(1))
        .and_then(|range| range.trim().trim_end_matches('-').parse().ok())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("no free port available")
        .local_addr()
        .unwrap()
        .port()
}

/// Waits until the port cannot be bound anymore, i.e. until `s_server` listens
/// on it. Probing with an actual connection is not possible because that would
/// consume the single connection the server accepts.
async fn wait_until_listening(port: u16) {
    for _ in 0..500 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("openssl s_server did not start listening on port {port}");
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn client(timeout: Duration) -> DownloadClient {
    DownloadClient::new(
        Some(&certs().path("ca.pem")),
        Some(&certs().path("client_identity.pem")),
        Some(timeout),
    )
    .await
    .expect("could not create download client")
}

/// Creates an empty download directory for a test.
fn test_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(TEST_ROOT).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Deterministic payload without any CR/LF bytes: `s_server` interprets single
/// letter lines on its stdin as interactive commands.
fn test_body(len: usize, seed: usize) -> Vec<u8> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..len)
        .map(|i| ALPHABET[(i * 7 + seed) % ALPHABET.len()])
        .collect()
}

/// Computes the expected hash with `sha256sum`, so that the hash helper of the
/// crate is not verified against itself.
fn sha256_hex(data: &[u8]) -> String {
    let mut child = std::process::Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("sha256sum is required to run these tests");
    child.stdin.take().unwrap().write_all(data).unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "sha256sum failed");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("unexpected sha256sum output")
        .to_string()
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn downloads_file_over_mtls() {
    let body = test_body(2048, 0);
    let server = TestServer::start(ServerCert::Trusted, Behavior::Serve(body.clone())).await;
    let dir = test_dir("mtls");
    let target = dir.join("update.bin");

    client(Duration::from_secs(10))
        .await
        .download(
            &server.url("update.bin"),
            &target,
            None,
            HashAlgorithm::Sha256,
            &sha256_hex(&body),
        )
        .await
        .expect("download failed");

    assert_eq!(tokio::fs::read(&target).await.unwrap(), body);
    assert!(
        !dir.join("update.bin.part").exists(),
        "the part file should have been renamed"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /update.bin HTTP/1.1"));
    assert!(!requests[0].to_ascii_lowercase().contains("range:"));
}

#[tokio::test]
async fn fails_on_untrusted_server_certificate() {
    let body = test_body(2048, 1);
    let server = TestServer::start(ServerCert::Untrusted, Behavior::Serve(body.clone())).await;
    let dir = test_dir("untrusted_cert");
    let target = dir.join("update.bin");

    let error = client(Duration::from_secs(10))
        .await
        .download(
            &server.url("update.bin"),
            &target,
            None,
            HashAlgorithm::Sha256,
            &sha256_hex(&body),
        )
        .await
        .expect_err("download should fail on a certificate mismatch");

    assert!(
        matches!(
            error,
            UpdateError::UpdateSourceError(UpdateSourceError::ConnectionError(_))
        ),
        "unexpected error: {error}"
    );
    assert!(!target.exists());
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn resumes_incomplete_download() {
    let body = test_body(2048, 2);
    let already_downloaded = 800;
    let server = TestServer::start(ServerCert::Trusted, Behavior::Serve(body.clone())).await;
    let dir = test_dir("resume");
    let target = dir.join("update.bin");

    tokio::fs::write(dir.join("update.bin.part"), &body[..already_downloaded])
        .await
        .unwrap();

    client(Duration::from_secs(10))
        .await
        .download(
            &server.url("update.bin"),
            &target,
            None,
            HashAlgorithm::Sha256,
            &sha256_hex(&body),
        )
        .await
        .expect("resumed download failed");

    assert_eq!(tokio::fs::read(&target).await.unwrap(), body);
    assert!(!dir.join("update.bin.part").exists());

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].contains(&format!("range: bytes={already_downloaded}-"))
            || requests[0].contains(&format!("Range: bytes={already_downloaded}-")),
        "missing range header in request: {}",
        requests[0]
    );
}

#[tokio::test]
async fn skips_already_complete_download() {
    let body = test_body(2048, 3);
    // the server would answer with a different payload if it was contacted
    let server = TestServer::start(ServerCert::Trusted, Behavior::Serve(test_body(2048, 4))).await;
    let dir = test_dir("complete");
    let target = dir.join("update.bin");

    tokio::fs::write(&target, &body).await.unwrap();

    client(Duration::from_secs(10))
        .await
        .download(
            &server.url("update.bin"),
            &target,
            Some(body.len()),
            HashAlgorithm::Sha256,
            &sha256_hex(&body),
        )
        .await
        .expect("download of an already complete file failed");

    assert_eq!(tokio::fs::read(&target).await.unwrap(), body);
    assert!(
        server.requests().is_empty(),
        "a complete file must not be downloaded again"
    );
}

#[tokio::test]
async fn discards_download_with_hash_mismatch() {
    let body = test_body(2048, 5);
    let server = TestServer::start(ServerCert::Trusted, Behavior::Serve(body.clone())).await;
    let dir = test_dir("hash_mismatch");
    let target = dir.join("update.bin");

    let error = client(Duration::from_secs(10))
        .await
        .download(
            &server.url("update.bin"),
            &target,
            None,
            HashAlgorithm::Sha256,
            &sha256_hex(b"some other content"),
        )
        .await
        .expect_err("download should fail on a hash mismatch");

    assert!(
        matches!(
            error,
            UpdateError::UpdateSourceError(UpdateSourceError::FetchError(_))
        ),
        "unexpected error: {error}"
    );
    assert!(!target.exists(), "the target file must not be created");
    assert!(
        !dir.join("update.bin.part").exists(),
        "the corrupted part file must be discarded"
    );
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn times_out_on_stalled_connection() {
    let body = test_body(2048, 6);
    let timeout = Duration::from_secs(2);
    let server = TestServer::start(
        ServerCert::Trusted,
        Behavior::Stall {
            announced_len: body.len(),
            prefix: body[..16].to_vec(),
        },
    )
    .await;
    let dir = test_dir("stalled");
    let target = dir.join("update.bin");

    let download_client = client(timeout).await;
    let url = server.url("update.bin");
    let hash = sha256_hex(&body);
    let error = tokio::select! {
        result = download_client.download(
            &url,
            &target,
            None,
            HashAlgorithm::Sha256,
            &hash,
        ) => result.expect_err("download should time out on a stalled connection"),
        _ = tokio::time::sleep(timeout + Duration::from_secs(1)) => {
            panic!("download did not time out within {:?}", timeout + Duration::from_secs(1))
        }
    };

    assert!(
        matches!(
            error,
            UpdateError::UpdateSourceError(UpdateSourceError::ConnectionError(_))
        ),
        "unexpected error: {error}"
    );
    assert!(!target.exists());
}
