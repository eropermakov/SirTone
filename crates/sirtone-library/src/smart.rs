//! Smart playlists (T3.13, FR-PLS-002; `05` §8.1, `01b` F-13): the Navidrome `.nsp` rule
//! schema (`all`/`any` groups, operators, `sort`, `limit`, `limitPercent`) with Apple semantics
//! (limit by time or size, live updating, read-only).
//!
//! Safety (`05` §13 «Умные плейлисты — 100 % безопасно»): field names, operators and sort keys
//! only select static SQL fragments from a whitelist; every value, the limit and the clock are
//! bound parameters. Rules are validated (types, depth ≤ 6, ≤ 64 conditions, strings ≤ 200
//! characters) and stored re-serialised in a canonical form.
//!
//! Text matches `norm()` forms (case, «ё», diacritics — as search does); `%`, `_` and `\` in a
//! value match literally. Negative operators include tracks where the field is empty («жанр не
//! рок» keeps tracks without a genre); positive comparisons on an empty field are false — the
//! analysis fields (BPM, energy, mood…) are empty until B1–B3 and match nothing yet. Sorting puts
//! empty values last and ends with the track id (stable order).

use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};
use serde_json::{Map, Value as Json, json};
use sirtone_core::text::norm;

use crate::collection::{TRACK_COLS, track_row};
use crate::error::LibraryError;
use crate::repo::TrackRow;

const MAX_DEPTH: usize = 6;
const MAX_LEAVES: usize = 64;
const MAX_TEXT: usize = 200;
const MAX_LIMIT: u32 = 100_000;
const DAY_MS: i64 = 24 * 3600 * 1000;

fn bad(msg: impl Into<String>) -> LibraryError {
    LibraryError::Invalid(format!("smart rules: {}", msg.into()))
}

// ---------- fields ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Num,
    Date,
    Bool,
    Enum,
}

/// A whitelisted field: SQL for its value, the `norm()` form for text, the sort key.
struct Field {
    name: &'static str,
    kind: Kind,
    expr: &'static str,
    /// Text compared against this (already normalised) expression.
    norm: &'static str,
    /// Allowed values of an enum field.
    values: &'static [&'static str],
}

const ARTIST_NORM: &str =
    "(SELECT a.name_norm FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
     WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1)";
const LIKED: &str = "coalesce((SELECT f.kind = 'like' FROM feedback f WHERE f.track_id = t.id
     AND f.kind IN ('like','unlike') ORDER BY f.id DESC LIMIT 1), 0)";

/// Mood bits of `track.mood_bits` (B3), in this order.
pub const MOODS: [&str; 6] = ["happy", "upbeat", "calm", "sad", "positive", "negative"];
pub const ACTIVITIES: [&str; 6] = ["workout", "sleep", "work", "road", "rest", "party"];
const SEASONS: [&str; 5] = ["winter", "spring", "summer", "autumn", "none"];

const FIELDS: &[Field] = &[
    Field {
        name: "title",
        kind: Kind::Text,
        expr: "t.title",
        norm: "t.title_norm",
        values: &[],
    },
    // The artist matches any of the track's artists (EXISTS), sorts by the first one.
    Field {
        name: "artist",
        kind: Kind::Text,
        expr: ARTIST_NORM,
        norm: "",
        values: &[],
    },
    Field {
        name: "album",
        kind: Kind::Text,
        expr: "al.title",
        norm: "al.title_norm",
        values: &[],
    },
    Field {
        name: "genre",
        kind: Kind::Text,
        expr: "t.genre_main",
        norm: "sirtone_norm(t.genre_main)",
        values: &[],
    },
    Field {
        name: "year",
        kind: Kind::Num,
        expr: "coalesce(t.year, al.year)",
        norm: "",
        values: &[],
    },
    Field {
        name: "added_at",
        kind: Kind::Date,
        expr: "t.added_at",
        norm: "",
        values: &[],
    },
    Field {
        name: "last_played",
        kind: Kind::Date,
        expr: "t.last_played_at",
        norm: "",
        values: &[],
    },
    Field {
        name: "play_count",
        kind: Kind::Num,
        expr: "t.play_count",
        norm: "",
        values: &[],
    },
    Field {
        name: "skip_count",
        kind: Kind::Num,
        expr: "t.skip_count",
        norm: "",
        values: &[],
    },
    Field {
        name: "favorite_score",
        kind: Kind::Num,
        expr: "sirtone_fav(t.play_count, t.completion_ema, t.skip_count)",
        norm: "",
        values: &[],
    },
    // Seconds.
    Field {
        name: "duration",
        kind: Kind::Num,
        expr: "(t.duration_ms / 1000.0)",
        norm: "",
        values: &[],
    },
    Field {
        name: "bitrate",
        kind: Kind::Num,
        expr: "t.bitrate_kbps",
        norm: "",
        values: &[],
    },
    Field {
        name: "lossless",
        kind: Kind::Bool,
        expr: "t.lossless",
        norm: "",
        values: &[],
    },
    Field {
        name: "format",
        kind: Kind::Text,
        expr: "t.codec",
        norm: "sirtone_norm(t.codec)",
        values: &[],
    },
    // Megabytes.
    Field {
        name: "size",
        kind: Kind::Num,
        expr: "(t.size_bytes / 1048576.0)",
        norm: "",
        values: &[],
    },
    Field {
        name: "bpm",
        kind: Kind::Num,
        expr: "t.bpm",
        norm: "",
        values: &[],
    },
    Field {
        name: "key",
        kind: Kind::Text,
        expr: "t.key_camelot",
        norm: "sirtone_norm(t.key_camelot)",
        values: &[],
    },
    Field {
        name: "energy",
        kind: Kind::Num,
        expr: "t.energy",
        norm: "",
        values: &[],
    },
    Field {
        name: "valence",
        kind: Kind::Num,
        expr: "t.valence",
        norm: "",
        values: &[],
    },
    Field {
        name: "arousal",
        kind: Kind::Num,
        expr: "t.arousal",
        norm: "",
        values: &[],
    },
    Field {
        name: "danceability",
        kind: Kind::Num,
        expr: "t.danceability",
        norm: "",
        values: &[],
    },
    Field {
        name: "acousticness",
        kind: Kind::Num,
        expr: "t.acousticness",
        norm: "",
        values: &[],
    },
    Field {
        name: "vocal",
        kind: Kind::Num,
        expr: "t.vocal",
        norm: "",
        values: &[],
    },
    Field {
        name: "lang",
        kind: Kind::Enum,
        expr: "t.lang",
        norm: "",
        values: &["ru", "foreign", "none", "unknown"],
    },
    Field {
        name: "mood",
        kind: Kind::Enum,
        expr: "t.mood_bits",
        norm: "",
        values: &MOODS,
    },
    Field {
        name: "activity",
        kind: Kind::Enum,
        expr: "t.activity_json",
        norm: "",
        values: &ACTIVITIES,
    },
    Field {
        name: "identity_state",
        kind: Kind::Enum,
        expr: "t.identity_state",
        norm: "",
        values: &["confirmed", "inferred", "user", "unknown"],
    },
    Field {
        name: "path",
        kind: Kind::Text,
        expr: "t.rel_path",
        norm: "sirtone_norm(t.rel_path)",
        values: &[],
    },
    Field {
        name: "liked",
        kind: Kind::Bool,
        expr: LIKED,
        norm: "",
        values: &[],
    },
    Field {
        name: "season_tag",
        kind: Kind::Enum,
        expr: "",
        norm: "",
        values: &SEASONS,
    },
    Field {
        name: "explicit",
        kind: Kind::Bool,
        expr: "t.explicit",
        norm: "",
        values: &[],
    },
];

fn field(name: &str) -> Result<&'static Field, LibraryError> {
    FIELDS
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| bad("unknown field"))
}

// ---------- rules ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    All(Vec<Node>),
    Any(Vec<Node>),
    /// `{ "<op>": { "<field>": value } }`.
    Cond {
        op: &'static str,
        field: &'static str,
        value: Json,
    },
    /// `{ "inPlaylist": { "id": N } }` — manual playlists only.
    InPlaylist {
        id: i64,
        not: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Limit {
    None,
    Count(u32),
    Percent(u32),
    Minutes(u32),
    Megabytes(u32),
}

/// `live` — evaluated every time; `hours(n)` / `manual` — a snapshot refreshed when older
/// (Navidrome `refreshDelay`) or on «Обновить».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    Live,
    Hours(u32),
    Manual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    pub root: Node,
    /// (field, descending); empty → by title.
    pub sort: Vec<(&'static str, bool)>,
    pub random: bool,
    pub limit: Limit,
    pub refresh: Refresh,
}

const OPS_TEXT: &[&str] = &[
    "is",
    "isNot",
    "contains",
    "notContains",
    "startsWith",
    "endsWith",
    "isMissing",
    "isPresent",
];
const OPS_NUM: &[&str] = &[
    "is",
    "isNot",
    "gt",
    "lt",
    "inTheRange",
    "isMissing",
    "isPresent",
];
const OPS_DATE: &[&str] = &[
    "before",
    "after",
    "inTheLast",
    "notInTheLast",
    "isMissing",
    "isPresent",
];
const OPS_BOOL: &[&str] = &["is"];
const OPS_ENUM: &[&str] = &["is", "isNot"];

fn ops(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Text => OPS_TEXT,
        Kind::Num => OPS_NUM,
        Kind::Date => OPS_DATE,
        Kind::Bool => OPS_BOOL,
        Kind::Enum => OPS_ENUM,
    }
}

/// `YYYY-MM-DD` → ms at UTC midnight.
fn date_ms(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let (y, m, d): (i32, u32, u32) = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as i32;
    Some(i64::from(era * 146_097 + doe - 719_468) * DAY_MS)
}

fn check_value(f: &Field, op: &str, v: &Json) -> Result<(), LibraryError> {
    let num = |v: &Json| v.as_f64().filter(|x| x.is_finite()).is_some();
    // Text is compared in norm() form: a value that normalises to nothing («_», «—») would
    // match everything, so it is refused.
    let text = |v: &Json| {
        v.as_str()
            .is_some_and(|s| s.chars().count() <= MAX_TEXT && norm(s).is_some())
    };
    let ok = match op {
        "isMissing" | "isPresent" => v.as_bool() == Some(true),
        "inTheRange" => v
            .as_array()
            .is_some_and(|a| a.len() == 2 && a.iter().all(num)),
        "inTheLast" | "notInTheLast" => v.as_f64().is_some_and(|d| d.is_finite() && d >= 0.0),
        "before" | "after" => v.as_str().and_then(date_ms).is_some(),
        _ => match f.kind {
            Kind::Text => text(v),
            Kind::Num => num(v),
            Kind::Bool => v.is_boolean(),
            Kind::Enum => v.as_str().is_some_and(|s| f.values.contains(&s)),
            Kind::Date => false,
        },
    };
    if ok { Ok(()) } else { Err(bad("bad value")) }
}

fn single(obj: &Map<String, Json>) -> Result<(&String, &Json), LibraryError> {
    let mut it = obj.iter();
    match (it.next(), it.next()) {
        (Some(kv), None) => Ok(kv),
        _ => Err(bad("one key expected")),
    }
}

fn parse_node(v: &Json, depth: usize, leaves: &mut usize) -> Result<Node, LibraryError> {
    if depth > MAX_DEPTH {
        return Err(bad("too deep"));
    }
    let obj = v.as_object().ok_or_else(|| bad("object expected"))?;
    let (key, body) = single(obj)?;
    match key.as_str() {
        "all" | "any" => {
            let items = body.as_array().ok_or_else(|| bad("array expected"))?;
            let nodes = items
                .iter()
                .map(|n| parse_node(n, depth + 1, leaves))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(if key == "all" {
                Node::All(nodes)
            } else {
                Node::Any(nodes)
            })
        }
        "inPlaylist" | "notInPlaylist" => {
            *leaves += 1;
            let o = body.as_object().ok_or_else(|| bad("object expected"))?;
            let (k, id) = single(o)?;
            let id = id
                .as_i64()
                .filter(|_| k == "id")
                .ok_or_else(|| bad("playlist id"))?;
            Ok(Node::InPlaylist {
                id,
                not: key == "notInPlaylist",
            })
        }
        op => {
            *leaves += 1;
            if *leaves > MAX_LEAVES {
                return Err(bad("too many conditions"));
            }
            let o = body.as_object().ok_or_else(|| bad("object expected"))?;
            let (name, value) = single(o)?;
            let f = field(name)?;
            let op = ops(f.kind)
                .iter()
                .copied()
                .find(|o| *o == op)
                .ok_or_else(|| bad("operator does not fit the field"))?;
            check_value(f, op, value)?;
            Ok(Node::Cond {
                op,
                field: f.name,
                value: value.clone(),
            })
        }
    }
}

impl Rules {
    /// Parses and validates `rules_json`.
    pub fn parse(text: &str) -> Result<Self, LibraryError> {
        if text.len() > 64 * 1024 {
            return Err(bad("too large"));
        }
        let v: Json = serde_json::from_str(text).map_err(|_| bad("not JSON"))?;
        let obj = v.as_object().ok_or_else(|| bad("object expected"))?;
        let mut leaves = 0;
        let mut root = None;
        let mut rules = Self {
            root: Node::All(Vec::new()),
            sort: Vec::new(),
            random: false,
            limit: Limit::None,
            refresh: Refresh::Live,
        };
        let mut descending = false;
        for (k, val) in obj {
            match k.as_str() {
                "all" | "any" => {
                    if root.is_some() {
                        return Err(bad("all and any on one level"));
                    }
                    let mut m = Map::new();
                    m.insert(k.clone(), val.clone());
                    root = Some(parse_node(&Json::Object(m), 0, &mut leaves)?);
                }
                "sort" => {
                    let s = val.as_str().ok_or_else(|| bad("sort"))?;
                    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                        if part == "random" {
                            rules.random = true;
                            continue;
                        }
                        let (desc, name) = match part.strip_prefix('-') {
                            Some(n) => (true, n),
                            None => (false, part.strip_prefix('+').unwrap_or(part)),
                        };
                        let f = field(name)?;
                        if f.name == "season_tag" || f.name == "mood" || f.name == "activity" {
                            return Err(bad("not sortable"));
                        }
                        rules.sort.push((f.name, desc));
                    }
                    if rules.sort.len() > 8 {
                        return Err(bad("sort"));
                    }
                }
                "order" => match val.as_str() {
                    Some("asc") => {}
                    Some("desc") => descending = true,
                    _ => return Err(bad("order")),
                },
                "limit" | "limitPercent" | "limitMinutes" | "limitMb" => {
                    let n = val
                        .as_u64()
                        .filter(|n| *n > 0 && *n <= u64::from(MAX_LIMIT))
                        .ok_or_else(|| bad("limit"))? as u32;
                    if rules.limit != Limit::None {
                        return Err(bad("one limit"));
                    }
                    rules.limit = match k.as_str() {
                        "limit" => Limit::Count(n),
                        "limitPercent" if n <= 100 => Limit::Percent(n),
                        "limitMinutes" => Limit::Minutes(n),
                        "limitMb" => Limit::Megabytes(n),
                        _ => return Err(bad("limit")),
                    };
                }
                "refresh" => {
                    let s = val.as_str().ok_or_else(|| bad("refresh"))?;
                    rules.refresh = match s {
                        "live" => Refresh::Live,
                        "manual" => Refresh::Manual,
                        _ => {
                            let (n, mul) = if let Some(h) = s.strip_suffix('h') {
                                (h, 1)
                            } else if let Some(d) = s.strip_suffix('d') {
                                (d, 24)
                            } else {
                                return Err(bad("refresh"));
                            };
                            let n: u32 = n.parse().map_err(|_| bad("refresh"))?;
                            if n == 0 || n > 24 * 30 {
                                return Err(bad("refresh"));
                            }
                            Refresh::Hours(n * mul)
                        }
                    };
                }
                _ => return Err(bad("unknown key")),
            }
        }
        if descending {
            for s in &mut rules.sort {
                s.1 = !s.1;
            }
        }
        rules.root = root.ok_or_else(|| bad("all or any expected"))?;
        Ok(rules)
    }

    /// Canonical JSON (what is stored).
    pub fn to_json(&self) -> String {
        fn node(n: &Node) -> Json {
            match n {
                Node::All(v) => json!({ "all": v.iter().map(node).collect::<Vec<_>>() }),
                Node::Any(v) => json!({ "any": v.iter().map(node).collect::<Vec<_>>() }),
                Node::Cond { op, field, value } => json!({ *op: { *field: value } }),
                Node::InPlaylist { id, not } => {
                    json!({ (if *not { "notInPlaylist" } else { "inPlaylist" }): { "id": id } })
                }
            }
        }
        let mut o = match node(&self.root) {
            Json::Object(o) => o,
            _ => Map::new(),
        };
        let mut sort: Vec<String> = self
            .sort
            .iter()
            .map(|(f, d)| format!("{}{f}", if *d { "-" } else { "+" }))
            .collect();
        if self.random {
            sort.push("random".into());
        }
        if !sort.is_empty() {
            o.insert("sort".into(), json!(sort.join(",")));
        }
        match self.limit {
            Limit::None => {}
            Limit::Count(n) => {
                o.insert("limit".into(), json!(n));
            }
            Limit::Percent(n) => {
                o.insert("limitPercent".into(), json!(n));
            }
            Limit::Minutes(n) => {
                o.insert("limitMinutes".into(), json!(n));
            }
            Limit::Megabytes(n) => {
                o.insert("limitMb".into(), json!(n));
            }
        }
        o.insert(
            "refresh".into(),
            json!(match self.refresh {
                Refresh::Live => "live".to_string(),
                Refresh::Manual => "manual".to_string(),
                Refresh::Hours(h) if h % 24 == 0 => format!("{}d", h / 24),
                Refresh::Hours(h) => format!("{h}h"),
            }),
        );
        Json::Object(o).to_string()
    }

    /// Playlists the rules reference (to check they are manual).
    pub fn playlist_refs(&self) -> Vec<i64> {
        fn walk(n: &Node, out: &mut Vec<i64>) {
            match n {
                Node::All(v) | Node::Any(v) => v.iter().for_each(|c| walk(c, out)),
                Node::InPlaylist { id, .. } => out.push(*id),
                Node::Cond { .. } => {}
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

// ---------- compiler ----------

/// SQL `WHERE` text (only static fragments and `?`) and its parameters.
#[derive(Debug, Default)]
pub struct Compiled {
    pub sql: String,
    pub params: Vec<Value>,
}

/// `LIKE` pattern with `%`, `_` and `\` escaped.
fn like(prefix: bool, v: &str, suffix: bool) -> String {
    let mut s = String::with_capacity(v.len() + 2);
    if prefix {
        s.push('%');
    }
    for c in v.chars() {
        if matches!(c, '%' | '_' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    if suffix {
        s.push('%');
    }
    s
}

fn text_param(v: &Json) -> String {
    v.as_str().and_then(norm).unwrap_or_default()
}

impl Compiled {
    fn push(&mut self, v: Value) {
        self.params.push(v);
    }

    fn node(&mut self, n: &Node, now_ms: i64) {
        match n {
            Node::All(v) | Node::Any(v) => {
                if v.is_empty() {
                    // Empty «all» — everything; empty «any» — nothing.
                    self.sql
                        .push_str(if matches!(n, Node::All(_)) { "1" } else { "0" });
                    return;
                }
                let join = if matches!(n, Node::All(_)) {
                    " AND "
                } else {
                    " OR "
                };
                self.sql.push('(');
                for (i, c) in v.iter().enumerate() {
                    if i > 0 {
                        self.sql.push_str(join);
                    }
                    self.node(c, now_ms);
                }
                self.sql.push(')');
            }
            Node::InPlaylist { id, not } => {
                if *not {
                    self.sql.push_str("NOT ");
                }
                self.sql.push_str(
                    "EXISTS (SELECT 1 FROM playlist_item pi JOIN playlist p ON p.id = pi.playlist_id
                     WHERE p.id = ? AND p.kind = 'manual' AND pi.track_id = t.id)",
                );
                self.push(Value::Integer(*id));
            }
            Node::Cond {
                op,
                field: name,
                value,
            } => {
                // `field` was validated against the whitelist at parse time.
                let Ok(f) = field(name) else {
                    self.sql.push('0');
                    return;
                };
                self.cond(f, op, value, now_ms);
            }
        }
    }

    fn cond(&mut self, f: &Field, op: &str, v: &Json, now_ms: i64) {
        let negative = matches!(op, "isNot" | "notContains" | "notInTheLast");
        // Special fields first.
        match f.name {
            "artist" if f.kind == Kind::Text => {
                if matches!(op, "isMissing" | "isPresent") {
                    if op == "isMissing" {
                        self.sql.push_str("NOT ");
                    }
                    self.sql.push_str(
                        "EXISTS (SELECT 1 FROM track_artist ta WHERE ta.track_id = t.id)",
                    );
                    return;
                }
                if negative {
                    self.sql.push_str("NOT ");
                }
                self.sql.push_str(
                    "EXISTS (SELECT 1 FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                     WHERE ta.track_id = t.id AND a.name_norm ",
                );
                self.text_match(op, v);
                self.sql.push(')');
                return;
            }
            "season_tag" => {
                let tag = match v.as_str() {
                    Some("none") => "no_season".to_string(),
                    Some(s) => format!("season_{s}"),
                    None => String::new(),
                };
                if op == "isNot" {
                    self.sql.push_str("NOT ");
                }
                self.sql.push_str(
                    "EXISTS (SELECT 1 FROM track_tag tg WHERE tg.track_id = t.id AND tg.tag = ?)",
                );
                self.push(Value::Text(tag));
                return;
            }
            "mood" => {
                let bit = MOODS
                    .iter()
                    .position(|m| Some(*m) == v.as_str())
                    .unwrap_or(0);
                // An unanalysed track (mood_bits 0) is neither the mood nor «not» it.
                self.sql.push_str(if op == "isNot" {
                    "(t.analysis_version > 0 AND (t.mood_bits & ?) = 0)"
                } else {
                    "((t.mood_bits & ?) <> 0)"
                });
                self.push(Value::Integer(1 << bit));
                return;
            }
            "activity" => {
                self.sql.push_str(if op == "isNot" {
                    "(t.activity_json IS NULL OR coalesce(json_extract(t.activity_json, ?), 0) < 0.5)"
                } else {
                    "(json_extract(t.activity_json, ?) >= 0.5)"
                });
                self.push(Value::Text(format!("$.{}", v.as_str().unwrap_or(""))));
                return;
            }
            _ => {}
        }
        let e = f.expr;
        match op {
            "isMissing" => {
                self.sql.push('(');
                self.sql.push_str(e);
                self.sql.push_str(" IS NULL)");
            }
            "isPresent" => {
                self.sql.push('(');
                self.sql.push_str(e);
                self.sql.push_str(" IS NOT NULL)");
            }
            "inTheLast" | "notInTheLast" => {
                let since = now_ms - (v.as_f64().unwrap_or(0.0) * DAY_MS as f64) as i64;
                self.sql.push('(');
                if op == "notInTheLast" {
                    self.sql.push_str(e);
                    self.sql.push_str(" IS NULL OR ");
                    self.sql.push_str(e);
                    self.sql.push_str(" < ?)");
                } else {
                    self.sql.push_str(e);
                    self.sql.push_str(" >= ?)");
                }
                self.push(Value::Integer(since));
            }
            "before" | "after" => {
                let at = v.as_str().and_then(date_ms).unwrap_or(0);
                self.sql.push('(');
                self.sql.push_str(e);
                self.sql
                    .push_str(if op == "before" { " < ?)" } else { " >= ?)" });
                self.push(Value::Integer(at));
            }
            "inTheRange" => {
                let a = v.as_array().map(|a| {
                    (
                        a.first().and_then(Json::as_f64).unwrap_or(0.0),
                        a.get(1).and_then(Json::as_f64).unwrap_or(0.0),
                    )
                });
                let (lo, hi) = a.unwrap_or((0.0, 0.0));
                self.sql.push('(');
                self.sql.push_str(e);
                self.sql.push_str(" BETWEEN ? AND ?)");
                self.push(Value::Real(lo.min(hi)));
                self.push(Value::Real(lo.max(hi)));
            }
            "gt" | "lt" => {
                self.sql.push('(');
                self.sql.push_str(e);
                self.sql
                    .push_str(if op == "gt" { " > ?)" } else { " < ?)" });
                self.push(Value::Real(v.as_f64().unwrap_or(0.0)));
            }
            _ => match f.kind {
                Kind::Text => {
                    self.sql.push('(');
                    if negative {
                        self.sql.push_str(f.norm);
                        self.sql.push_str(" IS NULL OR NOT ");
                    }
                    self.sql.push('(');
                    self.sql.push_str(f.norm);
                    self.sql.push(' ');
                    let positive = match op {
                        "isNot" => "is",
                        "notContains" => "contains",
                        o => o,
                    };
                    self.text_match(positive, v);
                    self.sql.push_str("))");
                }
                Kind::Num => {
                    self.sql.push('(');
                    if negative {
                        self.sql.push_str(e);
                        self.sql.push_str(" IS NULL OR ");
                        self.sql.push_str(e);
                        self.sql.push_str(" <> ?)");
                    } else {
                        self.sql.push_str(e);
                        self.sql.push_str(" = ?)");
                    }
                    self.push(Value::Real(v.as_f64().unwrap_or(0.0)));
                }
                Kind::Bool => {
                    self.sql.push_str("(coalesce(");
                    self.sql.push_str(e);
                    self.sql.push_str(", 0) = ?)");
                    self.push(Value::Integer(i64::from(v.as_bool() == Some(true))));
                }
                Kind::Enum => {
                    self.sql.push('(');
                    if negative {
                        self.sql.push_str(e);
                        self.sql.push_str(" IS NULL OR ");
                        self.sql.push_str(e);
                        self.sql.push_str(" <> ?)");
                    } else {
                        self.sql.push_str(e);
                        self.sql.push_str(" = ?)");
                    }
                    self.push(Value::Text(v.as_str().unwrap_or("").to_string()));
                }
                Kind::Date => self.sql.push('0'),
            },
        }
    }

    /// `<norm expr>` is already written; appends the comparison with a normalised value.
    fn text_match(&mut self, op: &str, v: &Json) {
        let n = text_param(v);
        let (sql, p) = match op {
            "is" | "isNot" => ("= ?", n),
            "contains" | "notContains" => ("LIKE ? ESCAPE '\\'", like(true, &n, true)),
            "startsWith" => ("LIKE ? ESCAPE '\\'", like(false, &n, true)),
            "endsWith" => ("LIKE ? ESCAPE '\\'", like(true, &n, false)),
            _ => ("= ?", n),
        };
        self.sql.push_str(sql);
        self.push(Value::Text(p));
    }
}

/// Compiles the rules: `WHERE` part and `ORDER BY` part (static text) with parameters.
pub fn compile(rules: &Rules, now_ms: i64) -> (Compiled, String) {
    let mut c = Compiled::default();
    c.node(&rules.root, now_ms);
    let mut order = String::new();
    for (name, desc) in &rules.sort {
        let Ok(f) = field(name) else { continue };
        let key = if f.kind == Kind::Text && !f.norm.is_empty() {
            f.norm
        } else {
            f.expr
        };
        order.push_str(key);
        order.push_str(" IS NULL, ");
        order.push_str(key);
        order.push_str(if *desc { " DESC, " } else { " ASC, " });
    }
    if rules.random {
        order.push_str("random(), ");
    }
    if rules.sort.is_empty() && !rules.random {
        order.push_str("t.title_norm, ");
    }
    order.push_str("t.id");
    (c, order)
}

/// Functions the compiled SQL uses: `sirtone_norm` (search normalisation) and `sirtone_fav`
/// (`05` §10.1 favourite score). Registered per call — cheap, and any connection works.
fn register(conn: &Connection) -> Result<(), LibraryError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    conn.create_scalar_function("sirtone_norm", 1, flags, |ctx| {
        Ok(ctx.get::<Option<String>>(0)?.and_then(|s| norm(&s)))
    })?;
    conn.create_scalar_function("sirtone_fav", 3, flags, |ctx| {
        let plays: f64 = ctx.get(0)?;
        let completion: f64 = ctx.get(1)?;
        let skips: f64 = ctx.get(2)?;
        let heard = (plays + skips).max(1.0);
        Ok(0.6 * (1.0 + plays).ln() + 0.4 * completion - 0.5 * skips / heard)
    })?;
    Ok(())
}

const FROM: &str =
    "FROM track t LEFT JOIN album al ON al.id = t.album_id WHERE t.state = 'ok' AND ";

/// How many tracks match (before the limit) — the editor's «подходит N треков».
pub fn count(conn: &Connection, rules: &Rules, now_ms: i64) -> Result<u32, LibraryError> {
    register(conn)?;
    let (c, _) = compile(rules, now_ms);
    let sql = format!("SELECT count(*) {FROM}{}", c.sql);
    Ok(conn.query_row(&sql, params_from_iter(c.params.iter()), |r| r.get(0))?)
}

/// SQL selecting `cols` of the matched tracks in playlist order with the limit applied, and
/// its parameters.
fn select(
    conn: &Connection,
    rules: &Rules,
    now_ms: i64,
    cols: &str,
) -> Result<(String, Vec<Value>), LibraryError> {
    let (c, order) = compile(rules, now_ms);
    let mut params = c.params.clone();
    let sql = match rules.limit {
        Limit::None => format!("SELECT {cols} {FROM}{} ORDER BY {order}", c.sql),
        Limit::Count(_) | Limit::Percent(_) => {
            let n = match rules.limit {
                Limit::Count(n) => i64::from(n),
                Limit::Percent(p) => {
                    let total = i64::from(count(conn, rules, now_ms)?);
                    (total * i64::from(p) + 99) / 100
                }
                _ => 0,
            };
            params.push(Value::Integer(n));
            format!("SELECT {cols} {FROM}{} ORDER BY {order} LIMIT ?", c.sql)
        }
        // Apple «limit to N minutes / MB»: tracks in order while the running total fits.
        Limit::Minutes(_) | Limit::Megabytes(_) => {
            let (col, cap) = match rules.limit {
                Limit::Minutes(m) => ("t.duration_ms", i64::from(m) * 60_000),
                Limit::Megabytes(mb) => ("t.size_bytes", i64::from(mb) * 1_048_576),
                _ => ("0", 0),
            };
            params.push(Value::Integer(cap));
            // One named window: the running total and the row order agree (also with random()).
            format!(
                "SELECT * FROM (SELECT {cols},
                   sum({col}) OVER (w ROWS UNBOUNDED PRECEDING) AS run_total,
                   row_number() OVER w AS rn
                 {FROM}{} WINDOW w AS (ORDER BY {order}))
                 WHERE run_total <= ? ORDER BY rn",
                c.sql
            )
        }
    };
    Ok((sql, params))
}

/// The playlist's tracks now: matched, sorted, limited.
pub fn evaluate(
    conn: &Connection,
    rules: &Rules,
    now_ms: i64,
) -> Result<Vec<TrackRow>, LibraryError> {
    register(conn)?;
    let (sql, params) = select(conn, rules, now_ms, TRACK_COLS)?;
    let mut st = conn.prepare(&sql)?;
    let rows = st
        .query_map(params_from_iter(params.iter()), track_row)?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// What the sidebar shows for a live smart playlist, aggregated in SQL (no rows built).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub tracks: u32,
    pub duration_ms: u64,
    pub bytes: u64,
    /// Album ids of the first tracks (≤ 64, for the mosaic).
    pub albums: Vec<i64>,
}

pub fn stats(conn: &Connection, rules: &Rules, now_ms: i64) -> Result<Stats, LibraryError> {
    register(conn)?;
    let (sql, params) = select(
        conn,
        rules,
        now_ms,
        "t.duration_ms AS d, t.size_bytes AS b, t.album_id AS a",
    )?;
    // One pass: totals over the whole set as window aggregates, the first 64 rows for covers.
    let mut st = conn.prepare(&format!(
        "SELECT count(*) OVER (), sum(d) OVER (), sum(b) OVER (), a FROM ({sql}) LIMIT 64"
    ))?;
    let mut rows = st.query(params_from_iter(params.iter()))?;
    let (mut tracks, mut d, mut b) = (0u32, 0i64, 0i64);
    let mut albums = Vec::new();
    while let Some(r) = rows.next()? {
        tracks = r.get(0)?;
        d = r.get::<_, Option<i64>>(1)?.unwrap_or(0);
        b = r.get::<_, Option<i64>>(2)?.unwrap_or(0);
        if let Some(a) = r.get::<_, Option<i64>>(3)? {
            albums.push(a);
        }
    }
    Ok(Stats {
        tracks,
        duration_ms: d.max(0) as u64,
        bytes: b.max(0) as u64,
        albums,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_validates_and_round_trips() {
        let r = Rules::parse(
            r#"{"all":[{"inTheLast":{"added_at":30}},{"gt":{"year":1999}},
                {"any":[{"contains":{"title":"Лето"}},{"is":{"liked":true}}]}],
               "sort":"-last_played,title","limit":50,"refresh":"12h"}"#,
        )
        .unwrap();
        assert_eq!(r.limit, Limit::Count(50));
        assert_eq!(r.refresh, Refresh::Hours(12));
        assert_eq!(r.sort, [("last_played", true), ("title", false)]);
        let again = Rules::parse(&r.to_json()).unwrap();
        assert_eq!(again, r);
        for bad in [
            r#"{"all":[{"gt":{"year":"1999"}}]}"#, // a string for a number
            r#"{"all":[{"gt":{"title":"a"}}]}"#,   // no gt for text
            r#"{"all":[{"is":{"titel":"a"}}]}"#,   // unknown field
            r#"{"all":[{"drop":{"title":"a"}}]}"#, // unknown operator
            r#"{"all":[],"any":[]}"#,              // both on one level
            r#"{"all":[{"is":{"lang":"klingon"}}]}"#, // not an enum value
            r#"{"all":[],"sort":"title; DROP TABLE track"}"#,
            r#"{"all":[],"limitPercent":150}"#,
            r#"{"all":[],"refresh":"soon"}"#,
            r#"{"all":[{"before":{"added_at":"yesterday"}}]}"#,
        ] {
            assert!(Rules::parse(bad).is_err(), "{bad}");
        }
        // Depth and size caps.
        let mut deep = String::from(r#"{"is":{"liked":true}}"#);
        for _ in 0..8 {
            deep = format!(r#"{{"all":[{deep}]}}"#);
        }
        assert!(Rules::parse(&deep).is_err());
        let many: Vec<String> = (0..70)
            .map(|i| format!(r#"{{"gt":{{"year":{i}}}}}"#))
            .collect();
        assert!(Rules::parse(&format!(r#"{{"all":[{}]}}"#, many.join(","))).is_err());
    }

    use crate::db::open;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};

    fn lib(tag: &str) -> Connection {
        let root = std::env::temp_dir().join(format!("sirtone-smart-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap()
    }

    fn track(c: &mut Connection, n: u32, title: &str, artist: &str, year: Option<i32>) -> i64 {
        let a = artist_get_or_create(c, artist).unwrap();
        track_insert(
            c,
            &NewTrack {
                rel_path: format!("A/{n:03}.mp3"),
                size_bytes: 3 * 1_048_576,
                mtime_ms: 0,
                content_hash: format!("h{n}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 180_000,
                title: title.into(),
                album_id: None,
                disc_no: None,
                track_no: None,
                year,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![a],
            },
        )
        .unwrap()
    }

    fn titles(c: &Connection, rules: &str) -> Vec<String> {
        let r = Rules::parse(rules).unwrap();
        evaluate(c, &r, 1_800_000_000_000)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect()
    }

    fn fixture(tag: &str) -> Connection {
        let mut c = lib(tag);
        track(&mut c, 1, "Кино", "Виктор Цой", Some(1988));
        track(&mut c, 2, "кИНО и вино", "Би-2", Some(2001));
        track(&mut c, 3, "Ёлка", "Ёлка", None);
        track(&mut c, 4, "50% скидка", "Ленинград", Some(2010));
        track(&mut c, 5, "a_b", "Band", Some(1999));
        track(&mut c, 6, "'); DROP TABLE track;--", "Hacker", Some(2020));
        c.execute(
            "UPDATE track SET genre_main = 'Rock' WHERE id IN (1, 2)",
            [],
        )
        .unwrap();
        c
    }

    #[test]
    fn text_matches_norm_forms_and_literal_wildcards() {
        let c = fixture("text");
        assert_eq!(
            titles(
                &c,
                r#"{"all":[{"contains":{"title":"КИНО"}}],"sort":"year"}"#
            ),
            ["Кино", "кИНО и вино"]
        );
        // «ё» = «е», case-insensitive.
        assert_eq!(titles(&c, r#"{"all":[{"is":{"title":"елка"}}]}"#), ["Ёлка"]);
        // % and _ are literal.
        assert_eq!(
            titles(&c, r#"{"all":[{"contains":{"title":"50%"}}]}"#),
            ["50% скидка"]
        );
        assert_eq!(
            titles(&c, r#"{"all":[{"contains":{"title":"a_b"}}]}"#),
            ["a_b"]
        );
        // A value that normalises to nothing would match everything: refused.
        assert!(Rules::parse(r#"{"all":[{"contains":{"title":"_"}}]}"#).is_err());
        // A hostile value is just text.
        assert_eq!(
            titles(
                &c,
                r#"{"all":[{"is":{"title":"'); DROP TABLE track;--"}}]}"#
            ),
            ["'); DROP TABLE track;--"]
        );
        // Any of the track's artists, by norm («Би 2» = «Би-2»).
        assert_eq!(
            titles(&c, r#"{"all":[{"is":{"artist":"би 2"}}]}"#),
            ["кИНО и вино"]
        );
        let n: i64 = c
            .query_row("SELECT count(*) FROM track", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 6);
    }

    #[test]
    fn empty_fields_negative_operators_and_sorting() {
        let c = fixture("null");
        // «жанр не rock» keeps tracks without a genre.
        assert_eq!(
            titles(&c, r#"{"all":[{"isNot":{"genre":"rock"}}]}"#).len(),
            4
        );
        // Analysis fields are empty: nothing matches either way.
        assert!(titles(&c, r#"{"all":[{"gt":{"bpm":100}}]}"#).is_empty());
        assert!(titles(&c, r#"{"all":[{"is":{"mood":"sad"}}]}"#).is_empty());
        assert!(titles(&c, r#"{"all":[{"isNot":{"mood":"sad"}}]}"#).is_empty());
        // Empty years sort last in both directions; ties by id.
        let asc = titles(&c, r#"{"all":[],"sort":"+year"}"#);
        let desc = titles(&c, r#"{"all":[],"sort":"-year"}"#);
        assert_eq!(asc.first().map(String::as_str), Some("Кино"));
        assert_eq!(asc.last().map(String::as_str), Some("Ёлка"));
        assert_eq!(desc.last().map(String::as_str), Some("Ёлка"));
        assert_eq!(
            titles(&c, r#"{"all":[{"isMissing":{"year":true}}]}"#),
            ["Ёлка"]
        );
        // Groups: any inside all.
        assert_eq!(
            titles(
                &c,
                r#"{"all":[{"gt":{"year":1990}},{"any":[{"contains":{"title":"вино"}},{"is":{"artist":"Band"}}]}],"sort":"year"}"#
            ),
            ["a_b", "кИНО и вино"]
        );
    }

    #[test]
    fn limits() {
        let c = fixture("limit");
        assert_eq!(titles(&c, r#"{"all":[],"limit":2}"#).len(), 2);
        assert_eq!(titles(&c, r#"{"all":[],"limitPercent":50}"#).len(), 3);
        // 3 min / 3 MB each: 7 minutes → 2 tracks; 7 MB → 2.
        assert_eq!(titles(&c, r#"{"all":[],"limitMinutes":7}"#).len(), 2);
        assert_eq!(titles(&c, r#"{"all":[],"limitMb":7}"#).len(), 2);
        let r = Rules::parse(r#"{"all":[],"limit":1}"#).unwrap();
        assert_eq!(count(&c, &r, 0).unwrap(), 6);
    }

    #[test]
    fn in_playlist_only_manual() {
        let mut c = fixture("inpl");
        let p = crate::playlists::create(&c, "Мой").unwrap();
        crate::playlists::add(&mut c, p, &[1, 3]).unwrap();
        let rules = format!(r#"{{"all":[{{"inPlaylist":{{"id":{p}}}}}],"sort":"title"}}"#);
        assert_eq!(titles(&c, &rules), ["Ёлка", "Кино"]);
        let not = format!(r#"{{"all":[{{"notInPlaylist":{{"id":{p}}}}}]}}"#);
        assert_eq!(titles(&c, &not).len(), 4);
    }

    mod safety {
        use super::*;
        use proptest::prelude::*;

        fn leaf() -> impl Strategy<Value = Json> {
            let names: Vec<&'static str> = FIELDS.iter().map(|f| f.name).collect();
            (
                proptest::sample::select(names),
                any::<String>(),
                any::<f64>(),
                any::<bool>(),
            )
                .prop_map(|(name, text, num, b)| {
                    let f = field(name).unwrap();
                    let list = ops(f.kind);
                    let op = list[(text.len() + usize::from(b)) % list.len()];
                    let text: String = text.chars().take(MAX_TEXT).collect();
                    let v = match op {
                        "isMissing" | "isPresent" => json!(true),
                        "inTheRange" => json!([num.clamp(-1e9, 1e9), 1.0]),
                        "inTheLast" | "notInTheLast" => json!(num.abs().min(1e6)),
                        "before" | "after" => json!("2020-02-29"),
                        _ => match f.kind {
                            Kind::Text => json!(text),
                            Kind::Num => json!(if num.is_finite() { num } else { 0.0 }),
                            Kind::Bool => json!(b),
                            Kind::Enum => json!(f.values[text.len() % f.values.len()]),
                            Kind::Date => json!(0),
                        },
                    };
                    json!({ op: { name: v } })
                })
        }

        fn tree() -> impl Strategy<Value = Json> {
            leaf().prop_recursive(4, 40, 5, |inner| {
                (proptest::collection::vec(inner, 0..5), any::<bool>()).prop_map(|(v, all)| {
                    if all {
                        json!({ "all": v })
                    } else {
                        json!({ "any": v })
                    }
                })
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(300))]
            /// Any valid rule compiles to SQL whose text holds no value — only `?` — and runs;
            /// a hostile string as a field, operator, sort key or group key is refused.
            #[test]
            fn values_never_reach_the_sql_text(t in tree(), hostile in "[^a-z]{1,40}") {
                let root = match t {
                    Json::Object(o) if o.contains_key("all") || o.contains_key("any") => Json::Object(o),
                    leaf => json!({ "all": [leaf] }),
                };
                let Ok(rules) = Rules::parse(&root.to_string()) else { return Ok(()) };
                let (c, order) = compile(&rules, 1_800_000_000_000);
                prop_assert_eq!(c.sql.matches('?').count(), c.params.len());
                prop_assert!(!order.contains('?'));
                for p in &c.params {
                    if let Value::Text(s) = p {
                        let core = s.trim_matches('%');
                        let plain = core.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '$');
                        if core.chars().count() > 3 && !plain {
                            prop_assert!(!c.sql.contains(core), "value in SQL: {}", core);
                        }
                    }
                }
                let conn = lib("prop");
                prop_assert!(evaluate(&conn, &rules, 0).is_ok());
                for bad in [
                    json!({ "all": [{ "is": { hostile.clone(): "x" } }] }),
                    json!({ "all": [{ hostile.clone(): { "title": "x" } }] }),
                    json!({ "all": [], "sort": hostile.clone() }),
                    json!({ hostile.clone(): [] }),
                ] {
                    // `hostile` has no lowercase Latin letter, so it is never a known name.
                    let accepted = Rules::parse(&bad.to_string()).is_ok();
                    let blank_sort = hostile.split(',').all(|p| p.trim().is_empty());
                    prop_assert!(!accepted || blank_sort, "accepted {}", bad);
                }
            }
        }
    }

    /// `cargo test --release -p sirtone-library smart_50k -- --ignored --nocapture`: evaluation
    /// and count on 50 000 tracks within the 100 ms rule (DESIGN.md §1).
    #[test]
    #[ignore = "timing, release build"]
    fn smart_50k() {
        let mut c = lib("perf");
        let tx = c.transaction().unwrap();
        for n in 0..50_000i64 {
            let title = format!("Песня номер {n}");
            tx.execute(
                "INSERT INTO track(uid, rel_path, size_bytes, mtime_ms, content_hash, container, codec,
                   lossless, duration_ms, title, title_norm, year, identity_state, added_at,
                   play_count, last_played_at, genre_main)
                 VALUES (?1, ?2, 4000000, 0, ?1, 'mp3', 'mp3', 0, 200000, ?3, ?4, ?5, 'inferred', ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    format!("u{n}"),
                    format!("A/{n}.mp3"),
                    title,
                    norm(&title),
                    1960 + n % 66,
                    n * 1000,
                    n % 30,
                    if n % 3 == 0 { Some(n * 7) } else { None },
                    ["Rock", "Pop", "Jazz"][(n % 3) as usize],
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let r = Rules::parse(
            r#"{"all":[{"gt":{"year":1990}},{"isNot":{"genre":"jazz"}},
                {"any":[{"contains":{"title":"номер 1"}},{"gt":{"play_count":20}}]}],
               "sort":"-play_count,title","limit":200}"#,
        )
        .unwrap();
        let t = std::time::Instant::now();
        let rows = evaluate(&c, &r, 1_800_000_000_000).unwrap();
        let eval_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = std::time::Instant::now();
        let n = count(&c, &r, 1_800_000_000_000).unwrap();
        let count_ms = t.elapsed().as_secs_f64() * 1e3;
        // No limit: the sidebar aggregates, the page builds every row.
        let open = Rules::parse(
            r#"{"all":[{"gt":{"year":1990}},{"isNot":{"genre":"jazz"}},
                {"any":[{"contains":{"title":"номер 1"}},{"gt":{"play_count":20}}]}],
               "sort":"-play_count,title"}"#,
        )
        .unwrap();
        let t = std::time::Instant::now();
        let st = stats(&c, &open, 1_800_000_000_000).unwrap();
        let stats_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = std::time::Instant::now();
        let all = evaluate(&c, &open, 1_800_000_000_000).unwrap();
        let page_ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!(
            "smart 50k: evaluate {eval_ms:.1} ms ({} rows), count {count_ms:.1} ms ({n}); \
             unlimited: stats {stats_ms:.1} ms ({} tracks), page {page_ms:.1} ms ({} rows)",
            rows.len(),
            st.tracks,
            all.len()
        );
        assert_eq!(st.tracks as usize, all.len());
        assert!(eval_ms < 100.0 && count_ms < 100.0 && stats_ms < 100.0 && page_ms < 100.0);
    }

    #[test]
    fn date_parsing() {
        assert_eq!(date_ms("1970-01-02"), Some(DAY_MS));
        assert_eq!(date_ms("2026-10-18"), Some(20_744 * DAY_MS));
        assert_eq!(date_ms("2026-13-01"), None);
    }
}
