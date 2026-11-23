# Доставка обращений SIRTONE

Статус: локально тестируемая Feedback API реализована в `services/feedback-api`; production deployment и реальные почтовые credentials не настроены.

## Схема

`SIRTONE → HTTPS Feedback API → SMTP submission over TLS → sir.kov.mail@gmail.com`

Клиент не содержит пароль Gmail, SMTP-пароль или адрес получателя. API принимает только `POST /v1/feedback/reports`, а получатель задаётся переменной окружения сервера. Входной JSON содержит тип, сообщение, опциональный email, Report ID, платформу, версию и опциональный ZIP как base64. Сервер не распаковывает ZIP. Windows-клиент читает endpoint исключительно из deployment environment `SIRTONE_FEEDBACK_API_URL`; значение обязано начинаться с `https://`. В текущей development-сборке отсутствие этой переменной честно оставляет черновик и ZIP локально.

iOS читает только `SirToneFeedbackAPIURL` из финального Info.plist; пустое значение отключает HTTPS-отправку, но не экспорт/Share Sheet/Mail fallback. Перед archive/re-signing подставьте утверждённый полный `https://…/v1/feedback/reports` через закрытую build-конфигурацию, а не через Git. Это значение не является секретом, но менять его может только владелец релизной конфигурации.

Если Windows API недоступен, форма сохраняет черновик и предлагает открыть почтовое приложение с получателем, темой и текстом. `mailto:` не умеет надёжно прикреплять файл: при явно включённой диагностике SirTone сначала сохраняет ZIP, а форма честно просит пользователя приложить этот файл вручную.

## Запуск и тест

Для проверки без реальной почты:

```powershell
pnpm --filter @sirtone/feedback-api test
```

Тест поднимает локальный HTTP endpoint и SMTP simulator. Он проверяет приём ZIP, фиксированного получателя, Reply-To, понятное тело письма, дедупликацию, malformed ZIP и rate limit.

## Конфигурация production

Переменные задаются только в secret store/deployment environment, никогда не в Git или в приложении:

```text
HOST=127.0.0.1                 # за reverse proxy; публичный TLS завершает proxy
PORT=8080
FEEDBACK_SUPPORT_TO=sir.kov.mail@gmail.com
FEEDBACK_SMTP_HOST=smtp.provider.example
FEEDBACK_SMTP_PORT=465
FEEDBACK_SMTP_SECURE=true
FEEDBACK_SMTP_FROM=noreply@your-feedback-domain.example
FEEDBACK_SMTP_USERNAME=submission-user       # optional, with PASSWORD only
FEEDBACK_SMTP_PASSWORD=stored-only-in-secret-manager
FEEDBACK_AUDIT_PATH=/var/lib/sirtone-feedback/audit.json
FEEDBACK_AUDIT_SALT=generate-a-long-random-secret-in-the-secret-manager
```

Сервис запускается `node services/feedback-api/src/index.mjs`. В production SMTP должен использовать implicit TLS. `FEEDBACK_SMTP_SECURE=false` допустим только для `localhost` вместе с `FEEDBACK_ALLOW_INSECURE_LOCAL_SMTP=true`, исключительно в тесте.

Перед открытием endpoint необходимо:

1. Развернуть сервис за HTTPS reverse proxy и ограничить допустимые origin/rate limit на proxy.
2. Создать отдельный SMTP submission credential у почтового провайдера и поместить его в secret manager. Для обычного SMTP submission задайте одновременно `FEEDBACK_SMTP_USERNAME` и `FEEDBACK_SMTP_PASSWORD`: transport использует стандартный `AUTH PLAIN` только внутри implicit TLS (`FEEDBACK_SMTP_SECURE=true`). Для relay с IP allow-list оба поля можно не задавать. Никогда не включайте credentials при тестовом plaintext localhost-транспорте.
3. Выполнить тестовое обращение, подтвердить письмо в `sir.kov.mail@gmail.com` и проверить Reply-To.
4. Настроить `FEEDBACK_AUDIT_PATH` на каталог, доступный только service account, а `FEEDBACK_AUDIT_SALT` — в secret manager. Встроенный file-backed store хранит 30 дней только Report ID, UTC-время и salted IP hashes; ни тела, ZIP, email, SMTP secrets или IP в открытом виде там нет. Один экземпляр сервиса поддерживается сразу; для нескольких replicas замените его эквивалентным общим транзакционным TTL-store.

## Защита

- Тип обращения — только `bug`, `suggestion`, `question`, `other`.
- Сообщение ограничено 6000 символами; request — 7 МБ, ZIP — 5 МБ.
- ZIP проверяется по central directory: ровно шесть известный файлов отчёта, только stored entries, без encryption и без распаковки. Path traversal невозможен, так как сервис ничего не извлекает.
- Report ID идемпотентен в настроенном audit store: повтор после перезапуска не посылает второе письмо. Один экземпляр резервирует ID до SMTP-отправки; несколько replicas требуют общего транзакционного TTL-store.
- Получатель фиксирован серверной переменной. User email используется только как Reply-To.
