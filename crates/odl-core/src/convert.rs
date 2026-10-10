//! Local media conversion with ffmpeg (the "Media Tools" menu).

use crate::manager::JobOutcome;
use crate::model::{AudioBitrate, AudioFormat, ConvertOptions, ConvertTarget, Progress};
use crate::process::{command, kill_tree, LineReader};
use crate::{JobError, RunCtx};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Picks a non-existing output path next to the source (or in `output_dir`).
pub fn output_path(opts: &ConvertOptions) -> PathBuf {
    let dir = opts
        .output_dir
        .clone()
        .or_else(|| opts.source.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = opts.source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
    let ext = opts.target.ext();
    let same_ext = opts.source.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
    let base = if same_ext { format!("{stem} (converted)") } else { stem };
    let mut candidate = dir.join(format!("{base}.{ext}"));
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    candidate
}

/// ffmpeg codec arguments for a conversion target.
pub fn codec_args(target: ConvertTarget, bitrate: AudioBitrate) -> Vec<String> {
    let kbps = |default: u32| match bitrate {
        AudioBitrate::Best => default,
        AudioBitrate::Kbps(k) => k,
    };
    let v: Vec<String> = match target {
        ConvertTarget::Audio(AudioFormat::Mp3) => match bitrate {
            AudioBitrate::Best => vec!["-vn", "-c:a", "libmp3lame", "-q:a", "0"].into_iter().map(String::from).collect(),
            AudioBitrate::Kbps(k) => vec!["-vn".into(), "-c:a".into(), "libmp3lame".into(), "-b:a".into(), format!("{k}k")],
        },
        ConvertTarget::Audio(AudioFormat::M4a) => {
            vec!["-vn".into(), "-c:a".into(), "aac".into(), "-b:a".into(), format!("{}k", kbps(256))]
        }
        ConvertTarget::Audio(AudioFormat::Ogg) => match bitrate {
            AudioBitrate::Best => vec!["-vn", "-c:a", "libvorbis", "-q:a", "8"].into_iter().map(String::from).collect(),
            AudioBitrate::Kbps(k) => vec!["-vn".into(), "-c:a".into(), "libvorbis".into(), "-b:a".into(), format!("{k}k")],
        },
        ConvertTarget::Audio(AudioFormat::Opus) => {
            vec!["-vn".into(), "-c:a".into(), "libopus".into(), "-b:a".into(), format!("{}k", kbps(192))]
        }
        ConvertTarget::Audio(AudioFormat::Flac) => vec!["-vn", "-c:a", "flac"].into_iter().map(String::from).collect(),
        ConvertTarget::Audio(AudioFormat::Wav) => vec!["-vn", "-c:a", "pcm_s16le"].into_iter().map(String::from).collect(),
        ConvertTarget::Mp4 => [
            "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k",
            "-movflags", "+faststart",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        ConvertTarget::Mkv => ["-map", "0", "-c", "copy"].into_iter().map(String::from).collect(),
    };
    v
}

/// Parses `Duration: 00:03:21.23` from ffmpeg's banner.
fn parse_duration(line: &str) -> Option<f64> {
    let rest = line.trim().strip_prefix("Duration:")?.trim();
    let ts = rest.split(',').next()?.trim();
    let mut parts = ts.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

pub async fn convert(
    ctx: &RunCtx,
    opts: &ConvertOptions,
    cancel: CancellationToken,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<JobOutcome, JobError> {
    let ffmpeg = ctx
        .tools
        .ffmpeg
        .as_ref()
        .ok_or_else(|| JobError::Failed("FFmpeg is not installed. Open Settings → Components to install it.".into()))?;
    if !opts.source.exists() {
        return Err(JobError::Failed(format!("{} no longer exists", opts.source.display())));
    }
    let out = output_path(opts);
    if let Some(dir) = out.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let out_dir = out.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut guard = crate::disk::SpaceGuard::new(&out_dir, ctx.settings.keep_free_mib);
    if let Some(g) = guard.as_ref() {
        // The output is rarely larger than the source (WAV from a compressed file can be).
        let source = std::fs::metadata(&opts.source).map(|m| m.len()).unwrap_or(0);
        let estimate = if opts.target == ConvertTarget::Audio(AudioFormat::Wav) { source * 4 } else { source };
        g.check_needed(estimate).map_err(JobError::Failed)?;
    }
    let mut cmd = command(&ffmpeg.path);
    cmd.args(["-hide_banner", "-nostdin", "-n", "-i"])
        .arg(&opts.source)
        .args(["-map_metadata", "0"])
        .args(codec_args(opts.target, opts.bitrate))
        .args(["-progress", "pipe:1", "-nostats"])
        .arg(&out);
    let mut child = cmd.spawn().map_err(|e| JobError::Failed(format!("could not start ffmpeg: {e}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let duration_ms = Arc::new(AtomicU64::new(0));
    let dur = duration_ms.clone();
    let stderr_task = tokio::spawn(async move {
        let mut reader = LineReader::new(stderr);
        let mut tail: Vec<String> = Vec::new();
        while let Ok(Some(line)) = reader.next_line().await {
            if dur.load(Ordering::Relaxed) == 0 {
                if let Some(d) = parse_duration(&line) {
                    dur.store((d * 1000.0) as u64, Ordering::Relaxed);
                }
            }
            tail.push(line);
            if tail.len() > 40 {
                tail.remove(0);
            }
        }
        tail
    });

    let mut reader = LineReader::new(stdout);
    let mut progress = Progress { stage: Some("Converting…".into()), ..Default::default() };
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                kill_tree(&mut child).await;
                let _ = tokio::fs::remove_file(&out).await;
                return Err(JobError::Cancelled);
            }
            line = reader.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Some((key, value)) = line.split_once('=') else { continue };
                match key {
                    "out_time_us" | "out_time_ms" => {
                        let total_ms = duration_ms.load(Ordering::Relaxed);
                        if let (Ok(us), true) = (value.trim().parse::<u64>(), total_ms > 0) {
                            progress.fraction = Some(((us / 1000) as f32 / total_ms as f32).clamp(0.0, 1.0));
                        }
                    }
                    "total_size" => progress.downloaded = value.trim().parse().unwrap_or(progress.downloaded),
                    "speed" => {
                        let v = value.trim();
                        progress.stage = Some(if v.ends_with('x') && v != "N/A" {
                            format!("Converting… {v}")
                        } else {
                            "Converting…".into()
                        });
                    }
                    "progress" => {
                        on_progress(progress.clone());
                        if let Some(msg) = guard.as_mut().and_then(|g| g.poll().err()) {
                            kill_tree(&mut child).await;
                            let _ = tokio::fs::remove_file(&out).await;
                            return Err(JobError::Failed(msg));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let status = child.wait().await?;
    let tail = stderr_task.await.unwrap_or_default();
    if !status.success() {
        let _ = tokio::fs::remove_file(&out).await;
        let msg = tail
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty() && !l.starts_with("  "))
            .cloned()
            .unwrap_or_else(|| format!("ffmpeg exited with {status}"));
        return Err(JobError::Failed(msg));
    }
    let size = tokio::fs::metadata(&out).await.ok().map(|m| m.len());
    Ok(JobOutcome {
        size,
        ext: Some(opts.target.ext().to_string()),
        height: None,
        fps: None,
        note: None,
        file: Some(out),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_line() {
        assert_eq!(parse_duration("  Duration: 00:03:21.50, start: 0.000000, bitrate: 128 kb/s"), Some(201.5));
        assert_eq!(parse_duration("Duration: N/A, bitrate: N/A"), None);
    }

    #[test]
    fn output_names() {
        let dir = std::env::temp_dir().join(format!("odl-conv-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("song.mp3");
        std::fs::write(&src, b"x").unwrap();
        let mut o = ConvertOptions {
            source: src.clone(),
            target: ConvertTarget::Audio(AudioFormat::Mp3),
            bitrate: AudioBitrate::Best,
            output_dir: None,
        };
        assert_eq!(output_path(&o), dir.join("song (converted).mp3"));
        o.target = ConvertTarget::Audio(AudioFormat::Ogg);
        assert_eq!(output_path(&o), dir.join("song.ogg"));
        std::fs::write(dir.join("song.ogg"), b"x").unwrap();
        assert_eq!(output_path(&o), dir.join("song (1).ogg"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn codecs() {
        assert_eq!(codec_args(ConvertTarget::Audio(AudioFormat::Mp3), AudioBitrate::Kbps(320))[4], "320k");
        assert!(codec_args(ConvertTarget::Audio(AudioFormat::Ogg), AudioBitrate::Best).contains(&"libvorbis".to_string()));
    }
}
