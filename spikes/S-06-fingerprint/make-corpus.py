"""S-06 corpus: 50 synthetic «songs» (30 s, pseudo-random melody + bass + hats, deterministic) and
variants (MP3 128k, AAC 96k, −8 dB, trimmed 0.5 s) + official Chromaprint fingerprints via ffmpeg.
Usage: python make-corpus.py <out dir>"""
import subprocess, sys, os
from concurrent.futures import ThreadPoolExecutor

out = sys.argv[1]
os.makedirs(out, exist_ok=True)

def ff(*args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)

def song(i):
    a, b, bpm = 1.37 + i * 0.731, 0.11 + i * 1.913, 80 + (i * 37) % 90
    note = f"floor(12*abs(sin(floor(t*{bpm}/30)*{a:.3f}+{b:.3f})))"
    bass = f"floor(5*abs(sin(floor(t*{bpm}/120)*{b:.3f}+{a:.3f})))"
    expr = (rf"0.25*sin(2*PI*t*220*pow(2\,{note}/12))+0.12*sin(4*PI*t*220*pow(2\,{note}/12))"
            rf"+0.2*sin(2*PI*t*55*pow(2\,{bass}/12))+0.1*(random(0)-0.5)*lt(mod(t\,60/{bpm})\,0.03)")
    f = os.path.join(out, f"song{i:02d}")
    ff("-f", "lavfi", "-i", f"aevalsrc={expr}:s=44100:d=30", "-ac", "2", "-c:a", "pcm_s16le", f"{f}_orig.wav")
    ff("-i", f"{f}_orig.wav", "-c:a", "libmp3lame", "-b:a", "128k", f"{f}_mp3.mp3")
    ff("-i", f"{f}_orig.wav", "-c:a", "aac", "-b:a", "96k", f"{f}_aac.m4a")
    ff("-i", f"{f}_orig.wav", "-af", "volume=-8dB", "-c:a", "pcm_s16le", f"{f}_quiet.wav")
    ff("-ss", "0.5", "-i", f"{f}_orig.wav", "-c:a", "pcm_s16le", f"{f}_trim.wav")
    ff("-i", f"{f}_orig.wav", "-f", "chromaprint", "-fp_format", "raw", f"{f}_orig.fp")
    ff("-i", f"{f}_mp3.mp3", "-f", "chromaprint", "-fp_format", "raw", f"{f}_mp3.fp")

with ThreadPoolExecutor(8) as ex:
    list(ex.map(song, range(50)))
print(len(os.listdir(out)), "files")
