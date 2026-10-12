//! Tests of [`crate::storyboard`]: scenes butt frame-exactly, are never shorter than their
//! narration, get transitions on the right edges, and the build is one undo step that is checked
//! up front and rolled back whole when a step fails.

use super::*;
use filmcraft_project::{ClipId, TransitionAlign};
use filmcraft_time::{FrameRate, Tick};
use serde_json::json;

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("filmcraft-storyboard-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d.to_string_lossy().into_owned()
}

fn rate(s: &Session) -> FrameRate {
    s.active_sequence().unwrap().settings.frame_rate
}

/// (video track index, start, end) of a clip in the active sequence.
fn place(s: &Session, id: u64) -> (usize, Tick, Tick) {
    let q = s.active_sequence().unwrap();
    for (i, t) in q.video_tracks.iter().enumerate() {
        if let Some(it) = t.items.iter().find(|it| it.id == ClipId(id)) {
            return (i, it.start, it.end());
        }
    }
    panic!("clip {id} is not on a video track");
}

fn clip_id(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

#[test]
fn scenes_are_built_back_to_back_on_their_tracks_as_one_undo_step() {
    let mut s = Session::default();
    let undo0 = s.history.undo.len();
    let r = s
        .execute(
            "storyboard.build",
            json!({"name": "Tips", "width": 1280, "height": 720, "fps": 30, "scenes": [
                {"seconds": 2, "background": "#0b2a4a", "title": "Three tips"},
                {"seconds": 3, "background": "#f2b705", "title": "One", "text": "Unplug chargers you are not using"},
                {"seconds": 2.5, "text": "Body only"}
            ]}),
        )
        .unwrap();
    let seq = filmcraft_project::ItemId(r["sequence"].as_u64().unwrap());
    assert_eq!(s.project.item(seq).unwrap().name, "Tips");
    assert_eq!(rate(&s), FrameRate::new(30, 1));
    assert_eq!(r["frames"], 60 + 90 + 75);
    let sc = r["scenes"].as_array().unwrap();
    let rt = rate(&s);
    let mut at = 0;
    for (n, (scene, frames)) in sc.iter().zip([60, 90, 75]).enumerate() {
        assert_eq!(scene["frames"], frames, "scene {n}");
        let (track, start, end) = place(&s, clip_id(&scene["background"]));
        assert_eq!((track, start, end), (0, rt.tick_of(at), rt.tick_of(at + frames)), "scene {n} background");
        if !scene["title"].is_null() {
            assert_eq!(place(&s, clip_id(&scene["title"])), (1, start, end), "scene {n} title");
        }
        if !scene["text"].is_null() {
            assert_eq!(place(&s, clip_id(&scene["text"])), (2, start, end), "scene {n} text");
        }
        at += frames;
    }
    assert!(sc[0]["text"].is_null() && sc[2]["title"].is_null());
    assert_eq!(r["duration"], rt.tick_of(225).0);
    // one undo step takes the whole storyboard, sequence included, away
    assert_eq!(s.history.undo.len(), undo0 + 1);
    assert_eq!(s.history.undo.last().unwrap().0, "Build Storyboard");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.project.sequences().count(), 0);
}

#[test]
fn a_scene_is_never_shorter_than_its_narration() {
    let mut s = Session::default();
    let dir = tmp("narration");
    let words = "This narration is a good deal longer than the half second the scene asks for.";
    let r = s
        .execute(
            "storyboard.build",
            json!({"dir": dir, "scenes": [{"seconds": 0.5, "title": "Hi", "narration": words, "voice": "basic-male"}, {"seconds": 1}]}),
        )
        .unwrap();
    let sc = &r["scenes"][0];
    let q = s.active_sequence().unwrap();
    let (_, voice) = q.find_item(ClipId(clip_id(&sc["narration"]))).unwrap();
    assert_eq!(voice.start, Tick::ZERO);
    let (_, start, end) = place(&s, clip_id(&sc["background"]));
    assert_eq!(start, Tick::ZERO);
    // the speech plus its half-second tail fits, rounded up to a frame
    assert!(end >= voice.end() + Tick::from_seconds_f64(0.5), "{end:?} vs {:?}", voice.end());
    assert!(end - (voice.end() + Tick::from_seconds_f64(0.5)) < rate(&s).frame_duration());
    // the next scene starts where this one ends
    assert_eq!(place(&s, clip_id(&r["scenes"][1]["background"])).1, end);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn transitions_cross_the_cut_fade_in_and_fade_out_by_track() {
    let mut s = Session::default();
    let r = s
        .execute(
            "storyboard.build",
            json!({"fps": 25, "transitionFrames": 10, "scenes": [
                {"seconds": 2, "title": "A", "text": "old body"},
                {"seconds": 2, "title": "B"},
                {"seconds": 2, "title": "C", "text": "new body"}
            ]}),
        )
        .unwrap();
    // backgrounds and titles cross at both cuts; the body fades out at the first and in at the second
    assert_eq!(r["transitions"], 6);
    let q = s.active_sequence().unwrap();
    let id = |scene: usize, key: &str| Some(ClipId(clip_id(&r["scenes"][scene][key])));
    let ten = rate(&s).tick_of(10);
    for track in [0, 1] {
        let key = if track == 0 { "background" } else { "title" };
        let tr = &q.video_tracks[track].transitions;
        assert_eq!(tr.len(), 2, "track {track}");
        for (k, t) in tr.iter().enumerate() {
            assert_eq!((t.from, t.to, t.align), (id(k, key), id(k + 1, key), TransitionAlign::CenterAtCut), "track {track} cut {k}");
            assert_eq!(t.duration, ten);
        }
    }
    let body = &q.video_tracks[2].transitions;
    assert_eq!(body.len(), 2);
    assert!(body.iter().any(|t| t.from == id(0, "text") && t.to.is_none()), "fade out: {body:?}");
    assert!(body.iter().any(|t| t.from.is_none() && t.to == id(2, "text")), "fade in: {body:?}");

    // "none" makes cuts
    let r = s.execute("storyboard.build", json!({"transition": "none", "scenes": [{"title": "A"}, {"title": "B"}]})).unwrap();
    assert_eq!(r["transitions"], 0);
    assert!(s.active_sequence().unwrap().video_tracks.iter().all(|t| t.transitions.is_empty()));
}

#[test]
fn transitions_are_at_most_half_a_scene() {
    let mut s = Session::default();
    s.execute("storyboard.build", json!({"fps": 30, "transitionFrames": 240, "scenes": [{"seconds": 0.5}, {"seconds": 4}]})).unwrap();
    let q = s.active_sequence().unwrap();
    let t = q.video_tracks[0].transitions.first().unwrap();
    // the first scene is 15 frames: a 7-frame transition, not 240
    assert_eq!(t.duration, rate(&s).tick_of(7));
}

#[test]
fn text_colour_contrasts_with_the_background() {
    assert_eq!(crate::storyboard::contrast_text("#000000"), "#ffffff");
    assert_eq!(crate::storyboard::contrast_text("#0b2a4a"), "#ffffff");
    assert_eq!(crate::storyboard::contrast_text("#ffffff"), "#141414");
    assert_eq!(crate::storyboard::contrast_text("#f2b705"), "#141414");
}

#[test]
fn bad_storyboards_are_refused_before_anything_changes() {
    let long = "x".repeat(2_001);
    let cases = [
        json!({}),
        json!({"scenes": []}),
        json!({"scenes": "one"}),
        json!({"scenes": vec![json!({}); 201]}),
        json!({"scenes": [1]}),
        json!({"scenes": [{"tittle": "typo"}]}),
        json!({"scenes": [{"seconds": -1}]}),
        json!({"scenes": [{"seconds": 0.1}]}),
        json!({"scenes": [{"seconds": 1e300}]}),
        json!({"scenes": [{"seconds": "4"}]}),
        json!({"scenes": [{"background": "blue"}]}),
        json!({"scenes": [{"color": "#12"}]}),
        json!({"scenes": [{"title": 7}]}),
        json!({"scenes": [{"text": long}]}),
        json!({"scenes": [{"voice": 3}]}),
        json!({"scenes": [{}], "transitionFrames": -1}),
        json!({"scenes": [{}], "transitionFrames": 1_000_000}),
        json!({"scenes": [{}], "transitionFrames": 2.5}),
    ];
    let mut s = Session::default();
    for p in cases {
        let (rev, undo) = (s.revision, s.history.undo.len());
        let e = s.execute("storyboard.build", p.clone());
        assert!(e.is_err(), "{p}");
        assert_eq!((s.revision, s.history.undo.len()), (rev, undo), "{p} changed the project");
        assert_eq!(s.project.sequences().count(), 0, "{p}");
    }
}

#[test]
fn a_failing_step_rolls_the_whole_build_back() {
    let mut s = Session::default();
    let dir = tmp("rollback");
    let undo = s.history.undo.len();
    // the second scene's voice does not exist: the first scene, the sequence and all go
    let e = s.execute(
        "storyboard.build",
        json!({"dir": dir, "scenes": [{"title": "Fine", "narration": "Hello."}, {"title": "Broken", "narration": "Hi.", "voice": "no-such-voice"}]}),
    );
    assert!(e.is_err());
    assert_eq!(s.project.sequences().count(), 0);
    assert_eq!(s.history.undo.len(), undo);
    // an unknown transition fails after every scene is built, and is rolled back too
    assert!(s.execute("storyboard.build", json!({"transition": "No Such Wipe", "scenes": [{}, {}]})).is_err());
    assert_eq!(s.project.sequences().count(), 0);
    assert_eq!(s.history.undo.len(), undo);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_sequence_settings_are_refused() {
    let mut s = Session::default();
    for p in [json!({"width": 0}), json!({"height": -5}), json!({"fps": 0}), json!({"width": 1e12})] {
        let mut p = p;
        p["scenes"] = json!([{}]);
        assert!(s.execute("storyboard.build", p.clone()).is_err(), "{p}");
        assert_eq!(s.project.sequences().count(), 0, "{p}");
    }
}

/// Regression: in a portrait frame the box was only as tall as the estimated text, and words that
/// wrap early pushed the last line out of it ("…mencegah sembelit." lost "sembelit.").
#[test]
fn wrapped_text_is_never_cut_off_by_its_box() {
    let mut s = Session::default();
    let r = s
        .execute(
            "storyboard.build",
            json!({"width": 1080, "height": 1920, "scenes": [
                {"title": "2. Melancarkan pencernaan", "text": "Air membantu tubuh mengolah serat dan mencegah sembelit."},
                {"title": "3. Kulit lebih segar", "text": "Tubuh yang terhidrasi membuat kulit tampak lebih cerah."},
                {"text": "Otak terdiri dari sekitar 75 persen air. Kurang minum sedikit saja membuat fokus menurun."}
            ]}),
        )
        .unwrap();
    for scene in r["scenes"].as_array().unwrap() {
        for key in ["title", "text"] {
            let Some(clip) = scene[key].as_u64() else { continue };
            let list = s.execute("graphics.list", json!({"clip": clip})).unwrap();
            assert_eq!(list["layers"][0]["overflow"], false, "{key} of {scene}: {list}");
        }
    }
}

#[test]
fn the_height_estimate_wraps_whole_words() {
    let est = crate::storyboard::estimated_height;
    // 10 characters a line at size 10 / width 56
    assert_eq!(est("short", 10.0, 56.0), 12.5);
    assert_eq!(est("aaaa bbbb cccc", 10.0, 56.0), 25.0, "the third word goes to a second line");
    assert_eq!(est("one\ntwo", 10.0, 56.0), 25.0);
    assert_eq!(est(&"x".repeat(25), 10.0, 56.0), 37.5, "a long word breaks across lines");
    assert!(est("", 0.0, 0.0).is_finite());
}
