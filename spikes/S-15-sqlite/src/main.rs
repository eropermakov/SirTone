//! S-15: SQLite at 100 000 tracks (docs/09 §4, P-08 page ≤ 10 ms, P-09 search p95 ≤ 80 ms).
//! Schema follows docs/03 §7 (track with its wide analysis blobs, artist, album, track_artist,
//! contentless FTS5 `search_fts` + trigram `search_tri`); search follows docs/05 §11.
//!
//! Usage: `cargo run --release -- [tracks=100000] [db path]`

use std::collections::HashMap;
use std::time::Instant;

use rusqlite::{Connection, params};

/// xorshift64* — deterministic synthetic data.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len() as u64) as usize]
    }
}

const RU: &[&str] = &[
    "любовь", "ночь", "город", "звезда", "дорога", "небо", "ветер", "море", "огонь", "зима", "лето",
    "сердце", "песня", "время", "мечта", "свет", "тень", "дождь", "солнце", "весна", "осень",
    "полковник", "кукушка", "группа", "крови", "танцы", "минус", "сплин", "вендетта", "искала",
];
const EN: &[&str] = &[
    "love", "night", "city", "star", "road", "sky", "wind", "sea", "fire", "winter", "summer",
    "heart", "song", "time", "dream", "light", "shadow", "rain", "sun", "spring", "black", "back",
    "problems", "highway", "thunder", "bohemian", "rhapsody", "yesterday", "imagine", "wonderwall",
];
const GENRES: &[&str] = &["rock", "pop", "electronic", "hip-hop", "jazz", "classical", "indie", "metal", "folk", "ambient"];

fn words(r: &mut Rng, n: usize) -> String {
    let dict = if r.below(2) == 0 { RU } else { EN };
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        let w = r.pick(dict);
        if i == 0 {
            let mut c = w.chars();
            if let Some(f) = c.next() {
                s.extend(f.to_uppercase());
                s.push_str(c.as_str());
            }
        } else {
            s.push_str(w);
        }
    }
    s
}

fn norm(s: &str) -> String {
    s.to_lowercase().replace('ё', "е")
}

fn schema(db: &Connection) {
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
         PRAGMA temp_store=MEMORY; PRAGMA mmap_size=268435456; PRAGMA cache_size=-65536;
         CREATE TABLE artist (id INTEGER PRIMARY KEY, name TEXT NOT NULL, name_norm TEXT NOT NULL, sort_name TEXT NOT NULL);
         CREATE INDEX ix_artist_sort ON artist(sort_name, id);
         CREATE TABLE album (id INTEGER PRIMARY KEY, title TEXT NOT NULL, title_norm TEXT NOT NULL,
           album_artist_id INTEGER NOT NULL REFERENCES artist(id), year INTEGER, created_at INTEGER NOT NULL);
         CREATE INDEX ix_album_artist ON album(album_artist_id, title_norm);
         CREATE INDEX ix_album_title ON album(title_norm, id);
         CREATE TABLE track (
           id INTEGER PRIMARY KEY, rel_path TEXT NOT NULL UNIQUE, size_bytes INTEGER NOT NULL, mtime_ms INTEGER NOT NULL,
           content_hash TEXT NOT NULL, container TEXT NOT NULL, codec TEXT NOT NULL, lossless INTEGER NOT NULL,
           sample_rate INTEGER, bit_depth INTEGER, channels INTEGER, bitrate_kbps INTEGER, duration_ms INTEGER NOT NULL,
           title TEXT NOT NULL, title_norm TEXT NOT NULL, album_id INTEGER REFERENCES album(id), disc_no INTEGER,
           track_no INTEGER, year INTEGER, genre_main TEXT, identity_state TEXT NOT NULL, identity_conf REAL NOT NULL,
           lufs_i REAL, true_peak_db REAL, peaks BLOB, env_head BLOB, env_tail BLOB, bpm REAL, key_camelot TEXT,
           valence REAL, arousal REAL, energy REAL, danceability REAL, embedding BLOB, fingerprint BLOB,
           play_count INTEGER NOT NULL DEFAULT 0, last_played_at INTEGER, added_at INTEGER NOT NULL,
           state TEXT NOT NULL DEFAULT 'ok');
         CREATE INDEX ix_track_album ON track(album_id, disc_no, track_no);
         CREATE INDEX ix_track_added ON track(added_at DESC, id);
         CREATE INDEX ix_track_title ON track(title_norm, id);
         CREATE INDEX ix_track_identity ON track(identity_state);
         CREATE TABLE track_artist (track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
           artist_id INTEGER NOT NULL REFERENCES artist(id), role TEXT NOT NULL, position INTEGER NOT NULL,
           PRIMARY KEY (track_id, artist_id, role)) WITHOUT ROWID;
         CREATE INDEX ix_ta_artist ON track_artist(artist_id);
         CREATE VIRTUAL TABLE search_fts USING fts5(title, artists, album, genre, aliases,
           content='', tokenize='unicode61 remove_diacritics 2');
         CREATE VIRTUAL TABLE search_tri USING fts5(text, content='', tokenize='trigram');",
    )
    .expect("schema");
}

struct Lib {
    titles: Vec<String>,
    artists: Vec<String>,
}

fn import(db: &mut Connection, n: usize) -> Lib {
    let mut r = Rng(0x5eed_1234_abcd_0001);
    let n_artists = (n / 10).max(10);
    let n_albums = (n * 3 / 20).max(10);
    let tx = db.transaction().expect("tx");
    let mut artists = Vec::with_capacity(n_artists);
    {
        let mut st = tx.prepare("INSERT INTO artist(id,name,name_norm,sort_name) VALUES (?,?,?,?)").expect("p");
        for id in 1..=n_artists {
            let name = format!("{} {}", { let k = 1 + r.below(2) as usize; words(&mut r, k) }, id);
            st.execute(params![id as i64, name, norm(&name), norm(&name)]).expect("artist");
            artists.push(name);
        }
        let mut st = tx
            .prepare("INSERT INTO album(id,title,title_norm,album_artist_id,year,created_at) VALUES (?,?,?,?,?,0)")
            .expect("p");
        for id in 1..=n_albums {
            let t = { let k = 1 + r.below(3) as usize; words(&mut r, k) };
            let a = 1 + (r.below(n_artists as u64 * 7) as usize % n_artists);
            st.execute(params![id as i64, t, norm(&t), a as i64, 1960 + r.below(65) as i64]).expect("album");
        }
    }
    let mut titles = Vec::with_capacity(n);
    {
        let mut st = tx
            .prepare(
                "INSERT INTO track(id,rel_path,size_bytes,mtime_ms,content_hash,container,codec,lossless,sample_rate,
                 bit_depth,channels,bitrate_kbps,duration_ms,title,title_norm,album_id,disc_no,track_no,year,genre_main,
                 identity_state,identity_conf,lufs_i,true_peak_db,peaks,env_head,env_tail,bpm,key_camelot,valence,arousal,
                 energy,danceability,embedding,fingerprint,play_count,added_at)
                 VALUES (?1,?2,40000000,0,?3,'flac','flac',1,44100,16,2,900,?4,?5,?6,?7,1,?8,2000,?9,'confirmed',0.97,
                 -9.5,-0.8,?10,?11,?11,?12,'8A',0.5,0.5,0.6,0.5,?13,?14,?15,?16)",
            )
            .expect("p");
        let mut ta = tx.prepare("INSERT INTO track_artist VALUES (?,?,?,?)").expect("p");
        let mut fts = tx.prepare("INSERT INTO search_fts(rowid,title,artists,album,genre,aliases) VALUES (?,?,?,?,?,'')").expect("p");
        let mut tri = tx.prepare("INSERT INTO search_tri(rowid,text) VALUES (?,?)").expect("p");
        let peaks = vec![128u8; 400];
        let env = vec![60u8; 300];
        let emb = vec![1u8; 68];
        let fp = vec![7u8; 900];
        let mut album_title: HashMap<i64, String> = HashMap::new();
        {
            let mut q = tx.prepare("SELECT id, title FROM album").expect("p");
            for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).expect("q") {
                let (id, t) = row.expect("row");
                album_title.insert(id, t);
            }
        }
        for id in 1..=n {
            let t = { let k = 1 + r.below(4) as usize; words(&mut r, k) };
            let album = 1 + r.below(n_albums as u64) as i64;
            let a1 = 1 + (r.below(n_artists as u64 * 7) as usize % n_artists);
            let genre = r.pick(GENRES);
            st.execute(params![
                id as i64,
                format!("Artist{a1}/Album{album}/{id:06} - {t}.flac"),
                format!("{:064x}", r.next()),
                120_000 + r.below(240_000) as i64,
                t,
                norm(&t),
                album,
                (id % 14 + 1) as i64,
                genre,
                peaks,
                env,
                80.0 + r.below(100) as f64,
                emb,
                fp,
                r.below(50) as i64,
                1_700_000_000_000i64 + id as i64 * 1000,
            ])
            .expect("track");
            ta.execute(params![id as i64, a1 as i64, "primary", 0]).expect("ta");
            let mut artist_text = artists[a1 - 1].clone();
            if r.below(8) == 0 {
                let a2 = 1 + r.below(n_artists as u64) as usize;
                if a2 != a1 {
                    ta.execute(params![id as i64, a2 as i64, "featured", 1]).expect("ta");
                    artist_text = format!("{artist_text} {}", artists[a2 - 1]);
                }
            }
            let alb = album_title.get(&album).cloned().unwrap_or_default();
            fts.execute(params![id as i64, t, artist_text, alb, genre]).expect("fts");
            tri.execute(params![id as i64, norm(&format!("{t} {artist_text} {alb}"))]).expect("tri");
            titles.push(t);
        }
    }
    tx.commit().expect("commit");
    db.execute_batch("ANALYZE; PRAGMA optimize;").expect("analyze");
    Lib { titles, artists }
}

fn pct(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn timed(f: &mut dyn FnMut() -> usize) -> (f64, usize) {
    let t = Instant::now();
    let n = f();
    (t.elapsed().as_secs_f64() * 1000.0, n)
}

/// Page of 100 rows with everything a TrackRow needs (title, artists, album, duration).
const PAGE_SQL: &str = "
  SELECT t.id, t.title, t.title_norm, t.duration_ms, al.title,
         (SELECT group_concat(a.name, ', ') FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
          WHERE ta.track_id = t.id ORDER BY ta.position)
  FROM track t LEFT JOIN album al ON al.id = t.album_id
  WHERE (t.title_norm, t.id) > (?1, ?2) ORDER BY t.title_norm, t.id LIMIT 100";

fn bench_pages(db: &Connection) {
    let mut by_title = db.prepare_cached(PAGE_SQL).expect("p");
    let mut added = db
        .prepare_cached(
            "SELECT id, title, duration_ms, added_at FROM track WHERE (added_at, id) < (?1, ?2)
             ORDER BY added_at DESC, id DESC LIMIT 100",
        )
        .expect("p");
    let mut albums = db
        .prepare_cached(
            "SELECT al.id, al.title, a.name, al.year FROM album al JOIN artist a ON a.id = al.album_artist_id
             WHERE (al.title_norm, al.id) > (?1, ?2) ORDER BY al.title_norm, al.id LIMIT 100",
        )
        .expect("p");
    let mut artist_tracks = db
        .prepare_cached(
            "SELECT t.id, t.title, t.duration_ms FROM track_artist ta JOIN track t ON t.id = ta.track_id
             WHERE ta.artist_id = ?1 ORDER BY t.album_id, t.track_no LIMIT 100",
        )
        .expect("p");

    let mut results: Vec<(&str, Vec<f64>)> = vec![("tracks by title (keyset, join artists+album)", vec![]), ("tracks by added (keyset)", vec![]), ("albums by title (keyset, join artist)", vec![]), ("tracks of artist", vec![])];
    // Walk the whole library page by page by title (worst + typical), 1 000 pages.
    let (mut k, mut id) = (String::new(), 0i64);
    for _ in 0..1000 {
        let (ms, n) = timed(&mut || {
            let mut rows = by_title.query(params![k, id]).expect("q");
            let mut c = 0;
            while let Some(r) = rows.next().expect("row") {
                id = r.get(0).expect("id");
                k = r.get(2).expect("k");
                c += 1;
            }
            c
        });
        if n == 0 {
            k.clear();
            id = 0;
        }
        results[0].1.push(ms);
    }
    let (mut at, mut aid) = (i64::MAX, i64::MAX);
    for _ in 0..1000 {
        let (ms, n) = timed(&mut || {
            let mut rows = added.query(params![at, aid]).expect("q");
            let mut c = 0;
            while let Some(r) = rows.next().expect("row") {
                aid = r.get(0).expect("id");
                at = r.get(3).expect("at");
                c += 1;
            }
            c
        });
        if n == 0 {
            at = i64::MAX;
            aid = i64::MAX;
        }
        results[1].1.push(ms);
    }
    let (mut ak, mut alid) = (String::new(), 0i64);
    for _ in 0..1000 {
        let (ms, n) = timed(&mut || {
            let mut rows = albums.query(params![ak, alid]).expect("q");
            let mut c = 0;
            while let Some(r) = rows.next().expect("row") {
                alid = r.get(0).expect("id");
                let t: String = r.get(1).expect("t");
                ak = norm(&t);
                c += 1;
            }
            c
        });
        if n == 0 {
            ak.clear();
            alid = 0;
        }
        results[2].1.push(ms);
    }
    for a in (1..=1000).map(|i| i * 7 % 10_000 + 1) {
        let (ms, _) = timed(&mut || artist_tracks.query_map(params![a], |_| Ok(())).expect("q").count());
        results[3].1.push(ms);
    }
    println!("\n| P-08 page of 100 rows | p50 ms | p95 ms | max ms |\n|---|---|---|---|");
    for (name, v) in results {
        println!("| {name} | {:.2} | {:.2} | {:.2} |", pct(v.clone(), 0.5), pct(v.clone(), 0.95), pct(v, 1.0));
    }
}

fn dl(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// docs/05 §11: FTS5 prefix with bm25 weights → if < 5 hits, trigram OR-match → DL ≤ 2 re-rank on
/// the top 200. Returns hit count.
fn search(db: &Connection, q: &str) -> usize {
    let tokens: Vec<String> = norm(q).split_whitespace().map(|t| format!("\"{}\"*", t.replace('"', ""))).collect();
    let mut hits: Vec<(i64, f64)> = Vec::new();
    if !tokens.is_empty() {
        let mut st = db
            .prepare_cached(
                "SELECT rowid, bm25(search_fts, 4.0, 5.0, 2.0, 1.0, 1.0) FROM search_fts WHERE search_fts MATCH ?1
                 ORDER BY bm25(search_fts, 4.0, 5.0, 2.0, 1.0, 1.0) LIMIT 200",
            )
            .expect("p");
        hits = st
            .query_map(params![tokens.join(" ")], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("q")
            .filter_map(Result::ok)
            .collect();
    }
    if hits.len() < 5 {
        // Typo tolerance: any trigram of the query, ranked by bm25 (≈ shared trigrams).
        let qn = norm(q);
        let chars: Vec<char> = qn.chars().collect();
        let mut grams: Vec<String> = chars.windows(3).map(|w| w.iter().collect::<String>()).filter(|g: &String| !g.contains(' ')).collect();
        grams.sort();
        grams.dedup();
        if !grams.is_empty() {
            let expr = grams.iter().map(|g| format!("\"{g}\"")).collect::<Vec<_>>().join(" OR ");
            let mut st = db
                .prepare_cached("SELECT rowid FROM search_tri WHERE search_tri MATCH ?1 ORDER BY rank LIMIT 200")
                .expect("p");
            let ids: Vec<i64> = st.query_map(params![expr], |r| r.get(0)).expect("q").filter_map(Result::ok).collect();
            // Re-rank by Damerau–Levenshtein of the best-matching title word (tokens ≥ 5 chars).
            let mut get = db.prepare_cached("SELECT title_norm FROM track WHERE id = ?1").expect("p");
            let qtok: Vec<&str> = qn.split_whitespace().filter(|t| t.chars().count() >= 5).collect();
            let mut scored: Vec<(i64, f64)> = ids
                .into_iter()
                .filter_map(|id| {
                    let t: String = get.query_row(params![id], |r| r.get(0)).ok()?;
                    let d = qtok
                        .iter()
                        .map(|q| t.split_whitespace().map(|w| dl(q, w)).min().unwrap_or(9))
                        .min()
                        .unwrap_or(0);
                    (d <= 2).then_some((id, d as f64))
                })
                .collect();
            scored.sort_by(|a, b| a.1.total_cmp(&b.1));
            hits.extend(scored);
        }
    }
    // Hydrate the first 50 rows like the UI would.
    let mut st = db.prepare_cached("SELECT title, duration_ms FROM track WHERE id = ?1").expect("p");
    for (id, _) in hits.iter().take(50) {
        let _ = st.query_row(params![id], |r| r.get::<_, String>(0));
    }
    hits.len()
}

fn typo(s: &str, r: &mut Rng) -> String {
    let mut c: Vec<char> = s.chars().collect();
    if c.len() > 4 {
        let i = 1 + r.below(c.len() as u64 - 2) as usize;
        c.swap(i, i + 1);
    }
    c.into_iter().collect()
}

fn bench_search(db: &Connection, lib: &Lib) {
    let mut r = Rng(42);
    let mut kinds: Vec<(&str, Vec<String>)> = vec![("prefix (2–3 chars)", vec![]), ("full word", vec![]), ("title + artist", vec![]), ("typo (transposition)", vec![])];
    for _ in 0..100 {
        let t = &lib.titles[r.below(lib.titles.len() as u64) as usize];
        let w = t.split_whitespace().next().unwrap_or("x").to_string();
        kinds[0].1.push(w.chars().take(2 + r.below(2) as usize).collect());
        kinds[1].1.push(w.clone());
        let a = &lib.artists[r.below(lib.artists.len() as u64) as usize];
        kinds[2].1.push(format!("{w} {}", a.split_whitespace().next().unwrap_or("")));
        let long = t.split_whitespace().max_by_key(|w| w.chars().count()).unwrap_or("x");
        kinds[3].1.push(typo(long, &mut r));
    }
    println!("\n| P-09 search (hydrate 50) | p50 ms | p95 ms | max ms | median hits |\n|---|---|---|---|---|");
    let mut all = Vec::new();
    for (name, qs) in kinds {
        let mut times = Vec::new();
        let mut hits = Vec::new();
        for q in &qs {
            let (ms, n) = timed(&mut || search(db, q));
            times.push(ms);
            hits.push(n as f64);
        }
        all.extend(times.iter().copied());
        println!("| {name} | {:.2} | {:.2} | {:.2} | {} |", pct(times.clone(), 0.5), pct(times.clone(), 0.95), pct(times, 1.0), pct(hits, 0.5));
    }
    println!("| **all 400 queries** | {:.2} | **{:.2}** | {:.2} | |", pct(all.clone(), 0.5), pct(all.clone(), 0.95), pct(all, 1.0));
    let ex = "плоковник";
    println!("\nexample typo «{ex}» → {} hits", search(db, ex));
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let path = std::env::args().nth(2).unwrap_or_else(|| std::env::temp_dir().join("s15-library.db").to_string_lossy().into());
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{path}{ext}"));
    }
    let mut db = Connection::open(&path).expect("open");
    println!("SQLite {} · {n} tracks · {path}", rusqlite::version());
    schema(&db);
    let t = Instant::now();
    let lib = import(&mut db, n);
    let import_s = t.elapsed().as_secs_f64();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").expect("ckpt");
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("import: {import_s:.1} s ({:.0} tracks/s, one transaction, incl. FTS + trigram), db {:.1} MB", n as f64 / import_s, size as f64 / 1e6);
    drop(db);
    // Cold-ish reopen as the app would.
    let t = Instant::now();
    let db = Connection::open(&path).expect("open");
    db.execute_batch("PRAGMA mmap_size=268435456; PRAGMA cache_size=-65536; PRAGMA temp_store=MEMORY;").expect("pragmas");
    let count: i64 = db.query_row("SELECT count(*) FROM track", [], |r| r.get(0)).expect("count");
    println!("reopen + count(*) = {count}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    bench_pages(&db);
    bench_search(&db, &lib);
}
