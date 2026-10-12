mod engines;
mod mem;
mod text;
mod translate;
mod web;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::process::{exit, Command};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engines::Engine;
use mem::Proc;
use text::{Cfg, Enc, Hit};

const USAGE: &str = "\
vntext - read Japanese text out of a running game or emulator

usage:
  vntext auto    <pid|name> [--min N] [--all] [--clip]   print new lines as they appear
  vntext strings <pid|name> [--min N] [--all]            dump every Japanese string in memory
  vntext find    <pid|name> <text> [--all]               locate a known string
  vntext watch   <pid|name> <hex addr> <utf8|utf16|sjis> [--clip]

  --min N   minimum characters per string (default 4)
  --all     also scan read-only memory
  --clip    copy each line to the clipboard (wl-copy)

when the game's engine is recognised (currently: the Entergram shin engine), auto also takes:
  --raw          print text as the engine stores it, control codes included
  --web          serve a live page for reading with Yomitan (http://127.0.0.1:8777)
  --port N       port for --web
  --translate    translate each message with the Gemini API (needs GEMINI_API_KEY)
  --to LANG      target language (default vi)
  --model ID     Gemini model (default: newest Flash-Lite the API offers)";

/// A string must stay unchanged this long before it is printed (typewriter effects).
const STABLE: Duration = Duration::from_millis(250);
const TICK: Duration = Duration::from_millis(50);
const READ_LEN: usize = 4096;
/// A batch of settled candidates is printed once nothing has changed for this long...
const QUIET: Duration = Duration::from_millis(300);
/// ...or after this long regardless, so constant background churn cannot block output.
const MAX_HOLD: Duration = Duration::from_millis(1500);
/// Complete lines an address must have held before it is polled directly.
const HOT_AFTER: u32 = 2;
const MAX_HOT: usize = 8;
/// A hot address that keeps yielding fragments of other lines is dropped.
const MAX_STRIKES: u32 = 3;
/// Printed lines remembered for duplicate / fragment suppression.
const HISTORY: usize = 30;

struct Opts {
    args: Vec<String>,
    min: usize,
    all: bool,
    clip: bool,
    raw: bool,
    translate: bool,
    to: String,
    model: Option<String>,
    web: bool,
    port: u16,
}

fn parse_args() -> Opts {
    let mut o = Opts {
        args: Vec::new(),
        min: 4,
        all: false,
        clip: false,
        raw: false,
        translate: false,
        to: "vi".to_string(),
        model: None,
        web: false,
        port: 8777,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--all" => o.all = true,
            "--clip" => o.clip = true,
            "--raw" => o.raw = true,
            "--translate" => o.translate = true,
            "--web" => o.web = true,
            "--to" => o.to = it.next().unwrap_or_else(|| die("--to needs a language")),
            "--model" => o.model = Some(it.next().unwrap_or_else(|| die("--model needs a model id"))),
            "--port" => o.port = it.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| die("--port needs a number")),
            "--min" => o.min = it.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| die("--min needs a number")),
            _ => o.args.push(a),
        }
    }
    o
}

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    exit(1)
}

fn open(target: &str) -> Proc {
    let pid = mem::resolve(target).unwrap_or_else(|e| die(&e));
    Proc::open(pid).unwrap_or_else(|e| die(&format!("cannot open pid {pid}: {e}")))
}

fn main() {
    let o = parse_args();
    let cfg = Cfg { min_chars: o.min, min_kana: 2 };
    let a: Vec<&str> = o.args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["auto", target] => {
            let p = open(target);
            eprintln!("scanning...");
            match engines::detect(&p) {
                Some(engine) => follow(p, engine.into(), &o),
                None => auto(p, cfg, &o),
            }
        }
        ["strings", target] => strings(open(target), cfg, &o),
        ["find", target, needle] => find(open(target), needle, &o),
        ["watch", target, addr, enc] => {
            let addr = u64::from_str_radix(addr.trim_start_matches("0x"), 16).unwrap_or_else(|_| die("bad address"));
            let enc = Enc::parse(enc).unwrap_or_else(|| die("encoding must be utf8, utf16 or sjis"));
            watch(open(target), addr, enc, &o)
        }
        _ => die(USAGE),
    }
}

fn full_scan(p: &Proc, cfg: &Cfg, all: bool) -> std::io::Result<(Vec<Hit>, mem::Stats)> {
    let mut hits = Vec::new();
    let stats = p.scan(all, |base, data, limit, _| text::scan(data, base, limit, cfg, &mut hits))?;
    Ok((hits, stats))
}

fn strings(p: Proc, cfg: Cfg, o: &Opts) {
    let t = Instant::now();
    let (hits, stats) = full_scan(&p, &cfg, o.all).unwrap_or_else(|e| die(&format!("scan failed: {e}")));
    let mut seen = HashSet::new();
    let mut out = std::io::stdout().lock();
    for h in &hits {
        if seen.insert(h.text.as_str()) {
            let _ = writeln!(out, "{:012x} {:5} {}", h.addr, h.enc.name(), h.text.replace('\n', "\\n"));
        }
    }
    eprintln!(
        "{} strings ({} unique) in {} MB, {} mappings, {:.2}s",
        hits.len(),
        seen.len(),
        stats.bytes >> 20,
        stats.maps,
        t.elapsed().as_secs_f32()
    );
}

fn find(p: Proc, needle: &str, o: &Opts) {
    let needles: Vec<(Enc, memchr::memmem::Finder)> = Enc::ALL
        .into_iter()
        .filter_map(|e| Some((e, memchr::memmem::Finder::new(&text::encode(needle, e)?).into_owned())))
        .collect();
    let mut count = 0;
    let res = p.scan(o.all, |base, data, limit, m| {
        for (enc, finder) in &needles {
            for off in finder.find_iter(data).take_while(|off| *off < limit) {
                count += 1;
                println!("{:012x} {:5} {} {}", base + off as u64, enc.name(), m.perms, m.path);
            }
        }
    });
    match res {
        Ok(stats) => eprintln!("{count} matches in {} MB", stats.bytes >> 20),
        Err(e) => die(&format!("scan failed: {e}")),
    }
}

fn read_text(p: &Proc, addr: u64, enc: Enc, buf: &mut [u8]) -> Option<String> {
    let n = p.read(addr, buf);
    text::decode_at(&buf[..n], enc)
}

struct Printer {
    clip: bool,
    recent: VecDeque<(String, Instant)>,
}

impl Printer {
    /// True if `text` is a piece of a longer line printed earlier (stale partial
    /// copies of old lines keep surfacing as memory gets reused).
    fn fragment(&self, text: &str) -> bool {
        self.recent.iter().any(|(s, _)| s.len() > text.len() && s.contains(text))
    }

    /// Prints a line unless the same text was just printed (several copies of a
    /// line usually live in memory at once).
    fn emit(&mut self, text: &str, addr: u64, enc: Enc, tag: &str) {
        let now = Instant::now();
        if self.fragment(text) || self.recent.iter().any(|(s, t)| s == text && now - *t < Duration::from_secs(2)) {
            return;
        }
        self.recent.push_back((text.to_string(), now));
        if self.recent.len() > HISTORY {
            self.recent.pop_front();
        }
        eprint!("\x1b[2m[{} {addr:x}{tag}]\x1b[0m ", enc.name());
        println!("{}", text.replace('\n', " "));
        let _ = std::io::stdout().flush();
        if self.clip {
            let _ = Command::new("wl-copy").arg("--").arg(text).status();
        }
    }
}

struct Track {
    addr: u64,
    enc: Enc,
    text: String,
    since: Instant,
    printed: String,
    strikes: u32,
}

fn watch(p: Proc, addr: u64, enc: Enc, o: &Opts) {
    let mut out = Printer { clip: o.clip, recent: VecDeque::new() };
    let mut t = Track { addr, enc, text: String::new(), since: Instant::now(), printed: String::new(), strikes: 0 };
    let mut buf = vec![0u8; READ_LEN];
    loop {
        if !poll(&p, &mut t, &mut buf) {
            die("target process is gone");
        }
        if t.text != t.printed && t.since.elapsed() >= STABLE {
            out.emit(&t.text, addr, enc, "");
            t.printed = t.text.clone();
        }
        thread::sleep(TICK);
    }
}

/// Refreshes `t.text` from memory; false once the process no longer exists.
fn poll(p: &Proc, t: &mut Track, buf: &mut [u8]) -> bool {
    let cur = read_text(p, t.addr, t.enc, buf).unwrap_or_default();
    if cur != t.text {
        t.text = cur;
        t.since = Instant::now();
    }
    std::path::Path::new(&format!("/proc/{}", p.pid)).exists()
}

/// With no new passage for this long, the game's reading position is located
/// again in the background: it moves when a save is loaded or the game
/// returns to the title. The wait doubles up to the maximum while the reader
/// stays on one passage, so a slow reader does not cause constant rescans.
const RELOCATE_FIRST: Duration = Duration::from_secs(3);
const RELOCATE_MAX: Duration = Duration::from_secs(30);
/// Upcoming passages translated together with the current one, so that they
/// are already cached when the reader gets there.
const LOOKAHEAD: usize = 8;
/// Earlier passages sent along for the model's reference.
const CONTEXT: usize = 4;

/// Engine-profile mode: follow the game's reading position and print each
/// passage as the game reaches it.
fn follow(p: Proc, engine: Arc<dyn Engine>, o: &Opts) {
    eprintln!("engine: {}, {} passages", engine.name(), engine.len());
    let hub = web::Hub::default();
    if o.web {
        match hub.serve(o.port) {
            Ok(()) => eprintln!("page for Yomitan: http://127.0.0.1:{}", o.port),
            Err(e) => die(&format!("cannot serve on port {}: {e}", o.port)),
        }
    }
    let jobs = o.translate.then(|| {
        let translator = translate::Translator::new(&o.to, o.model.clone()).unwrap_or_else(|e| die(&e));
        eprintln!("translating with {}", translator.model);
        let (tx, rx) = mpsc::channel();
        let (engine, hub) = (engine.clone(), hub.clone());
        thread::spawn(move || translator_worker(translator, engine, hub, rx));
        tx
    });

    let mut position = engine.locate(&p);
    match position.cells.first() {
        Some(c) => eprintln!("following reading position at {c:x}"),
        None => eprintln!("waiting for the game to show text..."),
    }
    // last reading of every cell: the live one is told apart by the fact that it moves
    let mut readings: HashMap<u64, Option<usize>> = HashMap::new();
    let mut relocating: Option<Receiver<engines::Position>> = None;
    let mut retry = RELOCATE_FIRST;
    let mut changed = Instant::now();
    let mut last = None;
    let mut seq = 0u64;
    loop {
        if !std::path::Path::new(&format!("/proc/{}", p.pid)).exists() {
            die("target process is gone");
        }
        let mut next = None;
        for (rank, cell) in position.cells.iter().enumerate() {
            let now = engine.read(&p, *cell);
            match readings.insert(*cell, now) {
                Some(before) if before != now => next = now.or(next),
                // nothing to compare with yet: believe the most trustworthy cell
                None if rank == 0 => next = now.or(next),
                _ => {}
            }
        }
        if let Some(index) = next.filter(|i| last != Some(*i)) {
            last = Some(index);
            changed = Instant::now();
            retry = RELOCATE_FIRST;
            seq += 1;
            let passage = engine.passage(index);
            eprintln!("\x1b[2m[#{index}]\x1b[0m");
            if o.raw {
                println!("{}", passage.raw);
            } else {
                passage.speaker.iter().chain(&passage.lines).for_each(|l| println!("{l}"));
            }
            let _ = std::io::stdout().flush();
            if o.clip {
                let _ = Command::new("wl-copy").arg("--").arg(passage.lines.join("\n")).status();
            }
            hub.publish(
                serde_json::json!({
                    "type": "message", "seq": seq, "index": index,
                    "name": passage.speaker, "lines": passage.lines, "translating": jobs.is_some(),
                })
                .to_string(),
            );
            if let Some(jobs) = &jobs {
                let _ = jobs.send((seq, index));
            }
        }

        match relocating.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(mut found)) => {
                relocating = None;
                changed = Instant::now();
                retry = (retry * 2).min(RELOCATE_MAX);
                // keep cells that no longer match: they may only be between passages
                let kept: Vec<u64> = position.cells.iter().copied().filter(|c| !found.cells.contains(c)).collect();
                found.cells.extend(kept);
                if found.cells.first() != position.cells.first() {
                    eprintln!("following reading position at {:x}", found.cells[0]);
                    readings.remove(&found.cells[0]);
                }
                position = found;
            }
            Some(Err(TryRecvError::Disconnected)) => relocating = None,
            Some(Err(TryRecvError::Empty)) => {}
            None if changed.elapsed() >= retry => {
                let (tx, rx) = mpsc::channel();
                let (engine, pid) = (engine.clone(), p.pid);
                thread::spawn(move || {
                    if let Ok(p) = Proc::open(pid) {
                        let _ = tx.send(engine.locate(&p));
                    }
                });
                relocating = Some(rx);
            }
            None => {}
        }
        thread::sleep(TICK);
    }
}

/// Translates passages as they are shown, off the polling thread. A passage
/// (a few sentences by one speaker) is translated as a whole: the sentences
/// lean on each other too much to be translated one at a time.
fn translator_worker(mut t: translate::Translator, engine: Arc<dyn Engine>, hub: web::Hub, jobs: Receiver<(u64, usize)>) {
    let unit = |index: usize| {
        let passage = engine.passage(index);
        translate::Line { speaker: passage.speaker, text: passage.lines.join("\n") }
    };
    while let Ok(mut job) = jobs.recv() {
        // when the reader is skipping ahead, only the newest message matters
        let mut waiting: Vec<(u64, usize)> = jobs.try_iter().collect();
        if waiting.len() > 3 {
            job = waiting.pop().unwrap();
            waiting.clear();
        }
        for (seq, index) in std::iter::once(job).chain(waiting) {
            let current = unit(index).text;
            if t.cached(&current).is_none() {
                let context: Vec<String> = (index.saturating_sub(CONTEXT)..index).map(|i| unit(i).text).collect();
                let upcoming = (index..engine.len().min(index + 1 + LOOKAHEAD)).map(unit);
                let batch: Vec<translate::Line> = upcoming.filter(|l| t.cached(&l.text).is_none()).collect();
                if let Err(e) = t.translate(&context, &batch) {
                    eprintln!("\x1b[33mtranslation failed: {e}\x1b[0m");
                    if e.contains("429") {
                        thread::sleep(Duration::from_secs(30));
                    }
                    // a smaller request is more likely to come back well-formed
                    if let Err(e) = t.translate(&context, &[unit(index)]) {
                        eprintln!("\x1b[33mtranslation failed: {e}\x1b[0m");
                    }
                }
            }
            if let Some(translation) = t.cached(&current) {
                println!("\x1b[36m{translation}\x1b[0m");
                let _ = std::io::stdout().flush();
                hub.publish(serde_json::json!({"type": "translation", "seq": seq, "tr": translation}).to_string());
            }
        }
    }
}

/// Calibration-free mode. A background thread rescans the whole process; any
/// address whose content changed since the previous scan becomes a candidate
/// and is polled until its text stops changing (typewriter effects). Settled
/// candidates are collected into a batch, and once things go quiet only the
/// longest strings of the batch are printed, which drops partial copies.
/// Addresses that repeatedly held a complete line are then polled directly.
fn auto(p: Proc, cfg: Cfg, o: &Opts) {
    let (tx, rx) = mpsc::channel();
    let (pid, all) = (p.pid, o.all);
    thread::spawn(move || {
        let Ok(p) = Proc::open(pid) else { return };
        loop {
            let t = Instant::now();
            let Ok((hits, stats)) = full_scan(&p, &cfg, all) else { return };
            if tx.send((hits, stats, t.elapsed())).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });

    let mut out = Printer { clip: o.clip, recent: VecDeque::new() };
    let mut seen: HashSet<String> = HashSet::new();
    let mut last: HashMap<(u64, Enc), u64> = HashMap::new();
    let mut baseline = true;
    let mut pending: Vec<Track> = Vec::new();
    let mut batch: Vec<Track> = Vec::new();
    let mut hot: Vec<Track> = Vec::new();
    let mut score: HashMap<(u64, Enc), u32> = HashMap::new();
    let mut activity = Instant::now();
    let mut batch_since = Instant::now();
    let mut buf = vec![0u8; READ_LEN];

    loop {
        match rx.recv_timeout(TICK) {
            Ok((hits, stats, took)) => {
                let mut cur = HashMap::with_capacity(hits.len());
                let total = hits.len();
                for h in hits {
                    let key = (h.addr, h.enc);
                    let hash = hash_of(&h.text);
                    cur.insert(key, hash);
                    if baseline {
                        seen.insert(h.text);
                        continue;
                    }
                    let tracked = |t: &Track| (t.addr, t.enc) == key;
                    if last.get(&key) != Some(&hash) && !pending.iter().chain(&batch).chain(&hot).any(tracked) {
                        pending.push(Track { addr: h.addr, enc: h.enc, text: h.text, since: Instant::now(), printed: String::new(), strikes: 0 });
                        activity = Instant::now();
                    }
                }
                last = cur;
                if baseline {
                    eprintln!(
                        "baseline: {total} strings in {} MB ({:.2}s per scan). Advance the text in game.",
                        stats.bytes >> 20,
                        took.as_secs_f32()
                    );
                    baseline = false;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => die("target process is gone"),
        }

        let mut i = 0;
        while i < hot.len() {
            let t = &mut hot[i];
            poll(&p, t, &mut buf);
            if t.text != t.printed && !t.text.is_empty() && t.since.elapsed() >= STABLE {
                t.printed = t.text.clone();
                if out.fragment(&t.text) {
                    t.strikes += 1;
                } else {
                    seen.insert(t.text.clone());
                    out.emit(&t.text, t.addr, t.enc, " hot");
                }
            }
            if hot[i].strikes >= MAX_STRIKES {
                hot.swap_remove(i);
            } else {
                i += 1;
            }
        }

        let mut i = 0;
        while i < pending.len() {
            let t = &mut pending[i];
            let before = t.since;
            poll(&p, t, &mut buf);
            if t.since != before {
                activity = t.since;
            }
            if t.text.is_empty() || !text::is_japanese(&t.text, t.enc, &cfg) {
                pending.swap_remove(i);
            } else if t.since.elapsed() >= STABLE {
                if batch.is_empty() {
                    batch_since = Instant::now();
                }
                batch.push(pending.swap_remove(i));
            } else {
                i += 1;
            }
        }

        let quiet = pending.is_empty() && activity.elapsed() >= QUIET;
        if !batch.is_empty() && (quiet || batch_since.elapsed() >= MAX_HOLD) {
            batch.sort_by(|a, b| b.text.len().cmp(&a.text.len()));
            let mut winners: Vec<String> = Vec::new();
            for mut t in batch.drain(..) {
                if winners.iter().any(|w| w.len() > t.text.len() && w.contains(&t.text)) || out.fragment(&t.text) {
                    continue;
                }
                if !winners.contains(&t.text) {
                    winners.push(t.text.clone());
                    if seen.insert(t.text.clone()) {
                        out.emit(&t.text, t.addr, t.enc, "");
                    }
                }
                let n = score.entry((t.addr, t.enc)).or_insert(0);
                *n += 1;
                if *n >= HOT_AFTER && hot.len() < MAX_HOT {
                    t.printed = t.text.clone();
                    hot.push(t);
                }
            }
        }
    }
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}
