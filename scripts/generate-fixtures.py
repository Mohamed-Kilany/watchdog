#!/usr/bin/env python3
"""Generate (or verify) the committed test fixtures under tests/fixtures/.

Small fixtures are committed to the repo so every machine can run the test
suite without media tooling. Regenerating requires ffmpeg and Pillow.

Usage:
    python scripts/generate-fixtures.py            # (re)create fixtures
    python scripts/generate-fixtures.py --verify   # check committed fixtures

Determinism: all lavfi sources and Pillow drawing use fixed seeds/phases;
however H.264 payload bytes may differ across ffmpeg versions, so --verify
checks *structure and content* (streams, durations, detectors) rather than
SHA-256. tests/fixtures/SHA256SUMS records the hashes produced with the
reference toolchain (ffmpeg 7.x) for convenience only.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURES = os.path.join(ROOT, "tests", "fixtures")
IMAGE_DIR = os.path.join(FIXTURES, "images")
VIDEO_DIR = os.path.join(FIXTURES, "video")
AUDIO_DIR = os.path.join(FIXTURES, "audio")

BITEXACT = ["-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact", "-map_metadata", "-1"]


def run(cmd):
    print("+", " ".join(cmd))
    subprocess.run(cmd, check=True)


def ffmpeg(*args):
    run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args, *BITEXACT])


def ffprobe_json(path, *entries):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json", "-show_entries", ":".join(entries), path],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def ffmpeg_stderr(*args):
    result = subprocess.run(["ffmpeg", "-hide_banner", *args], capture_output=True, text=True)
    return result.stderr


def gen_images():
    os.makedirs(IMAGE_DIR, exist_ok=True)
    ffmpeg("-f", "lavfi", "-i",
           "nullsrc=s=320x240,format=rgb24,geq=r='X*255/(W-1)':g='Y*255/(H-1)':b=128",
           "-frames:v", "1",
           os.path.join(IMAGE_DIR, "gradient-320x240.png"))
    for name, color in [("solid-black-320x240.png", "black"), ("solid-white-320x240.png", "white"), ("solid-green-320x240.png", "0x00FF00")]:
        ffmpeg("-f", "lavfi", "-i", f"color=c={color}:s=320x240:d=0.04", "-frames:v", "1",
               os.path.join(IMAGE_DIR, name))


def gen_spinner_frames(tmpdir):
    from PIL import Image, ImageDraw

    frames = 48  # 4 s @ 12 fps
    for i in range(frames):
        scale = 4
        w, h = 320 * scale, 240 * scale
        img = Image.new("RGB", (w, h), (18, 18, 24))
        draw = ImageDraw.Draw(img)
        cx, cy = w // 2, h // 2
        r = 44 * scale
        box = [cx - r, cy - r, cx + r, cy + r]
        start = (i * 30) % 360  # 30 degrees per frame at 12 fps
        draw.arc(box, start, start + 110, fill=(120, 220, 255), width=10 * scale)
        img = img.resize((320, 240), Image.LANCZOS)
        img.save(os.path.join(tmpdir, f"spinner_{i:02d}.png"), optimize=True)


def gen_videos():
    os.makedirs(VIDEO_DIR, exist_ok=True)
    common = ["-c:v", "libx264", "-preset", "ultrafast", "-crf", "28", "-pix_fmt", "yuv420p",
              "-c:a", "aac", "-shortest"]

    ffmpeg("-f", "lavfi", "-i", "testsrc2=s=320x240:r=12:d=4",
           "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
           *common, os.path.join(VIDEO_DIR, "clean-4s.mp4"))

    ffmpeg("-f", "lavfi", "-i", "testsrc2=s=320x240:r=12:d=2",
           "-f", "lavfi", "-i", "color=c=black:s=320x240:r=12:d=2",
           "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
           "-filter_complex", "[0:v][1:v]concat=n=2:v=1[v]",
           "-map", "[v]", "-map", "2:a",
           *common, os.path.join(VIDEO_DIR, "black-segment-4s.mp4"))

    ffmpeg("-f", "lavfi", "-i", "testsrc2=s=320x240:r=12:d=2,tpad=stop_mode=clone:stop_duration=2",
           "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
           *common, os.path.join(VIDEO_DIR, "frozen-segment-4s.mp4"))

    with tempfile.TemporaryDirectory() as tmpdir:
        gen_spinner_frames(tmpdir)
        ffmpeg("-framerate", "12", "-i", os.path.join(tmpdir, "spinner_%02d.png"),
               "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
               *common, os.path.join(VIDEO_DIR, "spinner-segment-4s.mp4"))


def gen_audio():
    os.makedirs(AUDIO_DIR, exist_ok=True)
    ffmpeg("-f", "lavfi", "-i", "sine=frequency=440:duration=1",
           "-f", "lavfi", "-i", "anullsrc=r=8000:cl=mono:d=3",
           "-filter_complex", "[0:a][1:a]concat=n=2:v=0:a=1[a]",
           "-map", "[a]", "-ar", "8000",
           os.path.join(AUDIO_DIR, "tone-then-silence-4s.wav"))


def sha256sums():
    import hashlib

    lines = []
    for dirpath, _, filenames in os.walk(FIXTURES):
        for name in sorted(filenames):
            if name == "SHA256SUMS":
                continue
            path = os.path.join(dirpath, name)
            digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
            lines.append(f"{digest}  {os.path.relpath(path, FIXTURES)}")
    with open(os.path.join(FIXTURES, "SHA256SUMS"), "w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(sorted(lines)) + "\n")


def expect(condition, message):
    if not condition:
        print(f"VERIFY FAILED: {message}")
        sys.exit(1)


def verify_video(name, min_seconds, max_seconds, expect_black=False, expect_freeze=False):
    path = os.path.join(VIDEO_DIR, name)
    expect(os.path.getsize(path) < 1_500_000, f"{name} is suspiciously large")
    info = ffprobe_json(path, "stream=codec_type,codec_name,width,height", "format=duration")
    streams = {s["codec_type"]: s for s in info["streams"]}
    expect("video" in streams, f"{name}: no video stream")
    expect(streams["video"]["codec_name"] == "h264", f"{name}: not h264")
    expect((streams["video"]["width"], streams["video"]["height"]) == (320, 240), f"{name}: resolution")
    duration = float(info["format"]["duration"])
    expect(min_seconds <= duration <= max_seconds, f"{name}: duration {duration}")
    if expect_black:
        stderr = ffmpeg_stderr("-i", path, "-vf", "blackdetect=d=0.5:pix_th=0.10", "-an", "-f", "null", "-")
        expect("black_start" in stderr, f"{name}: blackdetect found no black segment")
    if expect_freeze:
        stderr = ffmpeg_stderr("-i", path, "-vf", "freezedetect=n=-50dB:d=0.8", "-an", "-f", "null", "-")
        expect("freeze_start" in stderr, f"{name}: freezedetect found no frozen segment")
    print(f"ok {name}")


def verify():
    for name in ["gradient-320x240.png", "solid-black-320x240.png", "solid-white-320x240.png", "solid-green-320x240.png"]:
        path = os.path.join(IMAGE_DIR, name)
        expect(os.path.getsize(path) < 1_500_000, f"{name} too large")
        info = ffprobe_json(path, "stream=width,height,codec_name")
        stream = info["streams"][0]
        expect((stream["width"], stream["height"]) == (320, 240), f"{name}: resolution")
        print(f"ok {name}")

    verify_video("clean-4s.mp4", 3.5, 4.5)
    verify_video("black-segment-4s.mp4", 3.5, 4.5, expect_black=True)
    verify_video("frozen-segment-4s.mp4", 3.5, 4.5, expect_freeze=True)
    verify_video("spinner-segment-4s.mp4", 3.5, 4.5)

    audio_path = os.path.join(AUDIO_DIR, "tone-then-silence-4s.wav")
    info = ffprobe_json(audio_path, "stream=codec_name,sample_rate,channels", "format=duration")
    stream = info["streams"][0]
    expect(stream["codec_name"] == "pcm_s16le", "tone-then-silence: not pcm_s16le")
    expect(int(stream["sample_rate"]) == 8000, "tone-then-silence: sample rate")
    stderr = ffmpeg_stderr("-i", audio_path, "-af", "silencedetect=n=-45dB:d=0.5", "-f", "null", "-")
    expect("silence_start" in stderr, "tone-then-silence: no silence detected")
    print("ok tone-then-silence-4s.wav")

    print("fixture verification OK")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", action="store_true", help="verify committed fixtures instead of regenerating")
    args = parser.parse_args()
    if shutil.which("ffmpeg") is None:
        print("ffmpeg not found on PATH; install it to work with fixtures (CI provides it).", file=sys.stderr)
        return 1
    if args.verify:
        verify()
        return 0
    gen_images()
    gen_videos()
    gen_audio()
    sha256sums()
    print("fixtures generated under tests/fixtures/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
