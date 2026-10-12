# Prompt to video

Make a finished, narrated video from a prompt in Claude Code (or any MCP client). FilmCraft does
the editing; the agent writes the storyboard. This page describes the pieces and their state, and
is the hand-off note for whoever continues the work (branch `ccr-92db55d6-43b9dq`, PR #1).

FilmCraft composes from shapes, text, narration, transitions and imported media. It does not
synthesize footage (no text-to-video model); the skill says so to the user up front.

## Pieces

| Piece | Where | What it does |
|---|---|---|
| MCP servers | `.mcp.json` (`filmcraft`, `filmcraft-headless`) | already existed; both run `target/release/filmcraft-cli` |
| Skill | `.claude/skills/prompt-to-video/SKILL.md` | the agent's workflow: storyboard → `storyboard.build` → `render_frame` checks → export; limits and voice choice |
| Example | `.claude/skills/prompt-to-video/example.jsonl` | the manual route, clip by clip, as a `filmcraft-cli run` script |
| `storyboard.build` | `crates/engine/src/storyboard.rs`, tests in `storyboard_tests.rs` | one undoable command: scenes → background (V1), title (V2), body (V3), narration (A1), transitions by clip id ([architecture §5.1.2b](architecture.md)) |
| Indonesian front end | `crates/tts-text/src/indonesian.rs` | Indonesian script → Kokoro phonemes: numbers, Rp, %, ke-N, acronyms, EYD letter rules, the letter e, penultimate stress |
| Indonesian voices | `crates/tts/src/catalog.rs`, `neural.rs` | `kokoro-id-heart`, `kokoro-id-bella`, `kokoro-id-michael`: the American Kokoro packs fed by the front end; `LANGUAGES` has `id-ID`; Indonesian sample sentence |
| CLI natural voices | `apps/filmcraft-cli/Cargo.toml` | `--features neural-voices`, as the desktop app; without it the MCP/CLI route has no Kokoro voices at all |
| Evaluation | `crates/tts/scripts/indonesian-eval.sh`, `crates/tts/tests/data/kalimat-id.txt` | downloads Kokoro + Whisper small (pinned, SHA-256 checked), speaks 30 Indonesian sentences, reports Whisper WER with `--lang=id` |

## Use it

```sh
cargo build --release -p filmcraft-cli --features neural-voices   # ~10 min the first time
target/release/filmcraft-cli exec tts.downloadVoices '{"wait":true}'   # 336 MB, once
claude                                                             # approve the FilmCraft MCP servers
```

Then ask for a video, e.g. "buat video vertikal 15 detik tentang 3 manfaat minum air putih,
narasi kokoro-id-heart". Without the natural voices the skill offers on-screen text only or the
user's own voice-over.

Listen to an Indonesian voice directly:

```sh
target/release/filmcraft-cli exec tts.preview '{"voice":"kokoro-id-heart","sample":true,"path":"contoh.wav"}'
```

## State (2026-10-12)

Done, tested and pushed:

- the skill and example (run end to end: four scenes, narration, transitions, MP4);
- `storyboard.build` with 10 tests, including a regression for wrapped text cut off in portrait
  frames (found by looking at rendered frames, not by the tests);
- the Indonesian front end (9 tests: reading, letters, the letter e, stress, overrides, symbol set,
  hostile input), the three voices, the `id-ID` language, the per-language sample sentence and the
  CLI feature.

**Not done: nobody has heard the Indonesian voices yet.** The cloud sandbox the work was done in
blocks huggingface.co (also after the network setting was changed: a new session was blocked
too), so the Kokoro package and Whisper could not be downloaded. On a machine with network access:

```sh
cargo build --release -p filmcraft-cli --features neural-voices
cargo build --release -p filmcraft-tts --features kokoro --example narrate
cargo build --release -p filmcraft-speech --features whisper,download --example transcribe
crates/tts/scripts/indonesian-eval.sh                       # kokoro-id-heart, kokoro-id-michael
crates/tts/scripts/indonesian-eval.sh kokoro-heart          # baseline: English front end
```

The script keeps its downloads in `target/id-eval/data` (through `FILMCRAFT_DATA_DIR`), so it
behaves the same on Linux and macOS; it needs `ffmpeg` and `python3`. Results go to
`target/id-eval/<voice>/transcribe.log`.

## Next steps

1. **Measure and listen.** Run the evaluation; have a native speaker listen to a few sentences.
   Fix what the transcripts show is the front end's fault: the letter e (ə vs é), stress, final
   diphthongs, `ny` (approximated as n j), `r` (Kokoro's American ɹ). Re-measure after each change,
   keep each change under a unit test, and do not tune to the 30 test sentences alone. Record WER
   before and after, with the English-front-end baseline, in `crates/tts/README.md` ("Measured
   quality") and the Indonesian section of `crates/tts-text/README.md`.
2. **User media in storyboards.** An `image` (or `video`) key per scene as the background, through
   the existing import and placement commands.
3. **Text motion.** Fade or slide titles in with keyframes on the graphic layers' opacity and
   position.

## Known issues not caused by this work

These fail on `main` too (checked with the changes stashed), apparently because of the test
environment (ffmpeg/ffprobe oracle and fixture differences):

- `filmcraft-engine` `export_tests::every_builtin_preset_exports_and_ffprobe_confirms` (APV 422-10)
- `filmcraft-engine` `nest_editing_tests::with_the_toggle_off_a_sequence_edits_in_as_its_clips_with_their_transitions`
- `filmcraft-cli` `workflows::phone_vfr_clip_extract_gain_title_to_h264_mp4`

Workspace-wide clippy could not build in the Linux sandbox (`alsa-sys` needs the ALSA development
package); on macOS it should.

A SessionStart hook that builds `filmcraft-cli` at the start of cloud sessions was proposed but not
added: writing `.claude/settings.json` was refused by the sandbox's safety check. Add it by hand if
wanted.
