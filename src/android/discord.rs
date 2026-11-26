use crate::android::with_activity;
use jni::objects::{JObject, JValue};

#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub art: Option<String>,
    /// Unix ms when the song (virtually) started, so Discord draws elapsed time.
    pub start_ms: i64,
    pub end_ms: Option<i64>,
}


pub struct Discord;

impl Discord {
    pub fn spawn() -> Option<Self> {
        available().then_some(Self)
    }

    pub fn set(&self, presence: Option<Presence>) {
        if let Err(error) = with_activity(|env, activity| {
            let text = |value: &str| value.trim().chars().take(128).collect::<String>();
            let (title, artist, album, art, url, start, end) = match presence {
                Some(p) => {
                    let url = if p.video_id.len() == 11
                        && p.video_id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                    {
                        format!("https://music.youtube.com/watch?v={}", p.video_id)
                    } else {
                        String::new()
                    };
                    let title = text(&p.title);
                    (
                        env.new_string(if title.chars().count() >= 2 { &title } else { "ytm-rs" })?.into(),
                        env.new_string(text(&p.artist))?.into(),
                        env.new_string(text(p.album.as_deref().unwrap_or_default()))?.into(),
                        env.new_string(p.art.filter(|url| url.starts_with("https://")).unwrap_or_default())?.into(),
                        env.new_string(url)?.into(),
                        p.start_ms,
                        p.end_ms.unwrap_or_default(),
                    )
                }
                None => (JObject::null(), JObject::null(), JObject::null(), JObject::null(), JObject::null(), 0, 0),
            };
            env.call_method(
                activity,
                "setDiscordPresence",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JJ)V",
                &[
                    JValue::Object(&title), JValue::Object(&artist), JValue::Object(&album),
                    JValue::Object(&art), JValue::Object(&url), JValue::Long(start), JValue::Long(end),
                ],
            )?;
            Ok(())
        }) {
            log::debug!("Android Discord presence: {error:#}");
        }
    }
}

pub fn available() -> bool {
    with_activity(|env, activity| {
        env.call_method(activity, "hasDiscordPresence", "()Z", &[])?.z()
    }).unwrap_or_default()
}

