"""Deterministic import corpus (stage 1 exit test: «500 файлов с флешки»).

Usage: python tools/fixtures/make-import-corpus.py [out_dir]   (default fixtures/generated/import500)
Needs ffmpeg 9.x in PATH. Output is not committed. Re-running gives the same files.

Mix (500 files): tagged albums in «Artist/Album (Year)/NN - Title.ext» folders, singles named
«Artist - Title», feats, mojibake tags (cp1251 read as Latin-1), untagged files, junk tag values,
exact copies, broken audio, non-audio files and playlists. Formats: mostly MP3, plus FLAC, M4A,
OGG, Opus, WAV.
"""
import os
import random
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "fixtures", "generated", "import500")
rng = random.Random(20261016)

ARTISTS = ["Би-2", "Земфира", "Сплин", "Кино", "Мумий Тролль", "Ночные Снайперы", "Ленинград", "Агата Кристи",
           "Radiohead", "Massive Attack", "Portishead", "Daft Punk", "Björk", "Sigur Rós", "Moby", "Air"]
WORDS = ["Ночь", "Город", "Звезда", "Дорога", "Небо", "Ветер", "Море", "Огонь", "Зима", "Сердце", "Light",
         "Dream", "Highway", "Rain", "Shadow", "Silence", "Echo", "Глубина", "Танцы", "Весна"]
FORMATS = [("mp3", ["-c:a", "libmp3lame", "-b:a", "96k"]), ("flac", ["-c:a", "flac"]),
           ("m4a", ["-c:a", "aac", "-b:a", "96k"]), ("ogg", ["-c:a", "libvorbis", "-q:a", "2"]),
           ("opus", ["-c:a", "libopus", "-b:a", "64k"]), ("wav", ["-c:a", "pcm_s16le"])]
FMT_WEIGHTS = [60, 12, 10, 8, 5, 5]


def title():
    return " ".join(rng.sample(WORDS, rng.randint(1, 3)))


def mojibake(s):
    return s.encode("cp1251", errors="replace").decode("latin-1")


jobs = []  # (rel_path, ext, codec_args, metadata dict, seconds, freq)


used = set()


def audio(rel, meta, fmt=None):
    ext, args = fmt or rng.choices(FORMATS, weights=FMT_WEIGHTS)[0]
    base, n = rel, 2
    while f"{rel}.{ext}".lower() in used:
        rel, n = f"{base} {n}", n + 1
    used.add(f"{rel}.{ext}".lower())
    rel = f"{rel}.{ext}"
    jobs.append((rel, ext, args, meta, 2 + rng.random() * 2, 200 + rng.randint(0, 600)))
    return rel


# 1. Albums with full tags: 30 albums × ~11 tracks ≈ 330
for a in range(30):
    artist = ARTISTS[a % len(ARTISTS)]
    album, year = title(), 1985 + rng.randint(0, 39)
    fmt = rng.choices(FORMATS, weights=FMT_WEIGHTS)[0]
    for n in range(1, rng.randint(8, 13) + 1):
        t = title()
        audio(f"{artist}/{album} ({year})/{n:02d} - {t}", {"artist": artist, "album_artist": artist, "album": album,
                                                       "title": t, "track": str(n), "date": str(year)}, fmt)
# 2. Singles with feats: 40
for i in range(40):
    a1, a2 = rng.sample(ARTISTS, 2)
    t = title()
    artist = f"{a1} feat. {a2}" if i % 3 == 0 else a1
    audio(f"Сборка/{artist} - {t}", {"artist": artist, "title": t})
# 3. Mojibake tags: 40
for i in range(40):
    artist, t = rng.choice(ARTISTS[:8]), title()
    audio(f"Старые mp3/{i:03d}", {"artist": mojibake(artist), "title": mojibake(t), "album": mojibake("Сборник")},
          FORMATS[0])
# 4. No tags at all: 30
for i in range(30):
    audio(f"Без тегов/{rng.choice(ARTISTS)} - {title()}", {})
# 5. Junk tag values: 15
for i in range(15):
    audio(f"Мусор/Track {i + 1:02d}", {"artist": "Unknown Artist", "title": f"Track {i + 1:02d}", "album": "Unknown"},
          FORMATS[0])
total_audio = len(jobs)
dupes = rng.sample([j[0] for j in jobs], 10)
others = 500 - total_audio - len(dupes)


def render(job):
    rel, ext, args, meta, secs, freq = job
    path = os.path.join(OUT, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    md = []
    for k, v in meta.items():
        md += ["-metadata", f"{k}={v}"]
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
                    f"sine=frequency={freq}:duration={secs:.2f}:sample_rate=44100", "-ac", "2", *args,
                    "-map_metadata", "-1", *md, "-bitexact", path], check=True)


if os.path.exists(OUT):
    shutil.rmtree(OUT)
with ThreadPoolExecutor(8) as ex:
    list(ex.map(render, jobs))

# 6. Exact copies (different names) → duplicates
for i, rel in enumerate(dupes):
    base, ext = os.path.splitext(rel)
    shutil.copyfile(os.path.join(OUT, rel), os.path.join(OUT, f"{base} (копия){ext}"))
# 7. Broken, non-audio, playlists fill up to exactly 500
os.makedirs(os.path.join(OUT, "Разное"), exist_ok=True)
made = 0
src = os.path.join(OUT, jobs[0][0])
data = open(src, "rb").read()
while made < others:
    kind = made % 4
    p = os.path.join(OUT, "Разное", f"{made:03d}")
    if kind == 0:
        open(p + ".mp3", "wb").write(b"\0" * 0)              # empty → corrupt/not audio
    elif kind == 1:
        open(p + ".flac", "wb").write(b"this is text")        # text with audio extension
    elif kind == 2:
        open(p + ".txt", "w", encoding="utf-8").write("notes")
    else:
        open(p + ".m3u", "w", encoding="utf-8").write("#EXTM3U\n")
    made += 1

count = sum(len(f) for _, _, f in os.walk(OUT))
print(f"import corpus: {count} files ({total_audio} audio, {len(dupes)} exact copies, {others} junk) -> {OUT}")
