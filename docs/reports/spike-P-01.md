# Проверка П1 — xtool и Rust: SwiftUI-экран вызывает ядро на iPhone

- **Дата / сессия:** 2026-11-05, ветка `stage/8-mobile`
- **Связанные требования:** FR-MOB-001, FR-PLT-005 · **ADR:** D-137, D-138
- **Референсы (`01d`):** xtool, UniFFI, idevice

## 1. Вопрос и критерии (`docs/mobile/checks.md`)
- Успех: на iPhone виден ответ Rust-ядра, собранного и установленного из WSL.
- Отказ → план Б: xcframework из GitHub Actions (macOS-раннер).

## 2. Что сделано
- WSL2 Ubuntu 24.04 (`D:\WSL\Ubuntu-24.04`), Swift 6.4.2, Rust 1.99, xtool 1.24.0, iPhoneOS 27.1 SDK (`xtool sdk install` из Xcode 27.1.xip).
- Ядро `sirtone-mobile-ffi` под `aarch64-apple-ios`: `scripts/build-ios-core.sh`, только статическая библиотека (`cargo rustc --crate-type staticlib`) — у Linux нет линкера Apple для `.dylib`. C-зависимости (libopus через CMake, SQLite, libwebp, BLAKE3) собраны clang из swiftly с `SDKROOT` = iPhoneOS.sdk.
- Привязки Swift — из несжатой (`CARGO_PROFILE_RELEASE_STRIP=none`) сборки под Linux: `strip = "symbols"` профиля release вырезает метаданные UniFFI.
- Приложение `apps/mobile-ios` собрано `xtool dev build` из копии на ext4 (`~/build/mobile-ios`): на `/mnt/d` clang ломается на кэше модулей («module defined in both X and X»).
- Телефон в WSL — без захвата USB: ретранслятор `scripts/wsl-muxrelay.mjs` к службе Apple на Windows, `USBMUXD_SOCKET_ADDRESS`. Сопряжение остаётся у Windows; хаб ПК видит телефон одновременно.
- Подпись — бесплатный Apple ID владельца через `xtool dev run` (пароль и 2FA вводил владелец).

## 3. Измерения
| Метрика | Цель | Результат | Как мерили |
|---|---|---|---|
| Ответ ядра на экране | виден | «Rust 0.1.38 · aarch64-apple-ios · елка live» | глазами владельца, iPhone15,3, iOS 27.2 |
| Статическая библиотека ядра (debug-символы сняты не все) | — | 19 МБ | `ls` |
| Исполняемый файл приложения (debug) | — | 8,9 МБ | `stat` |
| Сборка приложения с нуля / повторно | — | 23 с / 3 с | `xtool dev build` |

## 4. Наблюдения и сюрпризы
- Виджет xtool 1.24 не собирает: расширение линкуется с `-e _NSExtensionMain`, код выбрасывается, zsign отказывается подписывать заглушку без `__text` (xtool-org/xtool#313, #379). Виджет убран из `xtool.yml`; П5 — через Xcode-проект на macOS-раннере.
- iOS требует **Режим разработчика** для приложения, подписанного бесплатным Apple ID: пункт появляется в настройках после установки, нужна перезагрузка и подтверждение кодом — совпадает с шагом 8 мастера (`docs/mobile/README.md` §6.1).
- Windows-служба Apple сначала не отдавала телефон (0 устройств 3 минуты); после переподключения кабеля — увидела.

## 5. Решение
Принято: цепочка WSL + xtool + UniFFI работает на реальном iPhone. План Б (xcframework из Actions) нужен только для виджета и для сборок без ПК.

## 6. Последствия для бэклога
- M1.3/M1.4: локальная сборка под iPhone рабочая; CI — по-прежнему ждёт scope `workflow`.
- П2 (фоновый звук) можно начинать: плеер ядра (`Player` + `sirtone_render`) подключается к AVAudioSourceNode.
- П5: нужен путь сборки виджета вне xtool.
