# 01c. Референсы: умная волна, сезоны, события, режимы (дополнение владельца §25–38)

> Версия 1.3 · 2026-10-16 · продолжение `01a`/`01b` для девяти новых фишек дополнения «Сиртон — ТЗ, дополнение: умная волна, сезоны, события и режимы» (`docs/source/Сиртон_ТЗ_дополнение_волна_2026-10-16.md`).
> Метод тот же: как фишка работает у 6+ популярных приложений → что копируем → как адаптируем под офлайн-локальный «Сиртон». Уровни: ✔ первоисточник · ○ вторичный источник (прочитан) · ◌ знание проектировщика без перепроверки. `[Sxx]` — источники в §13 и в `01_RESEARCH.md` §4.
> Где массовых аналогов меньше шести, это сказано прямо.

---

## W-01. Самообучение: сигналы, «не сейчас», «никогда», политика обучения (фича A)

**Референсы (6):** Pandora · Spotify · YouTube Music · Яндекс Музыка · Apple Music · Deezer.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Pandora | 👍/👎 по ходу; 👎 не только пропускает трек, но **перестраивает станцию**, чтобы она не брала «такое»; **«I'm tired of this track»** — убрать трек «на полку» на время **без** дизлайка; действует **только на текущую станцию**, срок не раскрывается («вернётся со временем»), отменить нельзя | ○ | S90, S91 |
| Pandora Modes | Лайки в Discovery/Deep Cuts/Newly Released **переносятся** в основную станцию | ○ | S89 |
| Spotify | **Exclude from your Taste Profile** — плейлист (с 2023) и отдельные треки (с 10.2025) не влияют на рекомендации, прошлое и будущее прослушивание исключается; **Private Session** — прослушивание не попадает в профиль вкуса (есть жалобы на «протечки»); 30 с — порог «понравилось/пропустил» в BaRT | ✔/○ | S92, S93, S95 |
| YouTube Music | «Не рекомендовать исполнителя» (тест), «Not interested»; по исследованию Mozilla «не рекомендовать канал» и удаление из истории действуют заметно сильнее, чем дизлайк (43 % и 29 % против 12 %) | ○ | S97 |
| Яндекс «Моя волна» | лайк/дизлайк; по умолчанию сама смешивает новое и знакомое «в подходящей пропорции»; настройки «по характеру» | ○ | S98, S73 |
| Apple Music | Тумблер **Use Listening History**; Focus-фильтр отключает историю на время режима — прослушанное не влияет на рекомендации | ✔/○ | S99 |
| Deezer Flow | **«Не рекомендовать этот трек / исполнителя»** — больше не играет; Flow Tuner (2026): можно задать, что **не должно** попадать в волну настроения | ✔/○ | S100 |

**Копируем:**
- Три действия в плеере: лайк, «не сейчас» (аналог Pandora «tired of this track»: временно и **только в текущем контексте**), «никогда в волне» (аналог Deezer «не рекомендовать», с вариантом «исполнителя»).
- «Никогда» сильнее дизлайка (вывод Mozilla по YouTube): это жёсткое исключение, а не просто минус в модели.
- Политики обучения по режимам: `none` = Spotify Exclude / Apple «без истории»; `separate_model` = Pandora Crowd Faves (учится на реакциях «всех»), `context_only` — наше расширение для сна.
- Перенос лайков из особых режимов в основной вкус (Pandora Modes): лайк и «никогда» действуют везде, остальные сигналы режима — только внутри режима.

**Адаптация:** всё обучение локальное; «не сейчас» помечает трек в текущем контекстном «ведре» (вечер × сезон × режим) на 7 дней и даёт отрицательный сигнал только контекстной модели; «никогда» отменяется в настройках (у Pandora нельзя — недостаток, который не копируем).

---

## W-02. Прозрачность: «Почему этот трек», чипы «Состав волны», ручная настройка (фича A)

**Референсы (6):** Pandora · Spotify DJ · Spotify «Explore, Exploit, Explain» · YouTube Music Tuner · Яндекс «Настроить волну» · Spotify daylist.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Pandora | «Why was this track selected?» — объяснение по атрибутам Music Genome («мягкая рок-инструментовка, вокальные гармонии…»); критика: слишком общие фразы | ○ | S91 |
| Spotify DJ | голосовой комментарий «почему этот трек» (давно не слушал / похоже на недавнее); по данным Spotify, объяснение снижает пропуски | ○ | S94 |
| Spotify EEE (RecSys 2018) | бандит выбирает **пару «трек + объяснение»**; эксплуатация — лучшее ожидаемое, исследование — неопределённое | ○ | S95 |
| YouTube Music Tuner | артисты + **Familiar / Blend / Discover** + фильтры (Popular, Deep cuts, New releases, Pump-up, Chill, Upbeat, Downbeat, Focus); пустой результат — «No songs to play»; сохранение своего радио в библиотеку | ○ | S96 |
| Яндекс | «Настроить»: занятие, характер, настроение, язык — комбинируются, есть сброс | ✔/○ | S73 |
| Spotify daylist | название подборки отражает контекст и время обновления | ○ | S74 |

**Копируем:** «почему» = 1–2 главных вклада компонентов скоринга (как у Pandora, но из наших реальных причин, а не из общих атрибутов); чипы влияний как фильтры тюнера YouTube Music (нажатие выключает влияние); честная надпись при нехватке треков (YouTube «No songs to play» → у нас «расширили подборку»); сохранение настройки как своей волны (YouTube «сохранить радио»).
**Адаптация:** «почему» строится по вкладу `w_i·s_i` относительно среднего по кандидатам; текст — из шаблонов i18n, без генерации ИИ.

---

## W-03. Архитектура движка: кандидаты → ранжирование → секвенсор, исследование/эксплуатация (фича A, §26)

**Референсы (7):** YouTube (DNN, двухступенчатая схема) · Spotify BaRT/EEE · Deezer (контекстные бандиты) · Chapelle & Li (Томпсон для логистической регрессии) · Pandora (геном + реакции) · Яндекс (офлайн-рекомендации на устройстве) · Roon/Plexamp (офлайн-радио по библиотеке).

| Источник | Что берём | Ур. | Ист. |
|---|---|---|---|
| YouTube, Covington 2016 | **две ступени**: генерация кандидатов сужает каталог до сотен, отдельная модель ранжирует короткий список | ✔ | S103 |
| Spotify BaRT / EEE | explore/exploit на главной; успех = > 30 с, пропуск раньше = отрицательный сигнал; объяснения — часть решения | ○ | S95 |
| Deezer, RecSys 2020 | персонализация каруселей как контекстный бандит; **Томпсон-сэмплинг** и полу-персонализация проверены офлайн и A/B-тестом | ○ | S101 |
| Chapelle & Li 2011 | Томпсон для **логистической регрессии с диагональным гауссовым постериором** (приближение Лапласа); на рекламе обгоняет UCB | ○ | S102 |
| Pandora | дизлайк перестраивает станцию, геном — «генератор гипотез», реакции их подтверждают | ○ | S91 |
| Яндекс | рекомендации «Моей волны» работают и офлайн на устройстве | ○ | S73 |
| Roon / Plexamp | Offline Radio по своей библиотеке; Deep Cuts по популярности Last.fm | ✔/○ | S70, S126 |

**Копируем:** двухступенчатую схему (у нас: SQL-префильтр → скоринг всей матрицы в памяти → топ-K по группам → секвенсор); модель Chapelle & Li (мгновенное обновление, сэмплирование весов для исследования); бета-распределение по треку (как «ручки» бандита); объяснения из тех же вкладов (EEE).
**Почему не LinUCB:** нужна матрица d×d на каждый контекст и обращение (O(d²) на событие), бонус UCB хуже объясняется пользователю, а у Chapelle & Li и Deezer Томпсон не хуже UCB и естественно управляется «силой исследования» (ползунок §34). Замеры прототипа — `docs/reports/spike-S-17.md`.

---

## W-04. Сезонная волна (фича B)

**Референсы (6, из них 2 слабых):** Spotify «Your Summer Rewind» · YouTube Music сезонные Recap · Spotify/Pandora сезонные подборки · Endel (подстройка под время суток и погоду) · Apple Music сезонные плейлисты ◌ · Яндекс ◌.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Spotify «Your Summer Rewind» (2017–2020) | персональный плейлист **того, что вы слушали прошлыми летами**; только северное полушарие, подписка ≥ 1 года | ○ | S104 |
| YouTube Music | **сезонные Recap** (50 треков за сезон) | ○ | S106 |
| Spotify, Pandora | сезонные/праздничные подборки (редакторские), Holiday-станции | ○ | S107 |
| Endel | звуковые ландшафты реагируют на время суток, погоду, шаги | ○ | S118 |
| Яндекс | сезонного режима «Моей волны» в источниках не нашли (только Алиса «новогоднее настроение») | ○ | S98 |
| Apple Music | сезонные редакторские плейлисты ◌ | ◌ | — |

**Копируем:** «личная история сезона» (Summer Rewind) как источник сезонности трека; полушарие — как у Spotify (у них — только север, у нас — сдвиг на полгода для южного).
**Расширение без массовых аналогов:** сезонность по звучанию, словари названий, плавное смешение сезонов на границе и поправка погодой — наше; все числа в конфиге.

---

## W-05. Волны под события и автопредложения (фича C)

**Референсы (7):** Pandora Holiday-станции · Spotify Holiday Collection · YouTube Music праздничный микс (тест) · Алиса «новогоднее настроение» · Apple Photos «Feature This Holiday Less» · Google Photos «скрыть дату» · Google Doodles.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Pandora | 60+ праздничных станций в сезон, «Holiday Thumb 100» — из любимого слушателями | ○ | S107 |
| Spotify | Holiday Collection (редакторская), праздничные подборки по жанрам | ○ | S107 |
| YouTube Music | персональный праздничный микс в «Mixed for you» (ограниченный тест) | ○ | S106 |
| Алиса | сценарий «новогоднее настроение»: плейлист + гирлянды | ○ | S98 |
| Apple Photos | воспоминания строятся вокруг **праздников из календаря**; «Feature This Holiday Less», «Reset Suggested Memories», общий тумблер | ○ | S108 |
| Google Photos | скрыть дату из воспоминаний; «Show less» | ✔ | S108 |
| Google Doodles | праздник знает календарь; оформление на день, затем возврат | ○ | S109 |

**Копируем:** календарь событий офлайн (Apple Photos/Doodles), карточку-предложение вместо системного уведомления, управление отказами (Apple «Feature Less», Google «скрыть дату») — у нас «Позже / Не в этом году / Никогда» и сброс в настройках; персональная праздничная волна из своих треков (YouTube Music) с честной подписью, если своих праздничных мало.

---

## W-06. Сезонные и событийные темы (фича D)

**Референсы (6, из них 2 слабых):** Telegram · Google Doodles/Chrome · Яндекс Почта · Material You (Material Color Utilities) · темы Chrome ◌ · акцент Windows из обоев ◌.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Telegram (Android) | конец декабря — 1 января: новогодние иконки меню и снег; снег в чатах только 1 января и **только если включена анимация в режиме энергосбережения** | ○ | S110 |
| Google | праздничный Doodle на главной и в новой вкладке Chrome, живёт день | ○ | S109 |
| Яндекс Почта | новогодние темы со снегом (подбирали плотность снежинок), «погодная тема» | ○ | S111 |
| Material You | вся схема из **одного исходного цвета** в пространстве HCT; помощники подбирают тон до нужного контраста (4,5:1) | ✔/○ | S116 |

**Копируем:** тема = один исходный цвет → палитра (Material, у нас OKLCH, `06` §4.2); праздничное оформление ограничено окном события и само откатывается (Doodle/Telegram); анимации темы подчиняются энергосбережению (Telegram) — у нас пресетам производительности.

---

## W-07. Живые погодные эффекты (фича E)

**Референсы (7):** Apple Weather · Яндекс Погода · Wallpaper Engine Live Weather · Lively Weather · Hello Weather (живые обои) · Telegram (снег) · Яндекс Почта (плотность снега).

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Apple Weather | тысячи вариантов анимированного фона: положение солнца, облака, дождь, снег; ночью — тёмные сцены; молнии в грозу (iOS 7) | ○ | S112 |
| Яндекс Погода | фон с анимацией дождя, снега, молний, ветра, тумана | ○ | S113 |
| Wallpaper Engine | официально погоды нет (нет дешёвого API без лимитов); сообщество: программа раз в час проверяет погоду и переключает обои | ○ | S114 |
| Lively Weather | погодное приложение автора Lively на **Open-Meteo** | ○ | S115 |
| Telegram / Яндекс Почта | снег поверх интерфейса; зависит от энергосбережения; подбор плотности | ○ | S110, S111 |

**Копируем:** набор состояний (солнце/облака/дождь/снег/туман/гроза, день/ночь) как у Apple/Яндекс; опрос погоды по таймеру (Wallpaper Engine: раз в час; у нас 30–60 мин) через Open-Meteo без ключа (Lively Weather); зависимость от энергосбережения (Telegram).
**Адаптация:** частицы — WebGL2 с инстансингом, движение считается в вершинном шейдере от `(seed, t, ветер)` без работы ЦП на кадр; музыка задаёт только «дыхание»; вспышки грозы ≤ 1 за 10 с.

---

## W-08. Сон-таймер и засыпательная волна (фича F)

**Референсы (7):** Spotify · YouTube Music · Яндекс Музыка · Pocket Casts · Audible · Kodi · foobar2000 (foo_scheduler).

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Spotify | 5/10/15/30/45 мин, 1 ч, «до конца трека/эпизода»; на десктопе с 2025 г. | ○ | S117 |
| YouTube Music | 5/15/30/60 мин, «End of track»; без своего времени | ○ | S117 |
| Яндекс Музыка | таймер отключения в плеере (значок секундомера) | ○ | S117 |
| Pocket Casts | **встряхнуть — перезапустить таймер** (до окончания), автоперезапуск, если снова включил в течение 5 мин; «до конца эпизода» | ✔ | S117 |
| Audible | **встряхнуть в течение 30 с после начала затухания** — продлить; «до конца главы» | ✔ | S117 |
| Kodi | действие после таймера: **выключение / гибернация / сон / выход / свернуть** | ✔ | S117 |
| foobar2000 foo_scheduler | списки действий: задержка → стоп → выход → выключение/сон; жалобы, что линейное затухание плохо звучит на длинных фейдах — просят логарифмическое | ○ | S117 |

**Копируем:** пресеты длительностей + «до конца трека» (Spotify/YouTube), «через N треков» (аналог «до конца главы»), продление действием во время затухания (Audible/Pocket Casts → у нас «любое действие» + кнопка «+15 мин»), действия после таймера (Kodi), затухание в децибелах, а не линейно (урок foobar2000).
**Адаптация Windows:** на ноутбуке владельца нет S3, только Modern Standby (`powercfg /a`, 2026-10-16), где `SetSuspendState` возвращает отказ (S121) → «сон» = выключить экран и снять наши запросы бодрствования; гибернация — отдельный вариант, если доступна (спайк S-18).

---

## W-09. Фокус-волна / помодоро (фича G)

**Референсы (5 приложений + метод):** Windows Clock «Сеансы фокусировки» · Brain.fm · Endel · Pomofocus · Apple Music Pomodoro Shortcut · метод Помодоро (Чирилло) ◌.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Windows Clock | фокус 15–60 мин, перерыв 5 мин (или 10–15), **отдельные звуки конца сессии и конца перерыва**, интеграция со Spotify (сломана с 2023) | ○ | S118 |
| Brain.fm | встроенный Помодоро 25/5, сигнал или голос на смене; на перерыв советуют режим Relax | ○ | S118 |
| Endel | Focus Timer «по Помодоро» со своим звуком | ○ | S118 |
| Pomofocus | длительности работы/короткого/длинного перерыва, **число циклов до длинного**, автостарт, звук будильника | ○ | S118 |
| Apple Music | быстрая команда «Pomodoro»: музыка на работу, пауза на перерыв | ○ | S118 |

**Копируем:** 25/5 и длинный 15 после 4 циклов (метод и Pomofocus), отдельный мягкий звук на границах (Windows Clock), автоматическую смену музыки работа → перерыв (у Brain.fm вручную — делаем автоматически), счётчик циклов за день (Windows Clock показывает дневной прогресс ◌).
**Адаптация:** упаковка треков под длину сегмента — наше (у референсов музыка просто прерывается).

---

## W-10. Ползунок «Знакомое ↔ Неожиданное» (фича H)

**Референсы (6):** YouTube Music Tuner · Яндекс «Незнакомое» · Pandora Modes · Spotify On Repeat / Repeat Rewind · Plexamp Deep Cuts · Spotify Smart Shuffle.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| YouTube Music | **Familiar / Blend / Discover** — насколько радио держится выбранных артистов | ○ | S96 |
| Яндекс | «Незнакомое» смещает к незнакомым исполнителям; без настроек волна сама смешивает новое и знакомое | ○ | S98 |
| Pandora Modes | Discovery (артисты, которых станция обычно не играет), Deep Cuts (менее популярное), Newly Released, Crowd Faves; «My Station» по умолчанию | ○ | S89 |
| Spotify | **Repeat Rewind** — то, что вы много слушали, но давно (> 1 месяца) не включали; не пересекается с On Repeat | ○ | S105 |
| Plexamp | Deep Cuts — менее популярное по данным Last.fm | ○ | S126 |
| Spotify Smart Shuffle | 1 рекомендация на 3 трека плейлиста | ○ | S70 |

**Копируем:** пять категорий знакомости (Repeat Rewind → «забытое»; Deep Cuts/Discovery → «новое и нетронутое»), пропорции категорий как квоты в очереди (Smart Shuffle «1 из 3»), отдельную ось «непохожесть» (Pandora Discovery = другие артисты, YouTube Discover).
**Проверено прототипом:** квоты выдерживаются точно на 0/50/100 (`spike-S-17`).

---

## W-11. Нейтральная волна «Для всех» (фича I)

**Референсы (7):** Pandora Crowd Faves · YouTube Music фильтр Popular · Plexamp (популярность Last.fm) · Spotify Private Session / Exclude · Apple Music «без истории» · Spotify Jam ◌ · Яндекс «Популярное» ◌.

| Приложение | Как работает | Ур. | Ист. |
|---|---|---|---|
| Pandora Crowd Faves | играет то, что **чаще всего лайкали все слушатели станции**, а не вы | ○ | S89 |
| YouTube Music | фильтр Popular в тюнере | ○ | S96 |
| Plexamp | популярность треков — из Last.fm | ○ | S126 |
| Spotify / Apple Music | гостевое прослушивание не портит вкус: Private Session, Exclude, «Use Listening History» выкл. | ○ | S92, S93, S99 |
| Spotify Jam | общее прослушивание компании ◌ | ◌ | — |

**Копируем:** «популярное у всех» как главный сигнал (Crowd Faves) — у нас ListenBrainz `total_user_count` (число **уникальных слушателей**, а не прослушиваний); изоляция вкуса владельца (Private Session).
**Адаптация:** без сети — звучание, жанр, настроение, метки; реакции гостей — отдельная модель.

---

## 12. Технические проверки (API, платформа)

| Тема | Факт | Ур. | Ист. |
|---|---|---|---|
| Open-Meteo | `current`: `weather_code`, `precipitation`, `snowfall`, `wind_speed_10m`, `wind_direction_10m`, `cloud_cover`, `is_day`, `temperature_2m`; `daily`: `sunrise`, `sunset`; `timezone=auto`; текущие данные — 15-минутные модели; коды WMO 0–99 (96/99 — только у моделей с градом, остальные дают 95/97) | ✔ | S120 |
| Open-Meteo, условия | бесплатно для «private or non-profit websites or apps without subscriptions or advertising»; < 10 000 вызовов/сутки, 5 000/ч, 600/мин; CC BY 4.0 | ✔ | S120 |
| ListenBrainz Popularity | `POST /1/popularity/recording` с `{"recording_mbids":[…]}` → `total_listen_count`, `total_user_count` (null, если нет данных), порядок сохраняется; лимит пакета `MAX_ITEMS_PER_GET` (число не указано) | ✔ | S119 |
| ListenBrainz, правила | **не чаще 1 запроса в секунду** на приложение; заголовки `X-RateLimit-*`, 429; **User-Agent обязателен** (`App/<ver> ( contact )`) | ✔ | S119 |
| ListenBrainz Metadata lookup | поиск MBID по «исполнитель + название» **требует токен пользователя** → нам недоступен; популярность есть только у треков с MBID | ✔ | S119 |
| Сон ПК | `SetSuspendState` требует `SE_SHUTDOWN_NAME`; на Modern Standby возвращает `ERROR_NOT_SUPPORTED` (сообщество OSR); ноутбук владельца — «S0 Low Power Idle», S3 нет, гибернация есть | ✔/○ | S121 |
| Звонки | `IAudioVolumeDuckNotification` (Win7+): система сообщает об открытии/закрытии коммуникационного потока; `IAudioSessionControl2::SetDuckingPreference` — отказ от системного приглушения | ✔ | S121 |
| WebView2 | GPU обязателен для производительности (`disable-gpu` — только для отладки); при скрытии `IsVisible=false` Chromium троттлит анимации; `TrySuspend` усыпляет рендерер скрытого WebView | ✔ | S122 |
| WebGL2 | инстансинг (`drawArraysInstanced`, `vertexAttribDivisor`) — часть ядра WebGL2, отдельное расширение не нужно; таймер-запросы GPU (`EXT_disjoint_timer_query_webgl2`) в Chromium доступны не всегда — замер кадра делаем через rAF, GPU-время — где расширение есть | ◌ | спецификация WebGL2; S-19 |
| reduced motion | Chromium на Windows читает `SPI_GETCLIENTAREAANIMATION` («Эффекты анимации») и обновляет по `WM_SETTINGCHANGE` → `prefers-reduced-motion: reduce` | ✔/○ | S123 |
| AudioSet (YAMNet) | есть классы **Christmas music**, **Lullaby**, Wedding music, Dance music, Background music, Happy/Sad/Tender/Exciting/Angry/**Scary** music, Singing, Speech | ✔ | S124 |
| Пасха, солнцестояния | алгоритмы Meeus/Jones/Butcher и юлианский Меёса совпали с таблицей 2026–2030; средние формулы Меёса (гл. 27) дали верные UTC-даты 2027 (USNO) | ✔ | S125, `spike-S-17` |

---

## 13. Источники (S89–S126)

- **S89** Pandora Modes: https://techcrunch.com/2019/03/19/pandora-switches-up-its-classic-listening-experience-with-modes · https://www.aol.com/news/pandora-modes-heres-music-streaming-174550814.html
- **S90** Pandora «I'm tired of this track»: https://community.pandora.com/t5/Desktop/quot-I-m-tired-of-this-song-quot-is-missing-on-Web/m-p/2516 · https://community.pandora.com/t5/Desktop/Web-quot-I-m-Tired-of-this-Track-quot-Option-Missing-with/m-p/15993
- **S91** Pandora «Why was this track selected»: https://bradwarthen.com/why-does-pandora-get-me-while-netflix-doesnt/ · https://commonwealthtimes.org/2006/09/07/discover-why-you-like-music/
- **S92** Spotify Exclude from Taste Profile: https://engineering.atspotify.com/2023/10/exclude-from-your-taste-profile · https://techcrunch.com/2025/10/01/spotify-now-lets-you-exclude-tracks-from-your-taste-profile
- **S93** Spotify Private Session: https://community.spotify.com/t5/Accounts/Does-quot-Private-Session-quot-or-quot-Offline-Mode-quot-affect/m-p/1643894/highlight/true
- **S94** Spotify DJ: https://www.npr.org/2024/11/13/nx-s1-5184493/spotify-ai-dj-music · https://newsroom.spotify.com/tag/the-dj
- **S95** McInerney et al., «Explore, Exploit, and Explain», RecSys 2018: https://patents.google.com/patent/US11301513 · BaRT (вторично): https://dynamoi.com/learn/spotify-algorithm/what-is-spotify-bart-algorithm
- **S96** YouTube Music «Create a radio» / Music Tuner: https://9to5google.com/2023/02/21/youtube-music-create-radio-launch/ · https://9to5google.com/2022/12/15/youtube-music-create-radio/
- **S97** YouTube рекомендации: https://support.google.com/youtube/answer/6342839 · https://www.androidauthority.com/youtube-music-dont-recommend-artist-button-test-3676352/ · https://androidcentral.com/apps-software/youtube-dislike-and-not-interested-buttons-are-useless
- **S98** Яндекс «Незнакомое» и настройки «Моей волны»: https://rozetked.me/news/32418-yandeks-muzyka-obnovila-algoritm-nastroyki-neznakomoe-v-moey-volne · https://mobile-review.com/all/news/v-yandeks-muzyke-poyavilsya-algoritm-neznakomoe/ · https://habr.com/ru/news/963670
- **S99** Apple Music Use Listening History / Focus filter: https://9to5mac.com/2023/10/26/ios-17-2-apple-music-listening-history-focus-mode/ · https://support.apple.com/guide/music/musf7da17c25/mac
- **S100** Deezer: https://support.deezer.com/hc/articles/115004367189 · https://www.tecmundo.com.br/software/411310-flow-tuner-do-deezer-testamos-o-recurso-que-refina-playlists-baseados-em-moods.htm · https://newsroom-deezer.com/2022/07/flow-moods-recommending-music-by-moods-on-deezer/
- **S101** Deezer, Carousel Personalization with Contextual Bandits (RecSys 2020): https://arxiv.org/abs/2009.06546 · https://github.com/deezer/carousel_bandits
- **S102** Chapelle & Li, An Empirical Evaluation of Thompson Sampling (NIPS 2011): https://papers.nips.cc/paper/2011/hash/e53a0a2978c28872a4505bdb51db06dc-Abstract.html · обзор: https://arxiv.org/pdf/1508.03326
- **S103** Covington, Adams, Sargin, Deep Neural Networks for YouTube Recommendations (RecSys 2016): https://research.google/pubs/pub45530
- **S104** Spotify «Your Summer Rewind»: https://www.engadget.com/2017/06/08/spotify-summer-rewind-playlist/ · https://newsroom.spotify.com/tag/your-summer-rewind
- **S105** Spotify On Repeat / Repeat Rewind: https://techcrunch.com/2019/09/24/spotify-adds-two-personalized-playlists-focused-on-your-repeat-listens · https://www.hypebot.com/?p=32882
- **S106** YouTube Music сезонные Recap и праздничный микс: https://9to5google.com/2023/03/23/youtube-music-winter-recap/ · https://www.androidpolice.com/youtube-music-isnt-wasting-any-time-getting-into-the-holiday-spirit/
- **S107** Праздничные подборки: https://newsroom.spotify.com/tag/holidays/page/1 · https://themusicuniverse.com/?p=74391 · https://www.macworld.com/article/215454/creating_the_perfect_pandora_holiday_station.html
- **S108** Apple Photos «Feature Less», Google Photos: https://www.popsci.com/diy/how-to-remove-someone-photo-memories/ · https://support.google.com/photos/answer/9454489
- **S109** Google Doodle в новой вкладке: https://chromeunboxed.com/google-celebrates-pac-mans-45th-anniversary-with-a-new-spooky-playable-doodle/
- **S110** Telegram сезонный снег: https://tx.me/s/tginfoen?after=2036 · https://rozetked.me/news/21000-v-beta-versii-telegram-dlya-ios-poyavilsya-sneg-v-chate-i-reakcii-na-soobscheniya
- **S111** Яндекс Почта, новогодние и погодные темы: https://yandex.ru/blog/mail/23208 · https://yandex.ru/blog/mail/8814 · https://yandex.ru/blog/mail/13757
- **S112** Apple Weather: https://techcrunch.com/?p=2162347 · https://appleinsider.com/articles/13/06/18/inside-ios-7-apples-weather-app-gets-animated
- **S113** Яндекс Погода: https://overclockers.ru/lab/show/72502_4/vybiraem-meteoservis-dlya-android-ustrojstv-yandeks-pogoda-gismeteo-go-weather-ex-i-yahoo-pogoda
- **S114** Wallpaper Engine и погода: https://steamcommunity.com/app/431960/discussions/0/3053988173741028962 · https://github.com/sonowz/wallpaper-engine-live-weather
- **S115** Lively Weather (Open-Meteo): https://www.deskmodder.de/blog/?p=182491 · https://www.justgeek.fr/?p=113298
- **S116** Material Color Utilities: https://chromium.googlesource.com/external/github.com/material-foundation/material-color-utilities/+/91da30d89e70c3dc9575ec71a1ebe8874d881f29/concepts/scheme_generation.md · https://api.flutter.dev/flutter/package-material_color_utilities_dynamiccolor_dynamic_color/DynamicColor-class.html
- **S117** Сон-таймеры: https://community.spotify.com/t5/Implemented-Ideas/Desktop-Other-Sleep-timer-for-desktop/idc-p/7110088 · https://9to5google.com/2023/04/27/youtube-music-sleep-timer-rolling-out/ · https://lifehacker.ru/10-funkcij-yandeks-muzyki/ · https://support.pocketcasts.com/article/sleep-timer/ · https://help.audible.co.uk/s/article/use-timer · https://kodi.wiki/view/Settings/System/Power_saving · https://hydrogenaudio.org/index.php?msg=518645
- **S118** Фокус/помодоро: https://www.windowscentral.com/how-get-started-clock-app-windows-11 · https://windowslatest.com/2021/08/20/hands-on-with-windows-11s-new-productivity-feature-focus-sessions · https://www.brain.fm/blog/pomodoro-music-deep-work · https://endel.io/adhd · https://cd.uj.edu.pl/en_GB/oprogramowanie/pomodoro · https://education.apple.com/resource/250011467
- **S119** ListenBrainz API: https://listenbrainz.readthedocs.io/en/latest/users/api/popularity.html · https://listenbrainz.readthedocs.io/en/latest/users/api/index.html · https://listenbrainz.readthedocs.io/en/latest/users/api/metadata.html (+ context7 `/websites/listenbrainz_readthedocs_io_en`)
- **S120** Open-Meteo: https://open-meteo.com/en/docs · https://open-meteo.com/en/terms (+ context7 `/websites/open-meteo_en`)
- **S121** Windows: https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-setsuspendstate · https://learn.microsoft.com/en-us/windows/win32/Power/system-power-states · https://community.osr.com/discussion/comment/304430 · https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiovolumeducknotification
- **S122** WebView2: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance · https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3 (TrySuspend) (+ context7 `/websites/learn_microsoft_en-us_microsoft-edge_webview2`)
- **S123** reduced motion в Chromium/Windows: https://chromium.googlesource.com/chromium/src.git/+/HEAD/ui/gfx/animation/animation_win.cc
- **S124** AudioSet/YAMNet классы: https://raw.githubusercontent.com/tensorflow/models/master/research/audioset/yamnet/yamnet_class_map.csv
- **S125** Даты: https://en.wikipedia.org/wiki/List_of_dates_for_Easter · USNO через weather.gov: https://weather.gov/media/ind/seasons.pdf · Meeus, «Astronomical Algorithms», гл. 8 и 27 ◌
- **S126** Plexamp: https://forums.plex.tv/t/how-to-plexamp-4-6-1/577024 · https://www.soundandvision.com/content/plexamp-s-sonic-sage-uses-chatgpt-improved-discovery
