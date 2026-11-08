"""Human names for the 500 MilkDrop presets, from the measured descriptors (hue, saturation,
brightness, motion, reactivity, chaos) in assets/viz/milkdrop/index.json -> names.json.
Deterministic: the same index gives the same names. Run: python tools/preset-names.py"""
import json, io, sys
root = __file__.replace("\\", "/").rsplit("/tools/", 1)[0]
idx = json.load(open(f"{root}/assets/viz/milkdrop/index.json", encoding="utf8"))["presets"]

HUES = [(15, "красный"), (40, "оранжевый"), (65, "жёлтый"), (100, "салатовый"), (160, "зелёный"),
        (190, "бирюзовый"), (215, "голубой"), (255, "синий"), (285, "фиолетовый"),
        (320, "пурпурный"), (345, "розовый"), (361, "красный")]

def colour(p):
    if p["sat"] < 0.12:
        return "почти чёрный" if p["bright"] < 0.08 else "серый" if p["bright"] < 0.25 else "серебристый"
    h = next(n for lim, n in HUES if p["hue"] < lim)
    return h

def light(p):
    b = p["bright"]
    return "тёмный" if b < 0.07 else "приглушённый" if b < 0.15 else "яркий" if b < 0.35 else "светящийся"

def motion(p):
    m = p["motion"]
    return ("почти неподвижный" if m < 0.006 else "медленный" if m < 0.012 else "плавный" if m < 0.025
            else "живой" if m < 0.045 else "быстрый")

def extra(p):
    if p["react"] > 0.03: return "бьётся в такт"
    if p["chaos"] > 0.018: return "хаотичный"
    if p["sat"] > 0.8 and p["sat"] >= 0.12: return "насыщенный"
    return ""

groups = {}
for p in idx:
    key = ", ".join(x for x in (colour(p).capitalize(), light(p), motion(p), extra(p)) if x)
    groups.setdefault(key, []).append(p["name"])
out = {}
for key, names in groups.items():
    for i, n in enumerate(sorted(names)):
        out[n] = key if len(names) == 1 else f"{key} · {i + 1}"
json.dump(out, open(f"{root}/assets/viz/milkdrop/names.json", "w", encoding="utf8"), ensure_ascii=False, indent=1)
print(len(out), "names,", len(groups), "groups, biggest", max(len(v) for v in groups.values()))
