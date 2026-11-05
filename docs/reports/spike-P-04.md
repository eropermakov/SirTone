# Проверка П4 — установка и продление с ПК (хаб)

- **Дата / сессия:** 2026-11-05, ветка `stage/8-mobile`, прототип `spikes/S-P4-isideload`
- **Связанные требования:** FR-HUB-001…004 · **ADR:** D-132, D-133, D-136, D-139
- **Референсы (`01d`):** isideload 0.5.2 [M7], idevice 0.1.68 [M8], iloader [M6]

## 1. Вопрос и критерии (`docs/mobile/checks.md`)
- Успех: `phone install` и `phone refresh` проходят по кабелю, по Wi-Fi и через обе точки доступа; Wi-Fi включается без iTunes.
- Отказ → план Б: `plumesign` и разовая настройка в iTunes.

## 2. Что сделано
- `sp4 login <apple-id>` — единственный шаг с паролем: в консоли владельца (пароль скрыт, 2FA — код с iPhone). Без консоли — сразу ошибка `login.no_console` (не зависает). Пароль не сохраняется; в Windows Credential Manager (`sirtone-p4`) — токен сервисов разработчика `xcode.auth`, adsid, состояние anisette и ключ сертификата.
- `sp4 info` / `sp4 install` — **без пароля**, только по сохранённому токену: `DeveloperSession::new(token, adsid, …)` поверх `AppleAccount` без входа.
- Подпись и установка — `isideload` (`SideloaderBuilder`, `MaxCertsBehavior::Error` — сертификаты никогда не отзываются), передача — `idevice` (usbmuxd службы Apple, AFC, installation_proxy).
- Wi-Fi: `sp4 wifi on` (lockdown `com.apple.mobile.wireless_lockdown`/`EnableWifiConnections`), `sp4 net-info|net-afc|net-install <ip> <udid>` — прямая TCP-связь с телефоном (lockdown 62078) с записью сопряжения из службы Apple (`ReadPairRecord`).

## 3. Измерения
| Метрика | Цель | Результат | Как мерили |
|---|---|---|---|
| Вход в Apple ID | один раз, пароль и 2FA — владелец | ✅ один раз; токен действует 1 год (до 2027-11-05) | `sp4 login` |
| Установка по кабелю без пароля | проходит | ✅ 17,1 с (первая, с регистрацией App ID и сертификата) | `sp4 install` |
| Повторная установка поверх («продление» в П4) | данные сохраняются | ✅ 9,4 с, файлы в Documents на месте, новый сертификат не создан | `sp4 install` + `ifuse --documents` |
| Идентификатор | D-136 | ✅ `studio.serkov.sirtone.mobile.<TEAMID>` | список приложений на iPhone |
| Wi-Fi: служба Apple видит телефон | видит | ❌ не видит (см. §4) | `sp4 list` 3 мин |
| Wi-Fi: прямая связь lockdown | — | ✅ `DeviceName/ProductType/ProductVersion` по сети | `sp4 net-info` |
| Wi-Fi: AFC и установка | проходит | ❌ iPhone рвёт TLS на первой операции AFC | `sp4 net-afc`, `net-install` |
| Точки доступа (телефон/ПК) | проходит | не проверялось | — |

## 4. Наблюдения и сюрпризы
- **isideload подписывает `.app` на месте и удаляет его после установки** — хаб подаёт временную копию.
- **Частный адрес Wi-Fi.** iPhone выходит в сеть под случайным MAC; служба Apple ищет телефон по настоящему MAC из записи сопряжения (`WiFiMACAddress`) и не узнаёт его. Лечится на телефоне: Настройки → Wi-Fi → (i) → «Частный адрес Wi-Fi» → Выкл. (только для домашней сети). Шаг для мастера хаба.
- **Служба Apple из iCloud (AppleMobileDeviceService 560.60.2.13) к телефону по сети не подключается**, даже когда телефон анонсирует настоящий MAC (`_apple-mobdev2._tcp`, Bonjour есть).
- **iOS 27 не пускает AFC через сетевой lockdown** (TLS обрывается сразу после `StartService`). Анонс телефона несёт `supportsRP-26`: по сети работает RemotePairing → RSD-туннель (CoreDevice). В isideload есть `install_app_rsd`, в idevice — `remote_pairing`/`rsd`; нужна запись RPPairing — та же, что ТЗ закладывает для самопродления (волна 2, П9, `docs/mobile/README.md` §11).
- **Лицензии (до встраивания в хаб):** `cargo deny` на дереве спайка — `apple-codesign-quick` (модуль подписи isideload) **LGPL-2.1-or-later** — по `08` §3 в бинарник без решения нельзя; `webpki-roots` — CDLA-Permissive-2.0 (данные корневых сертификатов, разрешительная); `rsa` 0.9 — RUSTSEC-2023-0071 (Marvin, исправления нет; у нас ключ только для локальной подписи, без сетевого оракула).
- В выводе `info` — имя владельца из сертификата Apple; в отчёты не попадает.

## 5. Решение
Кабель — **принято**: вход один раз, дальше установка и продление без пароля. Wi-Fi — **нужна доп. проверка** через RemotePairing/RSD (П4б), точки доступа — после неё. Модуль подписи — **решено (D-143, владелец 2026-11-05)**: своя прослойка `apple-codesign-quick` поверх `apple-codesign` (MPL-2.0) через `[patch.crates-io]` (`spikes/S-P4-isideload/shim-codesign-quick`). Проверено: установка поверх приложения хаба за 9,0 с, данные сохранились, приложение запускается на iPhone; LGPL в дереве нет. Встраивание в `sirtone-devicelink` — по этой схеме.

## 6. Последствия для бэклога
- M2.1: `IosInstaller` — вход, установка, продление по кабелю по схеме спайка; хранение под именем на каждый Apple ID (D-132).
- M2.2: мастер — шаг «Частный адрес Wi-Fi» для домашней сети.
- Новое: П4б — Wi-Fi через RemotePairing (RPPairing по кабелю один раз, затем RSD-туннель); общий код с П9.
- M2.1: перенести прослойку подписи в workspace (крейт под тем же именем, `[patch.crates-io]` в корне), исключения `cargo deny` и лицензии — по D-143; размер exe — сверить с `08` §1.
