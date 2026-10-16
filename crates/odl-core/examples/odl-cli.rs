//! Minimal command-line front end for the engine, handy for testing without the GUI.
//!
//! ```text
//! cargo run -p odl-core --example odl-cli -- install            # install yt-dlp/deno/ffmpeg (managed)
//! cargo run -p odl-core --example odl-cli -- pot [install|remove]     # YouTube PO-token provider
//! cargo run -p odl-core --example odl-cli -- update-check [--pre]  # look for a newer OpenDownloader
//! cargo run -p odl-core --example odl-cli -- probe <url>
//! cargo run -p odl-core --example odl-cli -- get <url> [video|mp3|m4a|ogg] [dir]
//! ```

use odl_core::manager::{DownloadJob, EngineEvent, Job, JobSpec, Manager};
use odl_core::tools::{self, Tool};
use odl_core::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut settings = store::load_settings();
    if let Ok(v) = std::env::var("ODL_KEEP_FREE_MIB") {
        settings.keep_free_mib = v.parse()?;
    }
    match args.first().map(String::as_str) {
        Some("install") => {
            let only = args.get(1).map(String::as_str);
            for tool in Tool::ALL {
                if only.is_some_and(|o| o != tool.label().to_lowercase()) {
                    continue;
                }
                if tools::resolve_one(tool, &settings).is_some() && only.is_none() {
                    println!("{}: already available", tool.label());
                    continue;
                }
                println!("installing {}…", tool.label());
                let path = tools::install(tool, |p| eprint!("\r{p:?}          ")).await?;
                println!("\n  -> {}", path.display());
            }
        }
        Some("tools") => {
            for tool in Tool::ALL {
                match tools::resolve_one(tool, &settings) {
                    Some(r) => println!(
                        "{:8} {:?} {} ({})",
                        tool.label(),
                        r.source,
                        r.path.display(),
                        tools::version(tool, &r.path).await.unwrap_or_else(|e| e.to_string())
                    ),
                    None => println!("{:8} missing", tool.label()),
                }
            }
        }
        Some("pot") => match args.get(1).map(String::as_str) {
            Some("install") => {
                let deno = tools::resolve_one(Tool::Deno, &settings)
                    .ok_or_else(|| anyhow::anyhow!("install Deno first"))?;
                let v = pot::install(&deno.path, |p| eprint!("\r{p:?}          ")).await?;
                println!("\nPO-token provider {v} installed");
            }
            Some("remove") => pot::remove()?,
            _ => println!("PO-token provider: {}", pot::installed_version().unwrap_or_else(|| "not installed".into())),
        },
        Some("update-check") => {
            let install = update::Install::detect();
            let pre = args.get(1).is_some_and(|a| a == "--pre");
            println!("OpenDownloader {} ({install:?})", update::CURRENT_VERSION);
            match update::check(&install, pre).await? {
                Some(r) => println!("newer: {} prerelease={} page={} asset={:?}", r.version, r.prerelease, r.page, r.asset),
                None => println!("up to date"),
            }
        }
        Some("probe") => {
            let link = parse_link(args.get(1).map(String::as_str).unwrap_or(""))?;
            println!("{link:?}");
            let ctx = RunCtx { tools: tools::resolve(&settings), settings: settings.clone() };
            match ytdlp::probe(&ctx, &link.url, link.site, false).await? {
                ProbeResult::Single(m) => {
                    println!("{} — {:?} ({:?}s)", m.title, m.uploader, m.duration);
                    println!("heights {:?}, subs {:?}", m.heights, m.subtitle_langs);
                    println!("sizes {:?} audio {:?}", m.video_sizes, m.audio_size);
                    println!("thumb {:?}", m.thumbnail);
                }
                ProbeResult::Playlist(p) => {
                    println!("playlist {} ({} entries)", p.title, p.entries.len());
                    for e in p.entries.iter().take(10) {
                        println!("  {:>3}. {} [{}] {:?}", e.index, e.title, e.url, e.duration);
                    }
                }
            }
        }
        Some("get") => {
            let link = parse_link(args.get(1).map(String::as_str).unwrap_or(""))?;
            let mut options = DownloadOptions::default();
            let mut format = args.get(2).map(String::as_str).unwrap_or("video");
            if let Some(f) = format.strip_suffix("+comments") {
                options.comments = true;
                format = f;
            }
            match format {
                "video" => options.quality = Quality::Height(360),
                fmt => {
                    options.media = MediaKind::Audio;
                    options.audio_format = AudioFormat::ALL
                        .into_iter()
                        .find(|f| f.ext() == fmt)
                        .ok_or_else(|| anyhow::anyhow!("unknown format {fmt}"))?;
                }
            }
            let dir = args.get(3).map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("odl-cli"));
            settings.max_concurrent = 1;
            let ctx = Arc::new(RunCtx { tools: tools::resolve(&settings), settings });
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let m = Manager::spawn(&tokio::runtime::Handle::current(), ctx, Arc::new(move |e| {
                let _ = tx.send(e);
            }));
            m.enqueue(Job {
                id: 1,
                spec: JobSpec::Download(DownloadJob {
                    url: link.video_only_url(),
                    site: link.site,
                    options,
                    output_dir: dir,
                    file_prefix: None,
                    work_key: "cli".into(),
                }),
            });
            while let Some(ev) = rx.recv().await {
                match ev {
                    EngineEvent::Progress(_, p) => eprint!(
                        "\r{:>5.1}% {} / {} {}/s {}            ",
                        p.fraction.unwrap_or(0.0) * 100.0,
                        format_bytes(p.downloaded),
                        p.total.map(format_bytes).unwrap_or_else(|| "?".into()),
                        format_bytes(p.speed.unwrap_or(0.0) as u64),
                        p.stage.unwrap_or_default()
                    ),
                    EngineEvent::Finished(_, o) => {
                        println!("\ndone: {o:?}");
                        break;
                    }
                    EngineEvent::Failed(_, e) => {
                        println!("\nfailed: {e}");
                        std::process::exit(1);
                    }
                    other => eprintln!("{other:?}"),
                }
            }
        }
        _ => eprintln!("usage: odl-cli install [tool] | tools | update-check [--pre] | probe <url> | get <url> [video|mp3|m4a|ogg|opus|flac|wav][+comments] [dir]"),
    }
    Ok(())
}
