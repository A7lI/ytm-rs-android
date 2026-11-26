use std::sync::{Arc, Mutex};
use std::time::Duration;

use jni::JNIEnv;
use jni::objects::{JClass, JValue};
use jni::sys::{jint, jlong};
use souvlaki::{MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};

type OnMedia = Arc<dyn Fn(MediaControlEvent) + Send + Sync>;
use crate::audio::{AudioCmd, AudioHandle, focus::Change};

static FOCUS_AUDIO: Mutex<Option<AudioHandle>> = Mutex::new(None);

pub fn set_focus_audio(audio: AudioHandle) {
    *FOCUS_AUDIO.lock().unwrap() = Some(audio);
}

pub fn cancel_automatic_resume() {
    let _ = super::with_activity(|env, activity| {
        env.call_method(activity, "cancelAutomaticResume", "()V", &[])?;
        Ok(())
    });
}

static CALLBACK: Mutex<Option<OnMedia>> = Mutex::new(None);

pub struct MediaControls;

impl MediaControls {
    pub fn new(_: PlatformConfig<'_>) -> anyhow::Result<Self> { Ok(Self) }

    pub fn attach(&mut self, listener: impl Fn(MediaControlEvent) + Send + Sync + 'static) -> anyhow::Result<()> {
        *CALLBACK.lock().unwrap() = Some(Arc::new(listener));
        Ok(())
    }

    pub fn set_metadata(&mut self, metadata: MediaMetadata<'_>) -> anyhow::Result<()> {
        super::with_activity(|env, activity| {
            let title = env.new_string(metadata.title.unwrap_or_default())?;
            let artist = env.new_string(metadata.artist.unwrap_or_default())?;
            let album = env.new_string(metadata.album.unwrap_or_default())?;
            let cover = env.new_string(metadata.cover_url.unwrap_or_default())?;
            let duration = metadata.duration.map_or(-1, |duration| duration.as_millis() as i64);
            env.call_method(activity, "mediaMetadata",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;J)V",
                &[JValue::Object(&title), JValue::Object(&artist), JValue::Object(&album),
                  JValue::Object(&cover), JValue::Long(duration)])?;
            Ok(())
        })
    }

    pub fn set_playback(&mut self, playback: MediaPlayback) -> anyhow::Result<()> {
        let (playing, progress) = match playback {
            MediaPlayback::Playing { progress } => (true, progress),
            MediaPlayback::Paused { progress } => (false, progress),
            MediaPlayback::Stopped => (false, None),
        };
        super::with_activity(|env, activity| {
            let position = progress.map_or(0, |position| position.0.as_millis() as i64);
            env.call_method(activity, "mediaPlayback", "(ZJ)V", &[JValue::Bool(u8::from(playing)), JValue::Long(position)])?;
            Ok(())
        })
    }
}

impl Drop for MediaControls {
    fn drop(&mut self) {
        *CALLBACK.lock().unwrap() = None;
        *FOCUS_AUDIO.lock().unwrap() = None;
        let _ = super::with_activity(|env, activity| env.call_method(activity, "closeMedia", "()V", &[]).map(|_| ()));
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlaybackService_mediaAction(
    _: JNIEnv<'_>, _: JClass<'_>, action: jint, position: jlong,
) {
    let event = match action {
        1 => MediaControlEvent::Play,
        2 => MediaControlEvent::Pause,
        3 => MediaControlEvent::Next,
        4 => MediaControlEvent::Previous,
        5 => MediaControlEvent::SetPosition(MediaPosition(Duration::from_millis(position.max(0) as u64))),
        6 => MediaControlEvent::Stop,
        _ => return,
    };
    let callback = CALLBACK.lock().unwrap().clone();
    if let Some(callback) = callback { callback(event); }
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlaybackService_audioFocus(
    _: JNIEnv<'_>, _: JClass<'_>, change: jint,
) {
    let change = match change {
        1 => Change::Gain,
        -1 => Change::PermanentLoss,
        -2 | -3 => Change::TemporaryLoss,
        _ => return,
    };
    if let Some(audio) = FOCUS_AUDIO.lock().unwrap().as_ref() {
        audio.send(AudioCmd::AudioFocus(change));
    }
}
