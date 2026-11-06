import importlib.util
from pathlib import Path
from types import SimpleNamespace
import sys
import tempfile
import threading
import time
import struct
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'integrations/aivoice2'))
sys.path.append('/home/admin/aivoice2-re/src/daemon')
from scheduler import Scheduler, Busy
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


if __name__ == '__main__':
    try:
        unittest.main()
    finally:
        server.scheduler.close()
