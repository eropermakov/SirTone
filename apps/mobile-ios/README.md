# Сиртон для iPhone

SwiftPM-проект (этап 8, `docs/mobile/README.md`). Ядро — Rust через UniFFI.

- Ядро и Swift-обёртки: `bash scripts/build-ios-core.sh` (из корня репозитория; на macOS или в WSL с SDK xtool) → `Frameworks/SirToneCoreFFI.xcframework`, `Sources/SirToneCore/`.
- Токены: `Sources/SirToneDesign/Tokens.swift` генерирует `node scripts/tokens.mjs --gen` — руками не правим.
- Разработка (WSL): `xtool dev` — собрать, поставить и запустить по кабелю.
- Релиз: GitHub Actions `mobile.yml` по тегу `mobile-v*` → неподписанный IPA; подписывает хаб ПК-Сиртона Apple ID владельца телефона.
