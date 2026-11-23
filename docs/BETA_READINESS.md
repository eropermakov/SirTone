# Готовность к публичной бете

Сверено: 2026-11-23. Этот документ различает работающий код, автоматическую проверку и ручную приёмку; отсутствие записи не означает разрешение на выпуск.

## Итог

**NOT READY.** Публичная бета заблокирована до устранения уязвимых транзитивных ZIP-зависимостей, macOS/iPhone-проверки и развёртывания production Feedback API. Выпуск, тег и push в рамках этой проверки не выполнялись.

## Подтверждено

- Фактическая линия: `stage/8-mobile`, исходный коммит аудита `948c436`; до работ она совпадала с `origin/stage/8-mobile`.
- На Windows уже были локальные rolling-журналы, panic-report и очистка старых журналов. D-180 теперь даёт стабильный Installation ID, Session ID на запуск, консервативный маркер незакрытой сессии, JSONL allow-list с лимитами, экспорт ограниченного ZIP, форму с локальным черновиком и добровольной HTTPS-отправкой.
- Есть локально тестируемый `services/feedback-api`: валидация, ZIP central-directory проверка без распаковки, rate limit, idempotency и SMTP simulator. iOS-код использует тот же JSONL/ZIP формат, форму, Share Sheet и `MFMailComposeViewController`; он не собран в этой Windows-среде.
- `pnpm typecheck`, `pnpm lint`, `pnpm test`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, тесты diagnostics и проверка IPC bindings прошли на рабочем ПК.
- `cargo nextest run --workspace --no-tests=warn` прошёл: 414 tests passed, 8 skipped. Debug NSIS bundle собран локально: `SirTone_0.2.7_x64-setup.exe`, SHA-256 `5EA53698106CAE3BE39D3E4E132A1CB48F45B023051520FF22F1FB4FFF42049F`; он ожидаемо **NotSigned** и не устанавливался.

## Блокеры

1. `cargo deny check` не проходит: RUSTSEC-2026-0545 и RUSTSEC-2026-0548 затрагивают `zip` 2.6.1 и 4.6.1; RUSTSEC-2026-0545…0548 — `zip` 8.6.0. Цепочки идут через `isideload`/`apple-codesign-quick` и `tauri-plugin-updater`. Нужен совместимый upgrade либо безопасное upstream-исправление с проверкой переноса iPhone и обновления Windows.
2. Production Feedback API не развёрнут: нет домена/HTTPS endpoint, настроенного persistent audit path/salt и SMTP credential в secret manager. Локальный тест не доказывает доставку в Gmail.
3. Нельзя подтвердить iOS-сборку или iPhone на физическом устройстве, установщик/обновление в изолированной среде и 24-часовое Windows-воспроизведение из этой среды.
4. Полный Playwright smoke пока не зелёный: новые IPC mocks и feedback-сценарий проходят, но визуальные baselines требуют отдельной ревизии, а не автоматического перезаписывания.

## Неотслеживаемые файлы

В worktree до начала работ уже были `.codex/` и `AGENTS.md`. Они не добавлялись и не менялись.

## Следующий безопасный шаг

Создать изолированную ветку обновления зависимостей, зафиксировать совместимые версии `isideload`, `apple-codesign` и Tauri, затем проверить `cargo deny`, тесты переносов и обновлений. Параллельно владелец разворачивает HTTPS Feedback API по `FEEDBACK_DELIVERY.md` и выполняет `MANUAL_ACCEPTANCE.md`.
