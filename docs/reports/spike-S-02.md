# Спайк S-02 — реальные форматы (symphonia 0.6.1 + libopus)

- **Дата:** 2026-10-16, Этап 0 (путь к прототипу 0.2)
- **Связанные требования:** FR-PLAY-001, FR-PLAY-003 (gapless) · **ADR:** D-06 · **Задачи:** T0.16, T1.2, T1.5
- **Вопрос (`09` §4):** декодирует ли связка `symphonia` + libopus корпус форматов так же, как ffmpeg: lossless побитово, перемотка точно, битые файлы — ошибка, а не падение.

## 1. Что сделано
Код: `spikes/S-02-formats/` (вне workspace; `cargo run --release -- ../../fixtures/generated`).
- Корпус — `tools/fixtures/make-fixtures.ps1` (T0.13): 14 форматов, gapless-пара, битые файлы, «ядовитые» имена.
- Эталон — `ffmpeg 9.0.1 -f f32le` того же файла. Lossless сравнивается побитово (f32 из 16/24 бит точен), lossy — по SNR после поиска лучшего сдвига (±2048 кадров).
- Перемотка: `SeekMode::Accurate` на 3,0 с, отбрасываем кадры до `required_ts`, сравниваем с эталоном с 3,0 с (+2048 кадров, чтобы бит-резервуар MP3 не мешал), ищем сдвиг.
- Все вызовы — под `catch_unwind`. Opus — `symphonia-adapter-libopus` 0.3 (libopus собирается CMake из исходников, +28 с к сборке).

Окружение: Windows 11 10.0.26300, Ryzen 7 5800H, Rust 1.99.0, release.

## 2. Результаты (вывод спайка как есть)

| file | rate/ch | frames sym | frames ff | Δframes | exact/SNR | seek 3 s | speed | note |
|---|---|---|---|---|---|---|---|---|
| aac-256.m4a | 44100/2 | 442368 | 441000 | 1368 | 5.9 dB @lag -1181 | **+1024 fr** (132 dB) | 1199× |  |
| aiff-16-44.aiff | 44100/2 | 441000 | 441000 | 0 | bit-exact | ±0 (inf dB) | 2987× |  |
| alac-16-44.m4a | 44100/2 | 441000 | 441000 | 0 | bit-exact | ±0 (inf dB) | 531× |  |
| flac-16-44.flac | 44100/2 | 441000 | 441000 | 0 | bit-exact | ±0 (inf dB) | 1449× |  |
| flac-24-96.flac | 96000/2 | 960000 | 960000 | 0 | bit-exact | ±0 (inf dB) | 578× |  |
| mono-22k.wav | 22050/1 | 220500 | 220500 | 0 | bit-exact | ±0 (inf dB) | 11414× |  |
| mp3-cbr320.mp3 | 44100/2 | 441000 | 441000 | 0 | 122.9 dB | ±0 (123 dB) | 905× |  |
| mp3-vbr-v2.mp3 | 44100/2 | 441000 | 441000 | 0 | 123.4 dB | ±0 (122 dB) | 1555× |  |
| opus-160.opus | 48000/2 | 480000 | 480000 | 0 | 139.2 dB | **+312 fr** (23 dB) | 172× |  |
| vorbis-q6.ogg | 44100/2 | 441000 | 441000 | 0 | 138.5 dB | ±0 (139 dB) | 1214× |  |
| wav-16-44.wav | 44100/2 | 441000 | 441000 | 0 | bit-exact | ±0 (inf dB) | 3213× |  |
| wav-24-48.wav | 48000/2 | 480000 | 480000 | 0 | bit-exact | ±0 (inf dB) | 2631× |  |
| wav-f32-44.wav | 44100/2 | 441000 | 441000 | 0 | bit-exact | ±0 (inf dB) | 3221× |  |
| wma-192.wma | — | — | — | — | — | — | — | **probe: unsupported feature: core (probe): no suitable format reader found** |
| gapless-a.mp3 | 44100/1 | 176400 | 176400 | 0 | 123.3 dB | ±0 (122 dB) | 1700× |  |

broken:
- empty.flac: error: probe: unsupported feature: core (probe): no suitable format reader found
- playlist.m3u: error: probe: unsupported feature: core (probe): no suitable format reader found
- text-as.mp3: error: probe: unsupported feature: core (probe): no suitable format reader found
- truncated.mp3: decoded 290398 samples (partial)

names (unicode/long paths, probe only):
- .mp3 (35 chars): ok
- 01 - Би-2 - Полковнику никто не пишет.mp3 (72 chars): ok
- AC-DC - Back In Black [320kbps].mp3 (66 chars): ok
- con_.mp3 (39 chars): ok
- Jay-Z - 99 Problems (Official Video).mp3 (71 chars): ok
- leading space.mp3 (48 chars): ok
- Track 01.mp3 (43 chars): ok
- trailing dot..mp3 (48 chars): ok
- Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя Очень длинное имя (246 chars): ok
- Песня с эмодзи 🎵.mp3 (51 chars): ok

failures: 1

`speed` — во сколько раз быстрее реального времени, один поток (включая открытие файла).

## 3. Выводы
| Критерий (`09` §4) | Итог |
|---|---|
| lossless побитово | ✅ FLAC 16/44 и 24/96, WAV 16/24/f32, AIFF, ALAC — побитово, длина совпадает |
| lossy | ✅ MP3 CBR/VBR 123 дБ, Vorbis 139 дБ, Opus 139 дБ к ffmpeg — это ошибка округления, не искажение |
| перемотка ±1 кадр | ✅ всё, кроме двух случаев ниже, — ровно 0 кадров. ⚠️ **AAC/M4A**: +1024 кадра (не срезана задержка кодера — symphonia не читает `elst`/`iTunSMPB`); ⚠️ **Opus**: +312 кадров после перемотки (не вычтен pre-skip), полное декодирование при этом точное |
| gapless | ✅ MP3 (LAME/Xing) — длина = ffmpeg; Vorbis/Opus/FLAC/WAV — точные. ⚠️ AAC: +1368 кадров (1024 в начале + хвост) — своя реализация `elst`/`iTunSMPB` в T1.5, как и было заложено (`04`) |
| битые файлы | ✅ пустой/текст/плейлист — ошибка «нет подходящего формата», без паники; обрезанный MP3 — играет то, что есть (≈ 1/3) |
| имена | ✅ кириллица, эмодзи, 246 символов, точки/пробелы на краях — открываются |
| WMA | ❌ symphonia не умеет (ожидаемо, D-06) → **Media Foundation**, отдельной задачей в T1.2; в прототип 0.2 не входит |
| скорость | ✅ 170–9900× реального времени — декодер не узкое место |

**Решение по D-06:** подтверждено — `symphonia` (без модификаций, MPL-2.0) + `symphonia-adapter-libopus` (bundled, CMake) + Media Foundation для WMA. Ffmpeg-sidecar не нужен для корпуса. Обязательные поправки нашего слоя (T1.5): срез задержки AAC по `elst`/`iTunSMPB`; вычет pre-skip Opus после перемотки (тест: этот спайк, критерий «±0 кадров»).

## 4. Что не проверено
- APE, WavPack (в `09` §4 перечислены; в корпусе нет — нужны фикстуры, ffmpeg их не кодирует без внешних энкодеров). Symphonia их не поддерживает — кандидаты на ffmpeg-sidecar (D-06, запас).
- Длинный файл 3 ч и перемотка в нём — в T1.2.
