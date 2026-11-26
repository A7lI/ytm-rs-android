use std::path::PathBuf;
use std::time::Duration;

pub fn obtain(_: PathBuf) -> anyhow::Result<String> {
    let answer = crate::android::request("verifyLyrics", None, Duration::from_secs(123))?
        .ok_or_else(|| anyhow::anyhow!("Karaoke verification was cancelled"))?;
    if let Some(error) = answer.strip_prefix("error:") {
        anyhow::bail!("{error}");
    }
    Ok(answer)
}
