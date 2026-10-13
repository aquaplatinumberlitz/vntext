mod dict;
mod doctor;
mod engines;
mod explain;
mod install;
mod kwin;
mod launcher;
mod mem;
mod ocr;
mod overlay;
mod screen;
mod text;
mod tray;
mod translate;
mod web;
mod yomitan;

use std::collections::{HashSet, VecDeque};
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
  vntext                                                 open the window that lists running games
  vntext install                                         put vntext in the application menu
  vntext doctor                                          check what vntext needs on this machine
  vntext auto    <pid|name> [--clip]                     follow a game whose engine has a profile
  vntext ocr     <pid|name> [--pick] [--clip]            read the text box of any game from pictures of it
                                                         (needs a Gemini API key)

for looking into a game that has no profile yet:
  vntext strings <pid|name> [--min N] [--all]            dump every Japanese string in memory
  vntext find    <pid|name> <text> [--all]               locate a known string
  vntext watch   <pid|name> <hex addr> <utf8|utf16|sjis> [--clip]
  vntext shot    <pid|name> <file.png>                   write the text box as ocr would send it

  --min N   minimum characters per string (default 4)
  --all     also scan read-only memory
  --clip    copy each line to the clipboard (wl-copy)
  --pick    ocr: point out the text box again (asked the first time a game is read)

auto and ocr also take:
  --raw          print text as the engine stores it, control codes included
  --debug        log every change of the cells that hold a reading position
  --web          serve a live page for reading with Yomitan (http://127.0.0.1:8777)
  --open         with --web: open the page in the browser
  --port N       port for --web
  --overlay      show each message, translated if --translate is given, in a strip by the game's window
  --translate    translate each message with the Gemini API (needs GEMINI_API_KEY)
  --to LANG      target language (default vi)
  --via NAME     who translates: gemini, openai, deepseek, claude, local (a server that speaks like
                 OpenAI's) or google; default: the one chosen on the overlay, or gemini. Keys come from
                 the overlay's settings or from GEMINI_API_KEY, OPENAI_API_KEY, DEEPSEEK_API_KEY,
                 ANTHROPIC_API_KEY
  --model ID     model to translate with (default: the one set up; for gemini the newest Flash-Lite)
  --google       the same as --via google: Google Translate, which needs no key. It also steps in
                 when a key is missing and for a passage the chosen service fails on";

/// A string must stay unchanged this long before it is printed (typewriter effects).
const STABLE: Duration = Duration::from_millis(250);
const TICK: Duration = Duration::from_millis(50);
const READ_LEN: usize = 4096;
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
    open: bool,
    port: u16,
    debug: bool,
    pick: bool,
    overlay: bool,
    via: Option<translate::Service>,
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
        open: false,
        port: 8777,
        debug: false,
        pick: false,
        overlay: false,
        via: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--all" => o.all = true,
            "--clip" => o.clip = true,
            "--raw" => o.raw = true,
            "--translate" => o.translate = true,
            "--web" => o.web = true,
            "--open" => o.open = true,
            "--debug" => o.debug = true,
            "--pick" => o.pick = true,
            "--overlay" => o.overlay = true,
            "--google" => o.via = Some(translate::Service::Google),
            "--via" => {
                let named = it.next().and_then(|v| translate::Service::named(&v));
                o.via = Some(named.unwrap_or_else(|| die("--via needs one of: gemini, openai, deepseek, claude, local, google")));
            }
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
    leave(1)
}

/// Serves the page for reading in a browser, and opens it there if asked.
fn serve(hub: &web::Hub, o: &Opts) {
    let page = format!("http://127.0.0.1:{}", o.port);
    match hub.serve(o.port) {
        Ok(()) => eprintln!("page for Yomitan: {page}"),
        Err(e) => die(&format!("cannot serve on port {}: {e}", o.port)),
    }
    if o.open {
        if let Err(e) = Command::new("xdg-open").arg(&page).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            eprintln!("\x1b[33mcannot open {page} in a browser: {e}\x1b[0m");
        }
    }
}

/// Ends the program, taking out of KWin what was put there while that can
/// still be done.
fn leave(code: i32) -> ! {
    kwin::cleanup();
    exit(code)
}

fn open(target: &str) -> Proc {
    let pid = mem::resolve(target).unwrap_or_else(|e| die(&e));
    Proc::open(pid).unwrap_or_else(|e| {
        let hint = if e.kind() == std::io::ErrorKind::PermissionDenied { "\nreading another process's memory is not allowed here; `vntext doctor` says how to allow it" } else { "" };
        die(&format!("cannot open pid {pid}: {e}{hint}"))
    })
}

fn main() {
    let o = parse_args();
    let cfg = Cfg { min_chars: o.min, min_kana: 2 };
    let a: Vec<&str> = o.args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] => launcher::run(),
        ["--help"] | ["-h"] => println!("{USAGE}"),
        ["install"] => match install::all() {
            Ok(done) => println!("{done}"),
            Err(e) => die(&e),
        },
        ["doctor"] => doctor::run(),
        ["auto", target] => {
            let p = open(target);
            eprintln!("scanning...");
            match engines::detect(&p) {
                Some(engine) => follow(p, engine.into(), &o),
                // without a profile the overlay offers to read pictures of the text box
                None if o.overlay => ocr::run(p.pid, &o, false),
                None => die(&format!("no profile knows this game's engine. Its text box can be read from pictures instead:\n  vntext ocr {target} --translate --overlay")),
            }
        }
        ["ocr", target] => ocr::run(mem::resolve(target).unwrap_or_else(|e| die(&e)), &o, true),
        ["shot", target, file] => ocr::shot(mem::resolve(target).unwrap_or_else(|e| die(&e)), file),
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
}

fn watch(p: Proc, addr: u64, enc: Enc, o: &Opts) {
    let mut out = Printer { clip: o.clip, recent: VecDeque::new() };
    let mut t = Track { addr, enc, text: String::new(), since: Instant::now(), printed: String::new() };
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
/// One search in this many is a thorough one.
const THOROUGH_EVERY: u32 = 10;
/// How far a cell may advance at once and still look like someone reading,
/// and how many such steps in a row make an unrecognised cell believable.
const READER_STEP: usize = 8;
const STEPS_TO_TRUST: u32 = 2;
/// Upcoming passages translated together with the current one, so that they
/// are already cached when the reader gets there.
const LOOKAHEAD: usize = 12;
/// Earlier passages sent along for the model's reference.
const CONTEXT: usize = 8;
/// Pause after a translation request that failed.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// Engine-profile mode: follow the game's reading position and print each
/// passage as the game reaches it.
fn follow(p: Proc, engine: Arc<dyn Engine>, o: &Opts) {
    eprintln!("engine: {}, {} passages", engine.name(), engine.len());
    if o.debug {
        let dirs: Vec<String> = p.game_dirs().iter().map(|d| d.display().to_string()).collect();
        eprintln!("\x1b[2m  game files are looked for in: {}\x1b[0m", dirs.join(", "));
    }
    let hub = web::Hub::default();
    if o.web {
        serve(&hub, o);
    }
    // while the reader has the text box read from pictures, the profile keeps quiet
    let pictures = Arc::new(ocr::Control::default());
    if o.overlay {
        overlay::start(hub.clone(), p.pid, pictures.clone(), ocr::Settings::of(o), false);
    }
    let translator = o.translate.then(|| translate::Translator::new(&o.to, o.model.clone(), o.via)).and_then(|made| match made {
        Ok(translator) => Some(translator),
        // a key can be entered on the overlay, for the next run
        Err(e) if o.overlay => {
            hub.publish(explain::event("notice", &e));
            eprintln!("\x1b[33mnot translating: {e}\x1b[0m");
            None
        }
        Err(e) => die(&e),
    });
    let jobs = translator.map(|translator| {
        eprintln!("translating with {}", translator.client.model);
        translate::set_at_work(translator.client.service);
        let (tx, rx) = mpsc::channel();
        let (engine, hub) = (engine.clone(), hub.clone());
        let to = o.to.clone();
        thread::spawn(move || translator_worker(translator, to, engine, hub, rx));
        tx
    });

    // A cell that looks like a reading position, and what it has been doing.
    struct Watch {
        cell: engines::Cell,
        seen: bool,
        reading: Option<usize>,
        /// last passage it pointed at, kept while it points at none
        passage: Option<usize>,
        /// consecutive small steps forward through the script
        steps: u32,
    }
    let watch = |cell| Watch { cell, seen: false, reading: None, passage: None, steps: 0 };

    let mut cells: Vec<Watch> = engine.locate(&p, true).cells.into_iter().map(watch).collect();
    match cells.iter().find(|w| w.cell.trusted) {
        Some(w) => eprintln!("following reading position at {:x}", w.cell.addr),
        None => eprintln!("waiting for the game to show text..."),
    }
    let mut relocating: Option<Receiver<engines::Position>> = None;
    let mut relocations = 0u32;
    let mut retry = RELOCATE_FIRST;
    let mut changed = Instant::now();
    let mut last = None;
    loop {
        if !std::path::Path::new(&format!("/proc/{}", p.pid)).exists() {
            die("target process is gone");
        }
        let mut next = None;
        for w in cells.iter_mut() {
            let now = engine.read(&p, w.cell.addr);
            if w.seen && now == w.reading {
                continue;
            }
            if o.debug {
                eprintln!("\x1b[2m  {:x} {} {:?} -> {now:?}\x1b[0m", w.cell.addr, if w.cell.trusted { "live" } else { "copy?" }, w.reading);
            }
            (w.seen, w.reading) = (true, now);
            let Some(index) = now else { continue };
            // A reader moves a few passages forward at a time; a buffer that
            // is being reused for something else jumps around.
            let forward = w.passage.is_some_and(|before| index > before && index - before <= READER_STEP);
            w.steps = if forward { w.steps + 1 } else { 0 };
            w.passage = Some(index);
            if !w.cell.trusted && w.steps >= STEPS_TO_TRUST {
                w.cell.trusted = true;
                eprintln!("following reading position at {:x}", w.cell.addr);
            }
            if w.cell.trusted {
                next = Some(index);
            }
        }
        if let Some(index) = next.filter(|i| last != Some(*i) && !pictures.on()) {
            last = Some(index);
            changed = Instant::now();
            retry = RELOCATE_FIRST;
            let seq = hub.next_seq();
            let passage = engine.passage(index);
            eprintln!("\x1b[2m[#{index}]\x1b[0m");
            if o.raw {
                println!("{}", passage.raw);
            } else {
                passage.speaker.iter().for_each(|name| println!("\x1b[1;33m{name}\x1b[0m"));
                passage.lines.iter().for_each(|l| println!("{l}"));
            }
            let _ = std::io::stdout().flush();
            if o.clip {
                let _ = Command::new("wl-copy").arg("--").arg(passage.lines.join("\n")).status();
            }
            hub.publish(
                serde_json::json!({
                    "type": "message", "seq": seq, "source": "profile", "index": index,
                    "name": passage.speaker, "lines": passage.lines, "translating": jobs.is_some(),
                })
                .to_string(),
            );
            if let Some(jobs) = &jobs {
                let _ = jobs.send((seq, index));
            }
        }

        let following = cells.iter().any(|w| w.cell.trusted);
        match relocating.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(found)) => {
                relocating = None;
                changed = Instant::now();
                retry = (retry * 2).min(RELOCATE_MAX);
                // cells that no longer match are kept: they may only be between passages
                for cell in found.cells {
                    if !cells.iter().any(|w| w.cell.addr == cell.addr) {
                        if cell.trusted {
                            eprintln!("following reading position at {:x}", cell.addr);
                        } else if o.debug {
                            eprintln!("\x1b[2m  {:x} found, not trusted yet\x1b[0m", cell.addr);
                        }
                        cells.push(watch(cell));
                    }
                }
            }
            Some(Err(TryRecvError::Disconnected)) => relocating = None,
            Some(Err(TryRecvError::Empty)) => {}
            // with nothing to follow, keep looking without pause
            None if !following || changed.elapsed() >= retry => {
                relocations += 1;
                let thorough = relocations % THOROUGH_EVERY == 0;
                let (tx, rx) = mpsc::channel();
                let (engine, pid) = (engine.clone(), p.pid);
                thread::spawn(move || {
                    if let Ok(p) = Proc::open(pid) {
                        let _ = tx.send(engine.locate(&p, thorough));
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
///
/// Requests run on their own thread and reach ahead of the reader: a new one
/// is started while half of the lookahead is still cached, so that a
/// translation is normally there by the time its passage is shown.
fn translator_worker(t: translate::Translator, o_to: String, engine: Arc<dyn Engine>, hub: web::Hub, jobs: Receiver<(u64, usize)>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    let unit = |index: usize| {
        let passage = engine.passage(index);
        translate::Line { speaker: passage.speaker, text: passage.lines.join("\n") }
    };
    let mut client = t.client.clone();
    let mut setups = translate::saves();
    let t = Arc::new(Mutex::new(t));
    let fetching = Arc::new(AtomicBool::new(false));
    // shown passages still waiting for their translation
    let mut waiting: VecDeque<(u64, usize)> = VecDeque::new();
    let mut newest = 0;
    let mut latest = None;
    loop {
        // the reader has set translation up anew on the overlay: go by that from here on
        if setups != translate::saves() {
            setups = translate::saves();
            match translate::Client::new(&o_to, None, None) {
                Ok(new) => {
                    eprintln!("translating with {} ({})", new.model, new.service.known().label);
                    translate::set_at_work(new.service);
                    client = new;
                }
                Err(e) => hub.publish(explain::event("notice", &e)),
            }
        }
        match jobs.recv_timeout(TICK) {
            Ok(job) => {
                newest = job.0;
                latest = Some(job.1);
                waiting.push_back(job);
                // when the reader is skipping ahead, only the newest passages matter
                while waiting.len() > 4 {
                    waiting.pop_front();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        waiting.retain(|(seq, index)| {
            let text = unit(*index).text;
            let (translation, google) = {
                let t = t.lock().unwrap();
                let Some(translation) = t.cached(&text).cloned() else { return true };
                (translation, t.by_google(&text))
            };
            // a translation that comes after its passage has scrolled by says which one it is for
            let late = if *seq == newest { String::new() } else { format!("\x1b[2m#{index}\x1b[0m ") };
            println!("{late}\x1b[36m{translation}\x1b[0m");
            let _ = std::io::stdout().flush();
            let via = if google { "google" } else { "gemini" };
            hub.publish(serde_json::json!({"type": "translation", "seq": seq, "tr": translation, "via": via}).to_string());
            false
        });

        // top up from the newest passage seen, whether or not it is still waiting
        let Some(index) = latest else { continue };
        let missing: Vec<translate::Line> = {
            let t = t.lock().unwrap();
            (index..engine.len().min(index + 1 + LOOKAHEAD)).map(unit).filter(|l| t.cached(&l.text).is_none()).collect()
        };
        let urgent = waiting.iter().any(|(_, i)| *i == index);
        if missing.is_empty() || !(urgent || missing.len() > LOOKAHEAD / 2) || fetching.swap(true, Ordering::SeqCst) {
            continue;
        }
        let context: Vec<String> = (index.saturating_sub(CONTEXT)..index).map(|i| unit(i).text).collect();
        let only = urgent.then(|| unit(index));
        let (client, t, fetching, hub) = (client.clone(), t.clone(), fetching.clone(), hub.clone());
        thread::spawn(move || {
            let mut result = client.translate(&context, &missing).map(|tr| (missing, tr));
            if let Err(e) = result.as_ref().map_err(String::clone) {
                eprintln!("\x1b[33mtranslation failed: {e}\x1b[0m");
                if let Some(only) = only.filter(|_| client.service != translate::Service::Google) {
                    // the service is out: Google translates the passage being read, so
                    // that the reader is not kept waiting
                    let only = vec![only];
                    result = client.google(&only).map(|tr| (only, (tr, translate::Via::Google)));
                    match &result {
                        Ok(_) => eprintln!("\x1b[33mthis passage is translated by Google instead\x1b[0m"),
                        Err(e) => eprintln!("\x1b[33mGoogle failed as well: {e}\x1b[0m"),
                    }
                }
                // said beside the passage if something has translated it, else in its place
                let kind = if urgent && result.is_err() { "notice" } else { "hint" };
                hub.publish(explain::event(kind, &e));
                if e.contains("429") {
                    thread::sleep(Duration::from_secs(30));
                }
            }
            match result {
                Ok((lines, (translations, via))) => t.lock().unwrap().store(&lines, translations, via),
                // do not ask again at once: the loop would retry every tick
                Err(_) => thread::sleep(RETRY_AFTER),
            }
            fetching.store(false, Ordering::SeqCst);
        });
    }
}
