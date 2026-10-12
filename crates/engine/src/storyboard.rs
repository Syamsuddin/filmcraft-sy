//! Storyboards: build a whole titled, narrated video from a list of scenes in one command, so an
//! agent (or a person with a script) gets a finished sequence without placing every clip.
//!
//! | command | does |
//! |---|---|
//! | `storyboard.build` | a new sequence with one scene after another: a full-frame background, an optional title and body text, optional narration, and transitions between scenes (one undo step) |
//!
//! **A scene** is `{seconds, background, title, text, color, narration, voice}`; every key is
//! optional, unknown keys are refused. The background is a full-frame rectangle on V1, the title is
//! bold paragraph text on V2 and the body text is paragraph text on V3, both centred and wrapped
//! to 84 % of the frame width. Text is white on dark backgrounds and near-black on light ones unless
//! `color` says otherwise. Narration is synthesized with `tts.create` onto A1 at the scene's start.
//!
//! **A scene is never shorter than its narration**: it lasts `seconds` (default 4) or the speech
//! plus half a second, whichever is longer, rounded up to whole frames, so scenes butt exactly and
//! one scene's voice never runs into the next.
//!
//! **Transitions** (`transition`, default Cross Dissolve; `"none"` for cuts) go on each track at
//! each scene change: centred on the cut where both scenes have a clip on that track, a fade in
//! where only the new scene has one, a fade out where only the old one has one. They are at most
//! `transitionFrames` long (default 15) and at most half of either scene.
//!
//! Everything is checked before the project changes, and a failure part-way (a voice that cannot
//! speak, a full disk) rolls the whole build back. Narration WAVs already written stay on disk,
//! as with `tts.create`.

use serde_json::{Map, Value, json};

use filmcraft_project::ClipId;
use filmcraft_time::{FrameRate, Tick};

use crate::commands::{CommandSpec, always, bad, str_p};
use crate::{EngineError, Result, Session};

const CMD: &str = "storyboard.build";
/// More scenes than this is not a storyboard any more; it also bounds the work one call can ask for.
const MAX_SCENES: usize = 200;
/// Longest scene in seconds (ten minutes), and shortest.
const MAX_SCENE_SECONDS: f64 = 600.0;
const MIN_SCENE_SECONDS: f64 = 0.5;
const DEFAULT_SCENE_SECONDS: f64 = 4.0;
/// Characters of on-screen text per field; a screenful is far less.
const MAX_TEXT_CHARS: usize = 2_000;
/// Silence after a scene's narration before the next scene starts.
const SPEECH_TAIL_SECONDS: f64 = 0.5;
const DEFAULT_TRANSITION_FRAMES: i64 = 15;
const MAX_TRANSITION_FRAMES: i64 = 240;
const DEFAULT_BACKGROUND: &str = "#101820";
const SCENE_KEYS: [&str; 7] = ["seconds", "background", "title", "text", "color", "narration", "voice"];

/// One scene, checked.
struct Scene {
    seconds: f64,
    background: String,
    title: Option<String>,
    text: Option<String>,
    color: Option<String>,
    narration: Option<String>,
    voice: Option<String>,
}

/// The clips one scene was built from.
#[derive(Default, Clone, Copy)]
struct Built {
    background: Option<ClipId>,
    title: Option<ClipId>,
    text: Option<ClipId>,
    narration: Option<ClipId>,
    frames: i64,
}

pub fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Build Storyboard",
        menu: &[],
        shortcut: None,
        params: r##"{"scenes":[{"seconds":f64=4,"background":"#rrggbb"="#101820","title":str?,"text":str?,"color":"#rrggbb"?,"narration":str?,"voice":str?}] (1-200),"name":str?,"width":u32=1920,"height":u32=1080,"fps":f64=30,"voice":str?,"dir":str? (narration WAVs),"transition":"cross_dissolve|Cross Dissolve|…|none"="Cross Dissolve","transitionFrames":0..240=15}"##,
        enabled: always,
        run: build,
        journal: true,
    }]
}

fn color_p(v: &Value, key: &str, scene: usize) -> Result<Option<String>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(c)) if filmcraft_color::parse_hex(c).is_some() => Ok(Some(c.clone())),
        Some(_) => Err(bad(CMD, format!("scene {}: `{key}` must be a colour like \"#1a2b3c\"", scene + 1))),
    }
}

fn text_p(v: &Value, key: &str, scene: usize) -> Result<Option<String>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) if t.trim().is_empty() => Ok(None),
        Some(Value::String(t)) if t.chars().count() > MAX_TEXT_CHARS => {
            Err(bad(CMD, format!("scene {}: `{key}` is longer than {MAX_TEXT_CHARS} characters", scene + 1)))
        }
        Some(Value::String(t)) => Ok(Some(t.clone())),
        Some(_) => Err(bad(CMD, format!("scene {}: `{key}` must be a string", scene + 1))),
    }
}

fn parse_scene(v: &Value, i: usize) -> Result<Scene> {
    let obj = v.as_object().ok_or_else(|| bad(CMD, format!("scene {} must be an object", i + 1)))?;
    if let Some(k) = obj.keys().find(|k| !SCENE_KEYS.contains(&k.as_str())) {
        return Err(bad(CMD, format!("scene {}: unknown key `{k}`; expected one of {}", i + 1, SCENE_KEYS.join(", "))));
    }
    let seconds = match v.get("seconds") {
        None | Some(Value::Null) => DEFAULT_SCENE_SECONDS,
        Some(s) => s
            .as_f64()
            .filter(|s| s.is_finite() && (MIN_SCENE_SECONDS..=MAX_SCENE_SECONDS).contains(s))
            .ok_or_else(|| bad(CMD, format!("scene {}: `seconds` must be a number from {MIN_SCENE_SECONDS} to {MAX_SCENE_SECONDS}", i + 1)))?,
    };
    let narration = text_p(v, "narration", i)?;
    let voice = match v.get("voice") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(bad(CMD, format!("scene {}: `voice` must be a string", i + 1))),
    };
    Ok(Scene {
        seconds,
        background: color_p(v, "background", i)?.unwrap_or_else(|| DEFAULT_BACKGROUND.to_string()),
        title: text_p(v, "title", i)?,
        text: text_p(v, "text", i)?,
        color: color_p(v, "color", i)?,
        narration,
        voice,
    })
}

/// White on dark backgrounds, near-black on light ones (relative luminance of the sRGB colour).
pub(crate) fn contrast_text(background: &str) -> &'static str {
    let [r, g, b, _] = filmcraft_color::parse_hex(background).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let y = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    if y > 0.4 { "#141414" } else { "#ffffff" }
}

/// Estimated height in px of `text` set at `size` px in a box `width` px wide, wrapping whole
/// words greedily: an average glyph is a little over half the size wide, a line 1.25 sizes tall.
/// Only for centring the text block; the box itself is as tall as the rest of the frame, so a
/// low estimate never cuts a line off.
pub(crate) fn estimated_height(text: &str, size: f64, width: f64) -> f64 {
    let per_line = (width / (size * 0.56)).max(1.0) as usize;
    let mut lines = 0usize;
    for para in text.split('\n') {
        let mut used = 0usize;
        lines = lines.saturating_add(1);
        for word in para.split_whitespace() {
            let n = word.chars().count();
            if used > 0 && used.saturating_add(1).saturating_add(n) > per_line {
                lines = lines.saturating_add(1);
                used = 0;
            }
            // a word longer than a line breaks across lines
            if n > per_line {
                lines = lines.saturating_add(n / per_line);
                used = n % per_line;
            } else {
                used = if used == 0 { n } else { used.saturating_add(1).saturating_add(n) };
            }
        }
    }
    lines as f64 * size * 1.25
}

/// Whole frames needed to show `d`, rounded up, at least one.
fn frames_for(rate: FrameRate, d: Tick) -> i64 {
    let f = rate.frame_at(d);
    let f = if rate.tick_of(f) < d { f.saturating_add(1) } else { f };
    f.max(1)
}

/// Add a paragraph text clip; returns its id.
fn add_text(
    s: &mut Session,
    text: &str,
    at: Tick,
    seconds: f64,
    track: usize,
    (x, y, w, h): (f64, f64, f64, f64),
    size: f64,
    color: &str,
    bold: bool,
) -> Result<ClipId> {
    let r = s.execute(
        "graphics.newText",
        json!({"text": text, "position": [x, y], "box": [w, h], "size": size, "seconds": seconds, "track": track, "time": at.0}),
    )?;
    let clip = r.get("clip").and_then(Value::as_u64).map(ClipId).ok_or_else(|| EngineError::Other("graphics.newText returned no clip".into()))?;
    let mut props = Map::new();
    props.insert("align".into(), json!("center"));
    props.insert("fill_color".into(), json!(color));
    if bold {
        props.insert("font_style".into(), json!("Bold"));
    }
    s.execute("graphics.set", json!({"clip": clip.0, "props": props}))?;
    Ok(clip)
}

fn build(s: &mut Session, p: &Value) -> Result<Value> {
    // check everything before changing anything
    let list = p.get("scenes").and_then(Value::as_array).ok_or_else(|| bad(CMD, "need `scenes`: a list of scene objects"))?;
    if list.is_empty() || list.len() > MAX_SCENES {
        return Err(bad(CMD, format!("`scenes` needs 1 to {MAX_SCENES} scenes")));
    }
    let scenes = list.iter().enumerate().map(|(i, v)| parse_scene(v, i)).collect::<Result<Vec<_>>>()?;
    let effect = match str_p(p, "transition") {
        Some(t) if t.eq_ignore_ascii_case("none") => None,
        Some(t) => Some(t.to_string()),
        None => Some("Cross Dissolve".to_string()),
    };
    let max_tf = match p.get("transitionFrames") {
        None | Some(Value::Null) => DEFAULT_TRANSITION_FRAMES,
        Some(v) => v
            .as_i64()
            .filter(|f| (0..=MAX_TRANSITION_FRAMES).contains(f))
            .ok_or_else(|| bad(CMD, format!("`transitionFrames` must be a whole number from 0 to {MAX_TRANSITION_FRAMES}")))?,
    };
    let dir = str_p(p, "dir").map(str::to_string);
    let default_voice = str_p(p, "voice").map(str::to_string);
    let mut seq_params = json!({"name": str_p(p, "name").unwrap_or("Storyboard"), "fps": 30.0, "video": 3, "audio": 2});
    for key in ["width", "height", "fps"] {
        if let (Some(v), Some(o)) = (p.get(key), seq_params.as_object_mut()) {
            o.insert(key.to_string(), v.clone());
        }
    }

    s.edit_group("Build Storyboard", |s| {
        let seq = s.execute("file.newSequence", seq_params)?.get("sequence").and_then(Value::as_u64).unwrap_or_default();
        let (w, h, rate) = {
            let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
            (f64::from(q.settings.width), f64::from(q.settings.height), q.settings.frame_rate.sane())
        };
        let (margin, box_w) = (w * 0.08, w * 0.84);
        let (title_size, text_size) = ((h / 12.0).round().max(8.0), (h / 18.0).round().max(8.0));
        let mut built: Vec<Built> = Vec::with_capacity(scenes.len());
        let mut frame: i64 = 0;
        for sc in &scenes {
            let start = rate.tick_of(frame);
            let mut b = Built::default();
            let mut length = Tick::from_seconds_f64(sc.seconds);
            if let Some(words) = &sc.narration {
                let mut q = json!({"text": words, "time": start.0, "track": "A1"});
                if let Some(o) = q.as_object_mut() {
                    if let Some(v) = sc.voice.as_ref().or(default_voice.as_ref()) {
                        o.insert("voice".into(), json!(v));
                    }
                    if let Some(d) = &dir {
                        o.insert("dir".into(), json!(d));
                    }
                }
                let r = s.execute("tts.create", q)?;
                b.narration = r.get("clip").and_then(Value::as_u64).map(ClipId);
                let speech = Tick(r.get("duration").and_then(Value::as_i64).unwrap_or_default());
                length = length.max(speech + Tick::from_seconds_f64(SPEECH_TAIL_SECONDS));
            }
            b.frames = frames_for(rate, length);
            let end_frame = frame.checked_add(b.frames).ok_or_else(|| bad(CMD, "the storyboard is too long"))?;
            let seconds = (rate.tick_of(end_frame) - start).seconds();

            let r =
                s.execute("graphics.newRectangle", json!({"position": [w / 2.0, h / 2.0], "size": [w, h], "seconds": seconds, "track": 0, "time": start.0}))?;
            let bg = r.get("clip").and_then(Value::as_u64).map(ClipId).ok_or_else(|| EngineError::Other("graphics.newRectangle returned no clip".into()))?;
            s.execute("graphics.set", json!({"clip": bg.0, "props": {"fill_color": sc.background}}))?;
            b.background = Some(bg);

            let color = sc.color.clone().unwrap_or_else(|| contrast_text(&sc.background).to_string());
            let title_h = sc.title.as_deref().map_or(0.0, |t| estimated_height(t, title_size, box_w));
            let text_h = sc.text.as_deref().map_or(0.0, |t| estimated_height(t, text_size, box_w));
            let gap = if title_h > 0.0 && text_h > 0.0 { text_size } else { 0.0 };
            // centre the block of title + text vertically, but never above the top margin
            let mut y = ((h - (title_h + gap + text_h)) / 2.0).max(h * 0.06);
            if let Some(t) = &sc.title {
                let bh = (h - y - h * 0.04).max(title_size);
                b.title = Some(add_text(s, t, start, seconds, 1, (margin, y, box_w, bh), title_size, &color, true)?);
                y += title_h + gap;
            }
            if let Some(t) = &sc.text {
                let bh = (h - y - h * 0.04).max(text_size);
                b.text = Some(add_text(s, t, start, seconds, 2, (margin, y, box_w, bh), text_size, &color, false)?);
            }
            built.push(b);
            frame = end_frame;
        }

        let mut transitions = 0usize;
        if let Some(effect) = effect.as_deref().filter(|_| max_tf >= 2) {
            for (prev, cur) in built.iter().zip(built.iter().skip(1)) {
                let tf = max_tf.min(prev.frames / 2).min(cur.frames / 2);
                if tf < 2 {
                    continue;
                }
                let pairs = [(prev.background, cur.background), (prev.title, cur.title), (prev.text, cur.text)];
                for (old, new) in pairs {
                    let (clip, edge) = match (old, new) {
                        (_, Some(n)) => (n, "in"),
                        (Some(o), None) => (o, "out"),
                        (None, None) => continue,
                    };
                    s.execute("sequence.applyVideoTransition", json!({"clip": clip.0, "effect": effect, "edge": edge, "frames": tf}))?;
                    transitions += 1;
                }
            }
        }

        let id = |c: Option<ClipId>| c.map_or(Value::Null, |c| json!(c.0));
        let mut at: i64 = 0;
        let out: Vec<Value> = built
            .iter()
            .map(|b| {
                let start = rate.tick_of(at);
                at = at.saturating_add(b.frames);
                json!({
                    "start": start.0,
                    "duration": (rate.tick_of(at) - start).0,
                    "frames": b.frames,
                    "background": id(b.background),
                    "title": id(b.title),
                    "text": id(b.text),
                    "narration": id(b.narration),
                })
            })
            .collect();
        let total = rate.tick_of(frame);
        Ok(json!({"sequence": seq, "duration": total.0, "seconds": total.seconds(), "frames": frame, "transitions": transitions, "scenes": out}))
    })
}
