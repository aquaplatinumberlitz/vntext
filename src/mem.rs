//! Reading another process's memory from the outside (/proc/<pid>/{maps,pagemap,mem}).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

const PAGE: u64 = 4096;
const CHUNK: usize = 8 << 20;
/// Strings starting in the last OVERLAP bytes of a chunk are left to the next chunk.
const OVERLAP: usize = 16 << 10;

const SKIP: &[&str] = &[
    "[vvar",
    "[vdso",
    "[vsyscall",
    "memfd:doublemapper",
    "memfd:pulseaudio",
    "frida-agent",
];

pub struct Mapping {
    pub start: u64,
    pub end: u64,
    pub perms: String,
    pub offset: u64,
    pub dev: String,
    pub inode: u64,
    pub path: String,
}

impl Mapping {
    fn shared(&self) -> bool {
        self.perms.as_bytes()[3] == b's'
    }

    fn wanted(&self, all: bool) -> bool {
        let p = self.perms.as_bytes();
        if p[0] != b'r' || (!all && p[1] != b'w') {
            return false;
        }
        let path = self.path.as_str();
        if SKIP.iter().any(|s| path.contains(s)) {
            return false;
        }
        // Device mappings (GPU etc.) must never be read.
        if path.starts_with("/dev/") && !path.starts_with("/dev/shm/") && !path.starts_with("/dev/zero") {
            return false;
        }
        if self.shared() {
            return ["/dev/shm/", "/dev/zero", "/memfd:", "/SYSV"].iter().any(|s| path.starts_with(s));
        }
        true
    }
}

pub fn read_maps(pid: u32) -> io::Result<Vec<Mapping>> {
    let text = fs::read_to_string(format!("/proc/{pid}/maps"))?;
    let mut maps = Vec::new();
    for line in text.lines() {
        let mut it = line.splitn(6, ' ');
        let (Some(range), Some(perms), Some(offset), Some(dev), Some(inode)) =
            (it.next(), it.next(), it.next(), it.next(), it.next())
        else {
            continue;
        };
        let Some((start, end)) = range.split_once('-') else { continue };
        let (Ok(start), Ok(end)) = (u64::from_str_radix(start, 16), u64::from_str_radix(end, 16)) else {
            continue;
        };
        if perms.len() < 4 {
            continue;
        }
        maps.push(Mapping {
            start,
            end,
            perms: perms.to_string(),
            offset: u64::from_str_radix(offset, 16).unwrap_or(0),
            dev: dev.to_string(),
            inode: inode.parse().unwrap_or(0),
            path: it.next().unwrap_or("").trim().to_string(),
        });
    }
    Ok(maps)
}

#[derive(Default)]
pub struct Stats {
    pub bytes: u64,
    pub maps: usize,
}

pub struct Proc {
    pub pid: u32,
    mem: File,
    pagemap: File,
}

impl Proc {
    pub fn open(pid: u32) -> io::Result<Proc> {
        Ok(Proc {
            pid,
            mem: File::open(format!("/proc/{pid}/mem"))?,
            pagemap: File::open(format!("/proc/{pid}/pagemap"))?,
        })
    }

    /// Directories the game's files are likely to be in, best guess first:
    /// where the executables mapped into the process live, then the working
    /// directory. The executable comes first because it does not depend on
    /// how the game was launched; under Wine it shows up with its real path.
    pub fn game_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let exes = read_maps(self.pid).unwrap_or_default().into_iter().filter(|m| m.path.to_lowercase().ends_with(".exe"));
        let cwd = fs::read_link(format!("/proc/{}/cwd", self.pid)).ok();
        for dir in exes.filter_map(|m| Path::new(&m.path).parent().map(Path::to_path_buf)).chain(cwd) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs
    }

    /// The first file in the game's directories whose name (lowercased, as
    /// Windows games are careless about case) satisfies `wanted`.
    pub fn game_file(&self, wanted: impl Fn(&str) -> bool) -> Option<PathBuf> {
        self.game_dirs().iter().find_map(|dir| {
            let mut names: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
            names.sort();
            names.into_iter().find(|path| path.file_name().is_some_and(|n| wanted(&n.to_string_lossy().to_lowercase())))
        })
    }

    /// Reads as much of `buf` as possible at `addr`; returns the number of bytes read.
    pub fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        let mut done = 0;
        while done < buf.len() {
            match self.mem.read_at(&mut buf[done..], addr + done as u64) {
                Ok(0) | Err(_) => break,
                Ok(n) => done += n,
            }
        }
        done
    }

    /// Walks every resident page once and hands it to `f(base, data, limit, mapping)`.
    /// Only strings starting before `limit` should be reported from a chunk.
    pub fn scan(&self, all: bool, f: impl FnMut(u64, &[u8], usize, &Mapping)) -> io::Result<Stats> {
        self.scan_where(all, |_| true, f)
    }

    /// Like `scan`, reading only the mappings that `keep` accepts.
    pub fn scan_where(
        &self,
        all: bool,
        keep: impl Fn(&Mapping) -> bool,
        mut f: impl FnMut(u64, &[u8], usize, &Mapping),
    ) -> io::Result<Stats> {
        let mut buf = vec![0u8; CHUNK];
        let mut stats = Stats::default();
        let maps = self.runs(all, keep, |run, m| self.read_run(run, &mut buf, m, &mut f, &mut stats))?;
        Ok(Stats { maps, ..stats })
    }

    /// Hands the first `len` bytes of every resident page to `f(page address, bytes)`.
    /// Far quicker than `scan` when what is searched for sits at a fixed place in a page.
    pub fn page_heads(&self, keep: impl Fn(&Mapping) -> bool, len: usize, mut f: impl FnMut(u64, &[u8])) -> io::Result<()> {
        let mut head = vec![0u8; len];
        self.runs(false, keep, |(start, end), _| {
            for page in (start..end).step_by(PAGE as usize) {
                if self.read(page, &mut head) == len {
                    f(page, &head);
                }
            }
        })?;
        Ok(())
    }

    /// Finds the runs of resident pages, each page once, and returns the
    /// number of mappings looked at.
    ///
    /// Emulators map the same shared memory file many times, so pages of shared
    /// mappings are deduplicated by (file, page offset). Non-resident pages are
    /// never touched: reading a hole in a shm mapping would allocate it.
    fn runs(&self, all: bool, keep: impl Fn(&Mapping) -> bool, mut f: impl FnMut((u64, u64), &Mapping)) -> io::Result<usize> {
        let maps = read_maps(self.pid)?;
        let mut seen: HashMap<(String, u64), Vec<u64>> = HashMap::new();
        let mut pm = vec![0u8; 8 * 65536];
        let mut visited = 0;

        for m in maps.iter().filter(|m| m.wanted(all) && keep(m)) {
            let mut bits = if m.shared() && m.inode != 0 {
                Some(seen.entry((m.dev.clone(), m.inode)).or_default())
            } else {
                None
            };
            let npages = (m.end - m.start) / PAGE;
            let fpage0 = m.offset / PAGE;
            if let Some(b) = &bits {
                if all_set(b, fpage0, npages) {
                    continue;
                }
            }
            visited += 1;

            let mut run: Option<(u64, u64)> = None;
            let mut pg = 0u64;
            while pg < npages {
                let want = (npages - pg).min(65536) as usize;
                let got = match self.pagemap.read_at(&mut pm[..want * 8], (m.start / PAGE + pg) * 8) {
                    Ok(n) => n / 8,
                    Err(_) => 0,
                };
                if got == 0 {
                    break;
                }
                for i in 0..got {
                    let entry = u64::from_le_bytes(pm[i * 8..i * 8 + 8].try_into().unwrap());
                    // bit 63: present, bit 62: swapped
                    let mut resident = entry >> 62 != 0;
                    if resident {
                        if let Some(b) = bits.as_mut() {
                            resident = !test_and_set(b, fpage0 + pg + i as u64);
                        }
                    }
                    let addr = m.start + (pg + i as u64) * PAGE;
                    match (&mut run, resident) {
                        (Some(r), true) => r.1 = addr + PAGE,
                        (None, true) => run = Some((addr, addr + PAGE)),
                        (Some(_), false) => f(run.take().unwrap(), m),
                        (None, false) => {}
                    }
                }
                pg += got as u64;
            }
            if let Some(r) = run.take() {
                f(r, m);
            }
        }
        Ok(visited)
    }

    fn read_run(
        &self,
        (mut pos, end): (u64, u64),
        buf: &mut [u8],
        m: &Mapping,
        f: &mut impl FnMut(u64, &[u8], usize, &Mapping),
        stats: &mut Stats,
    ) {
        while pos < end {
            let len = ((end - pos) as usize).min(CHUNK);
            let n = self.read(pos, &mut buf[..len]);
            if n == 0 {
                break;
            }
            let last = n < len || pos + len as u64 >= end;
            f(pos, &buf[..n], if last { n } else { n - OVERLAP }, m);
            stats.bytes += n as u64;
            if last {
                break;
            }
            pos += (len - OVERLAP) as u64;
        }
    }
}

fn all_set(bits: &[u64], start: u64, count: u64) -> bool {
    (start..start + count).all(|p| bits.get((p / 64) as usize).is_some_and(|w| w >> (p % 64) & 1 == 1))
}

fn test_and_set(bits: &mut Vec<u64>, page: u64) -> bool {
    let word = (page / 64) as usize;
    if word >= bits.len() {
        bits.resize(word + 1, 0);
    }
    let mask = 1u64 << (page % 64);
    let was = bits[word] & mask != 0;
    bits[word] |= mask;
    was
}

/// The kernel keeps only this many bytes of a process name.
const COMM_LEN: usize = 15;

/// Resolves a pid or a (case-insensitive) substring of a process name / command line.
///
/// A process whose own name matches beats one that merely has the text in
/// its command line: a game started through a launcher has wrapper processes
/// (umu-run, proton) that carry the game's path as an argument, run in the
/// game's directory and can be as large as the game itself. Among equals the
/// one with the largest RSS wins.
pub fn resolve(target: &str) -> Result<u32, String> {
    if let Ok(pid) = target.parse::<u32>() {
        return Ok(pid);
    }
    let needle = target.to_lowercase();
    let name: String = needle.chars().take(COMM_LEN).collect();
    let me = std::process::id();
    let mut best: Option<((bool, u64), u32, String)> = None;
    for entry in fs::read_dir("/proc").map_err(|e| e.to_string())?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().to_lowercase();
        let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        let named = comm.contains(&name);
        if !named && !cmdline.to_lowercase().contains(&needle) {
            continue;
        }
        let rss = fs::read_to_string(format!("/proc/{pid}/statm"))
            .ok()
            .and_then(|s| s.split(' ').nth(1).and_then(|v| v.parse::<u64>().ok()))
            .unwrap_or(0);
        if best.as_ref().is_none_or(|b| (named, rss) > b.0) {
            best = Some(((named, rss), pid, cmdline.trim().to_string()));
        }
    }
    let (_, pid, cmdline) = best.ok_or_else(|| format!("no process matching '{target}'"))?;
    eprintln!("target: pid {pid} ({cmdline})");
    Ok(pid)
}
