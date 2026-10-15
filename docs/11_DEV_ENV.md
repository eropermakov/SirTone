# 11. Среда разработки (что установлено и как пользоваться)

> Версия 1.3 · 2026-10-16 (уточнены команды запуска проверок) · установлено и проверено 2026-10-15. Машина: Windows 11 Home 10.0.26300, AMD Ryzen 7 5800H (8C/16T), 15,4 ГБ ОЗУ, Radeon Graphics (iGPU). Диски: **C: ~10 ГБ свободно** (мало), **D: ~133 ГБ**.

## 1. Установлено

| Компонент | Версия | Где | Как установлено |
|---|---|---|---|
| Rust (stable, MSVC) | **1.99.0** (cargo 1.99.0), `clippy`, `rustfmt` | `D:\DevTools\rustup`, `D:\DevTools\cargo` (переменные `RUSTUP_HOME`/`CARGO_HOME` — пользовательские) | официальный `rustup-init` (SHA256 проверен) |
| MSVC Build Tools 2022 | 17.14.41, toolset **14.44.35207** | `D:\DevTools\VSBuildTools` | `winget`, минимальные компоненты (VC.Tools.x86.x64 + Windows 11 SDK) |
| Windows SDK | **10.0.22621.0** | `C:\Program Files (x86)\Windows Kits\10` | вместе с Build Tools |
| CMake | **4.4.4** | `C:\Program Files\CMake` | `winget` |
| GitHub CLI (`gh`) | **2.102.0** | `C:\Program Files\GitHub CLI` | `winget` (**не авторизован** — OWN-04) |
| pnpm | **12.11.2** (store → `D:\DevTools\pnpm-store`) | `%APPDATA%\npm` | `npm i -g pnpm` |
| `cargo-deny` | **0.20.2** | `D:\DevTools\cargo\bin` | `cargo install --locked` |
| `cargo-nextest` | **0.9.148** | `D:\DevTools\cargo\bin` | `cargo install --locked` |
| Уже было | Node **24.19.0**, npm 11.17.0, Git 2.45.1, Python 3.12.3, **ffmpeg/ffprobe 9.0.1** (Gyan full), WebView2 Runtime **155.0.4283.45**, winget 1.29 | — | — |

Проверка: полный цикл «Tauri 2 + React-TS» собран (`tauri build --debug --bundles nsis`) за **2 мин 13 с**; установщик NSIS 2,5 МБ (debug). Каталог `target` debug-сборки ≈ 3,2 ГБ — держим проект на D:.

Системные параметры: LongPathsEnabled=1, Smart App Control выключен (0). **Исключения Defender не добавлялись** (это изменение настроек безопасности — решать вам): для скорости `cargo build` стоит добавить `D:\SirVault\SirTone\target`, `D:\DevTools\cargo`, `D:\DevTools\rustup`.

## 2. Как пользоваться

Новые оболочки получают PATH из реестра сами. В **уже запущенной** оболочке (например, в сессии Claude, открытой до установки) выполнить:

```powershell
. D:\SirVault\SirTone\scripts\dev-env.ps1
```

Контроль:

```powershell
rustc --version; cargo --version; pnpm --version; cmake --version; gh --version
cargo deny --version; cargo nextest --version
```

После появления каркаса (Этап 0): `pnpm install` · `pnpm tauri dev` · `cargo nextest run --workspace --no-tests=warn` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo deny check` · все проверки разом:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-all.ps1
```

Важно (проверено 2026-10-16): **`pwsh` (PowerShell 7) локально не установлен** — используется Windows PowerShell 5.1; политика выполнения скриптов на машине не задана (по умолчанию `Restricted`), поэтому скрипты запускаются с `-ExecutionPolicy Bypass` **только для одного процесса** — системные настройки не меняются. По той же причине в обычном терминале `pnpm` может не запуститься через `pnpm.ps1` — тогда вызывать `pnpm.cmd`. `cargo nextest` без тестов по умолчанию завершается с кодом 4 — на пустых крейтах Этапа 0 нужен `--no-tests=warn` (уже в `check-all.ps1`).

Прототипы спайков (`spikes/*`) — самостоятельные проекты вне workspace: `cargo test --release` и `cargo run --release` в их папке.

## 3. Ставится по мере необходимости (с разрешения владельца «установи сам»)

| Инструмент | Когда | Зачем |
|---|---|---|
| `fpcalc` (Chromaprint, официальный релиз) | S-06 | эталон отпечатков для паритет-теста |
| Python-окружение (`venv` на D:, PyTorch CPU) | S-07 | экспорт моделей в ONNX |
| Браузеры Playwright | Этап 2 | компонентные тесты UI |
| `tauri-driver` + `msedgedriver` | Этап 2/7 | E2E реального приложения |
| `cargo-about`, `cargo-fuzz`, `git-cliff`, gitleaks | Этапы 6–7 | уведомления о лицензиях, fuzz, changelog, сканер секретов |
| WebView2 Fixed Runtime (CAB) | S-13 | портативная сборка |
| NASM / LLVM(libclang) | только если потребует крейт | сейчас не нужны (SChannel вместо rustls/aws-lc) |

## 4. Заметки

- Временные каталоги разработки на D: (`D:\DevTools\target-cache\*`) можно удалять — это кэши сборки проверок.
- `D:\DevTools\smoke` (пробный проект) удалён после проверки.
- MCP-серверы проекта описаны в `.mcp.json` (версии зафиксированы); Figma-сервер (`https://mcp.figma.com/mcp`, подтверждено документацией Figma) потребует вашей авторизации через `/mcp`, когда дойдёт до макетов. Альтернатива от Figma — плагин `claude plugin install figma@claude-plugins-official` (MCP + навыки). По сторонним данным, Dev Mode MCP может требовать платный план Figma — проверить.
- Скилла `ui-ux-pro-max` из ТЗ в среде нет; для визуала используются `design-taste-frontend`, `shadcn`, `minimalist-ui`, `high-end-visual-design` (+ `impeccable`, `refero-design`).
