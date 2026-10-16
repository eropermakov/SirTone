"""Stage 1 exit check: «нет потерь при убийстве процесса во время импорта».

For each run: fresh library → start `sirtone-cli import` → kill the process at a random moment →
start it again (it recovers the journal and resumes the batch) → `sirtone-cli verify` must pass,
the library must hold exactly the expected tracks, and the source corpus must be byte-identical.

Usage: python tools/import-kill-harness.py [runs=20]
Needs: target/release/sirtone-cli.exe, fixtures/generated/import500 (make-import-corpus.py).
"""
import hashlib
import os
import random
import re
import shutil
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CLI = os.path.join(ROOT, "target", "release", "sirtone-cli.exe")
CORPUS = os.path.join(ROOT, "fixtures", "generated", "import500")
LIB = os.path.join(ROOT, "target", "kill-lib")
RUNS = int(sys.argv[1]) if len(sys.argv) > 1 else 20
EXPECTED_TRACKS = 438


def corpus_digest():
    h = hashlib.blake2b()
    for d, _, files in sorted(os.walk(CORPUS)):
        for f in sorted(files):
            p = os.path.join(d, f)
            h.update(p.encode())
            h.update(open(p, "rb").read())
    return h.hexdigest()


before = corpus_digest()
rng = random.Random(7)
failures = 0
for run in range(1, RUNS + 1):
    shutil.rmtree(LIB, ignore_errors=True)
    delay = rng.uniform(0.05, 1.0)
    p = subprocess.Popen([CLI, "import", LIB, CORPUS], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(delay)
    killed = p.poll() is None
    if killed:
        p.kill()
    p.wait()
    resumed = subprocess.run([CLI, "import", LIB], capture_output=True, text=True, encoding="utf-8")
    verify = subprocess.run([CLI, "verify", LIB], capture_output=True, text=True, encoding="utf-8")
    m = re.search(r"verify: (\d+) tracks", verify.stdout)
    tracks = int(m.group(1)) if m else -1
    ok = verify.returncode == 0 and tracks == EXPECTED_TRACKS
    recovered = re.search(r"recovered (\d+)", resumed.stdout)
    print(f"run {run:2d}: kill at {delay:.2f} s ({'killed' if killed else 'finished first'}), "
          f"recovered {recovered.group(1) if recovered else 0}, {tracks} tracks, verify {'OK' if ok else 'FAIL'}")
    if not ok:
        failures += 1
        print(resumed.stdout[-800:], verify.stdout[-1500:])

same = corpus_digest() == before
print(f"\n{RUNS - failures}/{RUNS} runs OK; sources unchanged: {same}")
shutil.rmtree(LIB, ignore_errors=True)
sys.exit(0 if failures == 0 and same else 1)
