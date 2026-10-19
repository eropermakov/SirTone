"""S-07: EfficientAT mn10_as → ONNX (10 s window), mel basis and a parity fixture.

Run with the isolated interpreter (-I); the repo and the weights are passed as paths:
  python -I export_mn10.py <repo> <weights.pt> <out_dir>
"""
import math
import os
import struct
import sys

repo, weights, out = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, repo)

import torch  # noqa: E402
from models.mn.model import get_model  # noqa: E402
from models.preprocess import AugmentMelSTFT  # noqa: E402

torch.manual_seed(0)
os.makedirs(out, exist_ok=True)

model = get_model(width_mult=1.0, pretrained_name=None)
state = torch.load(weights, map_location="cpu", weights_only=True)
model.load_state_dict(state)
model.eval()

SR = 32000
SECS = 10
mel = AugmentMelSTFT(n_mels=128, sr=SR, win_length=800, hopsize=320)
mel.eval()


class Net(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, x):
        logits, emb = self.m(x)
        return logits, emb


net = Net(model).eval()

# A deterministic test signal: a melody with drums, 10 s at 32 kHz.
n = SR * SECS
wave = []
for i in range(n):
    t = i / SR
    note = 220 * 2 ** (((math.floor(t * 3) * 5) % 12) / 12)
    v = 0.3 * math.sin(2 * math.pi * note * t) + 0.2 * math.sin(2 * math.pi * 110 * t)
    beat = t % 0.5
    v += 0.6 * math.exp(-beat / 0.05) * math.sin(2 * math.pi * 55 * beat)
    wave.append(v * 0.6)
x = torch.tensor(wave, dtype=torch.float32).unsqueeze(0)
with torch.no_grad():
    spec = mel(x)  # (1, 128, T)
    logits, emb = net(spec.unsqueeze(1))
print("mel", tuple(spec.shape), "logits", tuple(logits.shape), "emb", tuple(emb.shape))

# The mel basis exactly as the frontend builds it (eval: fmin 0, fmax 15000).
import torchaudio  # noqa: E402

basis, _ = torchaudio.compliance.kaldi.get_mel_banks(128, 1024, SR, mel.fmin, mel.fmax,
                                                       vtln_low=100.0, vtln_high=-500.0,
                                                       vtln_warp_factor=1.0)
basis = torch.nn.functional.pad(basis, (0, 1), mode="constant", value=0)  # (128, 513)
print("basis", tuple(basis.shape), "fmax", mel.fmax)


def dump(name, t):
    a = t.detach().contiguous().view(-1).tolist()
    with open(os.path.join(out, name), "wb") as f:
        f.write(struct.pack("<%df" % len(a), *a))


dump("mel_basis_128x513.f32", basis)
dump("fixture_wave_32k.f32", x)
dump("fixture_mel.f32", spec)
dump("fixture_logits.f32", logits)
dump("fixture_emb.f32", emb)

torch.onnx.export(
    net,
    (spec.unsqueeze(1),),
    os.path.join(out, "mn10_as.onnx"),
    input_names=["mel"],
    output_names=["logits", "embedding"],
    opset_version=17,
    dynamo=False,
)
print("exported")

# onnxruntime cross-check of the export.
import numpy as np  # noqa: E402
import onnxruntime as ort  # noqa: E402

s = ort.InferenceSession(os.path.join(out, "mn10_as.onnx"))
o = s.run(None, {"mel": spec.unsqueeze(1).numpy()})
print("onnx vs torch logits max diff", float(np.abs(o[0] - logits.numpy()).max()))
top = torch.sigmoid(logits)[0].topk(5)
print("top classes", top.indices.tolist(), [round(v, 3) for v in top.values.tolist()])
