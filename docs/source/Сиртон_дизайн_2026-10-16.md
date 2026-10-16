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
