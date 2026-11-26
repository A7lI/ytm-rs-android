use crate::{lyrics::{Line, Lyrics, Provider, Sync}, stream};
use anyhow::{Result, ensure};
use std::{ffi::{CStr, c_char, c_void}, sync::Arc};

const RATE: usize = 16_000;
pub const MAX_SECONDS: u64 = 600;
const MAX_SAMPLES: usize = RATE * MAX_SECONDS as usize;

pub fn audio(shared: Arc<stream::Shared>, cancelled: &impl Fn() -> bool) -> Result<Vec<f32>> {
    ensure!(shared.is_complete(), "audio still buffering");
    let decoder = stream::decoder(shared, true)?;
    let mono = rodio::source::UniformSourceIterator::new(decoder, 1.try_into()?, (RATE as u32).try_into()?);
    let mut pcm = Vec::new();
    for sample in mono.take(MAX_SAMPLES + 1) {
        if pcm.len() % RATE == 0 {
            ensure!(!cancelled(), "song changed");
        }
        pcm.push(sample);
    }
    ensure!((400..=MAX_SAMPLES).contains(&pcm.len()), "recording outside supported length");
    Ok(pcm)
}

unsafe extern "C" {
    fn ytm_transcribe(model: *const u8, model_len: usize, audio: *const f32, audio_len: i32,
        state: *mut c_void, abort: extern "C" fn(*mut c_void) -> bool,
        segment: extern "C" fn(*mut c_void, i32, i32, *const c_char)) -> i32;
}

struct Transcription<'a, F> {
    cancelled: &'a F,
    lines: Vec<Line>,
}

extern "C" fn aborted<F: Fn() -> bool>(ptr: *mut c_void) -> bool {
    let state = unsafe { &*ptr.cast::<Transcription<'_, F>>() };
    (state.cancelled)()
}

extern "C" fn segment<F>(ptr: *mut c_void, start: i32, end: i32, text: *const c_char) {
    let state = unsafe { &mut *ptr.cast::<Transcription<'_, F>>() };
    // Whisper owns the NUL-terminated string until the callback returns.
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    let text = text.trim();
    if start >= 0 && end > start && text.chars().any(char::is_alphabetic) && !repeats_excessively(text) {
        state.lines.push(Line { start, dur: end - start, text: text.into(), ..Default::default() });
    }
}

fn repeats_excessively(text: &str) -> bool {
    let words: Vec<String> = text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|word| !word.is_empty())
        .collect();
    if words.len() < 16 {
        return false;
    }
    let phrases = words.windows(4);
    let total = phrases.len();
    let unique = phrases.collect::<std::collections::HashSet<_>>().len();
    unique * 2 < total
}

pub fn transcribe<F: Fn() -> bool>(shared: Arc<stream::Shared>, cancelled: F) -> Result<Lyrics> {
    let pcm = audio(shared, &cancelled)?;
    #[cfg(target_os = "android")]
    let bytes = {
        use anyhow::Context;
        let app = crate::android::ALIGN_APP.lock().unwrap().clone().context("activity closed")?;
        app.asset_manager().open(c"alignment/ggml-base-q5_1.bin").context("missing transcription model")?
    };
    #[cfg(target_os = "android")]
    let mut model = bytes;
    #[cfg(target_os = "android")]
    let model = model.buffer()?;
    #[cfg(windows)]
    let model = include_bytes!("../target/lyric-alignment/ggml-base-q5_1.bin");

    let mut state = Transcription { cancelled: &cancelled, lines: Vec::new() };
    let result = unsafe {
        ytm_transcribe(model.as_ptr(), model.len(), pcm.as_ptr(), pcm.len() as i32,
            (&mut state as *mut Transcription<'_, F>).cast(), aborted::<F>, segment::<F>)
    };
    ensure!(!cancelled(), "song changed");
    ensure!(result == 0, "transcription failed ({result})");
    ensure!(!state.lines.is_empty(), "no clear words detected");
    Ok(Lyrics { lines: state.lines, provider: Provider::OnDeviceTranscript, sync: Sync::Line,
        language: None, songwriters: Vec::new() })
}

