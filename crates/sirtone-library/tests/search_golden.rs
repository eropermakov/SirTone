//! Search golden set (T3.15, `05` §13: «200 запросов (опечатки, раскладка, транслит, смысловые)
//! — top-3 ≥ 95 %»). A library of real-world-like RU/EN artists and titles; queries written by
//! hand in eight kinds: exact, prefix, typo, wrong keyboard layout, transliteration, word form,
//! artist + title, artist. A query hits when its target is among the first three results
//! (best + tracks, or best + artists for an artist). «Смысловые» queries need B3 and T3.16.

use std::collections::HashMap;

use sirtone_library::db::open;
use sirtone_library::repo::{Identity, NewTrack, artist_get_or_create, track_insert};
use sirtone_library::search::{Best, search};

const LIBRARY: &[(&str, &[&str])] = &[
    (
        "Кино",
        &[
            "Группа крови",
            "Звезда по имени Солнце",
            "Кукушка",
            "Пачка сигарет",
            "Восьмиклассница",
            "Перемен",
        ],
    ),
    (
        "Би-2",
        &[
            "Полковнику никто не пишет",
            "Варвара",
            "Медленная звезда",
            "Серебро",
            "Молитва",
        ],
    ),
    (
        "Земфира",
        &[
            "Искала",
            "Хочешь",
            "Ариведерчи",
            "Небо Лондона",
            "Бесконечность",
        ],
    ),
    (
        "Сплин",
        &[
            "Выхода нет",
            "Романс",
            "Орбит без сахара",
            "Моё сердце",
            "Мы сидели и курили",
        ],
    ),
    (
        "Аквариум",
        &[
            "Город золотой",
            "Поезд в огне",
            "Саргон",
            "Под небом голубым",
        ],
    ),
    (
        "Наутилус Помпилиус",
        &[
            "Крылья",
            "Я хочу быть с тобой",
            "Прогулки по воде",
            "Скованные одной цепью",
        ],
    ),
    ("ДДТ", &["Что такое осень", "Метель", "Это всё", "Родина"]),
    (
        "Мумий Тролль",
        &["Владивосток 2000", "Утекай", "Невеста", "Дельфины"],
    ),
    ("Ленинград", &["Экспонат", "В Питере пить", "Лабутены"]),
    (
        "Звери",
        &[
            "Районы-кварталы",
            "Всё, что тебя касается",
            "Напитки покрепче",
        ],
    ),
    ("Ария", &["Беспечный ангел", "Штиль", "Улица роз"]),
    ("Чайф", &["Аргентина-Ямайка 5:0", "Ой-йо", "Не спеши"]),
    (
        "Король и Шут",
        &["Лесник", "Кукла колдуна", "Ели мясо мужики"],
    ),
    (
        "Агата Кристи",
        &["Как на войне", "Опиум для никого", "Сказочная тайга"],
    ),
    (
        "Машина времени",
        &["Поворот", "Марионетки", "За тех, кто в море"],
    ),
    ("Ночные снайперы", &["31-я весна", "Катастрофически"]),
    ("Каста", &["Вокруг шум", "Ревность", "Радиосигналы"]),
    ("Монеточка", &["Каждый раз", "Последняя дискотека"]),
    (
        "The Beatles",
        &["Yesterday", "Let It Be", "Help!", "Something", "Hey Jude"],
    ),
    (
        "Queen",
        &[
            "Bohemian Rhapsody",
            "We Will Rock You",
            "Under Pressure",
            "Somebody to Love",
        ],
    ),
    (
        "Radiohead",
        &["Creep", "Karma Police", "No Surprises", "Paranoid Android"],
    ),
    (
        "Nirvana",
        &["Smells Like Teen Spirit", "Come as You Are", "Lithium"],
    ),
    (
        "Pink Floyd",
        &["Wish You Were Here", "Comfortably Numb", "Time", "Money"],
    ),
    (
        "Metallica",
        &["Nothing Else Matters", "Enter Sandman", "One"],
    ),
    (
        "Daft Punk",
        &["Get Lucky", "One More Time", "Around the World"],
    ),
    (
        "Coldplay",
        &["Yellow", "The Scientist", "Fix You", "Clocks"],
    ),
    ("Muse", &["Hysteria", "Starlight", "Uprising"]),
    (
        "Depeche Mode",
        &["Enjoy the Silence", "Personal Jesus", "Precious"],
    ),
    (
        "Rammstein",
        &["Sonne", "Du hast", "Engel", "Mein Herz brennt"],
    ),
    ("ABBA", &["Dancing Queen", "Mamma Mia", "Waterloo"]),
    ("Linkin Park", &["Numb", "In the End", "Faint"]),
    ("Arctic Monkeys", &["Do I Wanna Know?", "R U Mine?", "505"]),
];

/// (query, target): `T:<title>` — a track, `A:<artist>` — an artist.
const QUERIES: &[(&str, &str)] = &[
    // exact (lower case, ё/е)
    ("группа крови", "T:Группа крови"),
    ("кукушка", "T:Кукушка"),
    ("пачка сигарет", "T:Пачка сигарет"),
    ("варвара", "T:Варвара"),
    ("медленная звезда", "T:Медленная звезда"),
    ("искала", "T:Искала"),
    ("небо лондона", "T:Небо Лондона"),
    ("мое сердце", "T:Моё сердце"),
    ("город золотой", "T:Город золотой"),
    ("крылья", "T:Крылья"),
    ("что такое осень", "T:Что такое осень"),
    ("это все", "T:Это всё"),
    ("утекай", "T:Утекай"),
    ("лабутены", "T:Лабутены"),
    ("улица роз", "T:Улица роз"),
    ("лесник", "T:Лесник"),
    ("поворот", "T:Поворот"),
    ("каждый раз", "T:Каждый раз"),
    ("yesterday", "T:Yesterday"),
    ("bohemian rhapsody", "T:Bohemian Rhapsody"),
    ("creep", "T:Creep"),
    ("lithium", "T:Lithium"),
    ("money", "T:Money"),
    ("nothing else matters", "T:Nothing Else Matters"),
    ("one more time", "T:One More Time"),
    ("the scientist", "T:The Scientist"),
    ("enjoy the silence", "T:Enjoy the Silence"),
    ("du hast", "T:Du hast"),
    ("dancing queen", "T:Dancing Queen"),
    ("in the end", "T:In the End"),
    // prefix (typed so far)
    ("звез", "T:Звезда по имени Солнце"),
    ("полков", "T:Полковнику никто не пишет"),
    ("аривед", "T:Ариведерчи"),
    ("небо лон", "T:Небо Лондона"),
    ("выхода", "T:Выхода нет"),
    ("город зол", "T:Город золотой"),
    ("скован", "T:Скованные одной цепью"),
    ("владив", "T:Владивосток 2000"),
    ("лабут", "T:Лабутены"),
    ("беспеч", "T:Беспечный ангел"),
    ("аргентина", "T:Аргентина-Ямайка 5:0"),
    ("опиум", "T:Опиум для никого"),
    ("марион", "T:Марионетки"),
    ("катастроф", "T:Катастрофически"),
    ("радиосиг", "T:Радиосигналы"),
    ("последняя диск", "T:Последняя дискотека"),
    ("yester", "T:Yesterday"),
    ("bohem", "T:Bohemian Rhapsody"),
    ("karma", "T:Karma Police"),
    ("smells", "T:Smells Like Teen Spirit"),
    ("wish you", "T:Wish You Were Here"),
    ("nothing els", "T:Nothing Else Matters"),
    ("get luc", "T:Get Lucky"),
    ("enjoy the", "T:Enjoy the Silence"),
    ("dancing q", "T:Dancing Queen"),
    ("paranoid", "T:Paranoid Android"),
    // typos
    ("звезда по имни солнце", "T:Звезда по имени Солнце"),
    ("кукушко", "T:Кукушка"),
    ("полковнку никто не пишет", "T:Полковнику никто не пишет"),
    ("аривидерчи", "T:Ариведерчи"),
    ("бесконечнсть", "T:Бесконечность"),
    ("восьмикласница", "T:Восьмиклассница"),
    ("прогулки по водe", "T:Прогулки по воде"),
    ("скованые одной цепью", "T:Скованные одной цепью"),
    ("владивасток", "T:Владивосток 2000"),
    ("дельфыны", "T:Дельфины"),
    ("экспанат", "T:Экспонат"),
    ("беспечныи ангел", "T:Беспечный ангел"),
    ("марианетки", "T:Марионетки"),
    ("катострофически", "T:Катастрофически"),
    ("радиосегналы", "T:Радиосигналы"),
    ("yesturday", "T:Yesterday"),
    ("bohemian rhapsdy", "T:Bohemian Rhapsody"),
    ("karma polce", "T:Karma Police"),
    ("smels like teen spirit", "T:Smells Like Teen Spirit"),
    ("wish you where here", "T:Wish You Were Here"),
    ("nothing else maters", "T:Nothing Else Matters"),
    ("enter sandmen", "T:Enter Sandman"),
    ("get lukcy", "T:Get Lucky"),
    ("the scientst", "T:The Scientist"),
    ("hysteira", "T:Hysteria"),
    ("enjoy the silense", "T:Enjoy the Silence"),
    ("personal jsus", "T:Personal Jesus"),
    ("dancing quen", "T:Dancing Queen"),
    ("paranoid andriod", "T:Paranoid Android"),
    ("comfortabley numb", "T:Comfortably Numb"),
    ("starlite", "T:Starlight"),
    ("upriseing", "T:Uprising"),
    // wrong keyboard layout
    ("rereirf", "T:Кукушка"),
    ("uheggf rhjdb", "T:Группа крови"),
    ("gthtvty", "T:Перемен"),
    ("dfhdfhf", "T:Варвара"),
    ("bcrfkf", "T:Искала"),
    ("[jxtim", "T:Хочешь"),
    ("yt,j kjyljyf", "T:Небо Лондона"),
    ("hjvfyc", "T:Романс"),
    ("ujhjl pjkjnjq", "T:Город золотой"),
    ("gjtpl d juyt", "T:Поезд в огне"),
    ("rhskmz", "T:Крылья"),
    ("vtntkm", "T:Метель"),
    ("entrfq", "T:Утекай"),
    ("ytdtcnf", "T:Невеста"),
    ("kf,entys", "T:Лабутены"),
    ("inbkm", "T:Штиль"),
    ("ktcybr", "T:Лесник"),
    ("gjdjhjn", "T:Поворот"),
    ("rbyj", "A:Кино"),
    ("ptvabhf", "A:Земфира"),
    ("cgkby", "A:Сплин"),
    ("frdfhbev", "A:Аквариум"),
    ("нуыеуквфн", "T:Yesterday"),
    ("дуе ше иу", "T:Let It Be"),
    ("скууз", "T:Creep"),
    ("лфкьф зщдшсу", "T:Karma Police"),
    ("дшершгь", "T:Lithium"),
    ("тщерштп удыу ьфееукы", "T:Nothing Else Matters"),
    ("пуе дгслн", "T:Get Lucky"),
    ("нуддщц", "T:Yellow"),
    ("рныеукшф", "T:Hysteria"),
    ("ыщтту", "T:Sonne"),
    ("цфеукдщщ", "T:Waterloo"),
    ("йгуут", "A:Queen"),
    ("сщдвздфн", "A:Coldplay"),
    ("кфььыеушт", "A:Rammstein"),
    // transliteration
    ("kino", "A:Кино"),
    ("zemfira", "A:Земфира"),
    ("bi-2", "A:Би-2"),
    ("bi 2", "A:Би-2"),
    ("splin", "A:Сплин"),
    ("akvarium", "A:Аквариум"),
    ("nautilus pompilius", "A:Наутилус Помпилиус"),
    ("ddt", "A:ДДТ"),
    ("mumiy troll", "A:Мумий Тролль"),
    ("leningrad", "A:Ленинград"),
    ("zveri", "A:Звери"),
    ("aria", "A:Ария"),
    ("korol i shut", "A:Король и Шут"),
    ("agata kristi", "A:Агата Кристи"),
    ("mashina vremeni", "A:Машина времени"),
    ("kasta", "A:Каста"),
    ("monetochka", "A:Монеточка"),
    ("gruppa krovi", "T:Группа крови"),
    ("kukushka", "T:Кукушка"),
    ("zvezda po imeni solntse", "T:Звезда по имени Солнце"),
    ("vladivostok 2000", "T:Владивосток 2000"),
    ("utekai", "T:Утекай"),
    ("eksponat", "T:Экспонат"),
    ("labuteny", "T:Лабутены"),
    ("chto takoe osen", "T:Что такое осень"),
    ("gorod zolotoy", "T:Город золотой"),
    ("poezd v ogne", "T:Поезд в огне"),
    ("varvara", "T:Варвара"),
    // word forms (morphology)
    ("кукушку", "T:Кукушка"),
    ("звезду по имени солнце", "T:Звезда по имени Солнце"),
    ("группы крови", "T:Группа крови"),
    ("крыло", "T:Крылья"),
    ("поезда в огне", "T:Поезд в огне"),
    ("метелью", "T:Метель"),
    ("невесту", "T:Невеста"),
    ("дельфинов", "T:Дельфины"),
    ("марионеток", "T:Марионетки"),
    ("лесника", "T:Лесник"),
    ("куклы колдуна", "T:Кукла колдуна"),
    ("прогулка по воде", "T:Прогулки по воде"),
    ("улицы роз", "T:Улица роз"),
    ("городом золотым", "T:Город золотой"),
    ("молитвы", "T:Молитва"),
    ("родине", "T:Родина"),
    ("clock", "T:Clocks"),
    ("angels", "T:Engel"),
    ("dancing queens", "T:Dancing Queen"),
    ("surprise", "T:No Surprises"),
    // artist + title
    ("кино кукушка", "T:Кукушка"),
    ("земфира хочешь", "T:Хочешь"),
    ("сплин романс", "T:Романс"),
    ("ддт метель", "T:Метель"),
    ("ария штиль", "T:Штиль"),
    ("чайф не спеши", "T:Не спеши"),
    ("звери напитки", "T:Напитки покрепче"),
    ("queen under pressure", "T:Under Pressure"),
    ("metallica one", "T:One"),
    ("coldplay yellow", "T:Yellow"),
    ("muse uprising", "T:Uprising"),
    ("abba waterloo", "T:Waterloo"),
    ("rammstein sonne", "T:Sonne"),
    ("beatles help", "T:Help!"),
    ("radiohead creep", "T:Creep"),
    // artist
    ("кино", "A:Кино"),
    ("земфира", "A:Земфира"),
    ("би-2", "A:Би-2"),
    ("би 2", "A:Би-2"),
    ("наутилус", "A:Наутилус Помпилиус"),
    ("мумий тролль", "A:Мумий Тролль"),
    ("король и шут", "A:Король и Шут"),
    ("queen", "A:Queen"),
    ("radiohead", "A:Radiohead"),
    ("pink floyd", "A:Pink Floyd"),
    ("daft punk", "A:Daft Punk"),
    ("depeche mode", "A:Depeche Mode"),
    ("abba", "A:ABBA"),
    ("linkin park", "A:Linkin Park"),
    ("arctic monkeys", "A:Arctic Monkeys"),
    ("the beatles", "A:The Beatles"),
    ("beatles", "A:The Beatles"),
];

#[test]
fn top3_on_the_golden_set() {
    let root = std::env::temp_dir().join(format!("sirtone-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut c = open(
        &root.join(".sirtone/library.db"),
        sirtone_library::Medium::Fixed,
    )
    .unwrap();
    let mut n = 0u32;
    let mut by_title: HashMap<&str, &str> = HashMap::new();
    for (artist, titles) in LIBRARY {
        let a = artist_get_or_create(&c, artist).unwrap();
        for title in *titles {
            n += 1;
            by_title.insert(title, artist);
            track_insert(
                &mut c,
                &NewTrack {
                    rel_path: format!("lib/{n:04}.mp3"),
                    size_bytes: 5,
                    mtime_ms: 0,
                    content_hash: format!("h{n}"),
                    container: "mp3".into(),
                    codec: "mp3".into(),
                    lossless: false,
                    sample_rate: None,
                    channels: None,
                    duration_ms: 200_000,
                    title: (*title).into(),
                    album_id: None,
                    disc_no: None,
                    track_no: None,
                    year: None,
                    identity: Identity::Inferred,
                    identity_conf: 0.9,
                    unknown_reason: None,
                    artists: vec![a],
                },
            )
            .unwrap();
        }
    }
    let mut misses = Vec::new();
    for (q, want) in QUERIES {
        let r = search(&c, q).unwrap();
        let hit = if let Some(title) = want.strip_prefix("T:") {
            assert!(by_title.contains_key(title), "unknown target {title}");
            let mut top: Vec<String> = Vec::new();
            if let Some(Best::Track(t)) = &r.best {
                top.push(t.title.clone());
            }
            top.extend(r.tracks.iter().map(|t| t.title.clone()));
            top.iter().take(3).any(|t| t == title)
        } else {
            let name = want.trim_start_matches("A:");
            let mut top: Vec<String> = Vec::new();
            if let Some(Best::Artist(a)) = &r.best {
                top.push(a.name.clone());
            }
            top.extend(r.artists.iter().map(|a| a.name.clone()));
            top.iter().take(3).any(|a| a == name)
        };
        if !hit {
            let best = match &r.best {
                Some(Best::Track(t)) => format!("T:{}", t.title),
                Some(Best::Artist(a)) => format!("A:{}", a.name),
                Some(Best::Album(a)) => format!("L:{}", a.title),
                None => "—".into(),
            };
            misses.push(format!("{q} → {want} (best {best})"));
        }
    }
    let total = QUERIES.len();
    let rate = (total - misses.len()) as f64 / total as f64;
    eprintln!(
        "search golden set: {} queries, top-3 {:.1} % ({} misses)\n{}",
        total,
        rate * 100.0,
        misses.len(),
        misses.join("\n")
    );
    let _ = std::fs::remove_dir_all(&root);
    assert!(rate >= 0.95, "top-3 {rate:.3}");
}
