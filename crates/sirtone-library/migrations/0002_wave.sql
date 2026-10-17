-- 0002_wave: wave engine data (docs/14 §18.1, T1.30). Forward-only; frozen once shipped (D-66).
-- Stored codes: docs/14 §18.1 «Коды в хранимых полях» (D-69).

ALTER TABLE track  ADD COLUMN liked_at   INTEGER;                    -- лайк (NULL = нет)
ALTER TABLE track  ADD COLUMN wave_never INTEGER NOT NULL DEFAULT 0; -- «никогда в волне»
ALTER TABLE track  ADD COLUMN explicit   INTEGER;                    -- из метаданных; NULL = неизвестно
ALTER TABLE artist ADD COLUMN wave_never INTEGER NOT NULL DEFAULT 0;

CREATE TABLE track_features (
  track_id   INTEGER PRIMARY KEY REFERENCES track(id) ON DELETE CASCADE,
  version    INTEGER NOT NULL,              -- версия схемы вектора (§4.1)
  vec        BLOB NOT NULL,                 -- f32[32] LE
  conf       REAL NOT NULL,                 -- полнота анализа 0..1
  season_aff BLOB,                          -- f32[4] итог + f32[12] по источникам (звук/слова/история)
  kw_bits    INTEGER NOT NULL DEFAULT 0,    -- найденные словари сезонов/событий
  updated_at INTEGER NOT NULL
);

CREATE TABLE track_tag (                    -- ручные метки, перекрывают автоматику
  track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  tag   TEXT NOT NULL CHECK (tag IN ('season_winter','season_spring','season_summer','season_autumn','no_season',
                                     'sleep','focus','for_all_hit','not_for_company','event')),
  value TEXT NOT NULL DEFAULT '',           -- для 'event' — id события
  created_at INTEGER NOT NULL,
  PRIMARY KEY (track_id, tag, value)
) WITHOUT ROWID;

CREATE TABLE play_event (                   -- внутренний журнал, без раздела «История» в UI ✅
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,                      -- начало, unix-ms UTC
  track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  played_ms INTEGER NOT NULL, duration_ms INTEGER NOT NULL,
  end_reason TEXT NOT NULL CHECK (end_reason IN ('finished','mixed_out','skip','sleep','stop','quit','error')),
                                            -- mixed_out = ушёл в переход автомикса до конца (считается как finished)
  source TEXT NOT NULL CHECK (source IN ('wave','search','collection','playlist','album','artist','queue','hybrid','external')),
  profile TEXT,                             -- id профиля волны или NULL
  local_minute INTEGER NOT NULL, weekday INTEGER NOT NULL, daypart INTEGER NOT NULL,
  season_w BLOB NOT NULL,                   -- u8[4] (×255)
  weather INTEGER,                          -- класс погоды; NULL — нет/устарела
  event_id TEXT, novelty INTEGER,           -- 0..100
  category INTEGER                          -- группа знакомости на момент выбора
);
CREATE INDEX ix_pe_ts    ON play_event(ts);
CREATE INDEX ix_pe_track ON play_event(track_id, ts);

CREATE TABLE play_event_rollup (            -- старше 2 лет → агрегаты ✅
  track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  year INTEGER NOT NULL, season INTEGER NOT NULL, daypart INTEGER NOT NULL,
  plays INTEGER NOT NULL, skips INTEGER NOT NULL, played_ms INTEGER NOT NULL,
  PRIMARY KEY (track_id, year, season, daypart)
) WITHOUT ROWID;

CREATE TABLE feedback (
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL,
  track_id  INTEGER REFERENCES track(id)  ON DELETE CASCADE,
  artist_id INTEGER REFERENCES artist(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('like','unlike','not_now','never','unnever')),
  ctx_key TEXT, profile TEXT, expires_at INTEGER   -- not_now: ведро контекста и срок полки
);
CREATE INDEX ix_fb_track ON feedback(track_id, kind);

CREATE TABLE track_affinity (               -- бета «отношения к треку» по моделям
  track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  model TEXT NOT NULL CHECK (model IN ('main','focus','guests')),
  alpha REAL NOT NULL, beta REAL NOT NULL, updated_at INTEGER NOT NULL,
  PRIMARY KEY (track_id, model)
) WITHOUT ROWID;

CREATE TABLE taste_model (                  -- векторные модели (§8.2)
  key TEXT PRIMARY KEY,                     -- 'taste' | 'ctx:<daypart>' | 'ctx:<daypart>:<season>' | 'ctx:<daypart>:<season>:<weather>'
  version INTEGER NOT NULL, dim INTEGER NOT NULL,
  params BLOB NOT NULL,                     -- f32: m[dim], q[dim], b
  n_obs INTEGER NOT NULL, updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE wave_profile (
  id TEXT PRIMARY KEY,                      -- 'user:<uuid>' или id встроенного (переопределение)
  kind TEXT NOT NULL CHECK (kind IN ('user','builtin_override')),
  name TEXT, json TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE event_pref (
  event_id TEXT PRIMARY KEY,
  enabled INTEGER, auto_play INTEGER,
  declines_in_row INTEGER NOT NULL DEFAULT 0,
  disabled_reason TEXT CHECK (disabled_reason IN ('user','declined_3x','forever')),
  override_json TEXT                        -- задел под пользовательские события
) WITHOUT ROWID;
CREATE TABLE event_state (
  event_id TEXT NOT NULL, year INTEGER NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('shown','accepted','later','declined_year')),
  last_shown_day INTEGER,
  PRIMARY KEY (event_id, year)
) WITHOUT ROWID;

CREATE TABLE theme_preset (                 -- ручные переопределения акцента тем
  key TEXT PRIMARY KEY,                     -- 'season:winter' | 'event:new_year' | 'custom:<n>'
  accent TEXT NOT NULL, updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE weather_cache (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  fetched_at INTEGER NOT NULL, lat REAL NOT NULL, lon REAL NOT NULL,
  json TEXT NOT NULL,                       -- нормализованный WeatherState (§3.3) + sunrise/sunset
  first_snow_season INTEGER, first_warm_year INTEGER
);

CREATE TABLE popularity_cache (
  mbid TEXT PRIMARY KEY, listeners INTEGER, listens INTEGER, fetched_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE wave_state (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
-- focus_today ({day, cycles}), learning_frozen, last_suggestion_day, …
