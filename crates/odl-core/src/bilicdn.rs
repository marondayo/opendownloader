//! Bilibili CDN mirror selection.
//!
//! Bilibili hands overseas users an Akamai host that often stalls on data its edge hasn't
//! cached (tens of KB/s), while other `upos-*` mirrors serve the same signed URL at several
//! MB/s. Before a Bilibili download we race the mirrors on a slice from the middle of the
//! video, rewrite the stream URLs to the winner and let yt-dlp download from the edited
//! info JSON (`--load-info-json`).

use crate::auth::BROWSER_UA;
use futures_util::StreamExt;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Mirrors known to serve `/upgcxcode/…` paths with the same signed query.
pub const MIRRORS: &[&str] = &[
    "upos-sz-estgcos.bilivideo.com",
    "upos-sz-mirror08c.bilivideo.com",
    "upos-sz-mirrorhw.bilivideo.com",
    "upos-sz-mirrorcosov.bilivideo.com",
    "upos-sz-mirrorcos.bilivideo.com",
    "upos-sz-mirrorali.bilivideo.com",
    "upos-sz-mirroraliov.bilivideo.com",
    "upos-sz-estghw.bilivideo.com",
    "upos-hz-mirrorakam.akamaized.net",
];

/// Setting value meaning "pick the fastest mirror automatically".
pub const AUTO: &str = "auto";

/// Whether a stream URL can be moved to another mirror (signed `/upgcxcode/` paths only;
/// P2P/MCDN URLs use different paths and must be left alone).
fn rewritable(url: &url::Url) -> bool {
    url.path().starts_with("/upgcxcode/")
        && url.host_str().is_some_and(|h| h.ends_with(".bilivideo.com") || h.ends_with(".akamaized.net"))
}

/// Points every rewritable format URL in a yt-dlp info dict at `host`. Returns how many changed.
pub fn rewrite_hosts(info: &mut Value, host: &str) -> usize {
    let mut changed = 0;
    let mut fix = |v: &mut Value| {
        if let Some(s) = v.get("url").and_then(Value::as_str) {
            if let Ok(mut u) = url::Url::parse(s) {
                if rewritable(&u) && u.host_str() != Some(host) && u.set_host(Some(host)).is_ok() {
                    v["url"] = Value::String(u.to_string());
                    changed += 1;
                }
            }
        }
    };
    if let Some(formats) = info.get_mut("formats").and_then(Value::as_array_mut) {
        formats.iter_mut().for_each(&mut fix);
    }
    if let Some(requested) = info.get_mut("requested_formats").and_then(Value::as_array_mut) {
        requested.iter_mut().for_each(&mut fix);
    }
    if info.get("url").is_some() {
        fix(info);
    }
    changed
}

/// The URL of the largest video stream, used as the speed-test sample.
pub fn sample_url(info: &Value) -> Option<url::Url> {
    info.get("formats")?
        .as_array()?
        .iter()
        .filter(|f| f.get("vcodec").and_then(Value::as_str).is_some_and(|c| c != "none"))
        .max_by(|a, b| {
            let k = |f: &Value| f.get("tbr").and_then(Value::as_f64).unwrap_or(0.0);
            k(a).total_cmp(&k(b))
        })
        .and_then(|f| f.get("url")?.as_str())
        .and_then(|s| url::Url::parse(s).ok())
        .filter(rewritable)
}

/// Downloads up to 1 MiB from the middle of the stream on one mirror; returns bytes/second.
async fn measure(client: &reqwest::Client, sample: &url::Url, host: &str, offset: u64) -> Option<f64> {
    let mut u = sample.clone();
    u.set_host(Some(host)).ok()?;
    const SLICE: u64 = 1 << 20;
    let start = Instant::now();
    let resp = client
        .get(u)
        .header("Referer", "https://www.bilibili.com/")
        .header("Range", format!("bytes={offset}-{}", offset + SLICE - 1))
        .send()
        .await
        .ok()?;
    if resp.status().as_u16() != 206 && !resp.status().is_success() {
        return None;
    }
    let mut stream = resp.bytes_stream();
    let mut got = 0u64;
    while let Some(chunk) = stream.next().await {
        got += chunk.ok()?.len() as u64;
    }
    (got > 0).then(|| got as f64 / start.elapsed().as_secs_f64().max(0.001))
}

/// Size of the sample stream (via a one-byte range request), used to pick a mid-file offset.
async fn stream_size(client: &reqwest::Client, sample: &url::Url) -> Option<u64> {
    let resp = client
        .get(sample.clone())
        .header("Referer", "https://www.bilibili.com/")
        .header("Range", "bytes=0-0")
        .send()
        .await
        .ok()?;
    let range = resp.headers().get("content-range")?.to_str().ok()?;
    range.rsplit('/').next()?.parse().ok()
}

/// Races the mirrors and returns the fastest one with its speed (bytes/s).
pub async fn fastest_mirror(sample: &url::Url) -> Option<(String, f64)> {
    let client = reqwest::Client::builder()
        .user_agent(BROWSER_UA)
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    // Uncached data is what's slow, so test from the middle of the file, not the start.
    let offset = match stream_size(&client, sample).await {
        Some(size) if size > 4 << 20 => size / 2,
        _ => 0,
    };
    let races = MIRRORS.iter().map(|host| {
        let client = client.clone();
        async move { measure(&client, sample, host, offset).await.map(|speed| (host.to_string(), speed)) }
    });
    let results: Vec<(String, f64)> = futures_util::future::join_all(races).await.into_iter().flatten().collect();
    for (h, s) in &results {
        tracing::debug!("bilibili mirror {h}: {:.0} KB/s", s / 1024.0);
    }
    results.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rewrites_only_upos_paths() {
        let mut info = json!({
            "formats": [
                {"format_id": "30080", "vcodec": "avc1", "tbr": 3000.0,
                 "url": "https://upos-hz-mirrorakam.akamaized.net/upgcxcode/30/11/1/1-1-100026.m4s?e=x&os=akam&og=hw"},
                {"format_id": "30280", "vcodec": "none",
                 "url": "https://upos-sz-mirrorcos.bilivideo.com/upgcxcode/30/11/1/1-1-30280.m4s?e=y"},
                {"format_id": "mcdn", "vcodec": "avc1", "tbr": 10.0,
                 "url": "https://xy1x2x3x4xy.mcdn.bilivideo.cn:8082/v1/resource/abc.m4s?x=1"}
            ]
        });
        assert_eq!(sample_url(&info).unwrap().host_str(), Some("upos-hz-mirrorakam.akamaized.net"));
        assert_eq!(rewrite_hosts(&mut info, "upos-sz-estgcos.bilivideo.com"), 2);
        let f = info["formats"].as_array().unwrap();
        assert!(f[0]["url"].as_str().unwrap().starts_with("https://upos-sz-estgcos.bilivideo.com/upgcxcode/"));
        assert!(f[0]["url"].as_str().unwrap().ends_with("?e=x&os=akam&og=hw"));
        assert!(f[2]["url"].as_str().unwrap().contains("mcdn.bilivideo.cn"));
    }
}
