//! Native multipart upload keeps large MP4s as one private object without a recording disk.
use super::*;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

async fn response_xml(mut response: reqwest::Response) -> Res<String> {
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 65536) {
        return Err(Fail::unavailable("Storage upload will retry."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Fail::internal())? {
        if bytes.len() + chunk.len() > 65536 {
            return Err(Fail::unavailable("Invalid object-storage response."));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| Fail::unavailable("Invalid object-storage response."))
}

fn xml_field(body: &str, name: &str) -> Res<String> {
    let start = format!("<{name}>");
    let end = format!("</{name}>");
    let value = body
        .split_once(&start)
        .and_then(|(_, v)| v.split_once(&end))
        .map(|(v, _)| v)
        .ok_or_else(|| Fail::unavailable("Invalid object-storage response."))?;
    if value.is_empty() || value.len() > 2048 || value.contains('<') {
        return Err(Fail::unavailable("Invalid object-storage response."));
    }
    Ok(value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&"))
}
impl Storage {
    pub async fn begin_mp4(&self, http: &reqwest::Client, key: &str) -> Res<Option<String>> {
        if !valid_key(key) {
            return Err(Fail::internal());
        }
        match self {
            Storage::S3(s3) => {
                let response = s3
                    .request_query(
                        http,
                        reqwest::Method::POST,
                        key,
                        b"",
                        &[("uploads", String::new())],
                    )?
                    .header("content-type", "video/mp4")
                    .header("cache-control", "private, no-store")
                    .send()
                    .await
                    .map_err(|_| Fail::unavailable("Storage upload will retry."))?;
                Ok(Some(xml_field(&response_xml(response).await?, "UploadId")?))
            }
            Storage::Filesystem(_) => Ok(None),
            Storage::Disabled => Err(Fail::unavailable("Recording storage is unavailable.")),
        }
    }
    pub async fn abort_mp4(&self, http: &reqwest::Client, key: &str, upload: &str) -> Res<()> {
        if !valid_key(key) {
            return Err(Fail::internal());
        }
        if let Storage::S3(s3) = self {
            let response = s3
                .request_query(
                    http,
                    reqwest::Method::DELETE,
                    key,
                    b"",
                    &[("uploadId", upload.into())],
                )?
                .send()
                .await
                .map_err(|_| Fail::unavailable("Storage cleanup will retry."))?;
            if !response.status().is_success() && response.status() != StatusCode::NOT_FOUND {
                return Err(Fail::unavailable("Storage cleanup will retry."));
            }
        }
        Ok(())
    }
    pub async fn finish_mp4<R: AsyncRead + Unpin>(
        &self,
        http: &reqwest::Client,
        key: &str,
        upload: Option<&str>,
        mut reader: R,
    ) -> Res<u64> {
        if !valid_key(key) {
            return Err(Fail::internal());
        }
        match self {
            Storage::Filesystem(dir) => {
                let path = dir.join(key);
                tokio::fs::create_dir_all(path.parent().ok_or_else(Fail::internal)?)
                    .await
                    .map_err(|_| Fail::internal())?;
                let mut output = tokio::fs::File::create(path)
                    .await
                    .map_err(|_| Fail::internal())?;
                let count = tokio::io::copy(&mut reader, &mut output)
                    .await
                    .map_err(|_| Fail::internal())?;
                output.flush().await.map_err(|_| Fail::internal())?;
                Ok(count)
            }
            Storage::S3(s3) => {
                let upload = upload.ok_or_else(Fail::internal)?;
                let mut parts = Vec::new();
                let mut total = 0;
                loop {
                    let mut bytes = vec![0; 8 * 1024 * 1024];
                    let mut length = 0;
                    while length < bytes.len() {
                        let count = reader
                            .read(&mut bytes[length..])
                            .await
                            .map_err(|_| Fail::internal())?;
                        if count == 0 {
                            break;
                        }
                        length += count;
                    }
                    if length == 0 {
                        break;
                    }
                    bytes.truncate(length);
                    if parts.len() == 10_000 {
                        return Err(Fail::bad("This MP4 exceeds the storage upload limit."));
                    }
                    let response = s3
                        .request_query(
                            http,
                            reqwest::Method::PUT,
                            key,
                            &bytes,
                            &[
                                ("partNumber", (parts.len() + 1).to_string()),
                                ("uploadId", upload.into()),
                            ],
                        )?
                        .body(bytes)
                        .send()
                        .await
                        .map_err(|_| Fail::unavailable("Storage upload will retry."))?;
                    if !response.status().is_success() {
                        return Err(Fail::unavailable("Storage upload will retry."));
                    }
                    let etag = response
                        .headers()
                        .get("etag")
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(Fail::internal)?
                        .trim_matches('"')
                        .to_string();
                    if etag.is_empty() || !etag.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                    {
                        return Err(Fail::unavailable("Invalid storage checksum."));
                    }
                    parts.push(etag);
                    total += length as u64;
                }
                if total == 0 {
                    return Err(Fail::unavailable("Media assembly produced no output."));
                }
                let body = format!(
                    "<CompleteMultipartUpload>{}</CompleteMultipartUpload>",
                    parts
                        .iter()
                        .enumerate()
                        .map(|(i, tag)| format!(
                            "<Part><PartNumber>{}</PartNumber><ETag>\"{tag}\"</ETag></Part>",
                            i + 1
                        ))
                        .collect::<String>()
                )
                .into_bytes();
                let response = s3
                    .request_query(
                        http,
                        reqwest::Method::POST,
                        key,
                        &body,
                        &[("uploadId", upload.into())],
                    )?
                    .header("content-type", "application/xml")
                    // S3 completion can take minutes after all parts have arrived.
                    .timeout(std::time::Duration::from_secs(300))
                    .body(body)
                    .send()
                    .await
                    .map_err(|_| Fail::unavailable("Storage upload will retry."))?;
                let body = response_xml(response).await?;
                xml_field(&body, "ETag")?;
                Ok(total)
            }
            Storage::Disabled => Err(Fail::unavailable("Recording storage is unavailable.")),
        }
    }
    /// Bounded range reads let the HTTP player seek without buffering an entire download.
    pub async fn range(
        &self,
        http: &reqwest::Client,
        key: &str,
        start: u64,
        end: u64,
    ) -> Res<Vec<u8>> {
        if !valid_key(key) || end < start || end - start >= 8 * 1024 * 1024 {
            return Err(Fail::bad("Invalid media range."));
        }
        match self {
            Storage::Filesystem(dir) => {
                let mut file = tokio::fs::File::open(dir.join(key))
                    .await
                    .map_err(|_| Fail::missing())?;
                file.seek(std::io::SeekFrom::Start(start))
                    .await
                    .map_err(|_| Fail::internal())?;
                let mut bytes = vec![0; (end - start + 1) as usize];
                file.read_exact(&mut bytes)
                    .await
                    .map_err(|_| Fail::internal())?;
                Ok(bytes)
            }
            Storage::S3(s3) => {
                let mut response = s3
                    .request(http, reqwest::Method::GET, key, b"")?
                    .header("range", format!("bytes={start}-{end}"))
                    .send()
                    .await
                    .map_err(|_| Fail::unavailable("Playback storage is unavailable."))?;
                if response.status() != StatusCode::PARTIAL_CONTENT {
                    return Err(Fail::unavailable(
                        "Storage did not honor the requested range.",
                    ));
                }
                let expected = format!("bytes {start}-{end}/");
                let length = response
                    .headers()
                    .get("content-range")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix(&expected))
                    .and_then(|value| value.parse::<u64>().ok());
                if length.is_none_or(|length| length <= end) {
                    return Err(Fail::unavailable("Storage returned the wrong media range."));
                }
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|_| Fail::internal())? {
                    if bytes.len() + chunk.len() > (end - start + 1) as usize {
                        return Err(Fail::internal());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if bytes.len() != (end - start + 1) as usize {
                    return Err(Fail::internal());
                }
                Ok(bytes)
            }
            Storage::Disabled => Err(Fail::unavailable("Recording storage is unavailable.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn storage_errors_cannot_pass_as_missing_objects_or_valid_ranges() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let mode = Arc::new(AtomicUsize::new(0));
        let router = axum::Router::new().fallback({
            let mode = mode.clone();
            move |method: axum::http::Method| {
                let mode = mode.load(Ordering::SeqCst);
                async move {
                    let (status, range, body) = match mode {
                        0 => (503, "", "down"),
                        1 => (403, "", "deny"),
                        2 => (404, "", "gone"),
                        3 => (206, "bytes 4-7/8", "data"),
                        4 => (206, "bytes 0-3/8", "bad"),
                        5 => (200, "", "data"),
                        6 => (206, "bytes 0-3/8", "data"),
                        _ if method == axum::http::Method::PUT => (200, "", ""),
                        _ => (200, "", "<Error><Code>InternalError</Code></Error>"),
                    };
                    axum::http::Response::builder()
                        .status(status)
                        .header("content-range", range)
                        .header("etag", "\"abc123\"")
                        .body(axum::body::Body::from(body))
                        .unwrap()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let storage = Storage::S3(S3 {
            endpoint: format!("http://{}", listener.local_addr().unwrap()),
            bucket: "synthetic".into(),
            region: "auto".into(),
            access_key: "synthetic".into(),
            secret_key: "synthetic".into(),
            prefix: String::new(),
        });
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let http = reqwest::Client::new();
        assert!(storage.delete(&http, "test.mp4").await.is_err());
        assert!(storage.begin_mp4(&http, "test.mp4").await.is_err());
        for value in [0, 1] {
            mode.store(value, Ordering::SeqCst);
            assert!(storage.head(&http, "test.mp4").await.is_err());
            assert!(storage.get(&http, "test.mp4").await.is_err());
        }
        mode.store(2, Ordering::SeqCst);
        assert!(storage.head(&http, "test.mp4").await.unwrap().is_none());
        assert!(storage.get(&http, "test.mp4").await.unwrap().is_none());
        storage.delete(&http, "test.mp4").await.unwrap();
        for value in [3, 4, 5] {
            mode.store(value, Ordering::SeqCst);
            assert!(storage.range(&http, "test.mp4", 0, 3).await.is_err());
        }
        mode.store(6, Ordering::SeqCst);
        assert_eq!(
            storage.range(&http, "test.mp4", 0, 3).await.unwrap(),
            b"data"
        );
        // S3 can report a completion failure in XML even with an HTTP 200 response.
        mode.store(7, Ordering::SeqCst);
        assert!(
            storage
                .finish_mp4(&http, "test.mp4", Some("synthetic"), &b"data"[..])
                .await
                .is_err()
        );
        server.abort();
    }
    #[test]
    fn s3_query_and_xml_values_are_encoded_without_interpretation() {
        assert_eq!(aws_encode("a+b/c ="), "a%2Bb%2Fc%20%3D");
        assert_eq!(
            xml_field("<Result><UploadId>a&amp;b</UploadId></Result>", "UploadId").unwrap(),
            "a&b"
        );
        assert!(xml_field("<Error>No access</Error>", "UploadId").is_err());
    }
}
