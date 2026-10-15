# 12. Руководство по реализации (как именно мы будем строить)

> Версия 1.0 · 2026-10-15 · дополняет `09_ROADMAP.md` (что и когда) и `13_BACKLOG.md` (задачи). Здесь — **процесс, конкретные настройки каркаса, шаблоны кода и схемы конфигураций**, чтобы на старте не принимать решений «на ходу».

---

## 1. Процесс работы с владельцем

### 1.1. Цикл доработок (перед стартом и в любой момент)

```
Владелец присылает доработки (в любой форме: текст, голос, скриншоты, Figma)
   → Claude: разбор — новое требование / изменение / уточнение визуала / вопрос
   → анализ влияния: какие FR, ADR, задачи, документы затронуты
   → правка документов: 02_TZ (+версия), 10_DECISIONS (ADR), 13_BACKLOG, 01* (если нужен новый референс — сначала исследование 6+ приложений)
   → запись в CHANGELOG.md (версия спецификации 1.0 → 1.1 …)
   → короткая сводка владельцу «что поменялось и почему» → подтверждение → работа
```

Правило: **ни одна доработка не реализуется в коде раньше, чем отражена в спецификации** (иначе следующая сессия её не увидит).

### 1.2. Цикл этапа

| Шаг | Что происходит | Артефакт |
|---|---|---|
| Старт | перечитать `CLAUDE.md`, раздел этапа в `09`, задачи в `13`, связанные FR | список задач этапа в task-листе |
| Задачи | по порядку зависимостей; каждая — тонкий вертикальный срез с тестами | код + тесты + обновлённые документы |
| Гейт задачи | `scripts/check-all.ps1` зелёный, DoD задачи (`09` §1) | — |
| Демо этапа | скриншоты/запись экрана, замеры бюджетов, отчёт | `docs/reports/stage-N.md` (шаблон — `docs/templates/stage-report.md`) |
| Приёмка | владелец смотрит демо, присылает замечания → цикл 1.1 | отметка в отчёте |

### 1.3. Сессии и параллельность

- **Одна основная сессия на этап** — она держит контекст этапа. Новый этап — новая сессия (контекст восстанавливается из документов).
- Параллельные сессии — только для независимых задач (например, спайки S-01 и S-05) и **только в отдельных git-worktree**, что требует хотя бы одного коммита (см. 1.4).
- Каждая сессия начинает с чтения `CLAUDE.md` → `00_INDEX.md` → раздела этапа.

### 1.4. Git ✅ (OWN-11 → D-36)

Владелец разрешил (2026-10-15): **локальный коммит после каждой готовой задачи** (гейты зелёные), Conventional Commits (`feat(audio): …`, `docs(spec): …`), в теле — ID задачи (`T1.3`) и FR; **push запрещён** до настройки GitHub (OWN-04) и отдельной команды. Ветка этапа `stage/N-<имя>`, слияние в `main` после приёмки этапа владельцем. Автор коммитов — локальная настройка репозитория (`user.name`/`user.email`), в конце сообщения — строка соавторства Claude.

---

## 2. Каркас Этапа 0 — конкретные настройки

### 2.1. Идентичность приложения ✅ (OWN-12 → D-37, утверждено владельцем)

| Поле | Значение | Почему важно |
|---|---|---|
| `productName` | `SirTone` (отображаемое имя — «Сиртон» из i18n) | имя exe и ярлыков |
| `identifier` | `studio.serkov.sirtone` | от него зависят папки данных, WebView2-профиль, обновления — **позже менять нельзя без миграции** |
| `publisher` | `Серков Studio` | установщик, свойства файла |
| Версия | `0.1.0` (SemVer) | обновления |

### 2.2. Rust workspace (`Cargo.toml`, корень)

```toml
[workspace]
resolver = "3"
members = ["crates/*", "apps/desktop/src-tauri"]

[workspace.package]
edition = "2024"
rust-version = "1.95"        # MSRV Tauri 2.12; тулчейн 1.99 закреплён в rust-toolchain.toml
license = "UNLICENSED"       # до решения OWN-01
publish = false

[workspace.dependencies]     # версии проверены 2026-10-15 (08 §5.1); обновлять осознанно
tauri = { version = "2.12", features = [] }
tauri-build = "2.7"
symphonia = { version = "0.6.1", features = ["aac", "alac", "isomp4", "mp3", "aiff", "caf"] }
symphonia-adapter-libopus = "0.3"
wasapi = "0.26"
rubato = "5.0"
rtrb = "0.4"
ebur128 = "0.1.10"
lofty = "0.25"
rusqlite = { version = "0.40", features = ["bundled"] }   # bundled SQLite с FTS5
rusqlite_migration = "2.6"
windows = { version = "0.62", features = [] }             # фичи — по крейтам
tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "sync", "time", "fs"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
async-trait = "0.1"          # для dyn-совместимых асинхронных трейтов (JobHandler)
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"
uuid = { version = "1.28", features = ["v7", "serde"] }
blake3 = "1.8"
crossbeam-channel = "0.5"
parking_lot = "0.12"
rayon = "1.12"
reqwest = { version = "0.13", default-features = false, features = ["native-tls", "json", "gzip"] }
proptest = "1.12"
insta = "1.49"
criterion = "0.8"

[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
unused_must_use = "deny"

[workspace.lints.clippy]
unwrap_used = "warn"         # в тестах разрешено через #![cfg_attr(test, allow(...))]
expect_used = "warn"
dbg_macro = "deny"
todo = "deny"
print_stdout = "warn"
large_futures = "warn"

[profile.dev.package."*"]
opt-level = 2                # зависимости (декодеры, DSP) оптимизированы даже в debug — иначе звук «хрипит»

[profile.release]
lto = "thin"
codegen-units = 1
opt-level = 3
panic = "unwind"             # ВАЖНО: шаблон Tauri ставит "abort", но нам нужен catch_unwind вокруг декодеров
strip = "symbols"            # PDB с символами сохраняем в артефактах CI для разбора крэшей
```

`rust-toolchain.toml`: `channel = "1.99.0"`, `components = ["clippy", "rustfmt"]`, `targets = ["x86_64-pc-windows-msvc"]`.
`rustfmt.toml`: `edition = "2024"`, `max_width = 100`, `use_field_init_shorthand = true`.

### 2.3. `deny.toml` (лицензии и источники)

```toml
[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
         "ISC", "Zlib", "Unlicense", "CC0-1.0", "BSL-1.0", "MPL-2.0", "Unicode-3.0", "Unicode-DFS-2016"]
confidence-threshold = 0.9
[[licenses.exceptions]]               # пример: если появится файл без LICENSE — фиксируем явно
allow = ["MIT"]
crate = "rusty-chromaprint"
[bans]
multiple-versions = "warn"
wildcards = "deny"
[advisories]
yanked = "deny"
[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

### 2.4. Фронтенд

`pnpm-workspace.yaml`: `packages: ["apps/*", "tools/*"]`. Корневой `package.json` — скрипты: `dev`, `build`, `tauri`, `lint` (`biome check .` + `eslint` только для правила i18n), `typecheck` (`tsc -b`), `test` (`vitest run`), `e2e` (`playwright test`), `gen:ipc` (запуск экспорта биндингов), `fixtures`.

Зависимости (точные версии — `08` §5.2): react/react-dom 19.3, vite 8.3, @vitejs/plugin-react 6.1, typescript ~6.0.3, tailwindcss + @tailwindcss/vite 4.3, shadcn 4.21 (CLI), radix-ui 1.8, @tauri-apps/api 2.12 + плагины, zustand 5, @tanstack/react-virtual 3.14, motion 14.2, @dnd-kit/core 6.3, i18next 26.4 + react-i18next 17 + i18next-icu, lucide-react, butterchurn 2.6.7 (+ отобранные пресеты), vitest 5, @playwright/test 1.64, @biomejs/biome 2.5.

`tsconfig`: `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `verbatimModuleSyntax`, пути `@/shared/*`, `@/features/*`, `@/entities/*`.

### 2.5. `tauri.conf.json` — ключевые поля

```jsonc
{
  "productName": "SirTone", "identifier": "studio.serkov.sirtone", "version": "0.1.0",
  "app": {
    "windows": [{
      "label": "main", "title": "SirTone", "maximized": true, "visible": false,
      "dragDropEnabled": true, "minWidth": 960, "minHeight": 600, "decorations": true
    }],
    "security": {
      "csp": "default-src 'self'; img-src 'self' http://sirtone-cover.localhost data: blob:; media-src 'self' http://sirtone-bg.localhost blob:; connect-src ipc: http://ipc.localhost; script-src 'self'; style-src 'self' 'unsafe-inline'",
      "freezePrototype": true
    }
  },
  "bundle": {
    "active": true, "targets": ["nsis"], "publisher": "Серков Studio",
    "windows": { "webviewInstallMode": { "type": "embedBootstrapper" },
                 "nsis": { "installMode": "currentUser" } }
  }
}
```

Updater (`plugins.updater.pubkey`, `bundle.createUpdaterArtifacts`) включается на Этапе 6. Окна виджетов создаются из Rust динамически (`WebviewWindowBuilder`), у каждого свой файл `capabilities/<label>.json` с минимальным набором разрешений (`core:event`, свои команды).

---

## 3. Шаблоны кода (соглашения)

### 3.1. Именование

| Что | Правило | Пример |
|---|---|---|
| Крейты | `sirtone-<домен>` | `sirtone-library` |
| Команды IPC | `<домен>_<глагол>[_<объект>]`, snake_case | `library_list_tracks` |
| События | `<домен>://<имя>` | `library://changed` |
| Ключи i18n | `<раздел>.<экран>.<элемент>` | `collection.overview.total` |
| Коды ошибок | PascalCase enum → ключ `errors.<Code>` | `UnsupportedFormat` → `errors.UnsupportedFormat` |
| Миграции | `NNNN_описание.sql` | `0002_add_env_tail.sql` |
| Задачи (jobs) | `snake_case` kind | `analyze_basic` |
| Токены дизайна | `категория.группа.имя` (DTCG) | `color.surface.1`, `motion.duration.element` |

### 3.2. Команда IPC (Rust → TS)

```rust
/// Возвращает страницу треков для текущего фильтра (keyset-пагинация).
#[tauri::command]
#[specta::specta]
pub async fn library_list_tracks(
    state: tauri::State<'_, AppState>,
    query: TrackQuery,
    page: PageCursor,
) -> Result<Page<TrackRow>, AppError> {
    state.library.list_tracks(query, page).await.map_err(AppError::from)
}
```

На фронтенде вызов только через сгенерированный `shared/ipc/bindings.ts` и тонкую обёртку с кэшем страниц; прямой `invoke('…')` запрещён.

### 3.3. Ошибки

```rust
#[derive(Debug, thiserror::Error)]          // в каждом крейте свой enum
pub enum LibraryError {
    #[error("path too long: {0}")] PathTooLong(String),
    #[error(transparent)] Db(#[from] rusqlite::Error),
}
impl From<LibraryError> for AppError {     // в sirtone-app: маппинг на стабильный код
    fn from(e: LibraryError) -> Self {
        match e {
            LibraryError::PathTooLong(_) => AppError::new(ErrorCode::PathTooLong).with_details(&e),
            LibraryError::Db(_) => AppError::new(ErrorCode::DbBusy).retryable(),
        }
    }
}
```

### 3.4. События и шина

`EventBus::emit(Event)` — неблокирующая отправка в ограниченный канал; поток-диспетчер **склеивает** частые события (позиция ≤ 4 Гц, прогресс ≤ 10 Гц, `library://changed` с дебаунсом 150 мс и объединением диапазонов id) и вызывает `app.emit_to(label, …)` для нужных окон. Бинарные потоки — `tauri::ipc::Channel` (по документации Tauri события не предназначены для высокой частоты).

### 3.5. Фоновая задача

```rust
#[async_trait::async_trait]
pub trait JobHandler: Send + Sync {
    const KIND: &'static str;
    type Payload: serde::Serialize + serde::de::DeserializeOwned + Send;
    fn needs_network(&self) -> bool { false }
    async fn run(&self, ctx: &JobCtx, payload: Self::Payload) -> Result<JobOutcome, JobError>;
}
// JobOutcome::{Done, Retry{after}, Fail{reason}}; идемпотентность обязательна; отмена — ctx.cancelled()
```

### 3.6. Аудио RT-путь

- Колбэк вывода: `fn render(&mut self, out: &mut [f32])` — только `rtrb::Consumer::read_chunk` + множители; счётчики underrun — `AtomicU32`.
- Любая структура, используемая в RT, создаётся заранее (`with_capacity`), передаётся через `rtrb`/`arc-swap`; в RT-модуле `#![deny(clippy::unwrap_used)]` и запрет `Vec::push`/`Box::new` (ревью + `assert_no_alloc` в debug-сборке).

### 3.7. Логи

`tracing` с целями `sirtone::audio`, `sirtone::library`, `sirtone::net`…; `info` — события жизненного цикла, `debug` — подробности; **в info/warn никаких путей пользователя и названий треков** (только id); spans для замеров: `startup.phase`, `import.file`, `analysis.track`.

### 3.8. Фронтенд-модуль

```
features/<feature>/
  index.ts            # публичный API фичи
  api.ts              # обёртки над bindings + кэш
  store.ts            # zustand-срез (только UI-состояние)
  components/…tsx     # только токены, только i18n
  hooks/…ts
  <feature>.test.tsx
```

Правила: компоненты — именованный экспорт; никаких `useEffect` для данных, которые даёт событие ядра; списки — только через `shared/virtual`.

### 3.9. Тесты

Rust: юнит рядом с кодом, интеграционные — `crates/*/tests/`, снапшоты `insta`, свойства `proptest` (санитайзер, парсер, `norm`), бенчмарки `criterion` (`benches/`). Фронтенд: `*.test.tsx` (Vitest + Testing Library), `e2e/*.spec.ts` (Playwright с `mockIPC`), скриншоты — `e2e/visual/`. Фикстуры — `fixtures/generated/` (не в git, генерируются скриптом).

---

## 4. Схемы конфигураций и ассетов

Все файлы версионированы полем `"version"`; меняются без перекомпиляции (кроме встроенных значений по умолчанию).

### 4.1. `config/recognition.toml`

```toml
version = 1
[zones]               # 05 §4.4
strong_max_distance = 0.06
medium_max_distance = 0.25
min_components_for_strong = 3
acoustid_min_score = 0.85
acoustid_trust_score = 0.95
length_tolerance_s = 2.0
[weights]
acoustid_score = 5.0
mb_recording_id = 5.0
artist = 3.0
title = 3.0
album = 3.0
length = 2.0
track_no = 1.0
year = 1.0
[long_files]          # FR-IMP-007
always_flag_minutes = 25
flag_if_unidentified_minutes = 15
[filename]
separators = [" - ", " – ", " — ", " − "]
artist_separators = [" feat. ", " ft. ", " featuring ", " & ", ", ", " x ", " vs. ", " / ", " и ", " with "]
junk_values = ["unknown", "unknown artist", "track", "audio track", "исполнитель", "название"]
junk_brackets = ["official video", "lyrics", "hd", "audio", "320kbps", "www."]
[folder_consensus]
min_files = 3
min_share = 0.6
[mojibake]
min_latin1_share = 0.6
min_cyrillic_after = 0.7
```

### 4.2. `assets/vibe/moods.json`, `activities.json`, `context.json`

```jsonc
// moods.json — прототипы на плоскости valence/arousal (04 §10.2)
{ "version": 1, "sigma": 0.22, "min_membership": 0.45, "min_margin": 0.15,
  "moods": { "happy": { "v": 0.80, "a": 0.70 }, "upbeat": { "v": 0.65, "a": 0.85 },
             "calm": { "v": 0.60, "a": 0.20 }, "sad": { "v": 0.20, "a": 0.25 },
             "aggressive": { "v": 0.20, "a": 0.85, "aggression_min": 0.5 } },
  "positive_min_v": 0.62, "negative_max_v": 0.38, "hysteresis": 0.03 }

// activities.json — трапеции [a, b, c, d] по признакам
{ "version": 1,
  "activities": {
    "workout": { "bpm": [110, 120, 180, 190], "energy": [0.5, 0.6, 1, 1], "arousal": [0.5, 0.6, 1, 1] },
    "sleep":   { "bpm": [0, 0, 85, 95], "energy": [0, 0, 0.3, 0.4], "vocal": [0, 0, 0.5, 0.6] },
    "work":    { "arousal": [0.2, 0.25, 0.6, 0.7], "vocal": [0, 0, 0.35, 0.45] },
    "road":    { "bpm": [85, 90, 140, 150], "valence": [0.4, 0.45, 1, 1] },
    "relax":   { "arousal": [0, 0, 0.4, 0.5], "valence": [0.35, 0.4, 1, 1], "acousticness": [0.4, 0.5, 1, 1] },
    "party":   { "danceability": [0.6, 0.65, 1, 1], "energy": [0.6, 0.7, 1, 1], "valence": [0.5, 0.55, 1, 1] } },
  "workout_tempo": { "light": [110, 130], "medium": [130, 160], "intense": [160, 185] } }

// context.json — сдвиги вайба от контекста (05 §10.2); погода — коды WMO Open-Meteo
{ "version": 1,
  "daypart": { "night": { "arousal": -0.2 }, "morning": { "arousal": 0.1, "valence": 0.1 } },
  "weather": { "rain": { "valence": -0.1, "arousal": -0.1, "acousticness": 0.1 }, "clear": { "valence": 0.15 },
               "fog": { "arousal": -0.2 }, "snow": { "arousal": -0.1 } },
  "wmo_map": { "0": "clear", "1": "clear", "2": "cloudy", "3": "overcast", "45": "fog", "48": "fog",
               "51-57": "drizzle", "61-67": "rain", "71-77": "snow", "80-82": "rain", "95-99": "storm" } }
```

### 4.3. `assets/lexicon/ru.json` (смысловой поиск, `05` §9)

```jsonc
{ "version": 1, "stopwords": ["что-то", "что", "нибудь", "и", "для", "под", "мне", "хочу", "включи"],
  "entries": [ { "stems": ["грустн", "печальн", "тоскл"], "facet": "valence", "op": "lt", "value": 0.35, "w": 1.0 },
               { "phrase": "без слов", "facet": "vocal", "op": "lt", "value": 0.3, "w": 1.0 },
               { "phrase": "для сна", "facet": "activity", "value": "sleep", "w": 1.0 } ] }
```

### 4.4. `assets/eq/presets.json`

`{ "version": 1, "bands_hz": [31.25,62.5,125,250,500,1000,2000,4000,8000,16000], "presets": { "flat": [0,0,0,0,0,0,0,0,0,0], "bass_booster": [6,5,4,2,0,0,0,0,0,0], … } }` — значения тюнингуются на слух (Этап 4).

### 4.5. `design-tokens.json` (DTCG; значения — от владельца)

Обязательные группы: `color.surface.{0..3}`, `color.text.{1..3}`, `color.border.{default,strong}`, `color.accent.{50..950}` (генерируется), `color.semantic.{success,warning,danger,info}`, `radius.{xs,sm,md,lg,xl,full}`, `space.{0..12}`, `font.family.{ui,display,mono}`, `font.size.{xs..4xl}`, `font.weight.{regular,medium,semibold,bold}`, `shadow.{1..3}`, `blur.{sm,md,lg}`, `motion.duration.{micro,element,screen,hero}`, `motion.easing.{standard,emphasized,exit}`, `z.{base,overlay,modal,toast}`. Темы: `light` / `dark` переопределяют только `color.*`. До макетов — нейтральные значения (серые, системный шрифт).

### 4.6. Локализация

`assets/i18n/<lang>/<namespace>.json`, пространства имён: `common, nav, wave, viz, collection, playlists, search, unknown, import, duplicates, settings, onboarding, widgets, tray, errors`. Русский — полный; английский — те же ключи с пустыми значениями (fallback на русский). Плюрали — ICU: `"{count, plural, one {# трек} few {# трека} many {# треков} other {# трека}}"`.

### 4.7. Каталог кодов ошибок → тексты

`FileNotFound · UnsupportedFormat · DecodeFailed · PathTooLong · DiskFull · PermissionDenied · DeviceUnavailable · DeviceBusy · NetworkOffline · RateLimited · RemoteError · DbBusy · DbCorrupt · Cancelled · InvalidArgument · LibraryOffline · Internal` — для каждого ключ `errors.<Code>.title` и `errors.<Code>.hint` (что делать пользователю).

### 4.8. Настройки (`settings.json`)

Схема v1 — `03` §9. Правила: при загрузке неизвестные поля сохраняются; недостающие — по умолчанию; при повышении версии — функция миграции `migrate_vN_to_vN+1`; запись атомарная (temp + rename), не чаще 1 раза/с.

---

## 5. Шаблоны документов

- Отчёт спайка — `docs/templates/spike-report.md`
- Отчёт этапа — `docs/templates/stage-report.md`
- Протокол ручной проверки на железе — `docs/templates/qa-protocol.md`
- Запись решения (ADR) — `docs/templates/adr.md`
- Журнал изменений спецификации — `docs/CHANGELOG.md`

---

## 6. Готовность к старту (Definition of Ready)

| Пункт | Статус |
|---|---|
| Спецификация (ТЗ, архитектура, референсы, дорожная карта, решения) | ✅ готова (v1.0) |
| Детальный бэклог Этапов 0–7 с трассировкой к FR | ✅ `13_BACKLOG.md` |
| Тулчейн установлен и проверен сборкой Tauri | ✅ `11_DEV_ENV.md` |
| Конфигурации каркаса определены (§2) | ✅ |
| Шаблоны отчётов и протоколов | ✅ `docs/templates/` |
| Исходное ТЗ владельца сохранено в проекте | ✅ `docs/source/` |
| **Доработки владельца внесены** | ⏳ ждём ваш список (цикл §1.1) |
| OWN-11 (локальные коммиты) и OWN-12 (идентификатор приложения) | ✅ решены владельцем (D-36, D-37) |
| Визуал (OWN-07) | не блокирует Этапы 0–1 |
| Ключ AcoustID, контакт MB (OWN-02/03) | не блокируют до Этапа 3 |
| GitHub, подпись (OWN-04/05) | не блокируют до Этапа 6 |
