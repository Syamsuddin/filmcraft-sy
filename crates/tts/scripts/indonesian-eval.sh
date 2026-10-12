#!/usr/bin/env bash
# Indonesian narration check: download Kokoro + Whisper small (pinned, SHA-256 checked), speak
# crates/tts/tests/data/kalimat-id.txt with the Indonesian voices, transcribe with Whisper (--lang=id)
# and report the word error rate per voice. Needs network access to huggingface.co (and its CDN),
# ffmpeg, and these builds first:
#   cargo build --release -p filmcraft-cli --features neural-voices
#   cargo build --release -p filmcraft-tts --features kokoro --example narrate
#   cargo build --release -p filmcraft-speech --features whisper,download --example transcribe
# Usage: crates/tts/scripts/indonesian-eval.sh [voice ...]   (results in target/id-eval/)
set -euo pipefail
REPO=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
OUT="$REPO/target/id-eval"
VOICES=("${@:-kokoro-id-heart kokoro-id-michael}")
VOICES=(${VOICES[@]})
mkdir -p "$OUT"
cd "$REPO"
# models go to a known place on every OS (FilmCraft reads FILMCRAFT_DATA_DIR)
export FILMCRAFT_DATA_DIR="$OUT/data"
KDIR="$FILMCRAFT_DATA_DIR/models"

code=$(curl -sS -o /dev/null -w "%{http_code}" -m 20 -r 0-10 -L \
  https://huggingface.co/hexgrad/Kokoro-82M/resolve/f3ff3571791e39611d31c381e3a41a3af07b4987/config.json || true)
if [ "$code" != "200" ] && [ "$code" != "206" ]; then
  echo "huggingface.co is still blocked (HTTP $code): allow it in the environment's network settings."; exit 3
fi

# download and its job report in one process (a job lives in the session that started it)
dl=$(printf '%s\n' '{"id":"tts.downloadVoices","params":{"wait":true}}' '{"id":"jobs.list","params":{}}' \
  | target/release/filmcraft-cli run - --compact)
installed=$(target/release/filmcraft-cli exec tts.voices --compact | python3 -c "import json,sys;print(json.load(sys.stdin)['package']['installed'])")
if [ "$installed" != "True" ]; then
  echo "$dl"; echo "Kokoro download failed"; exit 4
fi
echo "Kokoro package: $KDIR/kokoro-82m"

# Whisper small, from the pins in crates/speech/src/models.rs
WDIR="$OUT/whisper-small"; mkdir -p "$WDIR"
python3 -I - "$REPO/crates/speech/src/models.rs" "$WDIR" <<'PY'
import hashlib, re, subprocess, sys, os
src, dest = sys.argv[1], sys.argv[2]
text = open(src).read()
block = text[text.index('id: "whisper-small"'):]
block = block[:block.index("ModelInfo {")] if "ModelInfo {" in block else block
for repo, rev, name, sha, size in re.findall(r'hf!\(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*"([0-9a-f]{64})",\s*([0-9_]+)', block):
    path = os.path.join(dest, name)
    if not (os.path.exists(path) and os.path.getsize(path) == int(size.replace("_", ""))):
        url = f"https://huggingface.co/openai/{repo}/resolve/{rev}/{name}"
        subprocess.run(["curl", "-sSfL", "-o", path, url], check=True)
    h = hashlib.sha256(open(path, "rb").read()).hexdigest()
    if h != sha:
        sys.exit(f"{name}: sha256 mismatch")
    print("ok", name)
PY

for voice in "${VOICES[@]}"; do
  d="$OUT/$voice"; rm -rf "$d"; mkdir -p "$d"
  n=0
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    n=$((n+1)); f=$(printf "%s/s%02d" "$d" "$n")
    printf "%s" "$line" > "$f.script"
    target/release/examples/narrate "$KDIR" "$voice" "$f.script" "$f.wav" 2>/dev/null
    ffmpeg -loglevel error -y -i "$f.wav" -ar 16000 -ac 1 -f f32le "$f.f32"
    printf "%s" "$line" > "$f.txt"
  done < crates/tts/tests/data/kalimat-id.txt
  echo "== $voice"
  target/release/examples/transcribe "$WDIR" --lang=id "$d"/*.f32 2>&1 | tee "$d/transcribe.log" | tail -4
done
