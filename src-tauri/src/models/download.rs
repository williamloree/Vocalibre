//! Robust model download: streamed to `<file>.part`, resumed with an HTTP
//! Range request after a cut, checked against the manifest's size and
//! SHA-256, then renamed into place. A corrupt file is deleted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::header::RANGE;
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::is_installed;
use super::manifest::RemoteFile;

/// Kept free on the disk on top of the file itself.
const SPACE_MARGIN: u64 = 100 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("network: {0}")]
    Network(String),
    #[error("server answered HTTP {0}")]
    Http(u16),
    #[error("not enough disk space: {needed} bytes needed, {available} available")]
    NoSpace { needed: u64, available: u64 },
    #[error("the downloaded file is corrupt (SHA-256 mismatch)")]
    Checksum,
    #[error("cancelled")]
    Cancelled,
    #[error("disk: {0}")]
    Io(#[from] std::io::Error),
}

impl DownloadError {
    /// Stable code the UI translates; the detail follows after a colon.
    pub fn code(&self) -> String {
        match self {
            Self::Network(e) => format!("network:{e}"),
            Self::Http(status) => format!("http:{status}"),
            Self::NoSpace { needed, .. } => format!("no_space:{}", needed.div_ceil(1024 * 1024)),
            Self::Checksum => "checksum".into(),
            Self::Cancelled => "cancelled".into(),
            Self::Io(e) => format!("io:{e}"),
        }
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        Self::Network(e.to_string())
    }
}

pub fn client() -> Result<reqwest::Client, DownloadError> {
    // Same TLS provider as the updater; installing it twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Vocalibre/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(30))
        .build()?)
}

fn part_path(dir: &Path, file: &RemoteFile) -> PathBuf {
    dir.join(format!("{}.part", file.file_name))
}

/// Downloads `file` into `dir` unless it is already there. `progress`
/// receives the bytes of this file on disk so far. A corrupt result is
/// deleted and downloaded again once.
pub async fn fetch(
    client: &reqwest::Client,
    file: &RemoteFile,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), DownloadError> {
    if is_installed(dir, file) {
        progress(file.size);
        return Ok(());
    }
    match fetch_once(client, file, dir, cancel, progress).await {
        Err(DownloadError::Checksum) => {
            tracing::warn!(file = %file.file_name, "checksum mismatch, downloading again");
            fetch_once(client, file, dir, cancel, progress).await
        }
        other => other,
    }
}

async fn fetch_once(
    client: &reqwest::Client,
    file: &RemoteFile,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), DownloadError> {
    tokio::fs::create_dir_all(dir).await?;
    let part = part_path(dir, file);
    let mut have = match tokio::fs::metadata(&part).await {
        Ok(m) if m.len() <= file.size => m.len(),
        Ok(_) => {
            tokio::fs::remove_file(&part).await?;
            0
        }
        Err(_) => 0,
    };

    let needed = file.size - have;
    let available = fs4::available_space(dir)?;
    if available < needed + SPACE_MARGIN {
        return Err(DownloadError::NoSpace { needed, available });
    }

    if have < file.size {
        let mut request = client.get(&file.url);
        if have > 0 {
            request = request.header(RANGE, format!("bytes={have}-"));
        }
        let mut response = request.send().await?;
        let resumed = match response.status() {
            StatusCode::PARTIAL_CONTENT if have > 0 => true,
            // The server ignored the range: start over.
            StatusCode::OK => false,
            status => return Err(DownloadError::Http(status.as_u16())),
        };
        if !resumed {
            have = 0;
        }
        tracing::info!(file = %file.file_name, from = have, "downloading");
        let mut out = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&part)
            .await?;
        progress(have);
        loop {
            if cancel.load(Ordering::Relaxed) {
                out.flush().await?;
                return Err(DownloadError::Cancelled);
            }
            let Some(chunk) = response.chunk().await? else {
                break;
            };
            have += chunk.len() as u64;
            if have > file.size {
                drop(out);
                tokio::fs::remove_file(&part).await?;
                return Err(DownloadError::Checksum);
            }
            out.write_all(&chunk).await?;
            progress(have);
        }
        out.flush().await?;
        out.sync_all().await?;
        if have < file.size {
            // Connection closed early: the part is kept for a resume.
            return Err(DownloadError::Network(format!(
                "connection closed at {have} of {} bytes",
                file.size
            )));
        }
    }

    let expected = file.sha256.to_ascii_lowercase();
    let path = part.clone();
    let actual = tokio::task::spawn_blocking(move || sha256_of(&path))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    if actual != expected {
        tokio::fs::remove_file(&part).await?;
        return Err(DownloadError::Checksum);
    }
    tokio::fs::rename(&part, dir.join(&file.file_name)).await?;
    tracing::info!(file = %file.file_name, "download complete");
    Ok(())
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// Minimal HTTP/1.1 server for one file. Honours `Range` unless
    /// `ignore_range`; closes the first `cut_first` responses after
    /// `cut_after` bytes of body.
    struct Server {
        url: String,
        requests: Arc<AtomicUsize>,
    }

    fn serve(body: Vec<u8>, ignore_range: bool, cut_first: usize, cut_after: usize) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.bin", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let n = count.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut from = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(r) = lower.strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                if ignore_range {
                    from = 0;
                }
                let rest = &body[from..];
                let head = if from > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{}\r\nConnection: close\r\n\r\n",
                        rest.len(),
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        rest.len()
                    )
                };
                let _ = stream.write_all(head.as_bytes());
                let sent = if n < cut_first {
                    &rest[..cut_after.min(rest.len())]
                } else {
                    rest
                };
                let _ = stream.write_all(sent);
                let _ = stream.flush();
            }
        });
        Server { url, requests }
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn remote(url: &str, body: &[u8]) -> RemoteFile {
        RemoteFile {
            url: url.into(),
            file_name: "model.bin".into(),
            sha256: format!("{:x}", Sha256::digest(body)),
            size: body.len() as u64,
        }
    }

    async fn run(file: &RemoteFile, dir: &Path) -> Result<(), DownloadError> {
        let cancel = AtomicBool::new(false);
        fetch(&client().unwrap(), file, dir, &cancel, &mut |_| {}).await
    }

    #[tokio::test]
    async fn downloads_and_verifies() {
        let body = body();
        let server = serve(body.clone(), false, 0, 0);
        let dir = tempfile::tempdir().unwrap();
        let file = remote(&server.url, &body);
        run(&file, dir.path()).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), body);
        assert!(!part_path(dir.path(), &file).exists());
    }

    #[tokio::test]
    async fn cut_connection_keeps_the_part_then_resumes() {
        let body = body();
        let server = serve(body.clone(), false, 1, 50_000);
        let dir = tempfile::tempdir().unwrap();
        let file = remote(&server.url, &body);
        let err = run(&file, dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::Network(_)), "{err:?}");
        let part = part_path(dir.path(), &file);
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 50_000);

        run(&file, dir.path()).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), body);
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn server_ignoring_range_restarts_from_zero() {
        let body = body();
        let server = serve(body.clone(), true, 0, 0);
        let dir = tempfile::tempdir().unwrap();
        let file = remote(&server.url, &body);
        std::fs::write(part_path(dir.path(), &file), &body[..1000]).unwrap();
        run(&file, dir.path()).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), body);
    }

    #[tokio::test]
    async fn wrong_checksum_is_deleted_after_one_retry() {
        let body = body();
        let server = serve(body.clone(), false, 0, 0);
        let dir = tempfile::tempdir().unwrap();
        let mut file = remote(&server.url, &body);
        file.sha256 = "00".repeat(32);
        let err = run(&file, dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::Checksum), "{err:?}");
        assert!(!part_path(dir.path(), &file).exists());
        assert!(!dir.path().join("model.bin").exists());
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancel_keeps_the_part() {
        let body = body();
        let server = serve(body.clone(), false, 0, 0);
        let dir = tempfile::tempdir().unwrap();
        let file = remote(&server.url, &body);
        let cancel = AtomicBool::new(true);
        let err = fetch(&client().unwrap(), &file, dir.path(), &cancel, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::Cancelled));
        assert!(!dir.path().join("model.bin").exists());
    }

    #[tokio::test]
    async fn http_error_is_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/missing", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let err = run(&remote(&url, b"x"), dir.path()).await.unwrap_err();
        assert!(matches!(err, DownloadError::Http(404)), "{err:?}");
    }

    /// Real download from Hugging Face (redirect to its CDN, TLS, resume):
    /// `cargo test -- --ignored real_vad`.
    #[tokio::test]
    #[ignore = "network"]
    async fn real_vad_download_and_resume() {
        let vad = crate::models::manifest::Manifest::embedded().unwrap().vad;
        let dir = tempfile::tempdir().unwrap();
        let client = client().unwrap();
        let cancel = AtomicBool::new(false);
        let mut seen = 0;
        fetch(&client, &vad, dir.path(), &cancel, &mut |n| seen = n)
            .await
            .unwrap();
        assert_eq!(seen, vad.size);
        let full = std::fs::read(dir.path().join(&vad.file_name)).unwrap();
        // Then as if an earlier attempt had stopped after 100 KB.
        std::fs::remove_file(dir.path().join(&vad.file_name)).unwrap();
        std::fs::write(part_path(dir.path(), &vad), &full[..100_000]).unwrap();
        fetch(&client, &vad, dir.path(), &cancel, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(dir.path().join(&vad.file_name)).unwrap(),
            full
        );
    }

    #[test]
    fn error_codes_are_stable() {
        let e = DownloadError::NoSpace {
            needed: 1024 * 1024 + 1,
            available: 0,
        };
        assert_eq!(e.code(), "no_space:2");
        assert_eq!(DownloadError::Checksum.code(), "checksum");
        assert_eq!(DownloadError::Http(503).code(), "http:503");
    }
}
