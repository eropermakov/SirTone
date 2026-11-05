# Проверка П2 — фоновый звук на iPhone

- **Дата / сессия:** 2026-11-05, ветка `stage/8-mobile`
- **Связанные требования:** FR-MOB-011 · **ADR:** D-137, D-138
- **Референсы (`01d`):** Apple «Becoming a now playable app» (MPNowPlayingInfoCenter, MPRemoteCommandCenter), AVAudioSourceNode

## 1. Вопрос и критерии (`docs/mobile/checks.md`)
- Успех: FLAC и Opus 30 минут на заблокированном экране без обрывов; работают остров и кнопки.
- Отказ → план Б: пересмотреть вывод звука.

## 2. Что сделано
- Ядро: `Player` (`crates/sirtone-mobile-ffi/src/player.rs`) — поток декодирования (symphonia + libopus из `sirtone-audio`) → кольцо `rtrb` → C-функция `sirtone_render`; заголовок `crates/sirtone-mobile-ffi/include/sirtone_render.h` входит в C-модуль xcframework.
- Приложение: `apps/mobile-ios/Sources/SirTone/PhonePlayer.swift` — AVAudioEngine, новый AVAudioSourceNode на каждый трек/перемотку в родном формате трека (interleaved Float32, микшер приводит к железу); блок рендера создаётся вне главного актора; аудиосессия `.playback`, `UIBackgroundModes: audio`; «Сейчас играет» и команды (play/pause/toggle/next/previous/позиция); пауза при прерывании и при снятии наушников; по кругу по папке приложения.
- Тестовые треки (сгенерированы ffmpeg, по 10 мин): FLAC 16 бит/44,1 кГц, FLAC 24 бит/96 кГц, Opus 160 кбит/с. Залиты по кабелю в Documents приложения (`ifuse --documents`, house_arrest) из WSL через ретранслятор к службе Apple.

## 3. Измерения
| Метрика | Цель | Результат | Как мерили |
|---|---|---|---|
| 30 мин на заблокированном экране | без обрывов | ✅ без обрывов | владелец, iPhone15,3, iOS 27.2 |
| Остров, экран блокировки, кнопки | работают | ✅ работают | владелец |
| Форматы | FLAC и Opus | ✅ FLAC 16/44, FLAC 24/96, Opus | список в приложении |

## 4. Наблюдения и сюрпризы
- xtool регистрирует приложение как `XTL-<TEAMID>.studio.serkov.sirtone.mobile` (префикс, а не суффикс `.<TEAMID>` из D-136) — хабу при своей подписи держать собственную схему; house_arrest ищет приложение по фактическому идентификатору.
- Код Swift собрался с первого раза в строгом режиме Swift 6 (без предупреждений).

## 5. Решение
Принято: вывод звука через AVAudioSourceNode + `sirtone_render` годится для М3. План Б не нужен.

## 6. Последствия для бэклога
- М3: плеер переносится на экраны DESIGN (S-экраны), обложки и теги — из медиатеки ядра (`Library`).
- М6/М7: заливка по кабелю через house_arrest работает — та же служба для переноса пакета (`afc`) в `sirtone-devicelink`.
