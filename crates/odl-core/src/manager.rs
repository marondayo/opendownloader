//! The download queue: runs up to N jobs concurrently and reports events.

use crate::model::{ConvertOptions, DownloadOptions, ItemId, Progress};
use crate::site::Site;
use crate::{JobError, RunCtx};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadJob {
    pub url: String,
    pub site: Site,
    pub options: DownloadOptions,
    pub output_dir: PathBuf,
    pub file_prefix: Option<String>,
    /// Unique, stable per download item; names its private working folder.
    pub work_key: String,
}

impl DownloadJob {
    /// Private working folder (inside the output folder, so the final move is a rename).
    ///
    /// yt-dlp treats any existing file with the target name as "already downloaded", and
    /// audio extraction then deletes it as an intermediate file. Downloading in an isolated
    /// folder means existing files are never reused, converted or deleted.
    pub fn work_dir(&self) -> PathBuf {
        self.output_dir.join(format!(".odl-partial-{}", self.work_key))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobSpec {
    Download(DownloadJob),
    Convert(ConvertOptions),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: ItemId,
    pub spec: JobSpec,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobOutcome {
    pub file: Option<PathBuf>,
    pub size: Option<u64>,
    pub ext: Option<String>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    Started(ItemId),
    Progress(ItemId, Progress),
    /// A (possibly partial) file the job is writing; used to clean up cancelled downloads.
    TempFile(ItemId, PathBuf),
    Finished(ItemId, JobOutcome),
    Failed(ItemId, String),
    /// Cancelled by the user (item is paused and can be resumed).
    Cancelled(ItemId),
}

pub type EventSink = Arc<dyn Fn(EngineEvent) + Send + Sync>;

enum Cmd {
    Enqueue(Job),
    Cancel(ItemId),
    CancelAll,
    SetContext(Arc<RunCtx>),
}

/// Handle to the queue task. Cheap to clone.
#[derive(Clone)]
pub struct Manager {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl Manager {
    pub fn spawn(rt: &tokio::runtime::Handle, ctx: Arc<RunCtx>, sink: EventSink) -> Manager {
        let (tx, rx) = mpsc::unbounded_channel();
        rt.spawn(run(rx, ctx, sink));
        Manager { tx }
    }

    pub fn enqueue(&self, job: Job) {
        let _ = self.tx.send(Cmd::Enqueue(job));
    }

    pub fn cancel(&self, id: ItemId) {
        let _ = self.tx.send(Cmd::Cancel(id));
    }

    pub fn cancel_all(&self) {
        let _ = self.tx.send(Cmd::CancelAll);
    }

    /// Applies new settings/tools to jobs started from now on (and the concurrency limit).
    pub fn set_context(&self, ctx: Arc<RunCtx>) {
        let _ = self.tx.send(Cmd::SetContext(ctx));
    }
}

async fn run(mut rx: mpsc::UnboundedReceiver<Cmd>, mut ctx: Arc<RunCtx>, sink: EventSink) {
    let mut pending: VecDeque<Job> = VecDeque::new();
    let mut running: HashMap<ItemId, CancellationToken> = HashMap::new();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<ItemId>();

    loop {
        while running.len() < ctx.settings.max_concurrent.max(1) {
            let Some(job) = pending.pop_front() else { break };
            let token = CancellationToken::new();
            running.insert(job.id, token.clone());
            tokio::spawn(run_job(job, ctx.clone(), token, sink.clone(), done_tx.clone()));
        }

        tokio::select! {
            cmd = rx.recv() => match cmd {
                None => {
                    for t in running.values() { t.cancel(); }
                    break;
                }
                Some(Cmd::Enqueue(job)) => {
                    if !running.contains_key(&job.id) && !pending.iter().any(|j| j.id == job.id) {
                        pending.push_back(job);
                    }
                }
                Some(Cmd::Cancel(id)) => {
                    if let Some(pos) = pending.iter().position(|j| j.id == id) {
                        pending.remove(pos);
                        sink(EngineEvent::Cancelled(id));
                    } else if let Some(t) = running.get(&id) {
                        t.cancel();
                    }
                }
                Some(Cmd::CancelAll) => {
                    for job in pending.drain(..) {
                        sink(EngineEvent::Cancelled(job.id));
                    }
                    for t in running.values() { t.cancel(); }
                }
                Some(Cmd::SetContext(c)) => ctx = c,
            },
            Some(id) = done_rx.recv() => {
                running.remove(&id);
            }
        }
    }
}

async fn run_job(
    job: Job,
    ctx: Arc<RunCtx>,
    token: CancellationToken,
    sink: EventSink,
    done: mpsc::UnboundedSender<ItemId>,
) {
    let id = job.id;
    sink(EngineEvent::Started(id));
    let progress_sink = sink.clone();
    let on_progress = move |p: Progress| progress_sink(EngineEvent::Progress(id, p));
    let result = match &job.spec {
        JobSpec::Download(d) => {
            let file_sink = sink.clone();
            let on_file = move |p: PathBuf| file_sink(EngineEvent::TempFile(id, p));
            crate::ytdlp::download(&ctx, d, token, &on_progress, &on_file).await
        }
        JobSpec::Convert(c) => crate::convert::convert(&ctx, c, token, &on_progress).await,
    };
    match result {
        Ok(outcome) => sink(EngineEvent::Finished(id, outcome)),
        Err(JobError::Cancelled) => sink(EngineEvent::Cancelled(id)),
        Err(JobError::Failed(msg)) => sink(EngineEvent::Failed(id, msg)),
    }
    let _ = done.send(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn failing_jobs_report_and_respect_queue() {
        // No yt-dlp configured: every download fails fast, which exercises the queue plumbing.
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let sink: EventSink = Arc::new(move |e| ev.lock().unwrap().push(e));
        let ctx = Arc::new(RunCtx::default());
        let m = Manager::spawn(&tokio::runtime::Handle::current(), ctx, sink);
        let tmp = std::env::temp_dir().join("odl-mgr-test");
        for id in 1..=4 {
            m.enqueue(Job {
                id,
                spec: JobSpec::Download(DownloadJob {
                    url: "https://www.youtube.com/watch?v=x".into(),
                    site: Site::YouTube,
                    options: DownloadOptions::default(),
                    output_dir: tmp.clone(),
                    file_prefix: None,
                    work_key: format!("test-{id}"),
                }),
            });
        }
        for _ in 0..100 {
            if events.lock().unwrap().iter().filter(|e| matches!(e, EngineEvent::Failed(..))).count() == 4 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let evs = events.lock().unwrap();
        assert_eq!(evs.iter().filter(|e| matches!(e, EngineEvent::Started(_))).count(), 4);
        assert_eq!(evs.iter().filter(|e| matches!(e, EngineEvent::Failed(..))).count(), 4);
        let _ = std::fs::remove_dir_all(tmp);
    }
}
