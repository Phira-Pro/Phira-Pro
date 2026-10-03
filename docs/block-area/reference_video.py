"""Extract original video frames for block-area visual checks (no cropping).

Uses an already installed FFmpeg/FFprobe. Metadata is saved beside the frames;
source video files are read only. Times are video seconds, not chart seconds.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path


def probe(path: Path, binary: str) -> dict:
    return json.loads(subprocess.check_output([
        binary, "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)
    ], text=True, encoding="utf-8"))


def extract(path: Path, times: list[float], out: Path, ffmpeg: str, ffprobe: str) -> None:
    out.mkdir(parents=True, exist_ok=True)
    metadata = probe(path, ffprobe)
    previous = out / "metadata.json"
    metadata["extraction"] = (json.loads(previous.read_text(encoding="utf-8")).get("extraction", [])
                              if previous.exists() else [])
    for at in times:
        filename = out / f"frame-{at:010.5f}s.png"
        result = subprocess.run([
            ffmpeg, "-hide_banner", "-loglevel", "info", "-copyts", "-ss", f"{at:.8f}",
            "-i", str(path), "-map", "0:v:0", "-vf", "showinfo", "-frames:v", "1", "-y", str(filename)
        ], check=True, capture_output=True, text=True, encoding="utf-8", errors="replace")
        pts = re.search(r"\bn:\s*0\s+pts:\s*\d+\s+pts_time:([0-9.]+)", result.stderr)
        metadata["extraction"] = [record for record in metadata["extraction"] if record["file"] != str(filename)]
        metadata["extraction"].append({"requested_video_seconds": at, "decoded_frame_pts_seconds": float(pts.group(1)) if pts else None, "file": str(filename)})
        print(filename)
    (out / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument("--times", required=True, help="comma-separated video seconds")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--ffmpeg", default="D:/phi-recorder/ffmpeg.exe")
    parser.add_argument("--ffprobe", default="D:/phi-recorder/ffprobe.exe")
    args = parser.parse_args()
    extract(args.video, [float(value) for value in args.times.split(",")], args.out, args.ffmpeg, args.ffprobe)
