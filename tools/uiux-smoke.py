#!/usr/bin/env python3
"""Exercise the real GTK overlay in a private, virtual KWin session.

Build with cargo test --release --locked --no-run, then pass the test binary.
No games or input on the user's desktop are used. Sentences/settings are fixtures.
--pointer optionally sends guarded pointer events only to the private compositor.
"""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile


@contextmanager
def private():
    with tempfile.TemporaryDirectory(prefix='vntext-uiux-') as temporary:
        root = Path(temporary)
        try:
            yield root
        finally:
            # The private portal may leave its FUSE mount after its bus ends.
            document = root / 'runtime/doc'
            mounts = Path('/proc/self/mountinfo').read_text().splitlines()
            if any(line.split()[4] == str(document) for line in mounts):
                result = subprocess.run(['fusermount3', '-u', '-z', str(document)], capture_output=True, text=True)
                # The portal may unmount it between reading mountinfo and fusermount.
                mounts = Path('/proc/self/mountinfo').read_text().splitlines()
                if result.returncode and any(line.split()[4] == str(document) for line in mounts):
                    raise RuntimeError(f'Cannot unmount private portal: {result.stderr.strip()}')


def children(pid):
    found = []
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            fields = path.read_text().rsplit(')', 1)[1].split()
            if int(fields[1]) == pid:
                child = int(path.parent.name)
                found.extend(children(child))
                found.append(child)
        except (OSError, ValueError, IndexError):
            continue
    return found


def run(binary, size, output):
    output.mkdir(parents=True, exist_ok=True)
    result = output / 'result.json'
    result.unlink(missing_ok=True)
    with private() as root:
        runtime = root / 'runtime'
        runtime.mkdir(mode=0o700)
        env = os.environ.copy()
        for key in list(env):
            if key.endswith(('_KEY', '_TOKEN')) or key in (
                'DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS',
                'DBUS_SESSION_BUS_PID', 'SESSION_MANAGER', 'XAUTHORITY',
            ):
                env.pop(key)
        env.update(
            XDG_CONFIG_HOME=str(root / 'config'),
            XDG_DATA_HOME=str(root / 'data'),
            XDG_CACHE_HOME=str(root / 'cache'),
            XDG_RUNTIME_DIR=str(runtime),
            XDG_CURRENT_DESKTOP='vntext-test',
            XDG_SESSION_TYPE='wayland',
            QT_QPA_PLATFORM='offscreen',
            LIBGL_ALWAYS_SOFTWARE='1',
            GDK_BACKEND='wayland',
            GSK_RENDERER='cairo',
            GTK_A11Y='none',
            GIO_USE_VFS='local',
            GTK_USE_PORTAL='0',
            VNTEXT_UI_TEST='isolated',
            VNTEXT_UI_OUTPUT=str(output.resolve()),
        )
        if env.get('VNTEXT_UI_POINTER'):
            # Only this private KWin process accepts the guarded test input helper.
            env['KWIN_WAYLAND_NO_PERMISSION_CHECKS'] = '1'
        runner = root / 'session.py'
        runner.write_text(
            '#!/usr/bin/env python3\n'
            'import os, subprocess, json\n'
            'os.environ["WAYLAND_DISPLAY"] = "vntext-uiux-test"\n'
            f'command = {str([str(binary), "--exact", "overlay::tests::uiux_smoke", "--ignored", "--nocapture"])}\n'
            'try:\n'
            '    code = subprocess.run(command, timeout=40).returncode\n'
            '    if code == 0:\n'
            '        command[2] = "launcher::tests::uiux_smoke"\n'
            '        code = subprocess.run(command, timeout=10).returncode\n'
            'except subprocess.TimeoutExpired:\n'
            '    code = 124\n'
            f'with open({str(str(result.resolve()))!r}, "w") as stream:\n'
            '    json.dump({"returncode": code}, stream)\n'
            'raise SystemExit(code)\n'
        )
        runner.chmod(0o700)
        width, height = size
        command = [
            'dbus-run-session', '--', 'kwin_wayland', '--virtual',
            '--socket', 'vntext-uiux-test', '--width', str(width),
            '--height', str(height), '--no-lockscreen',
            '--no-global-shortcuts', '--no-kactivities',
            '--exit-with-session', str(runner),
        ]
        with (output / 'session.log').open('w') as log:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=log)
            try:
                process.wait(timeout=55)
            except subprocess.TimeoutExpired:
                for pid in children(process.pid):
                    try:
                        os.kill(pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                process.terminate()
                process.wait(timeout=5)
                raise RuntimeError(f'Private compositor timed out; see {output}')
        if not result.exists() or json.loads(result.read_text())['returncode'] != 0:
            raise RuntimeError(f'GUI checks failed; see {output / "session.log"}')
        print(f'{width}x{height}: PASS; captures in {output}', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('test_binary', type=Path)
    parser.add_argument('--output', type=Path, default=Path('target/diagnostics/uiux'))
    parser.add_argument('--pointer', action='store_true', help='Also test pointer events in private KWin (requires cc and libwayland-client headers)')
    args = parser.parse_args()
    if args.pointer:
        args.output.mkdir(parents=True, exist_ok=True)
        helper = args.output.resolve() / 'private-pointer'
        subprocess.run(['cc', str(Path(__file__).with_name('uiux-pointer.c')), '-o', str(helper), '-lwayland-client', '-lm'], check=True)
        os.environ['VNTEXT_UI_POINTER'] = str(helper)
    for dimensions in [(1024, 768), (800, 600)]:
        run(args.test_binary.resolve(), dimensions,
            args.output / f'{dimensions[0]}x{dimensions[1]}')
