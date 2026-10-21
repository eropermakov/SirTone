//! Fuzz on stable (T7.3, `08` §4: «недоверенные аудиофайлы»): seeds of every kind the app reads
//! from the outside are mutated (bit flips, truncation, splicing, random bytes) and each input
//! runs in its own child process through the app's own entry points — decode (symphonia, libopus,
//! Media Foundation), tags and embedded covers (lofty), cover images (image + the WebP cache),
//! settings JSON, smart-playlist rules, search queries. A panic — even one the app catches — is
//! listed (a hook in the child reports it); an abort, a stack overflow, a hang (> 10 s) or a
//! non-zero exit is a crash. Every failing input is kept. Inputs are generated into
//! `target/fuzz` only.
//!
//! sirtone-cli fuzz --minutes N [--out DIR]
//! sirtone-cli fuzz-one <kind> <file>          (the child)

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const KINDS: [&str; 6] = ["audio", "tags", "image", "settings", "rules", "search"];
const HANG: Duration = Duration::from_secs(10);

/// xorshift64* — enough randomness for mutation, no dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn ffmpeg(args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Seeds per kind: (kind, file extension, bytes).
fn seeds(dir: &Path) -> Vec<(&'static str, String, Vec<u8>)> {
    let _ = std::fs::create_dir_all(dir);
    let tone = ["-f", "lavfi", "-i", "sine=frequency=440:duration=1"];
    let cover = dir.join("cover.jpg");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=s=64x64",
        "-frames:v",
        "1",
        &cover.to_string_lossy(),
    ]);
    let png = dir.join("cover.png");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=s=48x48",
        "-frames:v",
        "1",
        &png.to_string_lossy(),
    ]);
    let audio: [(&str, &[&str]); 9] = [
        ("mp3", &["-c:a", "libmp3lame", "-b:a", "64k"]),
        ("flac", &["-c:a", "flac"]),
        ("wav", &["-c:a", "pcm_s16le"]),
        ("ogg", &["-c:a", "libvorbis"]),
        ("opus", &["-c:a", "libopus", "-b:a", "48k"]),
        ("m4a", &["-c:a", "aac", "-b:a", "64k"]),
        ("aiff", &["-c:a", "pcm_s16be"]),
        ("wv", &["-c:a", "wavpack"]),
        ("wma", &["-c:a", "wmav2", "-b:a", "64k"]),
    ];
    let mut out = Vec::new();
    for (ext, codec) in audio {
        let f = dir.join(format!("seed.{ext}"));
        let mut a: Vec<&str> = tone.to_vec();
        a.extend_from_slice(codec);
        a.extend_from_slice(&[
            "-metadata",
            "title=Seed",
            "-metadata",
            "artist=Fuzz",
            "-metadata",
            "album=Corpus",
        ]);
        let fs = f.to_string_lossy().to_string();
        a.push(&fs);
        if ffmpeg(&a)
            && let Ok(b) = std::fs::read(&f)
        {
            out.push(("audio", ext.to_owned(), b));
        }
        // The same with an embedded cover: the tag/picture path.
        if matches!(ext, "mp3" | "flac" | "m4a") {
            let t = dir.join(format!("tagged.{ext}"));
            let ts = t.to_string_lossy().to_string();
            let cs = cover.to_string_lossy().to_string();
            let mut a: Vec<&str> = tone.to_vec();
            a.extend_from_slice(&["-i", &cs, "-map", "0:a", "-map", "1:v"]);
            a.extend_from_slice(codec);
            a.extend_from_slice(&[
                "-c:v",
                "mjpeg",
                "-disposition:v",
                "attached_pic",
                "-metadata",
                "title=Tagged",
                &ts,
            ]);
            if ffmpeg(&a)
                && let Ok(b) = std::fs::read(&t)
            {
                out.push(("tags", ext.to_owned(), b));
            }
        }
    }
    for f in [&cover, &png] {
        if let Ok(b) = std::fs::read(f) {
            let ext = f
                .extension()
                .map_or("bin".into(), |e| e.to_string_lossy().into_owned());
            out.push(("image", ext, b));
        }
    }
    let settings =
        serde_json::to_string(&sirtone_core::settings::Settings::default()).unwrap_or_default();
    out.push(("settings", "json".into(), settings.into_bytes()));
    out.push(("settings", "json".into(), br#"{"playback":{"volume":0.5,"eq":{"bands_db":[1,2,3]}},"wave":{"mine":[{"id":"a","name":"x","query":{}}]}}"#.to_vec()));
    out.push(("rules", "json".into(), br#"{"match":"all","rules":[{"field":"plays","op":"gt","value":3},{"field":"title","op":"contains","value":"love"}],"limit":{"count":50,"by":"random"},"refresh":"live"}"#.to_vec()));
    for q in [
        "queen bohemian",
        "ЦЩЙ ghbdtn",
        "\"exact\" -no артист:Кино",
        "вечер дождь спокойное",
    ] {
        out.push(("search", "txt".into(), q.as_bytes().to_vec()));
    }
    out
}

fn mutate(rng: &mut Rng, seed: &[u8], other: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(6) {
            0 if !b.is_empty() => {
                // A few bit flips.
                for _ in 0..=rng.below(8) {
                    let i = rng.below(b.len());
                    b[i] ^= 1 << rng.below(8);
                }
            }
            1 if !b.is_empty() => b.truncate(rng.below(b.len())),
            2 if !b.is_empty() && !other.is_empty() => {
                // Splice: our head, their tail.
                let at = rng.below(b.len());
                let from = rng.below(other.len());
                b.truncate(at);
                b.extend_from_slice(&other[from..]);
            }
            3 => {
                // Random bytes in.
                let at = rng.below(b.len() + 1);
                let n = 1 + rng.below(64);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                b.splice(at..at, junk);
            }
            4 if b.len() > 8 => {
                // Interesting numbers over a 4-byte field (lengths, counts).
                let i = rng.below(b.len() - 4);
                let v: [u8; 4] =
                    [[0xFF; 4], [0; 4], [0x7F, 0xFF, 0xFF, 0xFF], [0x80, 0, 0, 0]][rng.below(4)];
                b[i..i + 4].copy_from_slice(&v);
            }
            _ => {
                let i = rng.below(b.len().max(1));
                if let Some(x) = b.get_mut(i) {
                    *x = rng.next() as u8;
                }
            }
        }
    }
    b
}

enum Outcome {
    Ok,
    Panic(String),
    Crash(String),
}

fn run_child(exe: &Path, kind: &str, file: &Path) -> Outcome {
    let Ok(mut child) = Command::new(exe)
        .args(["fuzz-one", kind, &file.to_string_lossy()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Outcome::Crash("spawn failed".into());
    };
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                let panic = err
                    .lines()
                    .find(|l| l.starts_with("FUZZ-PANIC"))
                    .map(str::to_owned);
                return match (status.code(), panic) {
                    (Some(0), None) => Outcome::Ok,
                    (Some(0 | 3), Some(p)) => Outcome::Panic(p),
                    (code, p) => Outcome::Crash(format!("exit {code:?} {}", p.unwrap_or_default())),
                };
            }
            Ok(None) if t0.elapsed() > HANG => {
                let _ = child.kill();
                return Outcome::Crash("hang > 10 s".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(e) => return Outcome::Crash(format!("wait: {e}")),
        }
    }
}

pub fn fuzz(args: &[String]) -> ExitCode {
    let get = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
    };
    let minutes: u64 = get("--minutes").and_then(|v| v.parse().ok()).unwrap_or(1);
    let out = PathBuf::from(get("--out").map_or("target/fuzz", String::as_str));
    let Ok(exe) = std::env::current_exe() else {
        return ExitCode::FAILURE;
    };
    let corpus = seeds(&out.join("seeds"));
    if corpus.is_empty() {
        eprintln!("fuzz: no seeds (ffmpeg missing?)");
        return ExitCode::FAILURE;
    }
    let work = out.join("work");
    let found = out.join("found");
    let _ = std::fs::create_dir_all(&work);
    let _ = std::fs::create_dir_all(&found);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ std::process::id() as u64);
    let end = Instant::now() + Duration::from_secs(minutes * 60);
    let (mut runs, mut panics, mut crashes) = (0u64, 0u64, 0u64);
    let mut per = std::collections::BTreeMap::<&str, u64>::new();
    let mut n = 0usize;
    while Instant::now() < end {
        let kind = KINDS[n % KINDS.len()];
        n += 1;
        let pool: Vec<&(&str, String, Vec<u8>)> = corpus.iter().filter(|s| s.0 == kind).collect();
        if pool.is_empty() {
            continue;
        }
        let seed = pool[rng.below(pool.len())];
        let other = pool[rng.below(pool.len())];
        let input = mutate(&mut rng, &seed.2, &other.2);
        let file = work.join(format!("cur.{}", seed.1));
        if std::fs::write(&file, &input).is_err() {
            continue;
        }
        runs += 1;
        *per.entry(kind).or_default() += 1;
        let keep = |what: &str, why: &str| {
            let name = format!("{what}-{kind}-{runs}.{}", seed.1);
            let _ = std::fs::write(found.join(&name), &input);
            println!("{what:5} {kind:8} {name}  {why}");
        };
        match run_child(&exe, kind, &file) {
            Outcome::Ok => {}
            Outcome::Panic(p) => {
                panics += 1;
                keep("panic", &p);
            }
            Outcome::Crash(c) => {
                crashes += 1;
                keep("crash", &c);
            }
        }
    }
    println!("fuzz: {runs} inputs in {minutes} min — panics {panics}, crashes {crashes}");
    for (k, v) in per {
        println!("  {k:8} {v}");
    }
    if crashes == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The child: one input through the app's own code. A panic is reported even when caught.
pub fn fuzz_one(args: &[String]) -> ExitCode {
    let (Some(kind), Some(file)) = (args.first(), args.get(1)) else {
        return ExitCode::FAILURE;
    };
    std::panic::set_hook(Box::new(|info| {
        eprintln!("FUZZ-PANIC {info}");
    }));
    let path = PathBuf::from(file);
    let kind = kind.clone();
    let r = std::panic::catch_unwind(move || one(&kind, &path));
    if r.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}

fn one(kind: &str, path: &Path) {
    use sirtone_core::ports::{CoverReader, TagReader};
    match kind {
        "audio" => {
            if let Ok(mut s) = sirtone_audio::decode::Source::open(path) {
                let mut buf = Vec::new();
                let mut total = 0usize;
                while total < 4_000_000 {
                    buf.clear();
                    match s.next_into(&mut buf) {
                        Ok(true) => total += buf.len().max(1),
                        _ => break,
                    }
                }
                let _ = s.seek(300);
                buf.clear();
                let _ = s.next_into(&mut buf);
            }
        }
        "tags" => {
            let _ = sirtone_recognize::tags::LoftyTags.read(path);
            if let Some((bytes, src)) = sirtone_recognize::covers::LoftyCovers.read(path, None) {
                cover(&bytes, src);
            }
        }
        "image" => {
            if let Ok(b) = std::fs::read(path) {
                cover(&b, sirtone_core::ports::CoverSource::Embedded);
            }
        }
        "settings" => {
            if let Ok(t) = std::fs::read_to_string(path)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
            {
                let base = serde_json::to_value(sirtone_core::settings::Settings::default())
                    .unwrap_or_default();
                let _ = sirtone_core::settings::normalize(&v);
                let _ = sirtone_core::settings::apply_patch(&base, &v);
            }
        }
        "rules" => {
            if let Ok(t) = std::fs::read_to_string(path) {
                let _ = sirtone_library::smart::Rules::parse(&t);
            }
        }
        "search" => {
            if let Ok(b) = std::fs::read(path) {
                let t = String::from_utf8_lossy(&b);
                let _ = sirtone_library::search_text::forms(&t);
                let _ = sirtone_library::search_text::fts_query(&t, true);
                if let Ok(c) = sirtone_library::open_in_memory() {
                    let _ = sirtone_library::search::search(&c, &t);
                }
            }
        }
        _ => {}
    }
}

/// A cover through the app's cache: stored (dimensions read), then a thumbnail (WebP).
fn cover(bytes: &[u8], src: sirtone_core::ports::CoverSource) {
    let root = std::env::temp_dir().join(format!("sirtone-fuzz-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    if let Ok(c) = sirtone_library::open_in_memory()
        && let Ok(id) = sirtone_library::covers::store(&c, &root, bytes, src)
        && let Ok(hash) = c.query_row("SELECT hash FROM cover WHERE id = ?1", [id], |r| {
            r.get::<_, String>(0)
        })
    {
        let _ = sirtone_library::covers::variant(&root, &hash, 64);
    }
    let _ = std::fs::remove_dir_all(&root); // our own temp folder
}
