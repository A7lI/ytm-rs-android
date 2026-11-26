//! Forced alignment of a known transcript against wav2vec2 CTC output (20 ms frames).
use crate::lyrics::{Lyrics, Part, Provider, Sync};
use anyhow::{Result, ensure};

pub const LABELS: usize = 32;
const LETTER_IDS: [usize; 26] = [7,24,19,14,5,20,21,11,10,29,26,15,17,9,8,23,30,13,12,6,16,25,18,28,22,31];
const FRAME_MS: i32 = 20;

pub fn supported(lyrics: &Lyrics) -> bool {
    if lyrics.sync == Sync::Word { return false; }
    if let Some(language) = lyrics.language.as_deref().filter(|s| !s.is_empty() && *s != "und") {
        return language.split(['-', '_']).next().is_some_and(|s| s.eq_ignore_ascii_case("en"));
    }
    // Untagged plain lyrics are common. Avoid timing other languages with an English model.
    lyrics.lines.iter().flat_map(|l| l.text.split_whitespace())
        .map(|s| s.trim_matches(|c: char| !c.is_alphabetic()).to_ascii_lowercase())
        .filter(|s| matches!(s.as_str(), "i"|"you"|"my"|"the"|"your"|"me"|"with"|"and"|"it"|"love"))
        .take(2).count() >= 2
}

fn letters(word: &str) -> Vec<usize> {
    word.chars().filter_map(|c| match c.to_ascii_uppercase() {
        c @ 'A'..='Z' => Some(LETTER_IDS[c as usize - 'A' as usize]),
        '\'' | '’' => Some(27),
        _ => None,
    }).collect()
}

struct Word {
    line: usize,
    first: usize,
    last: usize,
    text: String,
}

pub fn align(input: &Lyrics, scores: &[[f32; LABELS]], cancelled: &impl Fn() -> bool) -> Result<Lyrics> {
    let mut output = input.clone();
    output.provider = Provider::AutoAligned;
    output.sync = Sync::Word;
    output.lines.retain(|line| {
        let s = line.text.trim();
        !line.instrumental && !s.is_empty() && !(s.starts_with('[') && s.ends_with(']'))
            && s.chars().any(char::is_alphabetic)
    });
    let mut labels = Vec::new();
    let mut windows = Vec::new();
    let mut words = Vec::new();
    for (line_idx, line) in output.lines.iter().enumerate() {
        let bounds = if input.sync == Sync::Line {
            let end = output.lines.get(line_idx+1).map(|l| l.start)
                .filter(|&t| t > line.start).unwrap_or(if line.dur > 0 { line.end() } else { scores.len() as i32 * FRAME_MS });
            ((line.start-1800).max(0)/FRAME_MS, (end+1800)/FRAME_MS)
        } else { (0, i32::MAX) };
        for (i, text) in line.text.split_whitespace().enumerate() {
            let chars = letters(text);
            if chars.is_empty() {
                if let Some(word) = words.last_mut().filter(|w: &&mut Word| w.line == line_idx) {
                    word.text.push(' ');
                    word.text.push_str(text);
                    continue;
                }
                anyhow::bail!("unspoken or unsupported lyric token");
            }
            if !labels.is_empty() {
                labels.push(4);
                windows.push((0, i32::MAX));
            }
            let first = labels.len();
            windows.extend(std::iter::repeat_n(bounds, chars.len()));
            labels.extend(chars);
            words.push(Word { line: line_idx, first, last: labels.len()-1,
                text: if i == 0 { text.to_owned() } else { format!(" {text}") } });
        }
    }
    let states = 2 * labels.len() + 1;
    ensure!(!words.is_empty() && labels.len() < scores.len() && states.saturating_mul(scores.len()) <= 128_000_000,
        "transcript outside supported alignment size");
    let mut trace = vec![0u8; states * scores.len()];
    let mut previous = vec![f32::NEG_INFINITY; states];
    previous[0] = 0.0;
    let mut next = previous.clone();
    for (frame, emission) in scores.iter().enumerate() {
        if frame % 64 == 0 { ensure!(!cancelled(), "song changed"); }
        for (state, value) in next.iter_mut().enumerate() {
            let token = if state % 2 == 0 { 0 } else { labels[state / 2] };
            let mut best = previous[state];
            let mut step = 0;
            if state >= 1 && previous[state - 1] > best { best = previous[state-1]; step = 1; }
            if state >= 3 && state % 2 == 1 && token != labels[state/2-1] && previous[state-2] > best {
                best = previous[state-2]; step = 2;
            }
            if state % 2 == 1 {
                let (lo, hi) = windows[state/2];
                if (frame as i32) < lo || frame as i32 > hi { best = f32::NEG_INFINITY; }
            }
            *value = best + emission[token];
            trace[frame * states + state] = step;
        }
        std::mem::swap(&mut previous, &mut next);
    }
    let mut state = states-1;
    if previous[state-1] > previous[state] { state -= 1; }
    ensure!(previous[state].is_finite(), "no consistent timing path");
    let mut spans = vec![(usize::MAX, 0usize, f32::NEG_INFINITY); labels.len()];
    for frame in (0..scores.len()).rev() {
        if state % 2 == 1 {
            let token = state / 2;
            let span = &mut spans[token];
            span.0 = frame;
            span.1 = span.1.max(frame);
            span.2 = span.2.max(scores[frame][labels[token]]);
        }
        state -= trace[frame * states + state] as usize;
    }
    let mut supported = 0usize;
    let mut total = 0usize;
    for w in &words {
        let token_spans = &spans[w.first..=w.last];
        ensure!(token_spans.iter().all(|s| s.0 != usize::MAX), "incomplete timing path");
        let matches = token_spans.iter().filter(|s| s.2 > -2.0).count();
        supported += matches;
        total += token_spans.len();
        let start = token_spans[0].0 as i32 * FRAME_MS;
        let end = (token_spans.last().unwrap().1 as i32 + 1) * FRAME_MS;
        let line = &mut output.lines[w.line];
        if line.parts.is_empty() { line.start = start; }
        line.dur = end - line.start;
        line.parts.push(Part { start, dur: end-start, bg: false, text: w.text.clone() });
    }
    ensure!(supported * 100 >= total * 45, "weak acoustic match ({supported}/{total})");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::Line;
    #[test]
    fn silence_and_uneven_words_follow_the_recording() {
        let text = Lyrics { provider: Provider::YouTubeMusicPlain, sync: Sync::None, language: Some("en".into()),
            songwriters: vec![], lines: vec![Line { text: "Go on".into(), ..Default::default() }] };
        let mut scores = vec![[-12.0; LABELS]; 180];
        for frame in &mut scores { frame[0] = 0.0; }
        for (frame, token) in [(51,21),(56,8),(58,4),(91,8),(100,9)] {
            scores[frame][0] = -12.0;
            scores[frame][token] = 0.0;
        }
        let aligned = align(&text, &scores, &||false).unwrap();
        let words = &aligned.lines[0].parts;
        assert_eq!((words[0].start, words[1].start), (1020,1820));
        assert_eq!(words.iter().map(|p| p.text.as_str()).collect::<String>(), "Go on");
        let silence = vec![scores[0]; 180];
        assert!(align(&text, &silence, &||false).is_err());
    }
}

