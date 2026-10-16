-- migrations/0001_init.sql (основные таблицы; полный DDL — в crate при реализации)

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
-- ключи: schema_version, library_uid, created_at, analysis_version, layout_template

CREATE TABLE artist (
  id          INTEGER PRIMARY KEY,
  uid         TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  name_norm   TEXT NOT NULL,
  sort_name   TEXT,
  mbid        TEXT,
  kind        TEXT NOT NULL DEFAULT 'artist' CHECK (kind IN ('artist','various','unknown')),
  merged_into INTEGER REFERENCES artist(id),          -- мягкое слияние: NULL = каноническая запись
  folder_name TEXT,                                   -- имя папки на диске (после санитизации)
  created_at  INTEGER NOT NULL
);
CREATE UNIQUE INDEX ux_artist_norm ON artist(name_norm) WHERE merged_into IS NULL;

CREATE TABLE artist_alias (
  alias_norm TEXT PRIMARY KEY,
  artist_id  INTEGER NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
  source     TEXT NOT NULL CHECK (source IN ('merge','translit','musicbrainz','user'))
) WITHOUT ROWID;

CREATE TABLE cover (
  id INTEGER PRIMARY KEY, hash TEXT NOT NULL UNIQUE,
  source TEXT NOT NULL CHECK (source IN ('embedded','folder','caa','user','generated')),
  width INTEGER, height INTEGER,
  palette TEXT,                                       -- JSON: 3–5 доминирующих цветов (OKLCH)
  created_at INTEGER NOT NULL
);

CREATE TABLE album (
  id INTEGER PRIMARY KEY, uid TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL, title_norm TEXT NOT NULL,
  album_artist_id INTEGER NOT NULL REFERENCES artist(id),
  year INTEGER,
  kind TEXT NOT NULL DEFAULT 'album' CHECK (kind IN ('album','single','ep','compilation','unknown')),
  mbid_release TEXT, mbid_group TEXT,
  disc_count INTEGER NOT NULL DEFAULT 1,
  cover_id INTEGER REFERENCES cover(id),
  cover_locked INTEGER NOT NULL DEFAULT 0,            -- 1 = обложку выбрал пользователь (ТЗ §13)
  lufs_album REAL,                                    -- громкость альбома целиком (нормализация «по альбому»)
  folder_name TEXT, created_at INTEGER NOT NULL
);
CREATE INDEX ix_album_artist ON album(album_artist_id, title_norm);

CREATE TABLE track (
  id INTEGER PRIMARY KEY, uid TEXT NOT NULL UNIQUE,
  rel_path TEXT NOT NULL UNIQUE,                      -- относительно корня библиотеки, разделитель '/', NFC
  size_bytes INTEGER NOT NULL, mtime_ms INTEGER NOT NULL,
  file_id TEXT,                                       -- NTFS FileId (том:индекс), если доступен
  content_hash TEXT NOT NULL,                         -- BLAKE3 всего файла (считается при копировании)
  container TEXT NOT NULL, codec TEXT NOT NULL, lossless INTEGER NOT NULL,
  sample_rate INTEGER, bit_depth INTEGER, channels INTEGER, bitrate_kbps INTEGER,
  duration_ms INTEGER NOT NULL,
  -- идентичность
  title TEXT NOT NULL, title_norm TEXT NOT NULL,
  album_id INTEGER REFERENCES album(id), disc_no INTEGER, track_no INTEGER, year INTEGER,
  genre_main TEXT,                                    -- из контролируемого словаря (05 §6)
  identity_state TEXT NOT NULL CHECK (identity_state IN ('confirmed','inferred','user','unknown')),
  identity_conf REAL NOT NULL DEFAULT 0,
  unknown_reason TEXT,                                -- no_metadata|conflict|low_confidence|offline_pending|possible_non_track|needs_review
  candidates_json TEXT,                               -- подсказки для раздела «Неизвестные»
  mbid_recording TEXT,
  user_normal INTEGER NOT NULL DEFAULT 0,             -- «пометить как нормальный» (ТЗ §7.5)
  no_mix INTEGER NOT NULL DEFAULT 0,                  -- не микшировать этот трек (автомикс)
  -- громкость и форма
  lufs_i REAL, true_peak_db REAL, lra REAL,
  peaks BLOB,                                         -- u8[400] обзор волны для таймлайна
  lead_silence_ms INTEGER, tail_silence_ms INTEGER,   -- порог −60 dBFS (как Mixxx Skip Silence)
  env_head BLOB, env_tail BLOB,                       -- u8 dB, 10 Гц, первые/последние 30 с (как «10 выборок/с» Plexamp) — для перекрытия
  -- темп и тональность
  bpm REAL, bpm_conf REAL, beat_grid BLOB, grid_stability REAL,   -- beat_grid: u32 delta-ms, сжато
  key_camelot TEXT, key_conf REAL,
  -- вайб (04 §6)
  valence REAL, arousal REAL, aggression REAL, energy REAL,
  danceability REAL, acousticness REAL, vocal REAL,
  lang TEXT CHECK (lang IN ('ru','foreign','none','unknown')), lang_conf REAL,
  mood_bits INTEGER NOT NULL DEFAULT 0,               -- битовая маска настроений для быстрых фильтров
  activity_json TEXT,                                 -- {"work":0.7,"workout":0.1,...}
  embedding BLOB,                                     -- i8[64] + масштаб
  fingerprint BLOB, fp_duration_ms INTEGER,           -- Chromaprint, сжатый
  analysis_version INTEGER NOT NULL DEFAULT 0, analyzed_at INTEGER,
  -- невидимая агрегированная статистика (ТЗ §22, вопрос 6)
  play_count INTEGER NOT NULL DEFAULT 0, skip_count INTEGER NOT NULL DEFAULT 0,
  last_played_at INTEGER, completion_ema REAL NOT NULL DEFAULT 0,
  plays_recent BLOB,                                  -- u8[13] недельные корзины за 13 недель (для «часто слушаю» без журнала событий)
  added_at INTEGER NOT NULL, import_batch_id INTEGER REFERENCES import_batch(id),
  state TEXT NOT NULL DEFAULT 'ok' CHECK (state IN ('ok','missing','quarantined'))
);
CREATE INDEX ix_track_album    ON track(album_id, disc_no, track_no);
CREATE INDEX ix_track_identity ON track(identity_state, unknown_reason);
CREATE INDEX ix_track_added    ON track(added_at DESC, id);  -- keyset (S-15)
CREATE INDEX ix_track_title    ON track(title_norm, id);     -- keyset «по названию» (S-15)
CREATE INDEX ix_track_played   ON track(last_played_at);
CREATE INDEX ix_track_hash     ON track(content_hash);
CREATE INDEX ix_track_dur      ON track(duration_ms);        -- кандидаты дублей (05 §8)
CREATE INDEX ix_track_fileid   ON track(file_id) WHERE file_id IS NOT NULL;

CREATE TABLE track_artist (
  track_id  INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  artist_id INTEGER NOT NULL REFERENCES artist(id),
  role      TEXT NOT NULL CHECK (role IN ('primary','featured','album')),
  position  INTEGER NOT NULL,                          -- порядок в названии; primary = первый
  PRIMARY KEY (track_id, artist_id, role)
) WITHOUT ROWID;
CREATE INDEX ix_ta_artist ON track_artist(artist_id);

CREATE TABLE playlist (
  id INTEGER PRIMARY KEY, uid TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('manual','smart')),
  rules_json TEXT,                                    -- AST правил для smart (05 §10)
  hybrid INTEGER NOT NULL DEFAULT 0,                  -- продолжать похожими из медиатеки (ТЗ §8)
  cover_id INTEGER REFERENCES cover(id), sort_order INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE playlist_item (
  playlist_id INTEGER NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
  position INTEGER NOT NULL, track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  added_at INTEGER NOT NULL, PRIMARY KEY (playlist_id, position)
) WITHOUT ROWID;
CREATE INDEX ix_pi_track ON playlist_item(track_id);

CREATE TABLE import_batch (
  id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, finished_at INTEGER,
  source TEXT NOT NULL CHECK (source IN ('drop','watch','external')),
  stats_json TEXT, report_json TEXT                   -- отчёт о дублях и итогах (ТЗ §7.6)
);

CREATE TABLE job (
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL, payload_json TEXT NOT NULL,
  track_id INTEGER, priority INTEGER NOT NULL DEFAULT 50,      -- меньше = важнее
  state TEXT NOT NULL DEFAULT 'queued' CHECK (state IN ('queued','running','done','failed','paused')),
  attempts INTEGER NOT NULL DEFAULT 0, next_run_at INTEGER NOT NULL DEFAULT 0, last_error TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX ix_job_pick ON job(state, priority, next_run_at);

CREATE TABLE fs_journal (                                -- crash-safe перемещения/удаления (§8)
  id INTEGER PRIMARY KEY, op TEXT NOT NULL CHECK (op IN ('copy','move','trash')),
  src TEXT NOT NULL, dst TEXT, state TEXT NOT NULL CHECK (state IN ('planned','done','rolled_back')),
  ts INTEGER NOT NULL
);

CREATE TABLE http_cache (key TEXT PRIMARY KEY, body BLOB NOT NULL, fetched_at INTEGER NOT NULL,
  ttl_s INTEGER NOT NULL, etag TEXT) WITHOUT ROWID;

-- Поиск (05 §11): две контентless-таблицы, обновляются из кода импорта/правок
CREATE VIRTUAL TABLE search_fts USING fts5(title, artists, album, genre, aliases,
  content='', tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE search_tri USING fts5(text, content='', tokenize='trigram');
