#!/usr/bin/env python3
"""Local A.I.VOICE2 HTTP service, one persistent Wine engine, serial synthesis."""
from __future__ import annotations

import hashlib
import argparse
from dataclasses import replace
from scheduler import Scheduler, Busy
from wav_cache import WavCache
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import select
import struct
import subprocess
import threading
import time
import uuid
import wave
import queue
import signal

from run_auth_probe import ENGINE_SHA256, credentials
from runtime_config import load_config, load_voice_registry, install_drive_aliases, acquire_runtime_lease

DEFAULT_VOICE = 'KotonohaAkane_ns_48/KotonohaAkane_ns_48'
CONFIG = None


def voices(config=None):
    config = config or CONFIG or load_config()
    registry = load_voice_registry(config.voice_registry)
    result = {}
    for package in sorted((config.app_dir / 'Voice').glob('*/*.aivdic')):
        canonical = package.parent / (package.parent.name + '.aivdic')
        if canonical.is_file() and package != canonical:
            continue
        license_path = package.parent / 'voice.lic'
        if license_path.is_file():
            identity = package.relative_to(config.app_dir / 'Voice').with_suffix('').as_posix()
            # Unknown dialects must be registered and checked before synthesis.
            entry = registry.get(package.parent.name)
            if entry is None:
                continue
            language = entry['language']
            language_package = config.app_dir / 'Lang' / language / (language + '.aildic')
            if not language_package.is_file():
                continue
            result[identity] = {'id': identity, 'name': package.stem,
                                'folder': package.parent.name,
                                'package': package, 'license': license_path,
                                'language': language,
                                'language_package': language_package}
    return result


class Engine:
    def __init__(self, config=None):
        self.config = config
        self.lock = threading.Lock()
        self.process = None
        self.ready = False
        self.log = None
        self.version = ''
        self.lease = None
        self.metrics = {}
        self.cache = WavCache(config.cache_dir) if config else None

    def read_exact(self, length, timeout=120):
        deadline = time.monotonic() + timeout
        data = bytearray()
        while len(data) < length:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.process.stdout], [], [], remaining)[0]:
                raise TimeoutError('Engine timed out; retry to restart it')
            part = os.read(self.process.stdout.fileno(), length - len(data))
            if not part:
                raise RuntimeError('Engine stopped; check journalctl --user -u aivoice2-tts')
            data.extend(part)
        return bytes(data)

    def read_reply(self, timeout=120, on_chunk=None):
        while True:
            status, count = struct.unpack('<iI', self.read_exact(8, timeout))
            if status != -2000:
                return status, count
            if not 0 < count <= 65536 or count % 2:
                raise RuntimeError('Invalid worker audio frame')
            data = self.read_exact(count, timeout)
            if on_chunk:
                on_chunk(data)

    def stop(self):
        self.ready = False
        if self.process:
            try:
                self.process.stdin.close()
                self.process.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired):
                self.process.kill()
                self.process.wait(timeout=5)
            self.process.stdout.close()
            self.process = None
        if self.log:
            self.log.close()
            self.log = None
        if self.lease:
            self.lease.close()
            self.lease = None

    def start(self):
        self.stop()
        config = self.config or CONFIG or load_config()
        self.config = config
        if self.cache is None:
            self.cache = WavCache(config.cache_dir)
        engine_path = config.app_dir / 'aitalk_engine.dll'
        if hashlib.sha256(engine_path.read_bytes()).hexdigest() != ENGINE_SHA256:
            raise ValueError('Engine differs from the inspected artifact')
        exe = config.worker
        if not exe.is_file():
            raise ValueError('Thiếu engine_worker.exe đã build. Chạy sửa cài đặt; runtime không tự biên dịch.')
        self.version = hashlib.sha256(exe.read_bytes()).hexdigest()
        license_path = config.app_dir / 'Lib' / 'AITalk' / 'aitalk.lic'
        product, code = credentials(license_path)
        env = os.environ.copy()
        env.update(WINEPREFIX=str(config.engine_prefix), WINEDEBUG='-all',
                   AITALK_PRODUCT_NAME=product, AITALK_LICENSE_CODE=code, AIVOICE2_WORKER_METRICS='1')
        config.cache_dir.mkdir(parents=True, exist_ok=True)
        config.log_dir.mkdir(parents=True, exist_ok=True)
        self.lease = acquire_runtime_lease(config)
        install_drive_aliases(config, config.engine_prefix)
        self.log = (config.log_dir / 'engine-worker.log').open('ab', buffering=0)
        command = ['wine', config.wine_path(exe), config.wine_path(engine_path),
                   config.wine_path(license_path),
                   config.wine_path(config.app_dir / 'Lang/standard/standard.aildic')]
        self.process = subprocess.Popen(command, env=env, cwd=config.cache_dir,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.log, bufsize=0)
        try:
            status, _ = self.read_reply()
            if status:
                raise RuntimeError(f'Engine initialization failed ({status})')
            self.ready = True
        except Exception:
            self.stop()
            raise

    def request(self, text, voice_id):
        if not isinstance(text, str) or not text.strip() or '\x00' in text:
            raise ValueError('Text is required')
        text = text.strip()
        if len(text) > 4096 or len(text.encode('utf-8')) > 16384:
            raise ValueError('Text is too long (maximum 4096 characters)')
        available = voices(self.config)
        if not isinstance(voice_id, str) or voice_id not in available:
            raise ValueError('Unknown voice; refresh the voice list')
        voice = available[voice_id]
        version = self.version or hashlib.sha256(self.config.worker.read_bytes()).hexdigest()
        signature = [text, voice_id, ENGINE_SHA256, version,
                     voice['package'].stat().st_mtime_ns,
                     voice['license'].stat().st_mtime_ns,
                     voice['language'], voice['language_package'].stat().st_mtime_ns]
        digest = hashlib.sha256(json.dumps(signature, ensure_ascii=False).encode()).hexdigest()
        return text, voice, self.config.cache_dir / f'aivoice2_{digest}.wav'

    def metrics_reply(self):
        data = bytearray()
        deadline = time.monotonic() + 5
        while len(data) < 16:
            if not select.select([self.process.stdout], [], [], max(0, deadline-time.monotonic()))[0]:
                raise TimeoutError('Worker timing reply missing')
            part = os.read(self.process.stdout.fileno(), 16-len(data))
            if not part:
                raise BrokenPipeError('Worker timing channel closed')
            data.extend(part)
        return dict(zip(('load_ms', 'first_pcm_ms', 'synth_ms', 'native_ms'), struct.unpack('<4I', data)))

    def perform(self, text, voice, output, warm=False, queue_ms=0, on_chunk=None):
        with self.lock, self.cache.hold(output):
            if not self.ready or self.process.poll() is not None:
                self.start()
            if not warm and self.cache.hit(output):
                return output, True, {'queue_ms': queue_ms}
            temporary = self.config.cache_dir / f'{uuid.uuid4().hex}.wav'
            fields = [text, voice['name'], self.config.wine_path(voice['package']),
                      self.config.wine_path(voice['license']), '@warm' if warm else self.config.wine_path(temporary),
                      voice['language'], self.config.wine_path(voice['language_package'])]
            encoded = [f.encode('utf-8') for f in fields]
            try:
                packet = struct.pack('<7I', *(len(f) for f in encoded)) + b''.join(encoded)
                view = memoryview(packet)
                while view:
                    count = self.process.stdin.write(view)
                    if not count:
                        raise BrokenPipeError('Engine input closed')
                    view = view[count:]
                self.process.stdin.flush()
                status, count = self.read_reply(on_chunk=on_chunk)
                timings = self.metrics_reply()
                timings['queue_ms'] = round(queue_ms, 2)
                self.metrics = timings
                if status:
                    raise RuntimeError(f'Synthesis failed ({status})')
                if not warm:
                    with wave.open(str(temporary), 'rb') as wav:
                        if (wav.getnchannels() != 1 or wav.getsampwidth() != 2 or wav.getframerate() != 48000
                                or not wav.getnframes() or wav.getnframes()*2 != count):
                            raise RuntimeError('Engine returned invalid audio')
                    self.cache.publish(temporary, output)
                print(json.dumps({'tts_timing': timings, 'warm': warm}), flush=True)
                return output, False, timings
            except (OSError, TimeoutError, RuntimeError):
                self.stop()
                raise
            finally:
                temporary.unlink(missing_ok=True)

    def speak(self, text, voice_id, session='', revision=0, on_chunk=None):
        text, voice, output = self.request(text, voice_id)
        # The filename is immutable and published by rename: hits never wait on synthesis.
        if self.cache.hit(output):
            return output, True, {'queue_ms': 0}
        return scheduler.submit(lambda wait: self.perform(text, voice, output, queue_ms=wait, on_chunk=on_chunk), session, revision)

    def prepare(self, text, voice_id, session, revision, warm=False):
        text, voice, output = self.request('。' if warm else text, voice_id)
        if not warm and self.cache.hit(output):
            return True
        if not warm and len(text) > 160:
            raise ValueError('Preparation is limited to 160 characters')
        return scheduler.prepare(session, revision,
            lambda wait: self.perform(text, voice, output, warm=warm, queue_ms=wait))


engine = Engine()
scheduler = Scheduler()


class Handler(BaseHTTPRequestHandler):
    def stream(self, body, default):
        # Native synthesis must never wait on a slow/disconnected HTTP consumer.
        frames = queue.Queue(maxsize=256)  # 16 MiB of PCM at most.
        abandoned, finished = threading.Event(), threading.Event()
        outcome = {}

        def chunk(data):
            if not abandoned.is_set():
                try:
                    frames.put_nowait(data)
                except queue.Full:
                    abandoned.set()

        def synthesize():
            try:
                _, _, output = engine.request(body.get('text'), body.get('voice', default))
                with engine.cache.hold(output):
                    path, cached, timings = engine.speak(body.get('text'), body.get('voice', default),
                        body.get('session', ''), body.get('revision', 0), chunk)
                    if cached:
                        with wave.open(str(path), 'rb') as audio:
                            while data := audio.readframes(32768):
                                chunk(data)
                outcome.update(cache=cached, timing=timings)
            except Exception:
                outcome['error'] = 'Synthesis failed or queue deadline exceeded'
            finally:
                finished.set()

        threading.Thread(target=synthesize, daemon=True).start()
        self.send_response(200)
        self.send_header('Content-Type', 'application/x-aivoice2-pcm')
        self.send_header('Connection', 'close')
        self.end_headers()
        self.close_connection = True
        def frame(kind, data):
            self.wfile.write(struct.pack('<BI', kind, len(data)) + data)
            self.wfile.flush()
        try:
            self.wfile.write(b'AVS1')  # PCM s16 mono 48 kHz; final frame confirms success.
            while not finished.is_set() or not frames.empty():
                try:
                    frame(1, frames.get(timeout=.1))
                except queue.Empty:
                    continue
            if abandoned.is_set():
                outcome['error'] = 'Audio exceeded the delivery buffer'
            frame(3 if 'error' in outcome else 2, json.dumps(outcome).encode())
        except (OSError, TimeoutError):
            abandoned.set()

    def send(self, status, body, content_type='application/json', **headers):
        if not isinstance(body, bytes):
            body = json.dumps(body, ensure_ascii=False).encode('utf-8')
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        for key, value in headers.items():
            self.send_header(key.replace('_', '-'), value)
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        if self.path == '/healthz':
            ready = bool(engine.ready and engine.process and engine.process.poll() is None)
            self.send(200 if ready else 503, {'ready': ready, 'engine': 'A.I.VOICE2',
                                            'wav_cache': engine.cache.summary() if engine.cache else None,
                                            'instance_id': CONFIG.instance_id, 'capabilities': ['prepare-v1', 'warm-v1', 'timing-v1', 'pcm-stream-v1']})
        elif self.path == '/v1/voices':
            items = [{k: v for k, v in item.items()
                      if k not in ('package', 'license', 'language_package')}
                     for item in voices().values()]
            preferred = CONFIG.default_voice
            default = preferred if any(v['id'] == preferred for v in items) else (
                items[0]['id'] if items else None)
            self.send(200, {'voices': items, 'default_voice': default})
        else:
            self.send(404, {'error': 'Not found'})

    def do_POST(self):
        if self.path not in ('/v1/speak', '/v1/prepare', '/v1/cancel', '/v1/speak/stream'):
            self.send(404, {'error': 'Not found'})
            return
        self.connection.settimeout(15)
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 65536:
                raise ValueError('Invalid request size')
            body = json.loads(self.rfile.read(length))
            if not isinstance(body, dict):
                raise ValueError('Expected a JSON object')
            available = voices(CONFIG)
            default = CONFIG.default_voice if CONFIG.default_voice in available else next(iter(available), None)
            session = body.get('session', '')
            revision = body.get('revision', 0)
            if not isinstance(session, str) or len(session) > 96 or not session.isascii():
                raise ValueError('Invalid session')
            if type(revision) is not int or not 0 <= revision < 2**63:
                raise ValueError('Invalid revision')
            if self.path in ('/v1/prepare', '/v1/cancel') and not session:
                raise ValueError('Session is required')
            if self.path == '/v1/cancel':
                self.send(200, {'cancelled': scheduler.cancel(session, revision)})
                return
            if self.path == '/v1/prepare':
                mode = body.get('mode', 'text')
                if mode not in ('warm', 'text'):
                    raise ValueError('Unknown preparation mode')
                accepted = engine.prepare(body.get('text'), body.get('voice', default), session, revision, mode == 'warm')
                self.send(202, {'accepted': accepted})
                return
            if self.path == '/v1/speak/stream':
                engine.request(body.get('text'), body.get('voice', default))
                self.stream(body, default)
                return
            _, _, output = engine.request(body.get('text'), body.get('voice', default))
            with engine.cache.hold(output):
                path, cached, timings = engine.speak(body.get('text'), body.get('voice', default), session, revision)
                self.send(200, path.read_bytes(), 'audio/wav',
                          X_Aivoice2_Filename=path.name, X_Aivoice2_Cache='hit' if cached else 'miss',
                          Server_Timing=', '.join(f'{key};dur={value}' for key, value in timings.items()))
        except (ValueError, UnicodeError) as error:
            self.send(400, {'error': str(error)})
        except Exception as error:
            print(f'Synthesis error: {error}', flush=True)
            self.send(503, {'error': str(error)})


def main():
    global CONFIG, engine
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config')
    parser.add_argument('--worker', type=Path, required=True)
    args = parser.parse_args()
    CONFIG = replace(load_config(args.config), runtime_dir=args.worker.resolve().parent)
    engine = Engine(CONFIG)
    if CONFIG.development_fallback:
        print('Development fallback: configure AIVOICE2_CONFIG for a managed installation.', flush=True)
    try:
        server = ThreadingHTTPServer(('127.0.0.1', CONFIG.port), Handler)
    except OSError as error:
        raise SystemExit(f'Không mở được {CONFIG.api_url}; kiểm tra dịch vụ/cổng đang dùng: {error}')
    server.daemon_threads = True
    stopped = threading.Event()
    cleaner = threading.Thread(target=engine.cache.maintain, args=(stopped,), daemon=True)
    try:
        engine.cache.clean()
        engine.start()
        cleaner.start()
        print(f'A.I.VOICE2 ready at {CONFIG.api_url}', flush=True)
        signal.signal(signal.SIGTERM, lambda *_: threading.Thread(target=server.shutdown, daemon=True).start())
        server.serve_forever()
    finally:
        stopped.set()
        if cleaner.is_alive():
            cleaner.join(timeout=3)
        server.server_close()
        scheduler.close()
        engine.stop()


if __name__ == '__main__':
    main()
