"""Bounded disk WAV cache. Leases protect files being synthesized or delivered."""
import collections
from contextlib import contextmanager
import os
from pathlib import Path
import re
import stat
import threading
import time

LIMIT = 512 << 20
MAX_AGE = 7 * 24 * 60 * 60
OWNED = re.compile(r'aivoice2_[0-9a-f]{64}\.wav\Z')


class WavCache:
    def __init__(self, directory, limit=LIMIT, max_age=MAX_AGE):
        self.directory = Path(directory)
        self.limit, self.max_age = limit, max_age
        self.lock = threading.RLock()
        self.held = collections.Counter()
        self.deferred = False
        self.stats = {'files': 0, 'bytes': 0, 'removed': 0}

    @contextmanager
    def hold(self, path):
        with self.lock:
            self.held[path] += 1
        try:
            yield
        finally:
            with self.lock:
                self.held[path] -= 1
                if not self.held[path]:
                    del self.held[path]
                if self.deferred:
                    self.clean()

    def hit(self, path):
        with self.lock:
            try:
                if not stat.S_ISREG(path.lstat().st_mode):
                    return False
                # Explicit mtime is reliable on filesystems mounted with noatime.
                os.utime(path, None, follow_symlinks=False)
                return True
            except FileNotFoundError:
                return False

    def publish(self, temporary, output):
        with self.lock:
            temporary.replace(output)
            os.utime(output, None, follow_symlinks=False)
            self.clean()

    def clean(self):
        try:
            self.prune()
        except OSError as error:
            self.deferred = True
            print(f'WAV cache cleanup: {error}', flush=True)

    def prune(self):
        with self.lock:
            entries = []
            for path in self.directory.glob('aivoice2_*.wav'):
                if not OWNED.fullmatch(path.name):
                    continue
                try:
                    info = path.lstat()
                except FileNotFoundError:
                    continue
                if stat.S_ISREG(info.st_mode):
                    entries.append((info.st_mtime, path, info.st_size))
            entries.sort()
            total = sum(size for _, _, size in entries)
            removed = 0
            cutoff = time.time() - self.max_age
            self.deferred = False
            for used, path, size in entries:
                if used >= cutoff and total <= self.limit:
                    continue
                if self.held.get(path):
                    self.deferred = True
                    continue
                try:
                    path.unlink()
                except FileNotFoundError:
                    pass
                except OSError as error:
                    self.deferred = True
                    print(f'WAV cache cleanup: {error}', flush=True)
                    continue
                total -= size
                removed += 1
            self.stats = {'files': len(entries) - removed, 'bytes': total,
                          'removed': self.stats['removed'] + removed}
            return dict(self.stats)

    def summary(self):
        with self.lock:
            return {**self.stats, 'limit_bytes': self.limit,
                    'max_age_seconds': self.max_age, 'deferred': self.deferred}

    def maintain(self, stopped):
        while not stopped.wait(60):
            self.clean()
