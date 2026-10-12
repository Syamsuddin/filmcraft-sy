---
name: prompt-to-video
description: Make a finished video file (MP4 etc.) from a user's prompt with FilmCraft — explainers, narrated slideshows, title/promo cards, tips lists, simple motion graphics, or an edit of the user's own media. Use when the user asks to "make/generate/create a video", "buat video", a short clip, an intro, a slideshow with narration, or a video from a script. FilmCraft composes from shapes, text, narration (TTS), transitions and imported media; it does not synthesize photoreal footage.
---

# Prompt to video with FilmCraft

You are the editor. Turn the prompt into a storyboard, build it with FilmCraft commands, look at
frames, fix, then export. Answer the user in their language.

## Be honest about what this makes

FilmCraft is an editor, not a text-to-video model. It can make: solid/gradient backgrounds, shapes,
styled text (point or wrapped paragraph), narration from text, captions, transitions, effects and
keyframes, and anything built from the user's own images, video and audio. It cannot invent
photoreal scenes ("a man walking on a beach"). If the prompt needs that, say so up front and offer
a text/graphics version or ask for footage.

Narration voices (`tts.voices`): `basic-female`, `basic-male` (built in, English, robotic) and the
Kokoro natural voices, which need `tts.downloadVoices` to fetch ~336 MB once (ask the user first;
it needs a FilmCraft built with natural voices, `neural-voices`). English: `kokoro-heart`,
`kokoro-bella`, `kokoro-michael`, …. **Indonesian:** `kokoro-id-heart`, `kokoro-id-bella`,
`kokoro-id-michael`. Write their narration in normal Indonesian: numbers, `Rp 15.000`, `75%` and
`ke-2` are read in Indonesian. They speak with an American accent (Kokoro has no Indonesian voice).
Fix a word that comes out wrong by writing é where it is said (`meréka`) or spelling it as it
sounds. Without the download, offer on-screen text only, or the user's own voice-over
(`media_import`). For other languages, say narration is not available.

## Step 0: make sure the tool is there

Two routes, same engine and same commands:

- **MCP** (preferred when connected): tools `command_run`, `command_batch`, `render_frame`,
  `sequence_inspect`, `media_import`. Server `filmcraft-headless` in `.mcp.json`.
- **CLI** (always works): `target/release/filmcraft-cli`.

If `target/release/filmcraft-cli` is missing, build it: `cargo build --release -p filmcraft-cli`
(about 5 minutes), or with the natural voices (needed for Indonesian narration; about 11 minutes)
`cargo build --release -p filmcraft-cli --features neural-voices`. Then download the voices once
with `tts.downloadVoices` (`wait: true`) and check `jobs.list`: the download needs huggingface.co
and raw.githubusercontent.com, and a sandbox that blocks them fails with a proxy error. Tell the
user the host was blocked rather than falling back silently. The MCP servers only connect after the
binary exists (reconnect with `/mcp` or restart the session); until then use the CLI.

Before using a command you have not used in this session, check its params:
`filmcraft-cli describe <id> --compact` (or MCP `command_list` with `filter`). Params change; trust
`describe` over this file.

## Step 1: storyboard first, then confirm

Write the plan as a short table before building anything: for each scene its time range, background,
on-screen text, narration line, and transition in. Defaults unless the user says otherwise:
1920×1080 (or 1080×1920 for "reels/shorts/TikTok", 1080×1080 for square), 30 fps, 3–6 s per scene,
text ≥ 60 px, high contrast, at most ~12 words on screen per scene. For a short or obvious request,
show the storyboard and go on; ask only when something essential is unclear (length, format,
language of narration).

## Step 2: build

### The quick way: `storyboard.build`

One command turns the storyboard into a sequence: per scene a full-frame background (V1), a bold
title (V2) and body text (V3), centred and wrapped, text colour picked for contrast, narration on
A1, and transitions between scenes. A scene is never shorter than its narration; the whole build
is one undo step and is rolled back if any part fails.

```json
{"id":"storyboard.build","params":{
  "name":"Air Putih","width":1080,"height":1920,"fps":30,
  "voice":"kokoro-id-heart","dir":"<output folder>/narration",
  "transition":"Cross Dissolve","transitionFrames":15,
  "scenes":[
    {"seconds":4,"background":"#0a3d62","title":"3 Manfaat Minum Air Putih","narration":"Tiga manfaat minum air putih."},
    {"seconds":5,"background":"#38ada9","title":"1. Menjaga konsentrasi","text":"Otak terdiri dari sekitar 75 persen air.","narration":"Pertama, air menjaga konsentrasi. Otak kita 75% air."}
  ]}}
```

Scene keys: `seconds` (0.5–600, default 4), `background`, `title`, `text`, `color` (text, else
automatic), `narration`, `voice`; unknown keys are refused. `transition: "none"` gives cuts. The
result lists each scene's `start`, `frames` and clip ids (`background`, `title`, `text`,
`narration`). Use them to restyle with `graphics.set` (`font`, `size`, `shadow`, …) or add things
on top. Use the manual way below only for what it cannot do: user media, shapes, keyframed motion,
or adding to an existing sequence.

### The manual way

Time is in ticks: **254 016 000 000 ticks per second** (`time` params). Durations of graphics are
`seconds`. Video track `track` is a 0-based index (0 = V1 background, 1 = V2 text, 2 = V3 extras);
narration `track` is `"A1"`.

Per scene, starting at `t` ticks and lasting `d` seconds:

```jsonl
{"id":"graphics.newRectangle","params":{"position":[W/2,H/2],"size":[W,H],"seconds":d,"track":0,"time":t}}
{"id":"graphics.set","params":{"props":{"fill_color":"#0b2a4a"}}}
{"id":"graphics.newText","params":{"text":"Headline","position":[W/2,H/2+20],"size":90,"seconds":d,"track":1,"time":t}}
{"id":"graphics.set","params":{"props":{"align":"center","fill_color":"#ffffff","font_style":"Bold"}}}
{"id":"tts.create","params":{"text":"Narration line.","voice":"basic-female","time":t,"track":"A1","dir":"<output folder>"}}
```

Start with `{"id":"file.newSequence","params":{"name":"…","width":W,"height":H,"fps":30}}`.

Notes learned the hard way:
- `graphics.set` without `clip` styles the graphic just created (it is selected). Pass `clip` when
  styling anything else.
- Point text: `position` is the alignment point on the first baseline, so with `align: center` use
  `x = W/2` and a `y` a little below the visual centre. Long text: use paragraph text with
  `"box":[w,h]` and `position` = the box's top-left corner (e.g. `box [W-280, 300]`,
  `position [140, H/2-100]`); it wraps.
- Other props (`graphics.set`): `size`, `font`, `font_style`, `tracking`, `leading`, `stroke`,
  `stroke_color`, `stroke_width`, `shadow`, `background`, `background_color`, `corner_radius`,
  `opacity`, `rotation`, `scale`, gradients via `fill_kind` / `gradient_*`. Full list:
  `docs/graphics.md`.
- `tts.create` returns `duration` in ticks. If narration is longer than the scene, lengthen the
  scene (rebuild it) rather than cutting the voice. Always pass `dir` so the WAVs live next to the
  project and not in a temp folder.
- **Transitions need the clip id.** Without `clip`, `sequence.applyVideoTransition` lands on the
  first clip (a fade from black at 0). Build all scenes first, read the clip ids (from the results,
  or `sequence_inspect` / `filmcraft-cli inspect sequence`), then for every scene after the first
  apply `{"clip": <id>, "effect": "Cross Dissolve", "edge": "in", "frames": 15}` on its V1 and V2
  clips. Use `"edge":"in"` on the first clip only if you want a fade from black.
- User media: `media_import` (absolute `paths`), then `timeline.place` with `item`, `track`
  (`"V1"`), `seconds`; `duration` (ticks) sets how long a still stays on screen.

With MCP, send each scene as one `command_batch` (`stop_on_error: true`). With the CLI, write the
steps to a `.jsonl` file and run
`filmcraft-cli run scene.jsonl --compact --save-as <out>/video.fcproj`, then later passes with
`--project <out>/video.fcproj --save`. A worked example of the manual way is in `example.jsonl` next to
this file.

## Step 3: look before you export

Render one frame from the middle of every scene and one from inside each transition, and look at
each image:
- MCP: `render_frame` with `seconds`.
- CLI: `filmcraft-cli --project p.fcproj render --seconds S --out frame.png`, then read the PNG.

Check: text fully inside the frame and readable, no overlaps, contrast, transitions where they
belong. Fix with `graphics.set` / `graphics.setText` (or `edit.undo`) and look again.

## Step 4: export and report

- CLI: `filmcraft-cli --project p.fcproj export <out>/video.mp4` (format from the extension; `--preset`
  from `export --list-presets`; `--scale 0.5` for a quick draft).
- MCP: `command_run` `{"id":"file.exportMedia","params":{"path":"…/video.mp4","wait":true}}`.

An export result with a non-empty `offlineMedia` means some media was missing; fix it, don't ship
placeholder frames. Tell the user the file path, length, resolution, and what you could not do
(e.g. narration language). Keep the `.fcproj` so they can open it in FilmCraft and keep editing.
