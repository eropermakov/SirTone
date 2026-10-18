# Generates the deterministic audio fixture corpus (T0.13, docs/08 §2) into fixtures/generated/.
# Needs ffmpeg 9.x in PATH. Output is NOT committed (.gitignore). Re-running overwrites everything.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File tools\fixtures\make-fixtures.ps1 [-SkipLong]
param([switch]$SkipLong)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $root 'fixtures\generated'
New-Item -ItemType Directory -Force -Path $out, "$out\formats", "$out\signals", "$out\tags", "$out\names", "$out\broken", "$out\long", "$out\dups" | Out-Null
$sw = [Diagnostics.Stopwatch]::StartNew()

function FF([string[]]$a) {
    # -bitexact + fixed metadata keep outputs reproducible between runs.
    $all = @('-hide_banner', '-loglevel', 'error', '-y') + $a
    & ffmpeg @all
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed: $($a -join ' ')" }
}

# 10 s stereo test tone (440 Hz left, 660 Hz right) as the shared source.
$src = "$out\signals\tone-10s.wav"
FF @('-f', 'lavfi', '-i', 'sine=frequency=440:duration=10:sample_rate=44100', '-f', 'lavfi', '-i', 'sine=frequency=660:duration=10:sample_rate=44100',
     '-filter_complex', '[0][1]amerge=inputs=2', '-c:a', 'pcm_s16le', '-bitexact', $src)

# --- formats (FR-PLAY-001): one file per codec/container/rate/depth ---
$formats = @(
    @('mp3-cbr320.mp3', @('-c:a', 'libmp3lame', '-b:a', '320k')),
    @('mp3-vbr-v2.mp3', @('-c:a', 'libmp3lame', '-q:a', '2')),
    @('flac-16-44.flac', @('-c:a', 'flac')),
    @('flac-24-96.flac', @('-c:a', 'flac', '-sample_fmt', 's32', '-ar', '96000')),
    @('wav-16-44.wav', @('-c:a', 'pcm_s16le')),
    @('wav-24-48.wav', @('-c:a', 'pcm_s24le', '-ar', '48000')),
    @('wav-f32-44.wav', @('-c:a', 'pcm_f32le')),
    @('aac-256.m4a', @('-c:a', 'aac', '-b:a', '256k')),
    @('alac-16-44.m4a', @('-c:a', 'alac')),
    @('vorbis-q6.ogg', @('-c:a', 'libvorbis', '-q:a', '6')),
    @('opus-160.opus', @('-c:a', 'libopus', '-b:a', '160k')),
    @('aiff-16-44.aiff', @('-c:a', 'pcm_s16be')),
    @('wma-192.wma', @('-c:a', 'wmav2', '-b:a', '192k')),
    @('mono-22k.wav', @('-c:a', 'pcm_s16le', '-ac', '1', '-ar', '22050'))
)
foreach ($f in $formats) { FF (@('-i', $src) + $f[1] + @('-bitexact', "$out\formats\$($f[0])")) }

# 5.1 surround (six different tones) and ffmpeg's own stereo downmix of it: the reference for
# the engine's downmix (T1.3, coefficients of `ffmpeg -ac 2`).
FF @('-f', 'lavfi', '-i', 'aevalsrc=0.3*sin(2*PI*220*t)|0.3*sin(2*PI*330*t)|0.3*sin(2*PI*440*t)|0.3*sin(2*PI*55*t)|0.3*sin(2*PI*550*t)|0.3*sin(2*PI*660*t):s=48000:d=3:c=5.1', '-c:a', 'flac', '-bitexact', "$out\formats\surround-51.flac")
FF @('-i', "$out\formats\surround-51.flac", '-ac', '2', '-c:a', 'pcm_f32le', '-bitexact', "$out\signals\surround-51-ffmpeg-stereo.wav")

# Gapless pair: one continuous sine split in two (phase must be continuous across the joint).
FF @('-f', 'lavfi', '-i', 'sine=frequency=1000:duration=8:sample_rate=44100', '-c:a', 'pcm_s16le', '-bitexact', "$out\signals\gapless-full.wav")
FF @('-i', "$out\signals\gapless-full.wav", '-t', '4', '-c:a', 'libmp3lame', '-b:a', '256k', "$out\signals\gapless-a.mp3")
FF @('-ss', '4', '-i', "$out\signals\gapless-full.wav", '-c:a', 'libmp3lame', '-b:a', '256k', "$out\signals\gapless-b.mp3")
# Lossless halves of the same sine: the gapless joint must be bit-continuous (T1.5).
FF @('-i', "$out\signals\gapless-full.wav", '-t', '4', '-c:a', 'pcm_s16le', '-bitexact', "$out\signals\gapless-a.wav")
FF @('-ss', '4', '-i', "$out\signals\gapless-full.wav", '-c:a', 'pcm_s16le', '-bitexact', "$out\signals\gapless-b.wav")
FF @('-i', "$out\signals\gapless-b.wav", '-ar', '48000', '-c:a', 'pcm_s16le', '-bitexact', "$out\signals\gapless-b48.wav")

# --- signals: log sweep (EQ/resampler), click tracks for BPM, loudness steps ---
FF @('-f', 'lavfi', '-i', 'aevalsrc=sin(2*PI*20*(pow(1000\,t/10)-1)/log(1000)*10):s=48000:d=10', '-c:a', 'pcm_f32le', '-bitexact', "$out\signals\sweep-20-20k-10s.wav")
foreach ($bpm in 60, 90, 120, 128, 140, 174, 180) {
    $period = [Math]::Round(60.0 / $bpm, 6)
    FF @('-f', 'lavfi', '-i', "aevalsrc='if(lt(mod(t\,$period)\,0.01)\,sin(2*PI*2000*t)\,0)':s=44100:d=30", '-c:a', 'pcm_s16le', '-bitexact', "$out\signals\click-$bpm.wav")
}
foreach ($db in -30, -20, -14, -9) {
    FF @('-f', 'lavfi', '-i', 'sine=frequency=1000:duration=10:sample_rate=48000', '-af', "volume=${db}dB", '-c:a', 'pcm_s16le', '-bitexact', "$out\signals\level$db.wav")
}

# --- tags: mojibake (cp1251 bytes read as Latin-1), junk values, multi-artist ---
$cp1251 = [Text.Encoding]::GetEncoding(1251); $latin1 = [Text.Encoding]::GetEncoding(28591)
$moj = { param($s) $latin1.GetString($cp1251.GetBytes($s)) }
$tagSets = @(
    @('mojibake.mp3', (& $moj 'Би-2'), (& $moj 'Полковнику никто не пишет'), (& $moj 'Би-2')),
    @('junk-values.mp3', 'Unknown Artist', 'Track 01', 'Unknown'),
    @('feat.mp3', 'Би-2 feat. Земфира', 'Варвара', 'Би-2'),
    @('cyrillic-ok.flac', 'Земфира', 'Искала', 'Вендетта')
)
foreach ($t in $tagSets) {
    $codec = if ($t[0].EndsWith('.flac')) { @('-c:a', 'flac') } else { @('-c:a', 'libmp3lame', '-b:a', '192k', '-id3v2_version', '3') }
    FF (@('-i', $src, '-t', '5') + $codec + @('-metadata', "artist=$($t[1])", '-metadata', "title=$($t[2])", '-metadata', "album=$($t[3])", "$out\tags\$($t[0])"))
}
# WMA tags are read through the Windows Property System (T1.15): every field it maps.
FF @('-i', $src, '-t', '5', '-c:a', 'wmav2', '-b:a', '128k', '-metadata', 'title=Город', '-metadata', 'artist=Кино',
     '-metadata', 'album=Группа крови', '-metadata', 'album_artist=Кино', '-metadata', 'track=3', '-metadata', 'disc=1/2',
     '-metadata', 'WM/Year=1988', '-metadata', 'genre=Rock', "$out\tags\wma-tagged.wma")

# --- names: «poison» and parser cases (no tags) ---
$names = @(
    '01 - Би-2 - Полковнику никто не пишет.mp3', 'Jay-Z - 99 Problems (Official Video).mp3', 'AC-DC - Back In Black [320kbps].mp3',
    'Track 01.mp3', 'con.mp3', 'trailing dot..mp3', ' leading space.mp3', 'Песня с эмодзи 🎵.mp3', ('Очень длинное имя ' * 12).Trim() + '.mp3'
)
$plain = "$out\names\_plain.mp3"
FF @('-i', $src, '-t', '3', '-map_metadata', '-1', '-c:a', 'libmp3lame', '-b:a', '128k', $plain)
foreach ($n in $names) {
    $safe = $n -replace '^con\.mp3$', 'con_.mp3'   # «CON» cannot be created on Windows — the sanitizer test covers it in code
    Copy-Item -LiteralPath $plain -Destination (Join-Path "$out\names" $safe.Trim()) -Force
}
Remove-Item -LiteralPath $plain

# --- broken: truncated, zero bytes, not audio with audio extension ---
$bytes = [IO.File]::ReadAllBytes("$out\formats\mp3-cbr320.mp3")
[IO.File]::WriteAllBytes("$out\broken\truncated.mp3", $bytes[0..([int]($bytes.Length / 3))])
[IO.File]::WriteAllBytes("$out\broken\empty.flac", [byte[]]@())
[IO.File]::WriteAllText("$out\broken\text-as.mp3", 'this is not audio')
[IO.File]::WriteAllText("$out\broken\playlist.m3u", "#EXTM3U`r`nformats\mp3-cbr320.mp3")

# --- dups (T3.5, AT-05): one melody as FLAC and MP3 under different names — «тот же звук»;
# another melody — not a duplicate. 25 s each: a fingerprint needs ≥ 64 items (≈ 8 s) of variety.
$melody = { param($a, $b) "aevalsrc=0.3*sin(2*PI*t*(220*pow(2\,mod(floor(t*3)*$a\,12)/12)))+0.2*sin(2*PI*t*(110*pow(2\,mod(floor(t*1.5)*$b\,12)/12))):s=44100:d=25" }
FF @('-f', 'lavfi', '-i', (& $melody 5 7), '-c:a', 'flac', '-metadata', 'artist=Мелодия', '-metadata', 'title=Утро', "$out\dups\Утро.flac")
FF @('-f', 'lavfi', '-i', (& $melody 5 7), '-c:a', 'libmp3lame', '-b:a', '192k', '-metadata', 'artist=Неизвестный', '-metadata', 'title=Рассвет (копия)', "$out\dups\Рассвет.mp3")
FF @('-f', 'lavfi', '-i', (& $melody 11 3), '-c:a', 'flac', '-metadata', 'artist=Мелодия', '-metadata', 'title=Вечер', "$out\dups\Вечер.flac")

# --- long: 3-hour mix (FR-IMP-007) — cheap low-bitrate silence+tone ---
if (-not $SkipLong) {
    FF @('-f', 'lavfi', '-i', 'sine=frequency=220:duration=10800:sample_rate=22050', '-af', 'volume=-30dB', '-c:a', 'libmp3lame', '-b:a', '32k', '-ac', '1', "$out\long\mix-3h.mp3")
}

$count = (Get-ChildItem -Recurse -File $out).Count
Write-Host ("fixtures: {0} files in {1:N1} s -> {2}" -f $count, $sw.Elapsed.TotalSeconds, $out)
