"""Third-party notices (T6.6, NFR-LIC-001, `08` §3): everything that ships inside Сиртон.

Rust: the normal (not build-only, not proc-macro) dependencies of `sirtone-app` for
x86_64-pc-windows-msvc, with the license texts each crate carries (LICENSE*, COPYING*, NOTICE*).
JS: the production dependencies of the UI (`pnpm licenses list --prod`). Assets: the font (full
OFL text — OFL requires it), the models, data and presets, and the online services' terms.
Identical license texts are printed once with the list of packages under them.

Writes:
  assets/legal/notices.json  — the «Лицензии» screen (the app embeds it)
  THIRD_PARTY_NOTICES.txt    — the same as plain text, next to the installer

Usage (offline, from the repo root): python tools/notices.py
"""

import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = "x86_64-pc-windows-msvc"
LICENSE_FILES = ("license", "licence", "copying", "notice", "unlicense", "copyright")


def run(cmd, cwd=ROOT):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8",
                          check=True, shell=(os.name == "nt" and cmd[0] == "pnpm")).stdout


def license_texts(path):
    """The license files of a package directory, in a stable order."""
    out = []
    try:
        names = sorted(os.listdir(path))
    except OSError:
        return out
    for n in names:
        low = n.lower()
        if low.startswith(LICENSE_FILES) and os.path.isfile(os.path.join(path, n)):
            try:
                with open(os.path.join(path, n), encoding="utf-8", errors="replace") as f:
                    out.append((n, f.read().strip()))
            except OSError:
                pass
    return out


def rust_packages():
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--offline",
                           "--filter-platform", TARGET]))
    by_id = {p["id"]: p for p in meta["packages"]}
    tree = run(["cargo", "tree", "-e", "normal", "--prefix", "none", "-p", "sirtone-app",
                "--target", TARGET, "--offline"])
    wanted = set()
    for line in tree.splitlines():
        line = line.replace(" (*)", "").strip()
        if not line or "(proc-macro)" in line:
            continue
        parts = line.split()
        if len(parts) >= 2:
            wanted.add((parts[0], parts[1].lstrip("v")))
    out = []
    for p in by_id.values():
        if (p["name"], p["version"]) not in wanted or p.get("source") is None:
            continue  # our own crates are not third-party
        out.append({
            "name": p["name"],
            "version": p["version"],
            "license": p.get("license") or "see text",
            "repo": p.get("repository") or "",
            "authors": p.get("authors") or [],
            "texts": license_texts(os.path.dirname(p["manifest_path"])),
        })
    return out


def js_packages():
    data = json.loads(run(["pnpm", "licenses", "list", "--prod", "--json"],
                          cwd=os.path.join(ROOT, "apps", "desktop")))
    out = []
    for lic, pkgs in data.items():
        for p in pkgs:
            texts = []
            for path in p.get("paths", []):
                texts = license_texts(path)
                if texts:
                    break
            out.append({"name": p["name"], "version": ", ".join(p.get("versions", [])),
                        "license": lic, "texts": texts})
    return out


def read(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
        return f.read().strip()


CC_BY_4 = ("Creative Commons Attribution 4.0 International (CC BY 4.0) — "
           "https://creativecommons.org/licenses/by/4.0/legalcode")


def assets():
    """Non-package parts: (title, license, attribution / full text)."""
    return [
        ("Шрифт Geologica", "SIL Open Font License 1.1", read("apps/desktop/src/assets/fonts/OFL.txt")),
        ("Модель EfficientAT mn10_as (Florian Schmid)", "MIT", read("assets/licenses/efficientat.txt")),
        ("Онтология и названия классов AudioSet (Google)", "CC BY 4.0",
         read("assets/licenses/audioset-ontology.txt") + "\n\n" + CC_BY_4),
        ("rusty-chromaprint", "MIT", read("assets/licenses/rusty-chromaprint.txt")),
        ("Signalsmith Stretch", "MIT", read("assets/licenses/signalsmith-stretch.txt")),
        ("Пресеты MilkDrop (butterchurn-presets, авторы сообщества)", "MIT",
         read("assets/viz/milkdrop/CREDITS.md")),
        ("Погода — Open-Meteo.com", "CC BY 4.0",
         "Weather data by Open-Meteo.com (https://open-meteo.com/), " + CC_BY_4),
        ("MusicBrainz, Cover Art Archive, ListenBrainz (MetaBrainz Foundation)", "CC0 / права авторов обложек",
         "Метаданные MusicBrainz — CC0; статистика ListenBrainz — CC0; обложки Cover Art Archive "
         "принадлежат их правообладателям и используются для показа в медиатеке."),
        ("AcoustID", "CC BY-SA 3.0 (данные)",
         "Распознавание по отпечатку — AcoustID (https://acoustid.org), некоммерческое использование."),
    ]


BSD_2 = """Copyright (c) {who}
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions
   and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""


def canonical(pkgs):
    """A license's standard text, taken from a package that carries exactly that one file."""
    out = {}
    for _, p in pkgs:
        if len(p["texts"]) == 1 and p["license"] not in out:
            out[p["license"]] = p["texts"][0][1]
    return out


def fill(pkgs):
    """A package without a license file gets the standard text of its license (the first
    licence of an «A OR B» choice) and, for MPL-2.0, where its source is."""
    canon = canonical(pkgs)
    for _, p in pkgs:
        if p["texts"]:
            continue
        first = p["license"].split(" OR ")[0].split("/")[0].strip()
        text = canon.get(first) or canon.get(p["license"])
        note = f"(Текст лицензии {first}; в пакете {p['name']} отдельного файла нет.)"
        if not text and first == "BSD-2-Clause":
            text = BSD_2.format(who=", ".join(a.split("<")[0].strip() for a in p.get("authors", []))
                                or p["name"] + " authors")
        if first == "MPL-2.0":
            text = ("Mozilla Public License 2.0 — https://mozilla.org/MPL/2.0/. "
                    "Используется без изменений; исходный код: " + (p.get("repo") or "crates.io"))
        if text:
            p["texts"] = [("LICENSE", f"{note}\n\n{text}")]


def build():
    pkgs = [("Rust", p) for p in rust_packages()] + [("JS", p) for p in js_packages()]
    fill(pkgs)
    # Group by identical license text (or, without a file, by the license expression).
    groups = {}
    for kind, p in pkgs:
        body = "\n\n".join(t for _, t in p["texts"]) or p["license"]
        key = hashlib.sha256(body.encode()).hexdigest()
        g = groups.setdefault(key, {"license": p["license"], "text": body, "packages": []})
        g["packages"].append(f"{p['name']} {p['version']}")
    sections = [{"title": t, "license": lic, "items": [], "text": text} for t, lic, text in assets()]
    for g in sorted(groups.values(), key=lambda g: (g["license"], g["packages"][0].lower())):
        sections.append({"title": g["license"], "license": g["license"],
                         "items": sorted(g["packages"], key=str.lower), "text": g["text"]})
    return {"rust": sum(1 for k, _ in pkgs if k == "Rust"),
            "js": sum(1 for k, _ in pkgs if k == "JS"), "sections": sections}


def main():
    doc = build()
    os.makedirs(os.path.join(ROOT, "assets", "legal"), exist_ok=True)
    with open(os.path.join(ROOT, "assets", "legal", "notices.json"), "w", encoding="utf-8",
              newline="\n") as f:
        json.dump(doc, f, ensure_ascii=False, indent=1)
        f.write("\n")
    lines = ["Сиртон — сторонние компоненты (THIRD_PARTY_NOTICES)",
             f"Rust: {doc['rust']} · JS: {doc['js']}", ""]
    for s in doc["sections"]:
        lines += ["=" * 78, f"{s['title']} — {s['license']}"]
        if s["items"]:
            lines.append("Пакеты: " + ", ".join(s["items"]))
        lines += ["-" * 78, s["text"], ""]
    with open(os.path.join(ROOT, "THIRD_PARTY_NOTICES.txt"), "w", encoding="utf-8",
              newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"rust {doc['rust']}, js {doc['js']}, sections {len(doc['sections'])}")


if __name__ == "__main__":
    sys.exit(main())
