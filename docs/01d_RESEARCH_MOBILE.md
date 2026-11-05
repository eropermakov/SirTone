# 01d. Референсы: мобильный Сиртон и хаб (3.0)

> Версия 3.0 · 2026-11-05. Принцип владельца — «не изобретать, копировать»: перед каждой задачей этапа 8 сначала сюда. Источники M1…M46 (свой ряд, чтобы не путать с S1…S126 в `01`/`01c`). Ссылки ТЗ владельца взяты из PDF (проверены владельцем 5.11.2026); лицензии и версии перепроверены через GitHub API и crates.io 2026-11-05. Раздел B проверен поиском 2026-11-05; перед задачами М3–М5 — дополнять по документации, не по памяти.

---

## A. Установка на iPhone без App Store и без Mac

| Инструмент | Что делает | Лицензия / состояние (2026-11-05) | Что берём |
|---|---|---|---|
| xtool [M1, M2] | Кросс-платформенная замена Xcode: собирает SwiftPM в iOS-приложение, подписывает и ставит; Linux/WSL/macOS; есть библиотека `XKit` | MIT · 1.24.0 (26.10.2026) · 14,8 тыс. ★ | Сборка в WSL при разработке (D-138) |
| xtool в WSL с usbipd [M3] | Проброс iPhone в WSL через `usbipd bind/attach --wsl` | статья | Шаги среды разработчика (`mobile/README.md` §9) |
| react-native-xtool [M4] | Что не собирается на Linux: каталоги ресурсов, storyboard, Metal | — | Список ограничений (§9) |
| xlinux [M5] | Сборка SwiftUI и App Intents на Linux, ограничения | GPL-3.0 · v0.5.0 | Только запасной внешний инструмент |
| iloader [M6] | Установщик-мастер на Rust поверх isideload | MIT · 4,5 тыс. ★ | Поток мастера хаба (идея и UX) |
| isideload [M7] | Rust-библиотека установки по Apple ID; использует idevice, apple-codesign | MIT · 0.5.2 (crates.io, 28.10.2026); README: «proper entitlement handling» — TODO | `IosInstaller` (D-139), версия `=` |
| idevice [M8] | Чистый Rust: lockdownd, usbmuxd, RSD, `afc`, `amfi`, `house_arrest`, `misagent`… | MIT · 0.1.68; README: «breaking changes at each point release until 0.2.0» | Связь с телефоном, Режим разработчика (`amfi`), перенос (`afc`) |
| Impactor [M9, M10] | Установщик по Apple ID (`plumesign`); чинил вход в сентябре и дважды в октябре 2026; была утечка 2FA в лог | MIT · v2.7.2 (24.10.2026) | Запасной путь `plumesign`; урок: секреты не в логах |
| AltStore [M11–M14] | Продление **только профилями** (`RefreshAppOperation`); диагностика сети, точка доступа как обход; лимит App ID; работа без компьютера | AGPL-3.0 | Идеи: продление профилями (D-139), сетевые подсказки (MR §10) |
| SideStore [M15] | Самопродление на телефоне; iOS 27 изменила файлы сопряжения | AGPL-3.0 | Идея; риск «обновление iOS ломает сопряжение» |
| SideInstaller [M16, M17] | Самопродление: RPPairing + токен, LocalDevVPN; релиз 1.4.0 | своя лицензия, **запрет коммерческого использования** | Только идея (волна 2, П9) |
| Sideloadly [M18, M19] | Автопродление по Wi-Fi; предупреждает, что экран iPhone должен быть включён | закрытый | Поведение: «подсказка от телефона» хабу (MR §10) |
| Apollo Reborn [M20] | Расширения тратят App ID | — | Лимит 10 App ID: приложение + виджет = 2 |
| NOOP (DX fork) [M21], ai_usage [M22] | Виджет и App Group при подписи бесплатным аккаунтом — работает / обратный опыт | — | П5 и запасной вид виджета |
| iPASide [M23] | Лимит 3 приложений считается на устройство (все бесплатные Apple ID) | — | Ошибка «Занято 3 приложения» |

## B. Плееры на телефоне (проверено 2026-11-05; дополнять перед задачами М3–М5)

| Приложение | Факт (источник) | Что берём |
|---|---|---|
| Apple HIG | Таб-бар — переход между разделами с сохранением навигации каждого раздела; подписи — короткие существительные [M38]. Метаданные Now Playing и команды — через `MPNowPlayingSession`/`MPNowPlayingInfoCenter`, система рисует экран блокировки и Пункт управления сама [M24, M39] | Три вкладки с независимой навигацией; своих «плееров» на системных поверхностях не рисуем (MR §4) |
| Apple Music | Мини-плеер над таб-баром, «Сейчас играет» раскрывается из него — общий приём iOS-плееров; в HIG текста о мини-плеере нет (поиск 2026-11-05) | Раскладка S04/S05; жесты проверяем на iOS 26 вживую (П8) |
| Яндекс Музыка | «Моя волна» — на главном экране, запуск ещё из карточки исполнителя/альбома и меню трека («Моя волна по треку»); настройки волны — занятие, настроение, характер, язык, сезонные варианты [M40, M41] | Волна первым экраном (S01), «Настроить волну» шторкой (S07, как D-130); «волна по треку» — кандидат в меню трека (волна 2) |
| Spotify | Мобильные детали не проверены; «почему этот трек» и подборки — `01c` | Только то, что уже записано в `01c` |
| Plexamp | Sweet Fades опираются на анализ громкости, не на sonic-анализ; при отсутствии анализа на сервере iOS-клиент анализирует трек на устройстве [M42, M43]; Sweet Fades, gapless и выравнивание громкости — в списке функций [M44] | Автомикс «Мягко» на телефоне — от анализа громкости (дёшево), «Умно» — после П6; анализ на устройстве, если признаков нет (FR-MOB-009) |
| Doppler | Офлайн-плеер своих файлов: импорт из «Файлов» и других приложений (iCloud, Google Drive, NAS), Wi-Fi-перенос через страницу в браузере ПК с кодом на экране телефона, приложение-компаньон для Mac (Wi-Fi/USB), автоимпорт новых файлов; FLAC/MP3/WAV/AAC/M4A [M45, M46] | Импорт S09 и «Входящие» (MR §5); перенос по Wi-Fi с кодом на экране — образец для FR-XFER-004 (волна 2, сопряжение по QR) |

## C. Платформа Apple

| Тема | Факт | Источник |
|---|---|---|
| Now Playing | Экран блокировки, Пункт управления и Динамический остров берут данные из `MPNowPlayingInfoCenter`; своего оформления у плеера нет | [M24], [M25] ответ инженера Apple про Live Activity, [M26] Gizmodo |
| CarPlay | Аудио-приложение в CarPlay — только с entitlement, который Apple выдаёт платному аккаунту разработчика | [M27] |
| Возможности бесплатного аккаунта | Какие capabilities доступны (App Groups, фон и т. д.) | [M28] |
| Импорт файлов | `CFBundleDocumentTypes`, `UIFileSharingEnabled`, `LSSupportsOpeningDocumentsInPlace` | [M29] |
| Оплата Apple Developer из России | Платный аккаунт затруднён | [M30] |
| Доступность Apple из России | Июль 2026: сбои App Store и сайта Apple | [M31] |

## D. Android и сборка

| Тема | Факт | Источник |
|---|---|---|
| Проверка разработчиков Google | С 30.09.2026 — регистрация на сертифицированных устройствах в 4 странах, 2027 — мировая | [M32] |
| Обход через ADB | Установка через ADB освобождена; ручная — «продвинутый путь» с ожиданием 24 ч | [M33] |
| `adb_client` | Чистый Rust, без сервера ADB | MIT · 3.2.3 [M34] |
| `wdi-rs` / libwdi | Установка WinUSB-драйвера; `wdi-rs` — MIT/Apache, libwdi — LGPL-3.0, Zadig — GPL-3.0 | [M35], GitHub `pbatard/libwdi` (Q-27) |
| GitHub Actions | macOS-раннер бесплатен для публичного репозитория; для закрытого — множитель 10 | [M36] |
| Tauri iOS | Сборка iOS только на macOS | [M37] |

## E. Что копируем — сводка

1. **Продление — только профили** (AltStore) → без переустановки и разрыва звука (П3).
2. **Мастер с картинками и «хаб сам замечает шаг»** (iloader, Impactor) → `mobile/README.md` §6.1.
3. **Сеть: точка доступа как обход** (AltStore) → таблица MR §10.
4. **«Подсказка от телефона»** — ответ на предупреждение Sideloadly про спящий iPhone.
5. **Самопродление RPPairing + LocalDevVPN** (SideInstaller, идея) → волна 2.
6. **Навигация Apple Music + волна Яндекс Музыки** → экраны S01…S11.

## Источники

M1 https://github.com/xtool-org/xtool · M2 https://github.com/xtool-org/xtool/releases · M3 https://zenn.dev/mnonamer/articles/ios-dev-on-wsl-with-xtool · M4 https://github.com/WillyAntunez/react-native-xtool/releases · M5 https://github.com/cesardev31/xlinux/releases/tag/v0.5.0 · M6 https://github.com/nab138/iloader · M7 https://github.com/nab138/isideload · M8 https://github.com/jkcoxson/idevice · M9 https://github.com/claration/Impactor · M10 https://github.com/claration/impactor/releases · M11 https://github.com/altstoreio/AltStore/blob/main/AltStore/Operations/RefreshAppOperation.swift · M12 https://faq.altstore.io/altstore-classic/troubleshooting-guide · M13 https://faq.altstore.io/altstore-classic/app-ids · M14 https://faq.altstore.io/altstore-classic/no-computer-instructions.md · M15 https://github.com/SideStore/SideStore/issues/1329 · M16 https://github.com/FrizzleM/SideInstaller · M17 https://github.com/FrizzleM/SideInstaller/releases/tag/v1.4.0 · M18 https://sideloadly.io/ · M19 https://sideloadly.io/faq · M20 https://github.com/Apollo-Reborn/Apollo-Reborn · M21 https://github.com/DX23876/noop/releases/tag/v9.3.3-dx-beta · M22 https://github.com/Neoo-Blue/ai_usage · M23 https://newreleases.io/project/github/pwnapplehat/iPASide/release/v1.0.1 · M24 https://developer.apple.com/documentation/mediaplayer/mpnowplayinginfocenter.md · M25 https://developer.apple.com/forums/thread/758469 · M26 https://gizmodo.com/iphone-14-pro-dynamic-island-how-to-use-best-apps-apple-1849571092/6 · M27 https://developer.apple.com/documentation/carplay/requesting-carplay-entitlements.md · M28 https://developer.apple.com/help/account/reference/supported-capabilities-ios · M29 https://developer.apple.com/forums/thread/85942 · M30 https://vc.ru/services/2968860-oplata-apple-developer-program-iz-rossii-s-pomoshchyu-bybit-card · M31 https://istories.media/news/2026/07/20/app-store-i-sait-apple-perestali-otkrivatsya-v-rossii/ · M32 https://developer.android.com/blog/posts/android-developer-verification-rolling-out-to-all-developers-on-play-console-and-android-developer-console · M33 https://www.androidauthority.com/how-to-bypass-android-developer-verification-sideloading-rules-3718002/ · M34 https://awesome.ecosyste.ms/projects/github.com%2Fcocool97%2Fadb_client · M35 https://docs.rs/wdi-rs · M36 https://docs.github.com/en/actions/reference/usage-limits-billing-and-administration · M37 https://v2.tauri.app/jp/distribute/app-store · M38 https://developer.apple.com/design/human-interface-guidelines/tab-bars · M39 https://developer.apple.com/videos/play/wwdc2022/110338 · M40 https://lifehacker.ru/yandeks-muzyka-moya-volna/ · M41 https://t-j.ru/lt-my-vibe-yandex-music/ · M42 https://forums.plex.tv/t/how-to-setup-sweet-fades/813304 · M43 https://forums.plex.tv/t/beta-ios-crossfade-issue-on-new-plexamp/943001 · M44 https://www.plex.tv/hi/blog/free-bird-plexamp-spreads-its-wings-for-every-music-lover/ · M45 https://apps.apple.com/app/id1468459747 · M46 https://brushedtype.co/docs/doppler/iphone-setup-which-option/
