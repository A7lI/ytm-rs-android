use anyhow::{Context, Result, ensure};
use crate::{lyric_alignment, lyrics::Lyrics, stream};
use std::{ffi::c_void, sync::Arc};

unsafe extern "C" {
    fn lyrics_engine_new(model: *const u8, len: usize) -> *mut c_void;
    fn lyrics_engine_free(engine: *mut c_void);
    fn lyrics_logits(engine: *mut c_void, pcm: *mut f32, len: usize, scores: *mut f32, capacity: usize) -> i32;
}

struct Engine(*mut c_void);
impl Drop for Engine {
    fn drop(&mut self) { unsafe { lyrics_engine_free(self.0); } }
}

// Run short overlapping windows to bound attention memory. Only keep the central
// predictions of each window so every frame has one absolute playback timestamp.
pub fn estimate(shared: Arc<stream::Shared>, lyrics: &Lyrics, cancelled: impl Fn() -> bool) -> Result<Lyrics> {
    const STRIDE: usize = 320;
    const FRAMES: usize = 300;
    const CONTEXT: usize = 32;
    const VOCAB: usize = lyric_alignment::LABELS;
    let pcm = crate::transcription::audio(shared, &cancelled)?;
    let frames = (pcm.len()-400)/STRIDE+1;
    let app = super::ALIGN_APP.lock().unwrap().clone().context("activity closed")?;
    let mut model = app.asset_manager().open(c"alignment/english.onnx").context("missing timing model")?;
    let bytes = model.buffer()?;
    let engine = Engine(unsafe { lyrics_engine_new(bytes.as_ptr(), bytes.len()) });
    ensure!(!engine.0.is_null(), "couldn't open timing model");
    let mut all = Vec::with_capacity(frames);
    for core in (0..frames).step_by(FRAMES) {
        ensure!(!cancelled(), "song changed");
        let start = core.saturating_sub(CONTEXT);
        let end = (core+FRAMES+CONTEXT).min(frames);
        let mut audio = pcm[start*STRIDE..(end-1)*STRIDE+400].to_vec();
        let mean = audio.iter().sum::<f32>()/audio.len() as f32;
        let scale = (audio.iter().map(|x| (x-mean)*(x-mean)).sum::<f32>()/audio.len() as f32+1e-7).sqrt();
        for x in &mut audio { *x = (*x-mean)/scale; }
        let mut logits = vec![[0.0f32; VOCAB]; end-start];
        let count = unsafe { lyrics_logits(engine.0, audio.as_mut_ptr(), audio.len(), logits.as_mut_ptr().cast(), logits.len()*VOCAB) };
        ensure!(count == logits.len() as i32, "unexpected timing-model output: {count}");
        for mut row in logits[core-start..(core+FRAMES).min(frames)-start].iter().copied() {
            let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let norm = max + row.iter().map(|x| (x-max).exp()).sum::<f32>().ln();
            for x in &mut row { *x -= norm; }
            all.push(row);
        }
    }
    drop(engine);
    lyric_alignment::align(lyrics, &all, &cancelled)
}

