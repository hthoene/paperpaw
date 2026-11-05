"""Bounded process supervision for #716. Importing this module opens nothing."""
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import time

FATAL = re.compile(
    r"lost (?:connection|link)|retrying|reconnecting|trying again|waiting for .*reconnect|"
    r"traceback|\bpanic\b|panicked|watchdog.*reset|\brst:|"
    r"(?m:^PP:(?:ERR|BUSY|PANIC)(?:\s|$))|PP:TEXTPROBE_(?:ERROR|UNSUPPORTED)\b|"
    r"serialexception|connection (?:reset|refused|closed)|"
    r"(?:error|err|busy):|failed to connect|negotiation.*fail",
    re.IGNORECASE,
)


class Stopped(RuntimeError):
    """No later operation may be started after this failure."""


class Child:
    """One child process group. Only this runner's group is killed."""

    def __init__(self, argv):
        self.process = subprocess.Popen(
            argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            start_new_session=True, bufsize=0,
        )
        self.selector = None
        try:
            self.selector = selectors.DefaultSelector()
            self.selector.register(self.process.stdout, selectors.EVENT_READ)
        except BaseException:
            self.kill()
            self.process.wait()
            self.process.stdout.close()
            if self.selector is not None:
                self.selector.close()
            raise

    def read(self, timeout):
        if not self.selector.select(timeout):
            return b""
        return os.read(self.process.stdout.fileno(), 65536)

    def poll(self):
        return self.process.poll()

    def kill(self):
        try:
            os.killpg(self.process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def close(self):
        if self.process.poll() is None:
            self.kill()
        self.process.wait()
        self.selector.close()
        self.process.stdout.close()


def supervise(child, seconds, log, consume=None, clock=time.monotonic, boot=False, deadline=None):
    """One absolute deadline, including process exit and both protocol markers.

    A partial loss/error line stops immediately, without waiting for newline.
    A timeout kills the local worker; it cannot preempt masked device execution.
    """
    deadline = clock() + seconds if deadline is None else deadline
    pending = b""
    output = bytearray()
    try:
        while True:
            remaining = deadline - clock()
            if remaining <= 0:
                raise Stopped(f"deadline exceeded ({seconds}s)")
            chunk = child.read(min(0.02, remaining))
            if clock() >= deadline:
                raise Stopped(f"deadline exceeded ({seconds}s)")
            if chunk:
                output.extend(chunk)
                pending += chunk
                # Bound malformed/no-newline output as well as execution time.
                if len(pending) > 65536 or len(output) > 4 * 1024 * 1024:
                    raise Stopped("output limit exceeded")
                scan = pending.decode("utf-8", "replace")
                if boot:
                    scan = re.sub(r"(?m)^rst:[^\n]*", "", scan)
                if FATAL.search(scan):
                    raise Stopped("first reported loss/error/retry/reset")
                while b"\n" in pending:
                    raw, pending = pending.split(b"\n", 1)
                    if consume:
                        consume(raw.decode("utf-8", "strict").rstrip("\r"))
            code = child.poll()
            if code is not None and not chunk:
                if code != 0 or pending.strip():
                    raise Stopped(f"child exit {code} or unterminated output")
                return output.decode("utf-8", "strict")
    except BaseException:
        child.kill()
        raise
    finally:
        try:
            child.close()
        finally:
            # Disk I/O cannot delay a running hardware worker's deadline.
            # A log failure happens only after close; caller permanently stops.
            log.write(output)
            log.flush()


class Processes:
    """Production process factory; constructed only for explicit --execute."""

    def __init__(self, python, worker, remote, directory, approval):
        self.python, self.worker, self.remote = python, worker, remote
        self.directory, self.approval = Path(directory), approval
        self.calls = []

    def __call__(self, action, *args):
        self.calls.append((action, args))
        return Child([
            self.python, "-u", "-I", str(self.worker), "--action", action,
            "--remote", self.remote, "--directory", str(self.directory),
            "--approval-ref", self.approval, *map(str, args),
        ])
