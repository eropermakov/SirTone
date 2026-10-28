# Сиртон — единый дизайн-принцип

Сиртон — музыкальный плеер для Windows от **Sir.kov Studio**. Этот файл — единственный источник правды по визуалу, поведению и UX. Любой экран и компонент строится только из токенов и правил ниже. Если чего-то здесь нет — выбирай самое тихое решение, которое не нарушает принципы.

**В одну фразу:** тёплая тихая рамка в фирменных цветах Sir.kov; цвет дают музыка, бордовые плашки и одна яркая точка акцента.

Три источника, по старшинству:
1. **Sir.kov Fabric** — общий принцип студии: шрифт Geologica, тёплая палитра, правило 60–30–10, законы UX. При конфликте побеждает он.
2. **Apple Music** — логика музыкального приложения: три зоны, обложка как главный объект, квадрат = альбом, круг = исполнитель, цвет из обложки.
3. **Десктопный ChatGPT** — язык контролов: слоистые поверхности, обводки вместо теней, крупные скругления, кнопки-пилюли, контурные иконки, пилюли подсветки.

## 0. Бренд Sir.kov

- **Sir.kov** — независимая команда разработки и автоматизации.
- **Sir.kov Team** — так обращаемся к пользователям (подпись уведомлений, писем, экрана «Что нового»).
- **Sir.kov Studio** — наши продукты. В «О программе»: «Сиртон · Sir.kov Studio».
- **Sir.kov Lab** — экспериментальные функции (раздел «Лаборатория» в настройках, если появится).
- **Sir.kov Fabric** — внутренние технологии и инструменты.

---

## 1. Законы UX Sir.kov Fabric (обязательны)

1. **Фиттс — крупно и под рукой.** Play/Pause — самая крупная кнопка (`play-size` 44px) в центре плеера. Минимальная зона клика 36px. Вся строка трека кликабельна. Прогресс ловит клик в полосе 16px, хотя рисуется в 4px. Меню открывается у курсора.
2. **Постел — мягко на входе, строго на выходе.** Поиск прощает опечатки, неверную раскладку («ktnj» → «лето»), транслит, регистр, ё/е. Импорт сам чинит кодировку тегов (cp1251 → UTF-8), убирает мусор вроде «(Official Video)», «[320kbps]», нормализует «feat.». Мусорные файлы молча пропускаются, итог — одной строкой. Никаких модальных ошибок там, где ввод можно исправить самим. На выходе — строго нормализованные теги и имена.
3. **Хик — меньше выбора за раз.** Одна Primary-кнопка на экран, остальное в «…». Контекстное меню ≤ 7 пунктов, дальше подменю. Настройки по разделам, редкое — в «Дополнительно». Первый запуск — 4 шага по одному.
4. **Миллер — порциями.** Полки по 6–8 карточек + «Ещё». Настройки блоками по 3–7 пунктов, между блоками `space-12`. Альбомы делятся по дискам, медиатека — по буквам. Чипы — не больше 7 в ряду.
5. **Теслер — сложность забираем в код.** Обложки, тексты песен, теги, нормализация громкости, авто-цвет — автоматически. У всего разумные значения по умолчанию.
6. **Якоб — как везде.** Раскладка как у Apple Music / Spotify. Пробел — пауза, двойной клик — играть, правый клик — меню, Ctrl+F — поиск, медиаклавиши, системные кнопки окна Windows.
7. **Эстетика-юзабилити.** Строгие токены без исключений, ровная сетка, плавные переходы, аккуратные пустые состояния вместо «Нет данных».
8. **Пик и конец.** Пик — переход в «Сейчас играет» (обложка вырастает из плеера, фон из её цветов). Конец — альбом доиграл: «Альбом закончился» + предложение продолжить похожим; после перезапуска трек и позиция на месте; после импорта — сводка и сразу «Слушать».
9. **Зейгарник — незавершённое держит.** Панель первой настройки «1 из 4» с прогрессом. «Продолжить прослушивание» на Главной. Прогресс импорта виден в сайдбаре. Черновик плейлиста сохраняется сам.
10. **Порог Доэрти — отклик ≤ 400 мс.**
    - **≤ 100 мс** (ощущается мгновенным): Play/Pause, лайк, громкость, перемотка, наведение, выделение, смена цвета.
    - **≤ 400 мс**: переходы между экранами, открытие альбома, результаты поиска.
    - **≤ 1 с** — только тяжёлое (первое сканирование) и с видимым прогрессом. > 3 с без обратной связи — недопустимо.
    - **Оптимистичный UI**: лайк, «В плейлист», «Воспроизвести далее» меняют состояние сразу; при ошибке — тихий откат и тост.
    - **Скелетоны** вместо спиннеров: контуры карточек и строк цветом `artwork-placeholder`.
    - Звук стартует из буфера сразу, обложка и метаданные догружаются после.

---

## 2. Цвет: правило 60–30–10

| Доля | Роль | В Сиртоне |
|---|---|---|
| **60%** | Доминантный нейтральный | Поверхности `bg-base`, `bg-layer`, `bg-raised`, `bg-flyout` — тёплый сигарный (тёмная) / матовый белый (светлая) |
| **30%** | Основной (бренд) | **Плашки** `primary`: выбранный пункт навигации, активный чип, бейджи, брендовая панель. Заголовки страниц и полок — `primary-text`. Остальную «тридцатку» отдаём обложкам |
| **10%** | Акцент (CTA) | `accent-fill`: Play, «Слушать», отмеченный чекбокс, переключатель, счётчик уведомлений. `accent-text`: ссылки, прогресс, индикатор «играет», включённые повтор/перемешать |

**Запрещено:** заливать `primary` большие фоны (весь сайдбар, весь плеер), использовать акцент как декор, ставить два акцента рядом.

### Пресет Sir.kov (по умолчанию)

| | Тёмная | Светлая |
|---|---|---|
| Фон (60%) | Сигарный `#1C1616` | Матовый белый `#F9F6F0` |
| Основной (30%) | Maroon Oak `#590212` | Maroon `#8C031C` |
| Акцент (10%) | Неоновый вольт `#CCFF00` | Цифровой лайм `#66BB00` |
| Текст | Алебастр `#F9F6F0` | Сигарный `#1C1616` |
| Неактивный | `#3D3535` | `#C2BCB2` |

---

## 3. Нейтральные токены (обе темы)

| Токен | Тёмная | Светлая | Где |
|---|---|---|---|
| `bg-base` | `#161111` | `#f2eee6` | Фон окна, сайдбара и плеера (под Mica) |
| `bg-layer` | `#1c1616` | `#f9f6f0` | Фирменный фон Sir.kov (60%): тёмная — сигарный, светлая — матовый белый |
| `bg-raised` | `#241d1d` | `#fdfcf9` | Карточки-панели — всегда с border и radius-xl, без тени |
| `bg-flyout` | `#2b2323` | `#fdfcf9` | Меню, контекстные меню, поповеры — с border, shadow-flyout, radius-lg |
| `bg-hover` | `rgba(249,246,240,0.07)` | `rgba(28,22,22,0.05)` | Пилюля наведения на пункт меню, сайдбара, строку трека |
| `bg-selected` | `rgba(249,246,240,0.11)` | `rgba(28,22,22,0.08)` | Выделенная строка трека, подсветка клавиатурой в меню |
| `inactive` | `#3d3535` | `#c2bcb2` | Фирменный «цвет неактивного» Sir.kov: пустая часть прогресса и слайдеров, неактивные заливки, деликатные линии |
| `border` | `rgba(249,246,240,0.10)` | `rgba(28,22,22,0.10)` | Обводка 1px у панелей, меню, вторичных кнопок, неактивных чипов |
| `separator` | `rgba(249,246,240,0.10)` | `rgba(28,22,22,0.09)` | Разделитель в меню с отступами 12px; линия над плеером |
| `text-primary` | `#f9f6f0` | `#1c1616` | Основной текст |
| `text-secondary` | `#c4bbb5` | `#5c5351` | Исполнитель, подписи, значения справа в меню, заголовки групп |
| `text-tertiary` | `#a39994` | `#6f6563` | Таймкоды, длительности, горячие клавиши, выполненные пункты |
| `icon` | `#c4bbb5` | `#5c5351` | Контурные иконки 20px |
| `focus-ring` | `#f9f6f0` | `#1c1616` | Кольцо фокуса 2px с отступом 2px |
| `artwork-placeholder` | `#2b2323` | `#e8e3da` | Пустая обложка до загрузки, скелетоны |
| `artwork-scrim` | `rgba(0,0,0,0.45)` | `rgba(0,0,0,0.45)` | Затемнение на обложке при наведении |
| `danger` | `#ff6b6b` | `#c62828` | Деструктивные действия и ошибки |

`inactive` — фирменный «цвет неактивного»: пустые дорожки прогресса и слайдеров, неактивные заливки, деликатные линии. **Для текста не использовать** (1.5–1.8:1) — подписи берут `text-secondary` / `text-tertiary`.

---

## 4. Цвет пользователя: пресет Sir.kov + 16 цветов + Авто

### Модель

Компоненты знают только **шесть активных токенов**:

| Токен | Роль |
|---|---|
| `accent-fill` | Заливка крупных CTA (кнопки, Play, чекбокс, переключатель) |
| `accent-text` | Текст, ссылки и **тонкие индикаторы** (прогресс, полоски «играет», точки) |
| `accent-on` | Текст/иконка поверх `accent-fill` |
| `primary` | Плашки (30%) |
| `primary-text` | Заголовки страниц и полок, иконки разделов |
| `on-primary` | Текст/иконка на плашке — всегда алебастр `#F9F6F0` |

Пользователь выбирает цвет → в эти токены подставляется набор этого цвета для текущей темы. Для пресета Sir.kov основной — фирменный Maroon, акцент — лайм/вольт. Для любого из 16 цветов акцентом становится сам цвет, а основным — его глубокий тон того же оттенка. Компонент никогда не обращается к конкретному цвету палитры напрямую (исключение — AccentPicker).

**Правило толщины:** тонкое (≤ 4px, текст, ссылки) — всегда `accent-text`; крупное (кнопки, кружки ≥ 22px) — `accent-fill`. Так контраст гарантирован даже у бледных заливок вроде лайма в светлой теме.

### Палитра (17 наборов, рассчитаны и проверены на контраст)

Гарантии: `-fill` ≥ 3:1 к фону (кроме фирменного лайма в светлой — 2.2:1, сознательно сохранён ради бренда); `-text` и `-primary-text` ≥ 4.5:1 к `bg-base`, `bg-layer`, `bg-flyout`; `-on` ≥ 4.5:1 к своей заливке; `on-primary` ≥ 7.4:1 к любой плашке.

| slug | Цвет | Тёмная: fill · text · on · primary · primary-text | Светлая: fill · text · on · primary · primary-text |
|---|---|---|---|
| `sirkov` | Sir.kov | `#ccff00` · `#ccff00` · `#1c1616` · `#590212` · `#ce7273` | `#66bb00` · `#407a00` · `#1c1616` · `#8c031c` · `#8c031c` |
| `lilac` | Лиловый | `#b98cf0` · `#b98cf0` · `#1c1616` · `#381b5b` · `#9a7fc8` | `#8a4fd6` · `#874cd3` · `#f9f6f0` · `#5b3291` · `#5b3291` |
| `coral` | Коралловый | `#ff8a73` · `#ff8a73` · `#1c1616` · `#590c03` · `#ca7565` | `#e5533d` · `#c5341f` · `#1c1616` · `#8d1c0d` · `#8d1c0d` |
| `blue` | Синий | `#6fa0ff` · `#6fa0ff` · `#1c1616` · `#0d2964` · `#688ccf` | `#2c68e7` · `#2662e0` · `#f9f6f0` · `#1d469d` · `#1d469d` |
| `gray` | Серый | `#a3a3a3` · `#a3a3a3` · `#1c1616` · `#342b2b` · `#f9f6f0` | `#6b6b6b` · `#6b6b6b` · `#f9f6f0` · `#534b4b` · `#1c1616` |
| `black` | Чёрный | `#f9f6f0` · `#f9f6f0` · `#1c1616` · `#342b2b` · `#f9f6f0` | `#1c1616` · `#1c1616` · `#f9f6f0` · `#534b4b` · `#1c1616` |
| `red` | Красный | `#ff5a5a` · `#ff5a5a` · `#1c1616` · `#590b0a` · `#ca746a` | `#dc1e20` · `#d50f18` · `#f9f6f0` · `#8d1b18` · `#8d1b18` |
| `brick` | Кирпичный | `#d9765a` · `#d9765a` · `#1c1616` · `#561100` · `#c67762` | `#b0482c` · `#b0482c` · `#f9f6f0` · `#8a2302` · `#8a2302` |
| `violet` | Фиолетовый | `#9b7bff` · `#9b7bff` · `#1c1616` · `#2c2061` · `#8a85cf` | `#6a3df0` · `#6a3df0` · `#f9f6f0` · `#4a3999` · `#4a3999` |
| `lavender` | Сиреневый | `#d4a8f0` · `#d4a8f0` · `#1c1616` · `#3f1855` · `#a57cc0` | `#a86ccf` · `#8c51b1` · `#1c1616` · `#662d88` · `#662d88` |
| `orange` | Оранжевый | `#ff9a3d` · `#ff9a3d` · `#1c1616` · `#4b2000` · `#b68263` | `#d76b09` · `#ac5300` · `#1c1616` · `#783904` · `#783904` |
| `yellow` | Жёлтый | `#f5c842` · `#f5c842` · `#1c1616` · `#3c2b00` · `#9e8a61` | `#ad8103` · `#88650d` · `#1c1616` · `#624802` · `#624802` |
| `ginger` | Рыжий | `#f08a45` · `#f08a45` · `#1c1616` · `#501a00` · `#bd7d63` | `#bf4d09` · `#b54a0d` · `#f9f6f0` · `#813001` · `#813001` |
| `darkred` | Тёмно-красный | `#bd2e30` · `#eb5b56` · `#f9f6f0` · `#590a0e` · `#ca736d` | `#a8121e` · `#a8121e` · `#f9f6f0` · `#8d1a1e` · `#8d1a1e` |
| `lightred` | Светло-красный | `#ff8a8a` · `#ff8a8a` · `#1c1616` · `#590a0f` · `#ca736e` | `#f05252` · `#cc2d35` · `#1c1616` · `#8d1a20` · `#8d1a20` |
| `burgundy` | Бургунди | `#ac3a61` · `#dc668a` · `#f9f6f0` · `#560929` · `#ca748c` | `#8f1d4a` · `#8f1d4a` · `#f9f6f0` · `#891846` · `#891846` |
| `bordeaux` | Бордовый | `#a64641` · `#d57069` · `#f9f6f0` · `#590a0d` · `#ca736c` | `#7a1c1c` · `#7a1c1c` · `#f9f6f0` · `#852725` · `#852725` |

### Особые случаи

- **Sir.kov / светлая:** лайм `#66BB00` идёт заливкой только с тёмным содержимым `#1C1616`; ссылки и прогресс — `#407A00`.
- **Чёрный** в тёмной теме — алебастр с тёмным содержимым, иначе исчез бы.
- **Серый и Чёрный** — монохромные режимы, их плашки — тёплый тёмно-серый.
- **Тёмно-красный, Бургунди, Бордовый** в тёмной теме остаются глубокими с алебастровым содержимым, не высветляются до розового.
- Содержимое на заливке выбирается между `#1C1616` и `#F9F6F0` по контрасту — всегда бери `accent-on`, не хардкодь.

### Авто-режим (цвет из обложки)

1. Уменьши обложку до 64×64, переведи пиксели в OKLab/OKLCH.
2. Отбрось почти чёрные (L < 0.12) и почти белые (L > 0.95).
3. k-means (k = 5) в OKLab. Доминанта = максимум `доля × (0.3 + chroma)`.
4. Chroma доминанты < 0.04 → **Серый** (или **Чёрный**, если L < 0.3 или > 0.85).
5. Иначе — ближайший по OKLab из 15 хроматических наборов (14 цветов + лайм Sir.kov — единственный зелёный), сравнивая со светлой заливкой набора.
6. Результат кэшировать на альбом. Смена цвета анимируется 400 мс (но начинается < 100 мс — Доэрти).

Сырые цвета обложки **никогда** не попадают в контролы — только в размытый фон «Сейчас играет» и текста песни.

---

## 5. Типографика

Везде один шрифт — **Geologica** (Google Fonts, OFL), вариативный: `wght` 100–900, `CRSV` 0–1, `SHRP` 0–100, `slnt` −12…0. Кириллица есть. Оси `CRSV` и `SHRP` держим в 0. Фолбэк: `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`. Шрифт **встраиваем в приложение**, не грузим из сети.

В Geologica есть `tnum`, но по умолчанию цифры пропорциональные — поэтому таймкоды, длительности, счётчики и проценты **всегда** с `font-variant-numeric: tabular-nums`.

| Стиль | Размер / строка | Вес | Где |
|---|---|---|---|
| `hero` | 40 / 48 | 700 | Название альбома/исполнителя в шапке |
| `page-title` | 28 / 36 | 700 | «Главная», «Медиатека» — цвет `primary-text` |
| `shelf-title` | 20 / 28 | 600 | Заголовок полки — `primary-text`; «Сиртон ⌄» в сайдбаре — `text-primary` |
| `card-title` | 16 / 22 | 600 | Заголовок панели |
| `body` | 15 / 22 | 400 | Базовый текст, пункты сайдбара, название трека |
| `body-strong` | 15 / 22 | 600 | Кнопки, название в плеере |
| `menu` | 14 / 20 | 400 | Пункты меню, чипы |
| `caption` | 13 / 18 | 400 | Исполнитель, подписи |
| `badge` | 11 / 14 | 600 | Бейджи, капсом, разрядка 0.06em |
| `group-label` | 13 / 18 | 600 | Заголовки групп сайдбара |
| `timecode` | 12 / 16 | 400 | Таймкоды — `tabular-nums` |
| `shortcut` | 12 / 16 | 400 | Горячие клавиши в меню |
| `lyrics` | 32 / 40 | 700 | Текст песни |

Пара «название + исполнитель»: название `body` `text-primary`, исполнитель `caption` `text-secondary` (ссылка). Длинное — многоточие.

---

## 6. Отступы, радиусы, размеры, тени

**Отступы:** `space-1` 4 · `space-1-5` 6 · `space-2` 8 · `space-3` 12 · `space-4` 16 · `space-6` 24 · `space-8` 32 · `space-12` 48. Между полками/блоками 48, внутри 16.

**Радиусы:** `radius-sm` 6 (мини-обложки) · `radius-md` 10 (пилюли пунктов, строки, обложки в карточках) · `radius-lg` 16 (меню, поиск, большая обложка) · `radius-xl` 20 (панели) · `radius-full` (кнопки, чипы, бейджи, Play, чекбокс, фото исполнителя, аватар).

**Размеры:** `titlebar-height` 48 · `sidebar-width` 260 (свёрнут 64) · `player-height` 84 · `item-height` 40 · `button-height` 36 · `play-size` 44 · `chip-height` 32 · `row-height` 52 · `icon-size` 20 · `artwork-card` 184.

**Тени:** `shadow-flyout` `0 12px 32px rgba(0,0,0,.32)` — только меню; `shadow-artwork` `0 4px 16px rgba(0,0,0,.24)` — только обложки. У панелей тени нет, только `border`.

---

## 7. Раскладка

| Зона | Размер | Фон | Роль |
|---|---|---|---|
| Сайдбар | 260 (64 свёрнут) | `bg-base` | Навигация |
| Контент | остальное, поля 24 | `bg-layer` | Полки, таблицы, шапки |
| Плеер | 84 снизу | `bg-base` + `separator` сверху | Управление, не прокручивается |

- Заголовок окна 48px, системные кнопки Windows справа.
- Сайдбар сворачивается в иконки при ширине окна < 900px.
- Карточки держат ширину 184; при ресайзе меняется число колонок.

## 8. Иконки

Контурные, 20px, обводка 1.5px, скруглённые концы — **Lucide** (`stroke-width: 1.5`), один набор на всё приложение. Цвет `icon`; активная — `text-primary`; на плашке — `on-primary`. Заливная только Play/Pause.

---

## 9. Компоненты

### Button (пилюли)
- **Primary** («Слушать»): `accent-fill`, текст `accent-on`, `body-strong`, 36px, `radius-full`. **Одна на экран.**
- **Secondary** («Перемешать»): прозрачный фон, `border`, `text-primary`, hover `bg-hover`.
- **Ghost**: без обводки, hover `bg-hover`.
- **Icon**: круг 36×36, иконка `icon`.
- Порядок: Primary → Secondary → Icon. Disabled — opacity 0.4. Нажатие — отклик < 100 мс.

### Chip и Badge (главное место `primary` в контенте)
- **Чип-фильтр**: 32px, `radius-full`, отступы 12, `menu`. Неактивный — `border` + `text-primary`; активный — плашка `primary` + `on-primary`. ≤ 7 в ряду, дальше «Ещё ⌄». Где: фильтры Медиатеки и поиска.
- **Бейдж**: 20px, `radius-full`, `badge` капсом. Основной — `primary` + `on-primary` («Lossless», «Hi-Res», «Новое»). Тихий — обводка `inactive` + `text-secondary` («Explicit», «Локальный»).
- **Счётчик уведомлений**: кружок `accent-fill` + `accent-on`.

### MenuFlyout
- `bg-flyout`, `border`, `radius-lg`, `shadow-flyout`, отступ 6, ширина 240–320.
- Шапка (необязательна): обложка/аватар 32–36 + `body-strong` + `caption`.
- Пункт: 40px, пилюля `radius-md`, иконка 20 + 12 + `menu`; справа горячая клавиша (`shortcut`, `text-tertiary`), значение (`text-secondary`) или `›`.
- Разделитель `separator` с отступами 12. Деструктивное — внизу, `danger`.
- Меню трека везде одинаковое (≤ 7): Воспроизвести далее → В конец очереди → Добавить в плейлист › → К альбому → К исполнителю → Поделиться → — → Удалить из медиатеки.
- Появление 150 мс (fade + сдвиг 4px), стрелки двигают подсветку.

### Sidebar
- Шапка: «Сиртон ⌄» (`shelf-title`, меню аккаунта/настроек) + иконки уведомлений и поиска.
- «Новый плейлист», Главная, Новое, Радио; группы «Медиатека», «Плейлисты» (`group-label`, шеврон ⌄, сворачиваются).
- Пункт 40px, иконка 20 + 12 + `body`. Вложенные — с отступом, без иконки.
- Hover: `bg-hover` + «…» и карандаш справа. **Выбранный: плашка `primary`, текст и иконка `on-primary`.** Играющий плейлист — полоски `accent-text` справа.
- Hover на плейлисте > 500 мс — поповер с деталями. Скроллбар тонкий, только при наведении. Внизу аватар-круг 40. Пока идёт импорт — строка прогресса в сайдбаре (Зейгарник).

### Panel
- **Нейтральная**: `bg-raised`, `border`, `radius-xl`, отступ 24, без тени. Шапка `card-title` + «1 из 4» `text-tertiary`. Прогресс 8px: дорожка `inactive`, заполнено `accent-text`. Чек-лист: кружок 22 с обводкой 1.5 `text-secondary`; выполненный — `accent-fill` + галочка `accent-on`, текст `text-tertiary` зачёркнут.
- **Брендовая**: заливка `primary`, без обводки, текст `on-primary`, заголовок `shelf-title`, внутри одна Primary-кнопка. Одна на экран — для главного незавершённого дела («Продолжить прослушивание»).

### AccentPicker (настройки → «Цвет»)
- «Авто» (4 сектора) → **Sir.kov** (кружок пополам Maroon/лайм, по умолчанию) → 16 кружков 32px `accent-<slug>-fill`: холодные → нейтральные → тёплые → красные.
- Выбранный: кольцо 2px `text-primary` с зазором 3px + галочка.
- Под сеткой превью: плашка `primary`, Primary-кнопка, прогресс.
- Применяется сразу (< 100 мс), перетекание 400 мс, без «Сохранить».

### PlayerBar
- Слева: обложка 56 `radius-sm`, название `body-strong`, исполнитель `caption` (ссылка). Клик по обложке → «Сейчас играет».
- Центр: перемешать, назад, **Play/Pause** (44px, `accent-fill`, иконка `accent-on`), вперёд, повтор. Под ними прогресс: таймкоды `timecode` `text-tertiary`, полоса 4px `inactive`, заполнено и бегунок 12px — `accent-text`; бегунок при наведении, зона клика 16px.
- Справа: текст песни, очередь, громкость (96px).
- Включённые перемешивание/повтор — иконка `accent-text` + точка 4px.
- Длинное название — бегущая строка только при наведении.

### AlbumCard
- 184 ширина, квадратная обложка `radius-md` + `shadow-artwork`; через 12 — название `body` (1 строка) и подпись `caption`.
- Hover: `artwork-scrim` + круглые Play (`accent-fill`) и «…» (`bg-flyout` + `border`).
- Полка: 6–8 карточек, заголовок `shelf-title` цветом `primary-text`, справа «Ещё» `accent-text`.

### ArtistCard
- Круглое фото, без тени, имя по центру `body`. Hover — пилюля `bg-hover` `radius-lg`. Без фото — `artwork-placeholder` + инициалы.

### TrackRow
- 52px, пилюля `radius-md`, отступы 12, без разделителей.
- Номер (`timecode`) → [мини-обложка 40 в плейлистах] → название + исполнитель → альбом → длительность справа.
- Hover: `bg-hover`, номер → Play, справа «…». Клик — `bg-selected`. Двойной клик — играть. Правый — меню.
- Играющий: полоски `accent-text` вместо номера, название `accent-text`.

### «Сейчас играет»
- Фон — размытый градиент 2–3 доминантных цветов обложки (blur 80–120px), дрейф 20–30 с на цикл, при необходимости `artwork-scrim`.
- Большая обложка `radius-lg`. Текст поверх — алебастр с уровнями прозрачности.
- Текст песни `lyrics`: активная строка яркая, остальные приглушены и слегка размыты, автопрокрутка.

### Пустые состояния и загрузка
- Загрузка — скелетоны `artwork-placeholder` с мягкой пульсацией, без спиннеров.
- Пусто — иллюстрация из примитивов (круги/пилюли) + одна строка + одна Primary-кнопка («Добавить папку с музыкой»).
- Ошибка — тост внизу справа, `bg-flyout`, иконка `danger`, действие «Повторить». Модальные окна ошибок запрещены.

---

## 10. Взаимодействие и движение

- Клик — выделить, двойной — играть, правый — меню.
- Клавиатура: Пробел — пауза, Ctrl+→/← — трек, Ctrl+L — текст, Ctrl+F — поиск, Ctrl+, — настройки, Del — удалить. Фокус виден всегда (`focus-ring` 2px, отступ 2px).
- Медиаклавиши и системная медиапанель Windows (SMTC) — обязательно.
- Анимации объясняют пространство: обложка вырастает в «Сейчас играет», меню — из точки вызова.
- Длительности: 150–250 мс на переходы, 400 мс на смену темы/цвета. Кривая — decelerate. Ни одна анимация не задерживает действие.
- Системное «уменьшить движение» — только fade.

## 11. Windows-специфика

- `bg-base` под **Mica**, если стек позволяет; иначе сплошной цвет.
- Меню можно на **Acrylic**, но цвет и обводка из токенов сохраняются.
- Системные кнопки окна справа, перетаскивание за заголовок.
- Корректно на 100–200% DPI, все размеры в логических пикселях.

## 12. Запреты

- Хардкод цвета, отступа, радиуса, шрифта — только токены.
- Любой шрифт, кроме Geologica.
- Текст цветом `inactive`.
- `primary` на больших фонах; акцент как декор; два акцента рядом.
- Белый текст на заливке «на глаз» — только `accent-on` / `on-primary`.
- Больше одной Primary-кнопки или одной брендовой панели на экран.
- Прямоугольные кнопки; тени у панелей.
- Цвета обложки в контролах напрямую.
- Модальные ошибки; спиннеры вместо скелетонов; действия без отклика > 100 мс.
- Кастомные кнопки окна.

---

## 13. Токены кодом (CSS custom properties)

Для веб-стека (Tauri / Electron / WebView2) — используй как есть. Для нативного (WinUI 3 / XAML) — перенеси те же имена и значения в ресурсы тем.

Переключение: `<html data-theme="dark|light" data-accent="<slug>">`. По умолчанию `data-accent="sirkov"`, тема — системная.

```css
@font-face {
  font-family: "Geologica";
  src: url("./fonts/Geologica-Variable.ttf") format("truetype");
  font-weight: 100 900;
  font-display: block;
}

:root {
  /* Шрифт: встроить Geologica-Variable.ttf через @font-face */
  --font-sans: Geologica, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  /* spacing */
  --space-1: 4px;
  --space-1-5: 6px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-6: 24px;
  --space-8: 32px;
  --space-12: 48px;
  /* radius */
  --radius-sm: 6px;
  --radius-md: 10px;
  --radius-lg: 16px;
  --radius-xl: 20px;
  --radius-full: 9999px;
  /* size */
  --titlebar-height: 48px;
  --sidebar-width: 260px;
  --player-height: 84px;
  --item-height: 40px;
  --button-height: 36px;
  --play-size: 44px;
  --chip-height: 32px;
  --row-height: 52px;
  --icon-size: 20px;
  --artwork-card: 184px;
  /* shadow */
  --shadow-flyout: 0 12px 32px rgba(0,0,0,0.32);
  --shadow-artwork: 0 4px 16px rgba(0,0,0,0.24);
  /* motion */
  --ease-out: cubic-bezier(0.1, 0.9, 0.2, 1);
  --dur-instant: 100ms;
  --dur-fast: 150ms;
  --dur-base: 250ms;
  --dur-theme: 400ms;
}

/* Нейтрали: тёмная — по умолчанию */
:root, [data-theme="dark"] {
  --bg-base: #161111;
  --bg-layer: #1c1616;
  --bg-raised: #241d1d;
  --bg-flyout: #2b2323;
  --bg-hover: rgba(249,246,240,0.07);
  --bg-selected: rgba(249,246,240,0.11);
  --inactive: #3d3535;
  --border: rgba(249,246,240,0.10);
  --separator: rgba(249,246,240,0.10);
  --text-primary: #f9f6f0;
  --text-secondary: #c4bbb5;
  --text-tertiary: #a39994;
  --icon: #c4bbb5;
  --focus-ring: #f9f6f0;
  --artwork-placeholder: #2b2323;
  --artwork-scrim: rgba(0,0,0,0.45);
  --danger: #ff6b6b;
  --on-primary: #f9f6f0;
}

[data-theme="light"] {
  --bg-base: #f2eee6;
  --bg-layer: #f9f6f0;
  --bg-raised: #fdfcf9;
  --bg-flyout: #fdfcf9;
  --bg-hover: rgba(28,22,22,0.05);
  --bg-selected: rgba(28,22,22,0.08);
  --inactive: #c2bcb2;
  --border: rgba(28,22,22,0.10);
  --separator: rgba(28,22,22,0.09);
  --text-primary: #1c1616;
  --text-secondary: #5c5351;
  --text-tertiary: #6f6563;
  --icon: #5c5351;
  --focus-ring: #1c1616;
  --artwork-placeholder: #e8e3da;
  --artwork-scrim: rgba(0,0,0,0.45);
  --danger: #c62828;
  --on-primary: #f9f6f0;
}

/* Цвет пользователя → 5 активных токенов (+ on-primary общий). По умолчанию data-accent="sirkov" */
[data-theme="dark"][data-accent="sirkov"], :root:not([data-theme])[data-accent="sirkov"] { --accent-fill: #ccff00; --accent-text: #ccff00; --accent-on: #1c1616; --primary: #590212; --primary-text: #ce7273; } /* Sir.kov */
[data-theme="light"][data-accent="sirkov"] { --accent-fill: #66bb00; --accent-text: #407a00; --accent-on: #1c1616; --primary: #8c031c; --primary-text: #8c031c; } /* Sir.kov */
[data-theme="dark"][data-accent="lilac"], :root:not([data-theme])[data-accent="lilac"] { --accent-fill: #b98cf0; --accent-text: #b98cf0; --accent-on: #1c1616; --primary: #381b5b; --primary-text: #9a7fc8; } /* Лиловый */
[data-theme="light"][data-accent="lilac"] { --accent-fill: #8a4fd6; --accent-text: #874cd3; --accent-on: #f9f6f0; --primary: #5b3291; --primary-text: #5b3291; } /* Лиловый */
[data-theme="dark"][data-accent="coral"], :root:not([data-theme])[data-accent="coral"] { --accent-fill: #ff8a73; --accent-text: #ff8a73; --accent-on: #1c1616; --primary: #590c03; --primary-text: #ca7565; } /* Коралловый */
[data-theme="light"][data-accent="coral"] { --accent-fill: #e5533d; --accent-text: #c5341f; --accent-on: #1c1616; --primary: #8d1c0d; --primary-text: #8d1c0d; } /* Коралловый */
[data-theme="dark"][data-accent="blue"], :root:not([data-theme])[data-accent="blue"] { --accent-fill: #6fa0ff; --accent-text: #6fa0ff; --accent-on: #1c1616; --primary: #0d2964; --primary-text: #688ccf; } /* Синий */
[data-theme="light"][data-accent="blue"] { --accent-fill: #2c68e7; --accent-text: #2662e0; --accent-on: #f9f6f0; --primary: #1d469d; --primary-text: #1d469d; } /* Синий */
[data-theme="dark"][data-accent="gray"], :root:not([data-theme])[data-accent="gray"] { --accent-fill: #a3a3a3; --accent-text: #a3a3a3; --accent-on: #1c1616; --primary: #342b2b; --primary-text: #f9f6f0; } /* Серый */
[data-theme="light"][data-accent="gray"] { --accent-fill: #6b6b6b; --accent-text: #6b6b6b; --accent-on: #f9f6f0; --primary: #534b4b; --primary-text: #1c1616; } /* Серый */
[data-theme="dark"][data-accent="black"], :root:not([data-theme])[data-accent="black"] { --accent-fill: #f9f6f0; --accent-text: #f9f6f0; --accent-on: #1c1616; --primary: #342b2b; --primary-text: #f9f6f0; } /* Чёрный */
[data-theme="light"][data-accent="black"] { --accent-fill: #1c1616; --accent-text: #1c1616; --accent-on: #f9f6f0; --primary: #534b4b; --primary-text: #1c1616; } /* Чёрный */
[data-theme="dark"][data-accent="red"], :root:not([data-theme])[data-accent="red"] { --accent-fill: #ff5a5a; --accent-text: #ff5a5a; --accent-on: #1c1616; --primary: #590b0a; --primary-text: #ca746a; } /* Красный */
[data-theme="light"][data-accent="red"] { --accent-fill: #dc1e20; --accent-text: #d50f18; --accent-on: #f9f6f0; --primary: #8d1b18; --primary-text: #8d1b18; } /* Красный */
[data-theme="dark"][data-accent="brick"], :root:not([data-theme])[data-accent="brick"] { --accent-fill: #d9765a; --accent-text: #d9765a; --accent-on: #1c1616; --primary: #561100; --primary-text: #c67762; } /* Кирпичный */
[data-theme="light"][data-accent="brick"] { --accent-fill: #b0482c; --accent-text: #b0482c; --accent-on: #f9f6f0; --primary: #8a2302; --primary-text: #8a2302; } /* Кирпичный */
[data-theme="dark"][data-accent="violet"], :root:not([data-theme])[data-accent="violet"] { --accent-fill: #9b7bff; --accent-text: #9b7bff; --accent-on: #1c1616; --primary: #2c2061; --primary-text: #8a85cf; } /* Фиолетовый */
[data-theme="light"][data-accent="violet"] { --accent-fill: #6a3df0; --accent-text: #6a3df0; --accent-on: #f9f6f0; --primary: #4a3999; --primary-text: #4a3999; } /* Фиолетовый */
[data-theme="dark"][data-accent="lavender"], :root:not([data-theme])[data-accent="lavender"] { --accent-fill: #d4a8f0; --accent-text: #d4a8f0; --accent-on: #1c1616; --primary: #3f1855; --primary-text: #a57cc0; } /* Сиреневый */
[data-theme="light"][data-accent="lavender"] { --accent-fill: #a86ccf; --accent-text: #8c51b1; --accent-on: #1c1616; --primary: #662d88; --primary-text: #662d88; } /* Сиреневый */
[data-theme="dark"][data-accent="orange"], :root:not([data-theme])[data-accent="orange"] { --accent-fill: #ff9a3d; --accent-text: #ff9a3d; --accent-on: #1c1616; --primary: #4b2000; --primary-text: #b68263; } /* Оранжевый */
[data-theme="light"][data-accent="orange"] { --accent-fill: #d76b09; --accent-text: #ac5300; --accent-on: #1c1616; --primary: #783904; --primary-text: #783904; } /* Оранжевый */
[data-theme="dark"][data-accent="yellow"], :root:not([data-theme])[data-accent="yellow"] { --accent-fill: #f5c842; --accent-text: #f5c842; --accent-on: #1c1616; --primary: #3c2b00; --primary-text: #9e8a61; } /* Жёлтый */
[data-theme="light"][data-accent="yellow"] { --accent-fill: #ad8103; --accent-text: #88650d; --accent-on: #1c1616; --primary: #624802; --primary-text: #624802; } /* Жёлтый */
[data-theme="dark"][data-accent="ginger"], :root:not([data-theme])[data-accent="ginger"] { --accent-fill: #f08a45; --accent-text: #f08a45; --accent-on: #1c1616; --primary: #501a00; --primary-text: #bd7d63; } /* Рыжий */
[data-theme="light"][data-accent="ginger"] { --accent-fill: #bf4d09; --accent-text: #b54a0d; --accent-on: #f9f6f0; --primary: #813001; --primary-text: #813001; } /* Рыжий */
[data-theme="dark"][data-accent="darkred"], :root:not([data-theme])[data-accent="darkred"] { --accent-fill: #bd2e30; --accent-text: #eb5b56; --accent-on: #f9f6f0; --primary: #590a0e; --primary-text: #ca736d; } /* Тёмно-красный */
[data-theme="light"][data-accent="darkred"] { --accent-fill: #a8121e; --accent-text: #a8121e; --accent-on: #f9f6f0; --primary: #8d1a1e; --primary-text: #8d1a1e; } /* Тёмно-красный */
[data-theme="dark"][data-accent="lightred"], :root:not([data-theme])[data-accent="lightred"] { --accent-fill: #ff8a8a; --accent-text: #ff8a8a; --accent-on: #1c1616; --primary: #590a0f; --primary-text: #ca736e; } /* Светло-красный */
[data-theme="light"][data-accent="lightred"] { --accent-fill: #f05252; --accent-text: #cc2d35; --accent-on: #1c1616; --primary: #8d1a20; --primary-text: #8d1a20; } /* Светло-красный */
[data-theme="dark"][data-accent="burgundy"], :root:not([data-theme])[data-accent="burgundy"] { --accent-fill: #ac3a61; --accent-text: #dc668a; --accent-on: #f9f6f0; --primary: #560929; --primary-text: #ca748c; } /* Бургунди */
[data-theme="light"][data-accent="burgundy"] { --accent-fill: #8f1d4a; --accent-text: #8f1d4a; --accent-on: #f9f6f0; --primary: #891846; --primary-text: #891846; } /* Бургунди */
[data-theme="dark"][data-accent="bordeaux"], :root:not([data-theme])[data-accent="bordeaux"] { --accent-fill: #a64641; --accent-text: #d57069; --accent-on: #f9f6f0; --primary: #590a0d; --primary-text: #ca736c; } /* Бордовый */
[data-theme="light"][data-accent="bordeaux"] { --accent-fill: #7a1c1c; --accent-text: #7a1c1c; --accent-on: #f9f6f0; --primary: #852725; --primary-text: #852725; } /* Бордовый */

/* Палитра целиком — только для AccentPicker */
:root, [data-theme="dark"] {
  --accent-sirkov-fill: #ccff00; --accent-sirkov-on: #1c1616; --accent-sirkov-primary: #590212;
  --accent-lilac-fill: #b98cf0; --accent-lilac-on: #1c1616; --accent-lilac-primary: #381b5b;
  --accent-coral-fill: #ff8a73; --accent-coral-on: #1c1616; --accent-coral-primary: #590c03;
  --accent-blue-fill: #6fa0ff; --accent-blue-on: #1c1616; --accent-blue-primary: #0d2964;
  --accent-gray-fill: #a3a3a3; --accent-gray-on: #1c1616; --accent-gray-primary: #342b2b;
  --accent-black-fill: #f9f6f0; --accent-black-on: #1c1616; --accent-black-primary: #342b2b;
  --accent-red-fill: #ff5a5a; --accent-red-on: #1c1616; --accent-red-primary: #590b0a;
  --accent-brick-fill: #d9765a; --accent-brick-on: #1c1616; --accent-brick-primary: #561100;
  --accent-violet-fill: #9b7bff; --accent-violet-on: #1c1616; --accent-violet-primary: #2c2061;
  --accent-lavender-fill: #d4a8f0; --accent-lavender-on: #1c1616; --accent-lavender-primary: #3f1855;
  --accent-orange-fill: #ff9a3d; --accent-orange-on: #1c1616; --accent-orange-primary: #4b2000;
  --accent-yellow-fill: #f5c842; --accent-yellow-on: #1c1616; --accent-yellow-primary: #3c2b00;
  --accent-ginger-fill: #f08a45; --accent-ginger-on: #1c1616; --accent-ginger-primary: #501a00;
  --accent-darkred-fill: #bd2e30; --accent-darkred-on: #f9f6f0; --accent-darkred-primary: #590a0e;
  --accent-lightred-fill: #ff8a8a; --accent-lightred-on: #1c1616; --accent-lightred-primary: #590a0f;
  --accent-burgundy-fill: #ac3a61; --accent-burgundy-on: #f9f6f0; --accent-burgundy-primary: #560929;
  --accent-bordeaux-fill: #a64641; --accent-bordeaux-on: #f9f6f0; --accent-bordeaux-primary: #590a0d;
}
[data-theme="light"] {
  --accent-sirkov-fill: #66bb00; --accent-sirkov-on: #1c1616; --accent-sirkov-primary: #8c031c;
  --accent-lilac-fill: #8a4fd6; --accent-lilac-on: #f9f6f0; --accent-lilac-primary: #5b3291;
  --accent-coral-fill: #e5533d; --accent-coral-on: #1c1616; --accent-coral-primary: #8d1c0d;
  --accent-blue-fill: #2c68e7; --accent-blue-on: #f9f6f0; --accent-blue-primary: #1d469d;
  --accent-gray-fill: #6b6b6b; --accent-gray-on: #f9f6f0; --accent-gray-primary: #534b4b;
  --accent-black-fill: #1c1616; --accent-black-on: #f9f6f0; --accent-black-primary: #534b4b;
  --accent-red-fill: #dc1e20; --accent-red-on: #f9f6f0; --accent-red-primary: #8d1b18;
  --accent-brick-fill: #b0482c; --accent-brick-on: #f9f6f0; --accent-brick-primary: #8a2302;
  --accent-violet-fill: #6a3df0; --accent-violet-on: #f9f6f0; --accent-violet-primary: #4a3999;
  --accent-lavender-fill: #a86ccf; --accent-lavender-on: #1c1616; --accent-lavender-primary: #662d88;
  --accent-orange-fill: #d76b09; --accent-orange-on: #1c1616; --accent-orange-primary: #783904;
  --accent-yellow-fill: #ad8103; --accent-yellow-on: #1c1616; --accent-yellow-primary: #624802;
  --accent-ginger-fill: #bf4d09; --accent-ginger-on: #f9f6f0; --accent-ginger-primary: #813001;
  --accent-darkred-fill: #a8121e; --accent-darkred-on: #f9f6f0; --accent-darkred-primary: #8d1a1e;
  --accent-lightred-fill: #f05252; --accent-lightred-on: #1c1616; --accent-lightred-primary: #8d1a20;
  --accent-burgundy-fill: #8f1d4a; --accent-burgundy-on: #f9f6f0; --accent-burgundy-primary: #891846;
  --accent-bordeaux-fill: #7a1c1c; --accent-bordeaux-on: #f9f6f0; --accent-bordeaux-primary: #852725;
}

/* Стили текста */
.t-hero { font: 700 40px/48px var(--font-sans); }
.t-page-title { font: 700 28px/36px var(--font-sans); }
.t-shelf-title { font: 600 20px/28px var(--font-sans); }
.t-card-title { font: 600 16px/22px var(--font-sans); }
.t-body { font: 400 15px/22px var(--font-sans); }
.t-body-strong { font: 600 15px/22px var(--font-sans); }
.t-menu { font: 400 14px/20px var(--font-sans); }
.t-caption { font: 400 13px/18px var(--font-sans); }
.t-badge { font: 600 11px/14px var(--font-sans); }
.t-group-label { font: 600 13px/18px var(--font-sans); }
.t-timecode { font: 400 12px/16px var(--font-sans); }
.t-shortcut { font: 400 12px/16px var(--font-sans); }
.t-lyrics { font: 700 32px/40px var(--font-sans); }
.t-badge { text-transform: uppercase; letter-spacing: 0.06em; }
.t-timecode, .t-shortcut, .tabular { font-variant-numeric: tabular-nums; }
html { font-family: var(--font-sans); font-variation-settings: "CRSV" 0, "SHRP" 0; }
@media (prefers-reduced-motion: reduce) { * { animation-duration: 0.01ms !important; transition-duration: 0.01ms !important; } }
```

---

## 14. Чек-лист приёмки

- [ ] Ни одного хардкода вне токенов; шрифт только Geologica.
- [ ] Проверено в обеих темах и минимум с четырьмя цветами: **Sir.kov** (бледный лайм в светлой), **Чёрный**, **Жёлтый**, **Бордовый**.
- [ ] Текст ≥ 4.5:1, иконки и заливки ≥ 3:1 (кроме задокументированного лайма).
- [ ] 60–30–10 соблюдено: нейтраль доминирует, `primary` только на плашках, акцент только на CTA.
- [ ] Тонкие индикаторы — `accent-text`, крупные заливки — `accent-fill`.
- [ ] У каждого интерактивного элемента есть hover, selected, focus, disabled.
- [ ] Отклик ≤ 100 мс на прямые действия, ≤ 400 мс на переходы; оптимистичный UI и скелетоны на месте.
- [ ] Ошибки ввода исправляются молча (Постел), без модалок.
- [ ] Таймкоды в `tabular-nums`.
- [ ] Визуальная проверка скриншотом после каждой заметной правки.

## 15. Как работать (для Claude Code)

- Скиллы для интерфейса: ui-ux-pro-max (паттерны и UX), design-taste-frontend (без шаблонного вида), minimalist-ui (минимум шума), high-end-visual-design (финальная полировка), shadcn (паттерны компонентов, если стек React).
- MCP: context7 — актуальная документация библиотек; playwright — проверка, что сборка работает и выглядит как задумано; figma — если появятся макеты; shadcn — компоненты из реестра. Supabase — только если у плеера появится облачная часть.
- Сомневаешься — выбирай тише, а не ярче.


---

## 16. Редакция 2026-10-28 — AAA, материалы, «Рубин», движение (ТЗ владельца, D-127…D-134)

> **Статус: одобрено владельцем 2026-10-28 («одобряю»; Q-24…Q-29 — по рекомендациям `10` §7).** Раздел добавлен отдельным коммитом по прямому требованию ТЗ владельца «записать в DESIGN.md новые цвета/правила Рубина и AAA». Прежние §1–§15 **не переписаны**; где §16 им противоречит, **§16 главнее** (список — §16.7). Значения посчитаны скриптом по формуле WCAG (относительная яркость sRGB) 2026-10-28; точные значения 17 наборов §4 регенерируются в задаче T8.2 генератором `color.ts` и тестом матрицы. Снимок прежней редакции — `docs/source/Сиртон_дизайн_2026-10-16.md`.

### 16.1. Пороги и что не проходит сейчас (расчёт по реальным токенам)

Пороги (WCAG 2.2 AAA): текст ≥ 7:1; крупный (≥ 24 px или ≥ 19 px жирный) ≥ 4,5:1; иконки и границы контролов ≥ 3:1. Проверка — против **всех** фонов темы: `bg-base`, `bg-layer`, `bg-raised`, `bg-flyout`, стёкла, подложка плеера, шапки с обложкой.

| Токен (текущее значение) | Результат (base · layer · raised · flyout) | Вердикт AAA |
|---|---|---|
| Тёмная (лестница D-75) `text-secondary` #dcdcdc | 9,21 · 8,29 · 7,80 · 7,33 | проходит |
| Тёмная `text-tertiary` #c4c4c4 | 7,24 · 6,52 · 6,13 · 5,76 | **не проходит** (ТЗ оценивал 6,4) |
| Светлая `text-secondary` #5c5351 | 6,46 · 6,93 · 7,28 | **не проходит** на `bg-base` |
| Светлая `text-tertiary` #6f6563 | 4,88 · 5,24 · 5,51 | **не проходит** (ТЗ: 5,2) |
| Синий, тёмная, `accent-text` #85aefc | 5,69 · 5,12 · 4,82 · 4,53 | **не проходит** (ТЗ: 6,9) |
| Sir.kov светлая, `accent-text` #407a00 | 4,53 · 4,86 · 5,11 | **не проходит** (ссылки) |
| `danger` тёмная #ff9494 / светлая #c62828 | 5,97…4,75 / 4,86…5,48 | **не проходит** |
| `inactive` (дорожка) | 2,2…1,75 / 1,63…1,84 | декоративно, реестр исключений |

### 16.2. Нейтральные токены — новые значения (заменяют §3 и `extra.css` D-75 для текста и `danger`)

| Токен | Тёмная (лестница #333333…#424242) | Светлая | Проверено |
|---|---|---|---|
| `text-primary` | `#fafafa` | `#1c1616` (без изменений) | 12,1…9,6 · 15,4…17,4 |
| `text-secondary` | `#e8e8e8` | `#3e3736` | 10,3…8,2 · 10,5…11,9 |
| `text-tertiary` | `#d9d9d9` | `#504845` | ≥ 7,12 · 7,71…8,70 |
| `danger` (текст) | `#ffd0d0` | `#9c1212` | ≥ 7,27 на flyout · 7,23…8,16 |
| `icon` | `#dcdcdc` | `#5c5351` | ≥ 3:1 с запасом |

- **Следствие:** на тёмной лестнице три уровня текста сжимаются в узкий диапазон (#fafafa → #d9d9d9): иерархию несут **размер и вес**, а не только яркость. **Расчёт для Q-24:** на текущей лестнице `accent-text` синего на `bg-flyout` #424242 требует ≈ #c8d8ff (почти белый: #bcd0ff — лишь 6,51:1). Вариант «сжать верхние ступени» (`layer` #353535 · `raised` #383838 · `flyout` #3b3b3b, `bg-base` #333333 не трогаем) помогает умеренно: `text-tertiary` проходит уже с `#d2d2d2` (≥ 7,41), `danger` с `#ffc8c8` (≥ 7,65), но синему всё равно нужно ≈ `#b8cdff` (`#b0c9ff` даёт 6,74 на `#3b3b3b`) — **бледный пастельный тон при любой лестнице**. Это цена AAA для *текста* акцента в тёмной теме; заливки (`accent-fill`, ≥ 3:1) остаются насыщенными. Окончательные значения — по выбранной лестнице, генератором T8.2.
- Новый токен **`fill-text`** — заливка залитых элементов с текстом (бейдж, счётчик, «Слушать»): такой яркости, чтобы текст на ней держал ≥ 7:1.

### 16.3. Цветовые наборы §4 под AAA

- Правило: `accent-text`, `primary-text` ≥ 7:1 ко всем фонам; `accent-on` ≥ 7:1 к `accent-fill`; `on-primary` ≥ 7:1 к `primary`; `accent-fill` ≥ 3:1 к фонам; границы контролов ≥ 3:1.
- Реестр исключений (единственный, D-127): лайм Sir.kov светлой `#66bb00` — **только заливка** (2,24:1 к `bg-layer`, ради бренда), текст на нём `#1c1616` (7,38:1), вокруг кнопки тонкая кромка `#2e5a00` (3,36:1 к лайму, 7,55:1 к странице); **ссылки и прогресс этого набора — `#2e5a00`** (7,04…7,94:1 на светлых фонах); дорожка `inactive`; логотип и заглушки обложек.
- «Свой цвет»: если набор не проходит, генератор сдвигает светлоту того же тона в OKLCH до прохода; выбор цвета показывает «применён оттенок …» и образец «было → стало».

### 16.4. «Рубин» и «Тёмный рубин»

Опорные цвета владельца: `#EA0012` яркий красный · `#FEFFFF` чистый белый · `#F0F1F4` холодный светло-серый · `#C3C3C4` серый. Холодная пара; красный — часть темы (не меняется; блок «Цвет» — «Цвет задан темой»).

| Токен | Рубин (светлая) | Тёмный рубин |
|---|---|---|
| `bg-base` (сайдбар, плеер) | `#F0F1F4` | `#1C1E22` |
| `bg-layer` (контент) | `#FEFFFF` | `#23252A` |
| `bg-raised` | `#FEFFFF` + `border` | `#2A2C32` |
| `bg-flyout` | `#FEFFFF` + `border` + кромка стекла | `#31343A` |
| `inactive` (пустые дорожки) | `#C3C3C4` | `#4A4D55` |
| `text-primary` | `#16181D` (17,7 на белом) | `#ECECF0` |
| `text-secondary` | `#3D4048` (10,3 · 9,2 · 8,1: белый · сайдбар · плашка) | `#C9CCD3` |
| `text-tertiary` | `#464952` (≥ 7,0 на плашке) | `#C4C8D0` (7,44 на flyout) |
| `border` контролов | `#767A84` (4,29 на белом, 3,80 на сайдбаре) | `rgba(236,236,240,0.38)` |
| `bg-hover` | `rgba(22,24,29,0.05)` | `rgba(236,236,240,0.07)` |
| **плашка** `primary` (выбранный пункт, чип, бейдж) | `#E2E3E7`, текст `#16181D` (13,9:1), иконка выбранного — красная | `#33363D`, текст `#ECECF0` (10,3:1) |
| яркий красный (всё нетекстовое, ≥ 3:1) | `#EA0012` (4,65 на белом · 4,13 на сайдбаре · 3,63 на плашке) | `#F4303A` (≥ 3,17 на самом светлом фоне · 3,07 на плашке) |
| красный текст / заливка с текстом | `#A5000D` (8,04 на белом · 7,14 на сайдбаре; белый на нём 8,04) | `#FFADB1` (≥ 7,05 на четырёх фонах, **не на плашке**: 6,84) |
| `danger` (оттенок отличается от акцента: тон OKLCH 65° и 70° против 27° и 16° у красного; всегда иконка + текст) | `#6E3F00` (8,82 на белом · 7,83 на сайдбаре) | `#FFB454` (≥ 7,08) |

Правила:
1. **60–30–10:** 60 % холодная нейтраль; 30 % серые плашки (`primary`), **не красные**; 10 % красный. Заголовки страниц и полок — `text-primary` (почти чёрные), не красные.
2. **Белый текст на `#EA0012` запрещён** (4,66:1): текст на красном — только на `#A5000D`/`fill-text`. Красный текст — только на нейтральных фонах, не на плашке: светлая — `#A5000D` на плашке 6,28:1; тёмная — `#FFADB1` на плашке `#33363D` 6,84:1 (оба не проходят 7:1).
3. **Выбранный пункт** (плашка к фону 1,14:1) всегда несёт второй признак: красная иконка + вес 600 + явная разница с наведением.
4. Дорожка `#C3C3C4` к заполнению `#EA0012` — 2,65:1: допустимо (заполнение к фону 4,13:1, дорожка декоративна).
5. **Не только цветом:** «играет» — полоски, повтор/перемешать — точка, ошибка/удаление — иконка и текст (красный здесь — цвет акцента, а не ошибки).
6. Красная обложка: Play остаётся ≥ 3:1 к реальной подложке, иначе светлая/тёмная кромка. Белое стекло на белом — кромка 1 px `border`.
7. Исключения из Sir.kov Fabric (D-130): холодная палитра; тёмный текст на плашке; красный закреплён за темой; два красных.

### 16.5. Материалы (D-128)

Настройка `appearance.material`: **Плоский** (§3, слои и обводки) · **Стекло** (Mica окна; сайдбар, плеер, заголовок, меню, поповеры — полупрозрачные с размытием и светлой кромкой 1 px; плотность вычисляется из худшего случая контраста 7:1) · **Живой свет** (дрейфующее свечение из цветов обложки за контентом; без музыки — цвета темы и акцента; цикл 20–30 с; в «Сейчас играет» и визуализации не рисуется) · **Стекло + свет**. Запасной вариант — плотная подложка `bg-base` (Windows 10; прозрачность выкл.; экономия батареи; высококонтрастный режим — системные цвета; слабые пресеты; автоснижение D-118). Стекло — только «хром», не контент. Числа прозрачности здесь не даны: подбираются в T8.5 по замеру худшего случая (спайк S-25).

### 16.6. Движение и детали (D-129; дополняет §10)

- Переход экрана ≤ 250 мс; смена темы/цвета/материала 400 мс без белой вспышки; старт любой анимации ≤ 100 мс; кривая `--ease-out`. Смена темы кликом — круг из точки клика (D-131), автопереключение — мягкое перетекание.
- Микровзаимодействия: Play↔Pause морфится; лайк — «пульс»; нажатие — лёгкая пружина; обложка растёт из плеера в «Сейчас играет» и возвращается; карточки и строки появляются волной **только при первом открытии экрана** (не при прокрутке, не в виртуальных списках). Конкретные кривые и амплитуды — в T8.7 (временные до приёмки).
- Настройка «Анимации интерфейса» (вкл/выкл) + системное «уменьшить движение»: выкл. → мгновенная смена или fade ≤ 100 мс; иконки статичны.
- Блик 1 px по верхнему краю панелей, кнопок, карточек — своей яркости в каждой теме (значения подбираются в T8.8), линия `inset 0 1px 0`, чёткая при 100–200 % DPI.
- Анимированные иконки (Play/Pause, лайк, перемешать, повтор, волна, громкость): Lucide 1,5, два состояния + интерполяция.
- Зона клика ≥ 44×44 расширяется невидимо; визуальные размеры §6 не меняются (кнопка 36, чип 32, Play 44). Кольцо фокуса — двойное, ≥ 3:1 на любом фоне.
- Заглушка обложки: из названия (хэш → палитра темы), первая буква Geologica; плавная смена на настоящую.

### 16.7. Что в §1–§15 заменено

§3 значения `text-secondary/tertiary`, `danger` → §16.2 · §4 «Гарантии» 4,5:1 → 7:1 (§16.1) · §4 «Авто-режим»: сырой цвет обложки разрешён также в шапках альбома/исполнителя и подложке плеера (D-129) · §6 «Тени» дополняются материалами (§16.5) · §9 AccentPicker → раздел «Оформление» (D-131) · §10 длительности и «выразительное» движение → §16.6 · §14 чек-лист: «Текст ≥ 4.5:1» → «≥ 7:1 (AAA), иконки ≥ 3:1, исключения — реестр §16.3».
