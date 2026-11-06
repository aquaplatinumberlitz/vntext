"""One native worker, bounded foreground queue and expendable preparations."""
import collections
import threading
import time


class Busy(RuntimeError):
    pass


class Job:
    def __init__(self, action, session='', revision=0, lifetime=45, background=False):
        self.action = action
        self.session = session
        self.revision = revision
        self.deadline = time.monotonic() + lifetime
        self.background = background
        self.done = threading.Event()
        self.result = None
        self.error = None
        self.queued = time.monotonic()


class Scheduler:
    def __init__(self, limit=24):
        self.limit = limit
        self.condition = threading.Condition()
        self.foreground = collections.deque()
        self.background = collections.OrderedDict()
        self.revisions = collections.OrderedDict()
        self.closed = False
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def cancel(self, session, revision):
        with self.condition:
            current = self.revisions.get(session, -1)
            if revision < current:
                return False
            self.revisions[session] = revision
            self.revisions.move_to_end(session)
            while len(self.revisions) > 256:
                self.revisions.popitem(last=False)
            old = self.background.pop(session, None)
            if old:
                old.error = Busy('Preparation replaced')
                old.done.set()
            kept = collections.deque()
            for job in self.foreground:
                if job.session == session and job.revision < revision:
                    job.error = Busy('Request replaced by a newer sentence')
                    job.done.set()
                else:
                    kept.append(job)
            self.foreground = kept
            return True

    def prepare(self, session, revision, action):
        with self.condition:
            if self.closed or len(self.background) >= self.limit and session not in self.background:
                raise Busy('Preparation queue is full')
            # Every session owns at most one pending job; native running work is not aborted.
            if revision < self.revisions.get(session, -1):
                return False
            self.revisions[session] = revision
            self.revisions.move_to_end(session)
            while len(self.revisions) > 256:
                self.revisions.popitem(last=False)
            self.background[session] = Job(action, session, revision, 8, True)
            self.condition.notify()
            return True

    def submit(self, action, session='', revision=0):
        job = Job(action, session, revision)
        with self.condition:
            if self.closed or len(self.foreground) >= self.limit:
                raise Busy('Synthesis queue is full')
            if session:
                if revision < self.revisions.get(session, -1):
                    raise Busy('Request replaced by a newer sentence')
                self.cancel(session, revision)
            self.foreground.append(job)
            self.condition.notify()
        if not job.done.wait(46):
            raise Busy('Synthesis deadline exceeded')
        if job.error:
            raise job.error
        return job.result

    def run(self):
        while True:
            with self.condition:
                while not self.foreground and not self.background and not self.closed:
                    self.condition.wait()
                if self.closed:
                    return
                job = self.foreground.popleft() if self.foreground else self.background.popitem(last=False)[1]
                stale = bool(job.session) and job.revision < self.revisions.get(job.session, -1)
            try:
                if stale or time.monotonic() > job.deadline:
                    raise Busy('Queued request expired or replaced')
                job.result = job.action((time.monotonic() - job.queued) * 1000)
            except Exception as error:
                job.error = error
            finally:
                job.done.set()

    def close(self):
        with self.condition:
            self.closed = True
            for job in [*self.foreground, *self.background.values()]:
                job.error = Busy('Service stopped')
                job.done.set()
            self.foreground.clear()
            self.background.clear()
            self.condition.notify_all()
        self.thread.join(timeout=125)
