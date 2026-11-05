# Сиртон для Android — заготовка

Приложение пишем позже отдельным этапом; решения — [`docs/mobile/android.md`](../../docs/mobile/android.md).

Уже есть:
- `design/Tokens.kt` — токены (генерирует `node scripts/tokens.mjs --gen`, руками не правим);
- CI `mobile.yml` (задача `android-core`): ядро `sirtone-mobile-ffi` собирается под arm64 через `cargo-ndk`, Kotlin-обёртки UniFFI генерируются.
