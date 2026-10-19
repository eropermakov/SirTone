# Модели анализа (входят в приложение — D-84)

Каждая модель: источник, лицензия, как получена, sha256. Модели встраиваются в бинарник
(`sirtone-analysis`) и работают без сети. Загрузки владельцем разрешены (D-84); в репозитории —
только экспортированные файлы.

## mn10_as.onnx — EfficientAT MobileNetV3, AudioSet (527 классов)

| | |
|---|---|
| Источник кода | https://github.com/fschmid56/EfficientAT (MIT, © 2022 Florian Schmid) |
| Веса | `mn10_as_mAP_471.pt` из GitHub Releases v0.0.1 (официальный релиз репозитория), sha256 `0bd7dc2443af498c289a2e739f02ebb515d6aa3fd3ab9db539c86123ae368a4e`; mAP AudioSet 0,471, 4,88 млн параметров |
| Лицензия | MIT (код и веса публикуются в одном репозитории под MIT; отдельных условий на веса нет) |
| Экспорт | PyTorch 2.14 CPU → ONNX opset 17 (`torch.onnx.export`, legacy), вход `mel` [1, 1, 128, 1000] (окно 10 с), выходы `logits` [1, 527] и `embedding` [1, 960]; загрузка весов `weights_only=True`; сверка ONNX ↔ PyTorch: 1,0·10⁻⁵ |
| sha256 | `29bfb539282217e6785a687a9a36064d9fabe84b99c61670c988970992157a19` |
| Скрипт | `spikes/S-07-model/export_mn10.py` |

## mn10_as_mel_basis.f32 — мел-банк фронтенда

Матрица 128 × 513 (f32 LE) — ровно та, что строит `AugmentMelSTFT` в режиме оценки
(`torchaudio.compliance.kaldi.get_mel_banks`, 32 кГц, n_fft 1024, fmin 0, fmax 15 000 + нулевой
столбец). sha256 `e93e5b600887ba87a5b3a6b03ba44b844ef60318d1fd206ffa67483e4b2c4783`.

## audioset_labels.csv — названия классов AudioSet

Онтология AudioSet (Google), **CC BY 4.0** — атрибуция в «О программе → Лицензии» (T6.6).
sha256 `a9c221f673b0c20bd8674c4594a77aea33fca3702a120182d2737b50d099a452`.
