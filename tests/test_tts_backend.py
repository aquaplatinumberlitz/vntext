import importlib.util
from pathlib import Path
from types import SimpleNamespace
import sys
import tempfile
import threading
import time
import struct
import os
import hashlib
import http.client
import io
import wave
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'integrations/aivoice2'))
support = Path(os.environ.get('AIVOICE2_SUPPORT_DIR', Path.home() / '.local/share/aivoice2-local/runtime'))
sys.path.append(str(support))
from scheduler import Scheduler, Busy
from wav_cache import WavCache
import server


class Scheduling(unittest.TestCase):
    def test_new_revision_drops_only_queued_foreground_of_its_own_session(self):
        scheduler = Scheduler()
        entered, release = threading.Event(), threading.Event()
        scheduler.prepare('running',1,lambda _: (entered.set(),release.wait(3)))
        self.assertTrue(entered.wait(2))
        result = []
        def pending():
            try:
                scheduler.submit(lambda _: result.append('stale'),'client-a',1)
            except Busy:
                result.append('cancelled')
        thread = threading.Thread(target=pending)
        thread.start()
        until = time.monotonic()+2
        while not scheduler.foreground and time.monotonic()<until:
            time.sleep(.005)
        scheduler.cancel('client-a',2)
        thread.join(2)
        release.set()
        scheduler.submit(lambda _:result.append('Anki'))
        scheduler.close()
        self.assertEqual(result,['cancelled','Anki'])

    def test_foreground_overtakes_pending_preparation_and_cancel_is_scoped(self):
        scheduler = Scheduler(4)
        entered, release = threading.Event(), threading.Event()
        order = []
        scheduler.prepare('running', 1, lambda _: (entered.set(), release.wait(3)))
        self.assertTrue(entered.wait(2))
        scheduler.prepare('a', 1, lambda _: order.append('old'))
        scheduler.prepare('a', 2, lambda _: order.append('replacement'))
        scheduler.prepare('b', 1, lambda _: order.append('other-session'))
        scheduler.cancel('a', 3)
        self.assertFalse(scheduler.prepare('a', 2, lambda _: order.append('stale')))
        foreground = threading.Thread(target=lambda: scheduler.submit(lambda _: order.append('foreground')))
        foreground.start()
        until = time.monotonic() + 2
        while not scheduler.foreground and time.monotonic() < until:
            time.sleep(.005)
        release.set()
        foreground.join(2)
        until = time.monotonic() + 2
        while len(order) < 2 and time.monotonic() < until:
            time.sleep(.005)
        scheduler.close()
        self.assertEqual(order, ['foreground', 'other-session'])

    def test_bounded_background_queue_and_close(self):
        scheduler = Scheduler(1)
        entered, release = threading.Event(), threading.Event()
        scheduler.prepare('running', 1, lambda _: (entered.set(), release.wait(3)))
        self.assertTrue(entered.wait(2))
        scheduler.prepare('a', 1, lambda _: None)
        with self.assertRaises(Busy):
            scheduler.prepare('b', 1, lambda _: None)
        scheduler.prepare('a', 2, lambda _: None)
        release.set()
        scheduler.close()


class CacheAndWarm(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        asset = root / 'asset'
        asset.write_bytes(b'fixture')
        self.voice = {'name': 'fixture', 'package': asset, 'license': asset,
                      'language': 'standard', 'language_package': asset}
        self.engine = server.Engine(SimpleNamespace(worker=asset, cache_dir=root))
        self.engine.version = 'fixture'
        self.voices = patch.object(server, 'voices', return_value={'v': self.voice})
        self.voices.start()

    def tearDown(self):
        self.voices.stop()
        self.temp.cleanup()

    def test_cache_hit_never_waits_on_native_lock(self):
        _, _, output = self.engine.request('synthetic', 'v')
        output.write_bytes(b'cached audio')
        self.engine.lock.acquire()
        try:
            began = time.monotonic()
            with patch.object(server.scheduler, 'submit', side_effect=AssertionError('Cache must bypass scheduler')):
                result = self.engine.speak('synthetic', 'v')
            self.assertLess(time.monotonic() - began, .2)
            self.assertEqual(result[:2], (output, True))
        finally:
            self.engine.lock.release()

    def test_warm_cannot_be_satisfied_by_cached_wav(self):
        _, _, output = self.engine.request('。', 'v')
        output.write_bytes(b'cached sample')
        with patch.object(server.scheduler, 'prepare') as enqueue, patch.object(self.engine, 'perform') as native:
            self.engine.prepare(None, 'v', 'session', 1, warm=True)
            enqueue.assert_called_once()
            enqueue.call_args.args[2](0)
            self.assertTrue(native.call_args.kwargs['warm'])

    def test_long_speculative_text_is_rejected(self):
        with self.assertRaises(ValueError):
            self.engine.prepare('あ' * 161, 'v', 'session', 1)

    def test_native_pcm_frames_are_drained_before_final_status(self):
        packets = [struct.pack('<iI', -2000, 4), b'\x01\x00\x02\x00', struct.pack('<iI', 0, 4)]
        chunks = []
        with patch.object(self.engine, 'read_exact', side_effect=packets):
            self.assertEqual(self.engine.read_reply(on_chunk=chunks.append), (0, 4))
        self.assertEqual(chunks, [b'\x01\x00\x02\x00'])
        with patch.object(self.engine, 'read_exact', return_value=struct.pack('<iI', -2000, 65538)):
            with self.assertRaises(RuntimeError):
                self.engine.read_reply()

    def test_http_wav_and_cached_pcm_are_leased_through_delivery(self):
        audio = io.BytesIO()
        with wave.open(audio, 'wb') as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(48000)
            wav.writeframes(b'\x01\x00\x02\x00')
        data = audio.getvalue()
        _, _, output = self.engine.request('synthetic', 'v')
        self.engine.cache.limit = 1  # Even a single response exceeds the quota.

        def cached(*args):
            self.assertGreater(self.engine.cache.held[output], 0)
            self.engine.cache.prune()
            self.assertTrue(output.exists())
            return output, True, {'queue_ms': 0}

        with patch.object(server, 'engine', self.engine), patch.object(server, 'CONFIG', SimpleNamespace(default_voice='v')), \
             patch.object(self.engine, 'speak', side_effect=cached):
            http_server = server.ThreadingHTTPServer(('127.0.0.1', 0), server.Handler)
            thread = threading.Thread(target=http_server.serve_forever, kwargs={'poll_interval': .01})
            thread.start()
            try:
                for endpoint in ['/v1/speak', '/v1/speak/stream']:
                    with self.subTest(endpoint=endpoint):
                        output.write_bytes(data)
                        client = http.client.HTTPConnection('127.0.0.1', http_server.server_port, timeout=3)
                        try:
                            client.request('POST', endpoint, body=b'{"text":"synthetic","voice":"v"}', headers={'Content-Type': 'application/json'})
                            response = client.getresponse()
                            self.assertEqual(response.status, 200)
                            body = response.read()
                        finally:
                            client.close()
                        if endpoint == '/v1/speak':
                            self.assertEqual(body, data)
                        else:
                            self.assertTrue(body.startswith(b'AVS1'))
                            self.assertEqual(body[4:9], struct.pack('<BI', 1, 4))
                            self.assertEqual(body[9:13], b'\x01\x00\x02\x00')
                            self.assertEqual(body[13], 2)
                        until = time.monotonic() + 1
                        while output.exists() and time.monotonic() < until:
                            time.sleep(.005)
                        self.assertFalse(output.exists(), 'Quota must be enforced once delivery releases the file')
            finally:
                http_server.shutdown()
                http_server.server_close()
                thread.join(2)


class DiskCache(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.cache = WavCache(self.root, limit=12, max_age=100)

    def tearDown(self):
        self.temp.cleanup()

    def wav(self, name, size=6, age=0):
        path = self.root / f'aivoice2_{hashlib.sha256(name.encode()).hexdigest()}.wav'
        path.write_bytes(b'a' * size)
        used = time.time() - age
        os.utime(path, (used, used))
        return path

    def test_eviction_uses_last_hit_after_cache_is_reopened(self):
        old = self.wav('old', age=50)
        recent = self.wav('recent', age=10)
        self.assertTrue(self.cache.hit(old))
        self.wav('new')
        # Last-use timestamps survive a service restart, without an in-memory index.
        restarted = WavCache(self.root, limit=12, max_age=100)
        self.assertEqual(restarted.prune()['bytes'], 12)
        self.assertTrue(old.exists())
        self.assertFalse(recent.exists())

    def test_expiry_does_not_touch_unowned_files_directories_or_symlinks(self):
        expired = self.wav('expired', age=200)
        current = self.wav('current')
        outside = self.root / 'anki-media.wav'
        outside.write_bytes(b'keep')
        temporary = self.root / 'temporary.wav'
        temporary.write_bytes(b'keep')
        link = self.root / ('aivoice2_' + 'a' * 64 + '.wav')
        link.symlink_to(outside)
        directory = self.root / ('aivoice2_' + 'b' * 64 + '.wav')
        directory.mkdir()
        self.assertEqual(self.cache.prune()['removed'], 1)
        self.assertFalse(expired.exists())
        self.assertTrue(current.exists() and link.is_symlink() and directory.is_dir())
        self.assertEqual(outside.read_bytes(), b'keep')
        self.assertEqual(temporary.read_bytes(), b'keep')

    def test_concurrent_cleanup_keeps_active_output_until_last_lease_closes(self):
        active = self.wav('active', size=16, age=200)
        with self.cache.hold(active):
            with self.cache.hold(active):
                cleaner = threading.Thread(target=self.cache.prune)
                cleaner.start()
                cleaner.join(2)
                self.assertFalse(cleaner.is_alive())
                self.assertTrue(active.exists())
                self.assertTrue(self.cache.summary()['deferred'])
            self.assertTrue(active.exists())
        self.assertFalse(active.exists())
        self.assertEqual(self.cache.summary()['bytes'], 0)
        self.assertFalse(self.cache.held)

    def test_publish_protects_new_output_and_removes_old_wav(self):
        old = self.wav('old', size=8, age=20)
        output = self.root / ('aivoice2_' + 'c' * 64 + '.wav')
        temporary = self.root / 'in-flight.wav'
        temporary.write_bytes(b'new audio')
        with self.cache.hold(output):
            self.cache.publish(temporary, output)
            self.assertEqual(output.read_bytes(), b'new audio')
            self.assertFalse(old.exists())
        self.assertFalse(temporary.exists())
        self.assertEqual(self.cache.summary()['bytes'], 9)

    def test_hit_of_missing_file_does_not_create_it_and_failed_consumer_releases_lease(self):
        output = self.root / ('aivoice2_' + 'd' * 64 + '.wav')
        self.assertFalse(self.cache.hit(output))
        self.assertFalse(output.exists())
        with self.assertRaises(RuntimeError):
            with self.cache.hold(output):
                raise RuntimeError('Consumer disconnected')
        self.assertFalse(self.cache.held)


if __name__ == '__main__':
    try:
        unittest.main()
    finally:
        server.scheduler.close()
