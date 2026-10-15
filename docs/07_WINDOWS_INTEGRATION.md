# 07. Интеграция с Windows: окно, трей, хоткеи, SMTC, панель задач, виджеты, Bluetooth, питание, установка

> Версия 1.3 · 2026-10-16 · основание — `01b` F-22…F-27, F-29, F-30 и `01c` §12 (Microsoft Learn, Spotify/Discord/foobar2000/VLC/MusicBee, Rainmeter, Wallpaper Engine, Lively, ToothTray, Tauri, Kodi, Audible). Всё, что зависит от конкретной сборки Windows и железа, помечено 🧪 и закрывается спайками S-09…S-14, S-18 на машине владельца (Win11 10.0.26300, Modern Standby).

---

## 1. Окно, трей, закрытие, запуск (F-22)

| Тема | Решение | Копия у |
|---|---|---|
| Окно | `maximized: true`, `visible: false` до готовности (без белой вспышки), фон `backgroundColor` = токен; **`dragDropEnabled: true`** (нужно для путей файлов из проводника; HTML5-DnD на Windows при этом не работает ✔ docs Tauri — поэтому внутренний DnD на pointer-событиях `dnd-kit`) | Spotify, Tauri docs |
| Закрытие | Перехват `CloseRequested` → скрыть; тот же путь для Alt+F4 и «Закрыть» из Alt+Tab (**без «исключений»**); выход — меню трея; при выходе — корректная остановка (flush БД, закрытие WASAPI, снятие хоткеев) | Spotify/Discord (минус: Alt+F4) |
| Трей | `TrayIconBuilder`; метки меню (Русский) **кэшируются в Rust** (нужны, когда WebView главного окна уничтожен для экономии памяти); левый клик — открыть/скрыть, правый — меню: «Открыть», «Играет: …», ⏯, ⏮, ⏭, **«Сон 30 мин» / «Сон: осталось N мин · +15 · Отменить»**, **«Фокус: работа 12:30»**, **строка предложения события** («Скоро Новый год — включить волну»), «Выход»; подсказка иконки — остаток фокуса/сна | Spotify, Discord, `01c` W-08/W-09 |
| Один экземпляр | `tauri-plugin-single-instance`: второй запуск показывает окно и передаёт аргументы (Jump List-команды, «Открыть с помощью») | — |
| Автозапуск | `tauri-plugin-autostart` (HKCU Run) с `--minimized`; отключён в портативном режиме (путь флешки нестабилен) | foobar2000 portable |
| Экономия памяти | Сразу при скрытии: `ICoreWebView2Controller.IsVisible = false` + `ICoreWebView2_3::TrySuspend` (доступ через `with_webview` Tauri); через 60 с в трее WebView `main` уничтожается; ядро, трей, хоткеи, SMTC, виджеты работают; открытие — пересоздание ≤ 400 мс со скелетоном (D-52) | 🔧 (урок LUMEN о памяти WebView; Microsoft Learn — `01c` §12) |
| DPI/мониторы | Per-Monitor v2 (по умолчанию в Tauri); виджеты запоминают монитор и координаты в долях | — |
| Экран не засыпает | `SetThreadExecutionState(ES_CONTINUOUS|ES_DISPLAY_REQUIRED)` в «ночном вайбе» и пока играет с видимой визуализацией; **снимается** в последние 10 мин сон-таймера и после него (§8.1) | WMP/VLC |

---

## 2. Горячие клавиши и медиаклавиши (F-23, F-24)

- **Глобальные хоткеи:** `tauri-plugin-global-shortcut` (RegisterHotKey) — play/пауза, следующий, предыдущий, громкость ±5 %, mute (по умолчанию `Ctrl+Alt+Space/→/←/↑/↓`); у каждого действия флаг «глобальный» (как foobar2000/VLC/MusicBee); при отказе регистрации — событие `platform://hotkey-conflict` и подсказка в настройках; **применение без перезапуска** (улучшение относительно VLC).
- **Громкость хоткеями** меняет только громкость Сиртона ✅.
- **Медиаклавиши** приходят через **SMTC** (системная маршрутизация: получает «текущая» медиа-сессия — последняя начавшая воспроизведение); опциональный тумблер «Перехватывать медиаклавиши напрямую» (RegisterHotKey для `VK_MEDIA_*`), если другое приложение «крадёт» клавиши.

---

## 3. SMTC — системные медиа-контролы (F-24)

- Реализация вручную (по документации Microsoft: автоинтеграция только для `MediaPlayer`; для собственного движка — `ISystemMediaTransportControlsInterop::GetForWindow`) — крейт **`souvlaki` 0.8.3 (MIT)**: на Windows **требует HWND открытого окна**; `MediaMetadata { title, album, artist, cover_url, duration }`, `cover_url` вида `file://…` загружается автоматически (проверено по docs.rs). Если скрытое окно не подойдёт — запасной путь: напрямую `windows`-крейт (`ISystemMediaTransportControlsInterop` + `SystemMediaTransportControlsDisplayUpdater`), ~200 строк.
- **Скрытое служебное окно** в потоке платформы (message loop) — владелец SMTC, независимое от WebView `main` (иначе SMTC исчезает при уничтожении WebView) 🧪 S-10.
- Метаданные: название, исполнитель, альбом, длительность; **обложка** — файл 512 px в `cache/smtc/current.jpg` (URI `file://`); позиция/состояние — обновление ≤ 1 Гц и по событиям; команды: Play/Pause/Next/Previous/Stop/Seek.
- Бонус: Windows 11 media flyout, экран блокировки, **совместимость с Rainmeter и сторонними виджетами** (сторонний плагин `MediaPlayer.dll` читает Windows media API ○) — «бесплатно».

---

## 4. Панель задач (F-25; Microsoft Learn Taskbar Extensions ✔)

| Элемент | Реализация | Ограничения |
|---|---|---|
| **Кнопки в превью** (⏮ ⏯ ⏭) | `ITaskbarList3::ThumbBarAddButtons` после сообщения `TaskbarButtonCreated`; обработка `THBN_CLICKED` через subclass (`SetWindowSubclass`) окна; иконки HICON из ресурсов (светлая/тёмная); обновление иконки play/pause через `ThumbBarUpdateButtons` | Максимум **7** кнопок, обрезаются справа налево — важное первым |
| **Прогресс** на кнопке | `SetProgressState`/`SetProgressValue` ≤ 2 Гц; пауза — `TBPF_PAUSED` | нужно окно с кнопкой на панели задач — при скрытом в трей `main` кнопки нет (прогресс/кнопки живут, пока окно показано; вне окна — SMTC/мини-бар/виджет) |
| **Overlay** play/пауза | `SetOverlayIcon` (опционально) | единый на группу |
| **Jump List** | Tasks: «Воспроизвести/Пауза», «Следующий», «Предыдущий» → запуск `SirTone.exe --cmd=…` → single-instance передаёт в ядро | список задач — статичный (рекомендация Microsoft) |
| **Мини-бар у панели задач** (опция) | Лёгкое окно (`widget-taskbar`): topmost, frameless, `skipTaskbar`; позиция — свободная область панели задач (`SHAppBarMessage(ABM_GETTASKBARPOS)`), следует за изменениями; скрывается при полноэкранных приложениях; Win10: можно попытаться встроить; **Win11: Deskband невозможен** ✔ | z-order с панелью задач Win11 нестабилен (мерцание) 🧪 S-10; запасной вариант — «плавающий» мини-бар над панелью |

Итог по вопросу 10 (✅ решение): «виджет на панели задач» = превью-кнопки + прогресс/overlay + Jump List + SMTC + опциональный мини-бар.

---

## 5. Виджет на рабочем столе (F-26; Rainmeter / Wallpaper Engine / Lively)

**Требования ТЗ ✅:** отдельный настраиваемый виджет (не обычное окно), крупнее, обложка + управление + (по пресету) мини-визуализация; **скрывается, когда поверх открывается другое окно**.

### 5.1. Варианты слоя (выбор — спайк S-09 на сборке 26300)

| Вариант | Идея | Плюсы | Риски |
|---|---|---|---|
| **A. Bottom-most top-level** | `HWND_BOTTOM`, `WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE`, `alwaysOnBottom`, `skipTaskbar`, прозрачный; удерживать в самом низу (`WM_WINDOWPOSCHANGING`) | Просто; «Rainmeter On Desktop» | Win+D (Показать рабочий стол) может скрыть; клики поднимают окно |
| **B. Дочернее окно Progman/WorkerW** | «Трюк обоев»: `SendMessageTimeout(Progman, 0x052C)` → найти `WorkerW` (без/с `SHELLDLL_DefView`) → `SetParent` | Остаётся при Win+D; живёт «на обоях» | Меняется между сборками (24H2+); WebView2 как дочерний к WorkerW — 🧪; z-order относительно иконок |
| **C. Закрепить поверх** (запасной режим пользователя) | topmost без активации | Всегда виден | Не соответствует «скрывается под окнами» → только как опция |

Общее: прозрачность per-pixel (`transparent: true`), `shadow: false`, `focus: false`, hit-test: прозрачные области — сквозные клики; перетаскивание за обложку; привязка к краям; масштаб 0,75–1,5; прозрачность; запоминание монитора/позиции; **переживает перезапуск explorer.exe** (реакция на `TaskbarCreated` → пересоздать привязку); **приостановка рендера** (Wallpaper Engine/Lively): виджет перекрыт ≥ 95 %, полноэкранное приложение, батарея ⇒ пауза анимаций/мини-визуализации (экономия GPU).
Страница `widget-desktop` ≤ 100 КБ JS, подписка на события ядра; клик по обложке открывает приложение. **(1.3)** Кнопка «Сон 30 мин» и кольцо фокуса (остаток, фаза) — `timer://*`; живых эффектов в виджете нет (Q-8).

---

## 6. Bluetooth (F-27)

**Что нужно ✅:** список сопряжённых устройств, быстрое подключение/отключение, доп. настройки; работает с системным Bluetooth, не заменяет его.

| Шаг | Реализация |
|---|---|
| Список | WinRT `DeviceInformation.FindAllAsync(BluetoothDevice.GetDeviceSelectorFromPairingState(true))` + `BluetoothDevice.ConnectionStatus`/`ConnectionStatusChanged`; фильтр «аудио» по классу устройства (Audio/Video, A2DP sink service UUID) |
| **Подключить/отключить — путь 1 (копия ToothTray ✔)** | Найти аудио-эндпоинт устройства (`IMMDeviceEnumerator`, свойство шины/перечислителя `BTHENUM`/BluetoothAudio); получить `IDeviceTopology` → connector → `IKsControl`; отправить `KSPROPSETID_BtAudio` + `KSPROPERTY_ONESHOT_RECONNECT` / `KSPROPERTY_ONESHOT_DISCONNECT` |
| Путь 2 (запасной) | `BluetoothSetServiceState` для A2DP Sink `{0000110B-0000-1000-8000-00805F9B34FB}` и Handsfree `{0000111E-0000-1000-8000-00805F9B34FB}` — **ставит/снимает драйвер**, возможна ошибка 87, может потребовать повышения 🧪 |
| Путь 3 (крайний) | Открыть системное окно: `ms-settings:bluetooth` |
| Вывод звука | В приложении выбрать **рендер-эндпоинт** устройства (не меняя системное «по умолчанию»); ждать появления эндпоинта через `IMMNotificationClient` (до 10 с) |
| Win11 нюанс | HFP и A2DP объединены в один эндпоинт, профиль выбирается автоматически (учесть в UI) |
| Тесты (S-12) | 3 устройства владельца: подключение ≤ 10 с, отключение, повторное, потеря связи во время воспроизведения (пауза), Win10 если доступен |

Референсы по теме (5 < 6, признано): ToothTray, StarTooth, AudioPlaybackConnector (обратное направление), EasyBluetoothAudio (приёмник), документация Windows.

---

## 7. Монитор чужого звука для приглушения (F-04)

Поток платформы (COM MTA): `IMMDeviceEnumerator` → устройство вывода → `IAudioSessionManager2` → `GetSessionEnumerator`; подписка `IAudioSessionNotification` на новые сессии; для каждой сессии — `IAudioSessionControl2` (`GetProcessId`, `IsSystemSoundsSession`), `IAudioMeterInformation::GetPeakValue` (опрос 50 мс). Дерево процессов Сиртона — снимок `CreateToolhelp32Snapshot` (обновляется при появлении/исчезновении WebView-процессов). Решение «приглушать» и огибающая — `04` §7. Лог в debug: PID, имя exe, пик, решение.

### 7.1. Звонки (D-49, FR-PLAY-011, FR-FOCUS-007)

- Своя сессия: `IAudioSessionControl2::SetDuckingPreference(TRUE)` — Windows не приглушает Сиртон на 80 % при звонках сама (иначе двойное приглушение с нашим).
- `IAudioSessionManager2::RegisterDuckNotification` (`IAudioVolumeDuckNotification`, Win7+): `OnVolumeDuckNotification` — открыт коммуникационный поток (звонок в Teams/Discord/Telegram и т.п.) → наше приглушение или **пауза** (опция «Пауза во время звонка»); `OnVolumeUnduckNotification` → снять приглушение / возобновить через 2 с, если паузу поставили мы. Регистрация — не из колбэка (требование Microsoft).
- Проверка на 20 сценариях спайка S-11 (ложных срабатываний 0).

---

## 8. Питание, система, съёмные носители

| Событие | API | Реакция |
|---|---|---|
| Батарея / экономия заряда | `GetSystemPowerStatus` (+ `PowerSettingRegisterNotification`) | авто-понижение уровня; пауза фонового анализа < 30 % |
| Троттлинг CPU | `CallNtPowerInformation(ProcessorInformation)` (CurrentMhz vs MaxMhz) | сигнал автоснижения (`06` §6.2) |
| Полноэкранное приложение/игра | `SHQueryUserNotificationState` | пауза визуализации/виджета/анализа |
| Сон/пробуждение | `WM_POWERBROADCAST` | при пробуждении — переоткрыть устройство вывода, проверить корень библиотеки |
| Смена устройств/дисков | `WM_DEVICECHANGE` / `RegisterDeviceNotification` | вынута флешка → библиотека «офлайн» (`05` §7) |
| Смена сети | `INetworkListManager` | `net://status` |
| Смена DPI/мониторов | `WM_DPICHANGED`, `WM_DISPLAYCHANGE` | пересчёт размеров, позиций виджетов |
| Анимации Windows выкл. | `SPI_GETCLIENTAREAANIMATION` → `prefers-reduced-motion` в WebView2 (Chromium, обновление по `WM_SETTINGCHANGE`) | эффекты — статично/выкл., анимации — уровень 0 (FR-UI-008) |

### 8.1. Сон-таймер: экран, сон ПК, «проснулся» (FR-SLEEP-006/007, D-48, спайк S-18)

| Действие | Реализация | Ограничения |
|---|---|---|
| Возможности ПК | `GetPwrCapabilities` → `SYSTEM_POWER_CAPABILITIES`: `SystemS3`, `AoAc` (Modern Standby), `SystemS4` + `HiberFilePresent` | у владельца: S0 Low Power Idle, S3 нет, гибернация есть (`powercfg /a`, 2026-10-16) |
| Отпустить систему | за 10 мин до конца и после — снять наши `SetThreadExecutionState` (`ES_DISPLAY_REQUIRED`/`ES_SYSTEM_REQUIRED`) | иначе экран/ПК не уснут по плану электропитания |
| Выключить экран | `SendMessageW(hwnd_service, WM_SYSCOMMAND, SC_MONITORPOWER, 2)` (обрабатывает `DefWindowProc`) | на Modern Standby погашенный экран + отсутствие активности → система уходит в standby сама; 🧪 S-18 |
| Перевести ПК в сон | S3: `SetSuspendState(FALSE, FALSE, FALSE)` после `AdjustTokenPrivileges(SE_SHUTDOWN_NAME)`; **Modern Standby: `SetSuspendState` возвращает `ERROR_NOT_SUPPORTED`** → выключить экран + отпустить систему (как выше) | 🧪 S-18: проверить время ухода в standby на ноутбуке владельца |
| Гибернация (Q-17) | `SetSuspendState(TRUE, FALSE, FALSE)` — только если `SystemS4 && HiberFilePresent` и выбрано явно | показывается, только если доступна |
| «Проснулся» | во время затухания — опрос `GetLastInputInfo` 1 Гц (любой ввод в системе, без хуков) + события приложения/хоткеев/трея | вне затухания не опрашиваем |

Все действия после таймера — **только явной настройкой** ✅ (по умолчанию «Ничего»); пауза воспроизведения — всегда перед действием; приложение не требует прав администратора (привилегия `SE_SHUTDOWN_NAME` есть у обычного пользователя, её надо только включить в токене).

---

## 9. Установка, обновления, портативность

### 9.1. Установщик и WebView2 (✔ Tauri docs)

| Вариант | Сборка | Размер (+) | Интернет |
|---|---|---|---|
| **Установщик (основной)** | NSIS, `installMode: currentUser` (без UAC), `webviewInstallMode: embedBootstrapper` | ≈ +1,8 МБ | нужен только если WebView2 отсутствует |
| Установщик «offline» | `offlineInstaller` | ≈ +127 МБ | не нужен |
| **Портативный ZIP** | `SirTone.exe` + `sirtone.portable` + `runtime\webview2\` (Fixed Runtime) | ≈ +180 МБ | не нужен |

На Windows 11 WebView2 есть по умолчанию (у владельца 155.0.4283.45); Windows 10 22H2 — обычно есть.

### 9.2. Портативный режим (F-29; foobar2000/AIMP/MusicBee)

- Режим определяется по `sirtone.portable` рядом с exe, **до создания окон**. Данные: `<exe>\SirToneData\` (настройки, логи, профиль WebView2 → `WEBVIEW2_USER_DATA_FOLDER`), библиотека по умолчанию `<exe>\Library\` (путь в БД относительный ⇒ переносится).
- WebView2: `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=<exe>\runtime\webview2` 🧪 S-13; **никаких** записей в реестр, автозапуска, ассоциаций файлов (урок foobar2000/AIMP).
- **Где брать Fixed Runtime в CI:** официальная страница держит только **две последние** версии CAB → берём NuGet-пакет **`WebView2.Runtime.X64`** (Fixed Version) с закреплённой версией и хэшем, либо кэшируем CAB в релиз-ассетах; распаковка `expand <cab> -F:* <dir>`; размер ≈ 180 МБ.
- Съёмный носитель: БД в `journal_mode=DELETE, synchronous=FULL`, атомарная запись настроек, обработка «внезапного» извлечения (`WM_DEVICECHANGE`), замер времени старта с USB 2.0/3.0.
- Обновление портативной: уведомление + «Скачать новый архив» (не установщик).

### 9.3. Обновления (F-30; Tauri updater ✔, electron-updater/Squirrel/Omaha как референс)

1. Через 60 с после старта и раз в сутки — запрос `latest.json` (GitHub Releases); подпись проверяется.
2. Новая версия → фоновая **загрузка**; пользователю — «Доступно обновление [Обновить сейчас / При выходе]» (по умолчанию «при выходе» — как `autoInstallOnAppQuit` у electron-updater).
3. Установка: NSIS `passive`; **приложение закрывается на время установки** (ограничение Windows ✔) — перед этим `on_before_exit`: остановка воспроизведения, flush БД; после — автозапуск новой версии свёрнутой, если оно было в трее.
4. Не обновляться во время воспроизведения без согласия; кнопка «Обновить» в настройках ✅.
5. Ключи: приватный ключ Tauri — секрет GitHub Actions + офлайн-копия владельца ❓OWN-04; публичный — в `tauri.conf.json`.

### 9.4. Подпись кода, SmartScreen, Smart App Control (риск R-03)

Неподписанный exe вызывает предупреждения SmartScreen и **может блокироваться Smart App Control** на новых установках Windows 11 (на машине владельца SAC выключен: `VerifiedAndReputablePolicyState=0`). Варианты (решение владельца ❓OWN-05): SignPath Foundation (бесплатно для открытых проектов), Azure Trusted Signing / иной платный сертификат, либо распространять без подписи с инструкцией. Рекомендация: открытый репозиторий + SignPath, если лицензия проекта это допускает.

---

## 10. Матрица возможностей по версиям Windows

| Функция | Win11 (24H2/25H2+) | Win10 22H2 |
|---|---|---|
| WebView2, Tauri, WASAPI, SMTC, превью-кнопки, Jump List | ✅ | ✅ |
| Mica/Acrylic (опционально, `windowEffects`) | Mica ✅ | Acrylic ✅ |
| Deskband / встроенная в панель задач полоса | ❌ (убрано) | ⚠️ технически возможно, не делаем |
| Слой виджета (S-09) | 🧪 | 🧪 |
| Bluetooth-аудио connect/disconnect | 🧪 (единый HFP/A2DP эндпоинт) | 🧪 |
| Сон ПК после таймера | S3 — `SetSuspendState`; Modern Standby — экран выкл. + отпустить систему 🧪 S-18 | то же |
| Уведомления о звонках (`IAudioVolumeDuckNotification`) | ✅ | ✅ |
| Поддержка ОС | основная | best effort (EOL у Microsoft) |
