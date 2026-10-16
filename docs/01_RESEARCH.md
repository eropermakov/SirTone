# 01. Исследование референсов: индекс, метод, сводка решений, источники

> Версия 1.3 · 2026-10-16 · статус: **основа для всех остальных документов**.
> Принцип владельца: **«не изобретать велосипед — взять то, как это работает у 6+ самых популярных приложений, скопировать и адаптировать».**
> Детали: `01a_RESEARCH_AUTOMIX_VIZ_ARCH.md` (автомикс, визуализация, архитектура) · `01b_RESEARCH_FEATURES.md` (остальные 33 фишки) · **`01c_RESEARCH_WAVE.md` (дополнение 2026-10-16: самообучение, сезоны, события, темы, эффекты, сон, фокус, ползунок, «Для всех»; источники S89–S126)**.

---

## 1. Метод и честные ограничения

1. Составлен перечень **всех фишек** SirTone (из ТЗ владельца + необходимые для их работы подсистемы) — 36 пунктов.
2. Для каждой найдены 6+ самых популярных приложений (там, где они существуют) и по ним прочитаны **первоисточники**: официальные документации и справка (Mixxx, djay, Spotify, Microsoft Learn, Picard, beets, Navidrome, Plex, Tauri, Wallpaper Engine, Lively, Last.fm, AcoustID, MusicBrainz, MPD…), **заголовки SDK / исходный код** (projectM, Kodi, Butterchurn, Winamp, iTunes Visual API), а при их отсутствии — пресса, форумы, обзоры.
3. Каждый факт помечен: **✔** первоисточник · **○** вторичный источник (прочитан) · **◌** знание проектировщика без перепроверки (при реализации — перепроверить).
4. Инструменты чтения: веб-поиск и веб-загрузчик с суммаризацией страницы. Поэтому часть страниц дала только оглавление или недоступна (403/404). **Не подтверждено и вынесено в проверки при реализации:**
   - точные значения порогов Picard по умолчанию (страница не отдала цифры);
   - внутренние алгоритмы AutoMix (Apple не раскрывает; «анализ тональности» — только заявление стороннего блога);
   - детали AIMP (SDK, кроссфейд, плагины) — первоисточники не найдены;
   - число полос эквалайзера Spotify/MusicBee/Winamp/VLC;
   - любые «умолчания» в приложениях, не подтверждённые документацией (например, длительность кроссфейда в JRiver);
   - реализация AMLL-фона (README не раскрывает алгоритм).
5. **Исключено как неподтверждённое:** «в Plexamp есть MilkDrop-визуализатор» (проверено: в Plexamp его нет, это запрос пользователей).

---

## 2. Матрица покрытия (фишка → референсы → где разобрано)

| № | Фишка | Референсы (≥6) | Раздел |
|---|---|---|---|
| A | **Автомикс / кроссфейд / переходы** | Apple AutoMix, Spotify, Plexamp, Mixxx, djay, VirtualDJ, MPD MixRamp, foobar2000, RadioBOSS, Rockbox, JRiver (11) | 01a §A |
| B | **Визуализация** | MilkDrop/Winamp, projectM, Butterchurn, Kodi, VLC, WMP, iTunes, Poweramp, Spotify Canvas, Apple (фон), Яндекс «Волна», Wallpaper Engine/Lively (12) | 01a §B |
| C | **Архитектура приложения** | foobar2000, Winamp, VLC, MPD, Strawberry, MusicBee, AIMP, Spotify, Plexamp, Roon, Navidrome, WaveFlow/LUMEN (12) | 01a §C |
| F-01 | Воспроизведение, форматы, вывод, gapless | 9 | 01b |
| F-02 | Нормализация громкости | 9 | 01b |
| F-03 | Эквалайзер | 7 | 01b |
| F-04 | Приглушение (ducking) | 6 | 01b |
| F-05 | Хранилище и раскладка файлов | 7 | 01b |
| F-06 | Импорт | 8 | 01b |
| F-07 | Распознавание (теги/имя/отпечаток/онлайн) | 8 | 01b |
| F-08 | «Неизвестные» | 6 | 01b |
| F-09 | Дубли | 7 | 01b |
| F-10 | Фиты, несколько исполнителей, слияние | 6 | 01b |
| F-11 | Пропавшие/перемещённые файлы | 7 | 01b |
| F-12 | Обложки и заглушки | 7 | 01b |
| F-13 | Плейлисты ручные/умные | 7 | 01b |
| F-14 | Продолжение плейлиста / автоплей | 7 | 01b |
| F-15 | Сходство треков, анализ | 6 | 01b |
| F-16 | Схема «настроение/энергия/язык» | 6 | 01b |
| F-17 | Поиск, смысловой поиск | 6 (слабые источники) | 01b |
| F-18 | Контекст: время, погода, занятие | 7 (по погоде аналогов нет) | 01b |
| F-19 | «Моя волна»: настройки потока | 6 | 01b |
| F-20 | Статистика прослушиваний | 6 | 01b |
| F-21 | Шафл и очередь | 6 | 01b |
| F-22 | Трей, закрытие, автозапуск | 6 | 01b |
| F-23 | Глобальные хоткеи | 6 | 01b |
| F-24 | SMTC | 6 | 01b |
| F-25 | Панель задач | 6 | 01b |
| F-26 | Виджеты рабочего стола | 6 | 01b |
| F-27 | Bluetooth | **5** (больше нет) | 01b |
| F-28 | Режимы производительности | 7 | 01b |
| F-29 | Портативный режим | 6 | 01b |
| F-30 | Обновления | 7 | 01b |
| F-31/32 | Локализация, звуки интерфейса | слабые (◌) | 01b |
| F-33 | Экспорт, waveform, первый запуск, хранилище | смешанные | 01b |
| W-01 | Самообучение: лайк, «не сейчас», «никогда», политики обучения | 6 | 01c |
| W-02 | «Почему этот трек», чипы влияний, ручная настройка | 6 | 01c |
| W-03 | Архитектура движка рекомендаций (кандидаты → ранжирование → секвенсор, Томпсон) | 7 | 01c |
| W-04 | Сезонная волна | 6 (2 слабых) | 01c |
| W-05 | Волны под события и автопредложения | 7 | 01c |
| W-06 | Сезонные и событийные темы | 6 (2 слабых) | 01c |
| W-07 | Живые погодные эффекты | 7 | 01c |
| W-08 | Сон-таймер и засыпательная волна | 7 | 01c |
| W-09 | Фокус-волна / помодоро | **5** + метод | 01c |
| W-10 | Ползунок «Знакомое ↔ Неожиданное» | 6 | 01c |
| W-11 | Нейтральная волна «Для всех» | 7 | 01c |

---

## 3. Сводка: что взято у кого (и как это изменило проект)

Эта таблица — главный итог исследования. Она же **список изменений относительно моего первого черновика архитектуры** (там, где я «придумал» — теперь указан референс).

| Область | Принятое решение | Взято у | Что поменялось в моём черновике |
|---|---|---|---|
| Режимы переходов | Gapless / Кроссфейд / Умный автомикс; альбомы не микшируются | Apple, Spotify, Plexamp, Rockbox | Добавлено исключение для альбомов и отдельное поведение «ручной пропуск» |
| Выбор перехода | Лестница отката: beatmatch → перекрытие по огибающей → срезка тишины + gapless | VirtualDJ, Apple, MPD, Mixxx | Вместо «придуманного» алгоритма — копия лестницы |
| Точка перекрытия | По огибающей громкости (10 Гц, порог −17 dB, мин. перекрытие 2,5 с) | Plexamp, MPD (−17 dB), RadioBOSS (2,5 с, −14 dB) | Раньше был «таймер до конца» |
| Длина перехода | 1–12 с (ручной), «авто» = min(аутро, интро) | Apple/Spotify, Mixxx | — |
| Темп при микшировании | Auto/Sync/Blend; допуск ≤ 6–10 %, учёт ×2/÷2 | djay, VirtualDJ | — |
| Time-stretch | Signalsmith Stretch (MIT) вместо Rubber Band (GPL)/SoundTouch (LGPL) | Mixxx (используют Rubber Band) → адаптация по лицензиям | — |
| Кадр для визуализации | 1024 отсчёта L/R/mono, 8 бит, timestamp, компенсация задержки | Butterchurn, WMP, iTunes, Kodi (`AudioGetSyncDelay`) | Подтверждено: `render({audioLevels})` без WebAudio |
| Пресеты MilkDrop | 30 с на пресет, мягкий переход 3 с, hard-cut выкл., Lock/Rate | projectM, Kodi, Poweramp | — |
| Качество визуализации | mesh 24×18 → 32×24 → 48×36; FPS 30/45/60; масштаб текстур | projectM, Butterchurn, Wallpaper Engine | Добавлены конкретные числа |
| Цветовые волны | Бит-синхронно, treble → блики, клик/лайк → импульс, белые импульсы курсора, минимальный вариант | Яндекс «Моя волна», Apple/AMLL | Добавлены импульсы и «минималистичный» экран |
| Видео-фон | Зацикленные 3–15 с, без звука | Spotify Canvas (3–8 с) | — |
| Пауза визуализации | Окно перекрыто (95 %), полноэкранное приложение, батарея | Lively, Wallpaper Engine, Chrome Energy Saver | Добавлено |
| Иммерсивный режим | UI плавно скрывается, возвращается по мыши | Poweramp (Fade/Hidden), Яндекс (скрытие UI) | — |
| Архитектура | Rust-ядро + WebView-UI; SQLite; конвейер decode→resample→DSP→output; vis — тап после DSP | Spotify (CEF), Plexamp, Roon, Tauri-плееры; foobar2000 v2 (SQLite) | Подтверждена; добавлено: **события асинхронные** (урок MusicBee) |
| Missing-файлы | Состояние `missing`, тихая очистка через 24 ч; защита «корень недоступен» | Navidrome, Plex | Было «сразу удаляем» |
| Импорт | Копия + хэш на лету; отчёт о пропущенном | iTunes (Copy files, «Not Added»), beets | — |
| Раскладка файлов | `Артист/Альбом/NN - Название`, `Singles`, `Compilations`; правила очистки имён | beets, Picard, iTunes | Добавлены Singles/Compilations, ограничения Windows |
| Распознавание | Расстояние + три зоны (авто/предложение/Неизвестные); допуск длительности 2 с | beets (0,04), Picard (2 с) | Вместо «своей формулы» |
| Умные плейлисты | JSON-схема с группами all/any, операторами, sort/order/limit/limitPercent/refreshDelay | Navidrome `.nsp`, Apple | Вместо «собственного AST» |
| Продолжение плейлиста | Тумблер ∞ в очереди; «+» / «−» на рекомендованном; офлайн-радио из библиотеки | Roon, Spotify Smart Shuffle, Apple, YouTube Music | Добавлено «−» |
| Нормализация | Тихо −19 / Обычно −14 / Громко −11, по альбому, ReplayGain/R128-теги | Spotify, Apple, ReplayGain | По умолчанию −14 (было −16) |
| Эквалайзер | 10 полос + preamp + пресеты Apple; модель на N полос | AIMP/foobar2000 (18), Poweramp (5–32) | — |
| Ducking | Сила в %, attack/hold/release, исключения | Android, Windows (80 %), Discord | Добавлен процентный ползунок |
| Шафл | N кандидатов → лучший по «свежести» + разнос исполнителей | Spotify «Fewer Repeats», iTunes Smart Shuffle | Вместо «обычного Fisher–Yates» |
| Статистика | Засчитывать по правилу Last.fm | Last.fm | — |
| Поиск | Лучшее совпадение + группы; язык/раскладка/морфология; FTS5 | Spotify/Apple, Яндекс, SQLite | — |
| Контекст | Время суток + день недели + устройство + ручное переопределение; погода — расширение | Яндекс, Spotify daylist | — |
| Настройки волны | Занятие/характер/настроение/язык, комбинируются, сброс, «волна по треку/артисту/альбому», темп тренировки | Яндекс | Добавлены «сброс» и «по треку» |
| Закрытие окна | Крестик, Alt+F4, Alt+Tab — одинаково → трей | Spotify, Discord (учтены их недостатки) | — |
| Хоткеи | Флаг global на действии, обнаружение конфликтов | foobar2000, VLC, MusicBee | — |
| SMTC | Ручная интеграция | Microsoft Learn | Совместимость с Rainmeter «бесплатно» |
| Панель задач | 3 кнопки в превью (лимит 7), прогресс, overlay, Jump List | Microsoft Learn (Taskbar Extensions) | Deskband невозможен — подтверждено |
| Bluetooth | `IKsControl` + `KSPROPSETID_BtAudio` (ONESHOT_RECONNECT/DISCONNECT) | ToothTray | **Основной способ заменён** (было `BluetoothSetServiceState`) |
| Портативный режим | Маркер-файл, всё в папке, нет реестра/ассоциаций, WAL выкл. на флешке | foobar2000, AIMP/MusicBee (уроки) | — |
| Обновления | Фон → установка при выходе/по кнопке; per-user установка | electron-updater, Squirrel, Omaha, Tauri | — |
| Перформанс-пресеты | Пауза/Остановка с освобождением памяти, FPS-лимит, масштаб, правила по приложениям | Wallpaper Engine, Lively, Chrome, Discord | — |
| **(1.3)** Движок волны | Профили одного движка; кандидаты → скоринг → секвенсор; Томпсон + бета + байесовская логистическая регрессия | YouTube (two-stage), Spotify BaRT/EEE, Deezer, Chapelle & Li | Предложение владельца подтверждено прототипом S-17 (3 мс на 50 000) |
| Обратная связь | Лайк / «не сейчас» (полка в контексте) / «никогда» (жёсткое исключение); режимы без влияния на вкус | Pandora, Deezer, Spotify Exclude/Private Session, Apple «без истории» | «Не сейчас» отменяемо (у Pandora — нет) |
| Ползунок новизны | Квоты пяти категорий знакомости + «непохожесть» + сила исследования | YouTube Music Tuner, Pandora Modes, Spotify Repeat Rewind, Яндекс «Незнакомое» | — |
| Сезоны и события | Личная история сезона; офлайн-календарь праздников; карточка вместо уведомления; управление отказами | Spotify Summer Rewind, Apple Photos «Feature Less», Google Photos, Doodles | Окно смены сезона ±21 день (✅Q-10) |
| Живые эффекты | Состояния погоды как у Apple/Яндекс Погоды; опрос раз в 30–60 мин; зависимость от энергосбережения | Apple Weather, Яндекс Погода, Wallpaper Engine, Lively Weather, Telegram | Частицы без работы ЦП на кадр |
| Сон и фокус | Пресеты + «до конца трека», продление действием во время затухания, действия после таймера; 25/5/15×4, отдельные звуки границ | Spotify, YouTube Music, Pocket Casts, Audible, Kodi; Windows Clock, Brain.fm, Pomofocus | Modern Standby — свой путь «сна» (S-18) |
| «Для всех» | Популярность у всех (уникальные слушатели), изоляция вкуса владельца | Pandora Crowd Faves, ListenBrainz, Spotify Private Session | Только ListenBrainz (✅Q-7) |

---

## 4. Список источников

> Источники, прочитанные в этой сессии (2026-10-15). Если страница недоступна по ссылке — искать по заголовку.

**Автомикс**
- **S1** Mixxx 2.5 manual — Auto DJ: https://manual.mixxx.org/2.5/en/chapters/djing_with_mixxx.html
- **S2** Mixxx — Beat/Key detection preferences: https://manual.mixxx.org/2.7/ca/chapters/preferences/beat_detection · https://manual.mixxx.org/2.5/ru/chapters/preferences/key_detection
- **S3** djay — Automix settings: https://help.algoriddim.com/user-manual/djay-pro-mac/settings/automix
- **S4** djay — Using Automix: https://help.algoriddim.com/user-manual/djay-pro-mac/mixing-basics/using-automix
- **S5** Spotify Support — Transitions between tracks: https://support.spotify.com/us/article/tracks-transitions/
- **S6** TechCrunch — Spotify Mix (2025-08-19): https://techcrunch.com/2025/08/19/spotifys-latest-feature-lets-you-add-your-own-transitions-to-playlists
- **S7** 9to5Mac — AutoMix in iOS 26: https://9to5mac.com/2025/06/16/automix-apple-music-ios-26/ · TechRadar: https://www.techradar.com/audio/apple-music/automix-is-the-apple-music-feature-that-made-me-love-listening-to-music-on-my-iphone-again · Apple (iPhone guide): https://support.apple.com/guide/iphone/iphcba31c44b/ios
- **S8** Plex Forums — Sweet Fades: https://forums.plex.tv/t/plexamp-sweet-fades/894454 · https://forums.plex.tv/t/sweet-fades-problem-choppy/873222 · https://forums.plex.tv/t/plexamp-nahtlose-wiedergabe/857302
- **S9** MPD MixRamp (docs commit): https://git.pvv.ntnu.no/oysteikt/mpd/commit/bea821f1945484d3b99ec6339bf2017b0f1e117c · инструмент: https://www.sourceforge.net/projects/mixramp
- **S10** foobar2000 fading: https://hydrogenaud.io/index.php/topic,92627.0.html · Sqrsoft Advanced Crossfader: https://foobar2000.org/components/view/foo_dsp_sqracf
- **S11** RadioBOSS — Crossfades: https://manual.djsoft.net/radioboss/en/crossfades.htm
- **S12** Rockbox — Playback options: https://git.lenord.me/devs/foxbox/raw/commit/34b0ca49838fc3e21f823e086e61983ba23a014c/manual/configure_rockbox/playback_options.tex
- **S13** VirtualDJ forum: https://virtualdj.com/forums/7652/General_Discussion/Playlists_automix_-_how_it_works.html · https://virtualdj.com/forums/248130/Wishes_and_new_features/Automix_Configuration.html · https://virtualdj.com/forums/91225/Wishes_and_new_features/Automix_improvement.html
- **S14** JRiver Interact forum: https://yabb.jriver.com/interact/index.php/topic,13771.0.html
- **S15** Apple Music crossfade 1–12 с: https://www.macworld.com/article/1993764/how-to-ios17-set-adjust-crossfade-apple-music.html · https://appleinsider.com/inside/apple-music/tips/how-to-crossfade-apple-music-tracks-in-ios-17

**Визуализация**
- **S20** projectM headers: https://raw.githubusercontent.com/projectM-visualizer/projectm/master/src/api/include/projectM-4/parameters.h · …/audio.h
- **S21** Butterchurn: https://github.com/jberg/butterchurn · https://raw.githubusercontent.com/jberg/butterchurn/master/src/rendering/renderer.js · https://raw.githubusercontent.com/jberg/butterchurn/master/src/audio/audioProcessor.js
- **S22** MilkDrop preset authoring (Geiss): https://www.geisswerks.com/milkdrop/milkdrop_preset_authoring.html
- **S23** Kodi `Visualization.h`: https://raw.githubusercontent.com/xbmc/xbmc/master/xbmc/addons/kodi-dev-kit/include/kodi/addon-instance/Visualization.h
- **S24** Winamp `VIS.H` (зеркало): https://git.txlyre.website/txlyre/winamp/src/main/Src/Winamp/VIS.H
- **S25** WMP `TimedLevel`: https://learn.microsoft.com/ru-ru/previous-versions/windows/desktop/api/effects/ns-effects-timedlevel · https://learn.microsoft.com/zh-cn/previous-versions/windows/desktop/wmp/using-timed-levels
- **S26** iTunes Visual API (зеркало в projectM): https://gfiber.googlesource.com/vendor/opensource/projectM/+/master/src/projectM-iTunes/win32/iTunesVisualAPI.h
- **S27** VLC визуализации: https://dev.to/vlc_guru/how-to-see-audio-visualizations-in-vlc-media-player-5d8d · https://mailman.videolan.org/pipermail/vlc-devel/2019-April/124116.html
- **S28** Poweramp: https://forum.powerampapp.com/topic/26525-poweramp-is-just-milkdrop-2x-for-android/ · https://forum.powerampapp.com/topic/17596-immersive-mode-option-for-full-screen-visualization/
- **S29** Webamp / Music Assistant: https://docs.webamp.org/docs/features/mikdrop · https://www.music-assistant.io/plugins/milkdrop-visualizer/
- **S30** Spotify Canvas (сторонние гайды): https://www.artist.tools/post/spotify-canvas-specifications-a-complete-guide · https://www.wiseband.com/blog/canvas-spotify/
- **S31** Т—Ж о «Моей волне» (визуал и настройки): https://t-j.ru/lt-my-vibe-yandex-music/
- **S32** AMLL: https://github.com/Steve-xmh/applemusic-like-lyrics · https://newreleases.io/project/github/amll-dev/applemusic-like-lyrics/release/main-nightly
- **S33** Wallpaper Engine: https://help.wallpaperengine.io/en/performance/game.html · https://help.wallpaperengine.io/en/functionality/applicationrules.html
- **S34** Lively — Performance: https://github.com/rocksdanister/lively/wiki/Performance
- **S35** Chrome Energy Saver: https://developer.chrome.com/blog/memory-and-energy-saver-mode · **S35-dc** Discord (Reduced Motion/HW accel/Game Mode): https://www.guidingtech.com/fix-discord-lagging-on-windows-mac/ · https://www.digitalcitizen.life/discord-game-mode-toggle-found-desktop-client/

**Архитектура**
- **S40** Spotify Engineering — desktop на CEF: https://engineering.atspotify.com/2021/4/building-the-future-of-our-desktop-apps
- **S41** Plexamp (Electron/MPD; v3): https://thenextweb.com/apps/2017/12/19/plex-new-winamp-inspired-music-player-for-your-desktop-is-pretty-but-needs-work/ · https://medium.com/@elanf/plexamp-v3-9af3b10063b4
- **S42** VLC Hacker Guide: https://wiki.videolan.org/LibVLCcore · https://wiki.videolan.org/Hacker_Guide
- **S43** MPD plugins: https://mpd.readthedocs.io/en/latest/plugins.html
- **S44** Strawberry: https://github.com/strawberrymusicplayer/strawberry
- **S45** MusicBee API и аудио: https://getmusicbee.com/help/api/ · https://thewelltemperedcomputer.com/SW/Players/MusicBee/MusicBee_audio.htm
- **S46** Roon: https://desk.zoho.com/portal/roonlabsllc/kb/articles/architecture · https://headfonia.com/q-n-a-saturday-roon-labs/2
- **S47** Navidrome — Missing files: https://www.navidrome.org/docs/usage/library/missing-files/
- **S48** foobar2000 SDK / v2 SQLite: https://www.foobar2000.org/SDK · https://www.neowin.net/amp/foobar2000-20-beta-13/
- **S49** Winamp: https://en.wikipedia.org/wiki/Winamp
- **S50** WaveFlow / LUMEN: https://github.com/InstaZDLL/WaveFlow · https://github.com/RyuHayato/LUMEN
- **S51** Tauri 2 config / updater: https://v2.tauri.app/reference/config/ · https://v2.tauri.app/plugin/updater/
- **S52** Tauri IPC (обсуждение производительности): https://github.com/orgs/tauri-apps/discussions/11915

**Остальные фишки**
- **S60** Нормативы громкости: https://www.izotope.com/en/learn/mastering-for-streaming-platforms.html · https://www.forasoft.com/learn/audio-for-video/articles-audio/lufs-targets-per-platform-2026
- **S61** ReplayGain: https://wiki.hydrogenaudio.org/index.php/ReplayGain · https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/2821
- **S62** Ducking: https://developer.android.com/reference/android/media/AudioFocusRequest · https://android-developers.googleblog.com/2013/08/respecting-audio-focus.html · https://www.elevenforum.com/t/change-adaptive-communication-sound-levels-in-windows-11.47293/ · https://support.discord.com/hc/en-us/articles/206342888
- **S63** Plex — Empty Trash: https://support.plex.tv/articles/200289526-library/ · https://forums.plex.tv/t/understanding-empty-trash-automatically-after-each-scan/174717
- **S64** beets: https://beets.readthedocs.io/en/stable/reference/config.html · https://beets.readthedocs.io/en/stable/reference/pathformat.html
- **S65** Picard: https://picard-docs.musicbrainz.org/en/config/options_matching.html · https://picard-docs.musicbrainz.org/en/config/options_filerenaming_compat.html
- **S66** AcoustID / MusicBrainz: https://acoustid.org/webservice · https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting · https://music-assistant.io/audio-analysis/acoustid/ · **S66-ess** Essentia (лицензии): https://essentia.upf.edu/licensing_information.html · https://essentia.upf.edu/models/LICENSE
- **S67** SongKong / dupeGuru: https://www.jthink.net/songkong/summary4.jsp · https://community.jthink.net/t/tutorial-delete-duplicates-song-is-a-duplicate-if-has-same-option/11685 · https://www.addictivetips.com/windows-tips/find-removes-duplicate-songs-by-tags-audio-content-dupeguru-me
- **S68** iTunes / MusicBee / WMP: https://www.macworld.com/article/200083/itunes_automatic.html · https://support.apple.com/HT201169 · https://musicbee.fandom.com/wiki/Library_Preferences · https://learn.microsoft.com/en-us/previous-versions/aa393184(v=vs.85)
- **S69** Умные плейлисты: https://support.apple.com/en-ng/guide/music/mus1712973f4/mac · https://www.navidrome.org/docs/usage/features/smart-playlists/ · MusicBee release notes (вложенные And/Or) — результат поиска
- **S70** Радио/автоплей: https://kb.roonlabs.com/FAQ:_What's_Radio_and_how_does_it_work%3F · https://newsroom.spotify.com/tag/song-recommendation · https://kirkville.com/understanding-the-autoplay-button-in-the-ios-music-app/ · https://www.bgr.com/2238236/youtube-music-playlist-settings-disable-autoplay/
- **S71** Plex Super Sonic / bliss: https://www.plex.tv/en-ca/blog/super-sonic-get-closer-to-your-music-in-plexamp/ · https://techcrunch.com/2021/08/12/plexs-new-feature-matches-your-sonically-similar-music-to-make-playlists · https://docs.rs/crate/bliss-audio/ · https://gitea.antonlyap.pp.ua/antonlyap/bliss-rs
- **S72** Spotify audio features: https://spotify.js.org/interfaces/AudioFeatures.html · https://help.spotontrack.com/article/what-do-the-audio-features-mean
- **S73** Яндекс Музыка: https://t-j.ru/lt-my-vibe-yandex-music/ · https://3dnews.ru/1141969/v-yandeks-muzike-radikalno-obnovilas-moya-volna/print · https://rozetked.me/news/46480 · https://www.weproject.media/articles/detail/lyubimye-pesni-na-kazakhskom-yazyke-komanda-weproject-media-testiruet-obnovlennuyu-sistemu-rekomenda/ · https://yandex.ru/company/technologies/query/
- **S74** Spotify daylist: https://www.trustedreviews.com/explainer/what-is-spotify-daylist-4417204 · https://www.imore.com/apps/music-apps/spotify-launches-personalized-playlist-that-changes-throughout-the-day
- **S75** Last.fm Scrobbling 2.0: https://www.last.fm/api/scrobbling
- **S76** Шафл: https://engineering.atspotify.com/2025/11/shuffle-making-random-feel-more-human · https://discussions.apple.com/thread/2003430
- **S77** Закрытие в трей: https://community.spotify.com/t5/Desktop-Windows/Close-button-should-minimize-and-Alt-F4/m-p/5960580 · https://support.discord.com/hc/en-us/community/posts/360072275671--Minimize-to-tray-option-is-all-or-nothing-Can-this-be-tweaked
- **S78** Глобальные хоткеи: https://wikiwiki.jp/foobar2000/Preferences/Keyboard%20Shortcuts · https://wiki.videolan.org/VLC_HowTo/Global_hotkeys · https://tutorialtactic.com/blog/musicbee-shortcuts/ · https://community.spotify.com/t5/Closed-Ideas/Desktop-Global-HotKeys-for-Pause-Play-Skip-etc/idi-p/57061
- **S79** SMTC / панель задач / Rainmeter: https://learn.microsoft.com/en-us/windows/apps/develop/media-playback/integrate-with-systemmediatransportcontrols · https://learn.microsoft.com/en-us/windows/win32/shell/taskbar-extensions · https://docs.rainmeter.net/manual/measures/nowplaying/
- **S80** Bluetooth: https://github.com/m2jean/ToothTray · https://learn.microsoft.com/en-us/windows/desktop/api/BluetoothAPIs/nf-bluetoothapis-bluetoothsetservicestate · https://learn.microsoft.com/windows/apps/develop/media-playback/enable-remote-audio-playback · https://winget.ragerworks.com/package/Slohmaier.StarTooth
- **S81** Портативные версии: https://hydrogenaud.io/index.php/topic,124801.0.html · https://portableapps.com/node/57345 · https://www.videohelp.com/software/MusicBee
- **S82** Обновления: https://www.mintlify.com/electron-userland/electron-builder/api/electron-updater · https://omaha-consulting.com/chromium-updater-omaha-4-tutorial · https://dev.solita.fi/2016/03/14/automatic-updater-for-windows-desktop-app.html
- **S83** Gapless: https://en.wikipedia.org/wiki/Gapless_playback · https://hydrogenaudio.org/index.php/topic,63480.0.html
- **S84** Теги и декодеры: https://docs.rs/lofty/latest/lofty/file/enum.FileType.html · https://github.com/pdeljanov/Symphonia · https://www.mintlify.com/pdeljanov/Symphonia/resources/supported-codecs
- **S85** Мульти-артисты: https://www.navidrome.org/docs/usage/library/tagging · https://musicbrainz-docs-development.readthedocs.io/en/latest/style_guides/artist_credits.html · https://docs.funkwhale.audio/specs/multi-artist/mb-content.html
- **S86** Эквалайзеры: https://www.neowin.net/amp/aimp-355-1345/ · https://forum.powerampapp.com/topic/22476-equalizer-bands/ · https://support.spotify.com/us/article/equalizer/ · https://avisynthplus.readthedocs.io/en/latest/avisynthdoc/corefilters/supereq.html
- **S87** VLC WASAPI exclusive (патч): https://mailman.videolan.org/pipermail/vlc-devel/2019-October/129033.html
- **S88** Библиотеки и лицензии: собственная проверка через crates.io API и npm registry, 2026-10-15 (таблица — `08_QUALITY_SECURITY_LICENSES.md` §5).
- **S89–S126** — источники дополнения «Умная волна» (Pandora, Spotify, YouTube Music, Яндекс, Apple, Deezer, исследования рекомендаций, погода, сон-таймеры, помодоро, ListenBrainz, Open-Meteo, Microsoft Learn, AudioSet, даты Пасхи и солнцестояний): список — `01c_RESEARCH_WAVE.md` §13.
