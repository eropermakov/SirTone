# Доставка обращений SIRTONE

Статус: локально тестируемая Feedback API реализована в `services/feedback-api`; production deployment и реальные почтовые credentials не настроены.

## Схема

`SIRTONE → HTTPS Feedback API → SMTP submission over TLS → sir.kov.mail@gmail.com`

Клиент не содержит пароль Gmail, SMTP-пароль или адрес получателя. API принимает только `POST /v1/feedback/reports`, а получатель задаётся переменной окружения сервера. Входной JSON содержит тип, сообщение, опциональный email, Report ID, платформу, версию и опциональный ZIP как base64. Сервер не распаковывает ZIP. Windows-клиент читает endpoint исключительно из deployment environment `SIRTONE_FEEDBACK_API_URL`; значение обязано начинаться с `https://`. В текущей development-сборке отсутствие этой переменной честно оставляет черновик и ZIP локально.

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
```

Сервис запускается `node services/feedback-api/src/index.mjs`. В production SMTP должен использовать implicit TLS. `FEEDBACK_SMTP_SECURE=false` допустим только для `localhost` вместе с `FEEDBACK_ALLOW_INSECURE_LOCAL_SMTP=true`, исключительно в тесте.

Перед открытием endpoint необходимо:

1. Развернуть сервис за HTTPS reverse proxy и ограничить допустимые origin/rate limit на proxy.
2. Создать отдельный SMTP submission credential у почтового провайдера и поместить его в secret manager. Текущий минимальный SMTP adapter не выполняет SMTP AUTH; выберите provider/relay с IP allow-list или расширьте adapter с проверенным auth-механизмом до production.
3. Выполнить тестовое обращение, подтвердить письмо в `sir.kov.mail@gmail.com` и проверить Reply-To.
4. Настроить хранение минимального delivery-audit без тела обращения, ZIP или email. Память текущего процесса предназначена для локального теста; production требует TTL storage для дедупликации и rate limiting.

## Защита

- Тип обращения — только `bug`, `suggestion`, `question`, `other`.
- Сообщение ограничено 6000 символами; request — 7 МБ, ZIP — 5 МБ.
- ZIP проверяется по central directory: ровно шесть известный файлов отчёта, только stored entries, без encryption и без распаковки. Path traversal невозможен, так как сервис ничего не извлекает.
- Report ID идемпотентен в пределах живого процесса: повтор не посылает второе письмо.
- Получатель фиксирован серверной переменной. User email используется только как Reply-To.
