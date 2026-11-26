use crate::backend::{Backend, Resp};
use crate::lyrics::{Lyrics, LyricsSet, Provider};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) struct Job {
    id: String,
    generation: u64,
    source: Option<Provider>,
    running: bool,
}

impl Backend {
    /// Called while viewing fallback lyrics. The screen keeps drawing the original
    /// text while a worker waits for the existing playback buffer and aligns it.
    #[cfg(target_os = "android")]
    pub fn auto_sync(&self, id: &str, lyrics: &Lyrics) -> bool {
        crate::lyric_alignment::supported(lyrics) && self.start_lyric_job(id, Some(lyrics))
    }

    pub fn transcribe_lyrics(&self, id: &str) -> bool {
        self.start_lyric_job(id, None)
    }

    pub fn retry_transcript(&self, id: &str) {
        let mut job = self.alignment.lock().unwrap();
        if job.id == id && job.source.is_none() && !job.running {
            *job = Job::default();
        }
    }

    fn start_lyric_job(&self, id: &str, lyrics: Option<&Lyrics>) -> bool {
        let source = lyrics.map(|l| l.provider);
        let generation = self.audio.status.wanted_gen.load(Ordering::Relaxed);
        let mut job = self.alignment.lock().unwrap();
        if job.id == id && job.generation == generation && job.source == source {
            return job.running;
        }
        *job = Job { id: id.into(), generation, source, running: true };
        let (this, id, base) = (self.clone(), id.to_owned(), lyrics.cloned());
        self.rt.spawn(async move {
            let same = || this.audio.status.wanted_gen.load(Ordering::Relaxed) == generation
                && matching(&this.alignment, &id, generation, source);
            let result: anyhow::Result<Lyrics> = async {
                let mut hash = Sha256::new();
                let (provider, sync) = if let Some(base) = &base {
                    // Keep the existing .17 alignment cache keys.
                    hash.update(b"word-alignment-v1");
                    hash.update(id.as_bytes());
                    hash.update(serde_json::to_vec(base)?);
                    (Provider::AutoAligned, crate::lyrics::Sync::Word)
                } else {
                    hash.update(b"whisper-base-transcription-v1");
                    hash.update(id.as_bytes());
                    (Provider::OnDeviceTranscript, crate::lyrics::Sync::Line)
                };
                let dir = this.data_dir.join("word-timing");
                let path = dir.join(format!("{:x}.json", hash.finalize()));
                if let Some(saved) = std::fs::read(&path).ok().and_then(|data| serde_json::from_slice::<Lyrics>(&data).ok())
                    .filter(|l| l.provider == provider && l.sync == sync)
                { return Ok(saved); }
                let stream = tokio::time::timeout(std::time::Duration::from_secs(180), async {
                    loop {
                        anyhow::ensure!(same(), "song changed");
                        let current = this.current.lock().unwrap().clone();
                        if let Some((stream_id, stream)) = current.filter(|(stream_id,_)| stream_id == &id) {
                            anyhow::ensure!(stream.duration_ms <= crate::transcription::MAX_SECONDS * 1000, "recording outside supported length");
                            if stream.is_complete() { return Ok::<_,anyhow::Error>(stream); }
                            anyhow::ensure!(stream.error().is_none(), "audio unavailable for {stream_id}");
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    }
                }).await??;
                static WORKER: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
                let permit = WORKER.acquire().await?;
                anyhow::ensure!(same(), "song changed");
                let state = this.alignment.clone();
                let (status, track) = (this.audio.status.clone(), id.clone());
                let synced = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let cancelled = || status.wanted_gen.load(Ordering::Relaxed) != generation ||
                        !matching(&state, &track, generation, source);
                    #[cfg(target_os = "android")]
                    if let Some(base) = base {
                        return crate::android::lyric_audio::estimate(stream, &base, cancelled);
                    }
                    crate::transcription::transcribe(stream, cancelled)
                }).await??;
                if std::fs::create_dir_all(&dir).is_ok() {
                    let staging = path.with_extension("part");
                    if std::fs::write(&staging, serde_json::to_vec(&synced)?).is_ok() {
                        let _ = std::fs::rename(staging, path);
                    }
                }
                Ok(synced)
            }.await;
            if same() {
                match result {
                    Ok(lyrics) => {
                        let _ = this.tx.send(Resp::Lyrics { video_id: id.clone(), lyrics: LyricsSet { sources: vec![lyrics] } });
                    }
                    Err(e) => { log::info!("on-device lyrics: {e:#}"); }
                }
            }
            let mut job = this.alignment.lock().unwrap();
            if job.id == id && job.generation == generation && job.source == source { job.running = false; }
            drop(job);
            this.wake();
        });
        true
    }
}

fn matching(state: &Arc<Mutex<Job>>, id: &str, generation: u64, provider: Option<Provider>) -> bool {
    let job = state.lock().unwrap();
    job.id == id && job.generation == generation && job.source == provider
}

