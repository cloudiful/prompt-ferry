#!/usr/bin/env python3
"""Own one per-run test directory, run a command inside it, then remove it.

Requires Python >= 3.11 (stdlib `tomllib` reads Cargo config files).

`scripts/ci/with-test-env.sh` execs this helper instead of the command
directly, so every guarded invocation — full `run-tests.sh` runs and focused
commands alike — gets exactly one fresh directory. The helper:

- creates `run-<uuid>` under the already-exported absolute
  `PROMPT_FERRY_TEST_TMPDIR` root (never the root itself, never anywhere
  else), with a private mode, an owner marker, and a held lock;
- reports (never deletes) residual `run-*` directories left by a SIGKILLed or
  crashed run;
- runs the command in its own process group with `TMPDIR` and
  `PROMPT_FERRY_TEST_TMPDIR` pointed at the owned directory; when the effective
  Cargo compiler wrapper (`RUSTC_WRAPPER` / `RUSTC_WORKSPACE_WRAPPER`,
  resolved from env then Cargo config files) names the sccache executable, the
  key is set empty in the owned child environment only — empty overrides Cargo
  config and disables the wrapper, so no cache daemon is created or relied upon
  with the disposable directory. Parent env, config files, and the shared
  daemon are never touched;
- forwards SIGINT/SIGTERM to the owned group, waits for the group to drain
  (escalating to SIGKILL only inside the owned group when needed), preserves
  the exit code or fatal signal, then removes exactly the owned directory.

Discovery root for Cargo config is the helper's own working directory;
commands that change directory before invoking cargo, Cargo `--config` CLI
overrides, and config `include` directives are not inspected (documented as
unsupported, never guessed). Standard library only. No secrets are ever
printed: diagnostics name keys and config paths, never environment values.
"""

from __future__ import annotations

import errno
import fcntl
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
import uuid

RUN_PREFIX = "run-"
MARKER_NAME = ".run-owner.json"
LOCK_NAME = ".run.lock"

# Bounded waits so a wedged group fails instead of hanging the suite.
SIGNAL_GRACE_SECONDS = 10.0
KILL_GRACE_SECONDS = 5.0
GROUP_DRAIN_SECONDS = 10.0
POLL_INTERVAL_SECONDS = 0.05


class LifecycleError(Exception):
    """A prerequisite or cleanup step the helper refuses to work around."""


def eprint(message: str) -> None:
    print(f"test-run-tmp: {message}", file=sys.stderr)


def load_root() -> str:
    """Returns the configured absolute test root, or raises."""
    root = os.environ.get("PROMPT_FERRY_TEST_TMPDIR", "")
    if not root:
        raise LifecycleError(
            "PROMPT_FERRY_TEST_TMPDIR is not exported; refusing to invent a root"
        )
    if not os.path.isabs(root):
        raise LifecycleError("the test root must be absolute")
    if os.path.islink(root):
        raise LifecycleError("the test root must not be a symlink")
    if not os.path.isdir(root):
        raise LifecycleError("the test root is not a directory")
    return root


def is_own_run_dir(root: str, path: str, token: str) -> bool:
    """Re-validates a directory before removal: inside root, real dir, ours."""
    if os.path.islink(path) or not os.path.isdir(path):
        return False
    if os.path.commonpath([root, path]) != root:
        return False
    if os.path.basename(path) == os.path.basename(root):
        return False
    try:
        with open(os.path.join(path, MARKER_NAME), encoding="utf-8") as handle:
            marker = json.load(handle)
    except (OSError, ValueError):
        return False
    return isinstance(marker, dict) and marker.get("token") == token


def report_residuals(root: str) -> list[str]:
    """Reports stale `run-*` directories without touching any of them.

    A residual whose lock can be taken has no live owner (SIGKILL or host
    crash); anything else is left alone silently. Only metadata is reported:
    names and ages, never contents.
    """
    reported: list[str] = []
    try:
        entries = sorted(os.listdir(root))
    except OSError as error:
        raise LifecycleError(f"cannot list the test root: {error}") from None
    for entry in entries:
        if not entry.startswith(RUN_PREFIX):
            continue
        path = os.path.join(root, entry)
        if os.path.islink(path) or not os.path.isdir(path):
            continue
        lock_path = os.path.join(path, LOCK_NAME)
        try:
            descriptor = os.open(lock_path, os.O_RDONLY)
        except OSError:
            continue
        try:
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as error:
                if error.errno in (errno.EACCES, errno.EAGAIN):
                    continue
                raise
            try:
                age = time.time() - os.lstat(path).st_mtime
            except OSError:
                age = -1.0
            reported.append(entry)
            eprint(
                f"residual run directory from an earlier run (not removed): "
                f"{entry} age {age:.0f}s"
            )
        finally:
            os.close(descriptor)
    return reported


def create_run_dir(root: str) -> tuple[str, str, int]:
    """Creates, marks, and locks one owned run directory.

    Returns the path, the owner token, and the held lock descriptor. The lock
    stays held until the process exits, so a later invocation can tell a live
    directory from a crashed one.
    """
    try:
        path = tempfile.mkdtemp(prefix=RUN_PREFIX, dir=root)
    except OSError as error:
        raise LifecycleError(f"cannot create a run directory: {error}") from None
    if os.path.islink(path) or os.path.commonpath([root, path]) != root:
        raise LifecycleError("the run directory escaped the test root")
    token = uuid.uuid4().hex
    try:
        with open(os.path.join(path, MARKER_NAME), "w", encoding="utf-8") as handle:
            json.dump({"token": token, "pid": os.getpid()}, handle)
        os.chmod(os.path.join(path, MARKER_NAME), 0o600)
        descriptor = os.open(os.path.join(path, LOCK_NAME), os.O_CREAT | os.O_RDWR, 0o600)
    except OSError as error:
        raise LifecycleError(f"cannot mark the run directory: {error}") from None
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError as error:
        os.close(descriptor)
        raise LifecycleError(f"cannot lock the run directory: {error}") from None
    return path, token, descriptor


def group_alive(pgid: int) -> bool:
    """True while any process remains in the group; never signals anyone."""
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def kill_group(pgid: int, signum: int) -> None:
    try:
        os.killpg(pgid, signum)
    except (ProcessLookupError, PermissionError):
        pass


def wait_for_exit(process: subprocess.Popen[bytes], deadline: float) -> bool:
    """Waits until the deadline; True when the child reaped in time."""
    while time.monotonic() < deadline:
        if process.poll() is not None:
            return True
        time.sleep(POLL_INTERVAL_SECONDS)
    return process.poll() is not None


def wrapper_names_sccache(value: str) -> bool:
    """True when a Cargo wrapper value names the sccache executable.

    Cargo wrapper values are program paths, never shell commands, so the
    whole value is the path and only its exact basename counts:
    `/usr/bin/sccache` and `/opt/my dir/sccache` match, while a blank value,
    `sccache-foo`, or `sccache --flag` never match.
    """
    return bool(value) and os.path.basename(value) == "sccache"


def read_build_value(config_path: str, key: str) -> str | None:
    """Reads one Cargo `[build]` string value without executing anything.

    Returns None when the file is missing, unparsable, or the key is absent
    or not a string. Only the two Cargo config filenames are ever read.
    """
    try:
        with open(config_path, "rb") as handle:
            data = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return None
    build = data.get("build")
    if not isinstance(build, dict):
        return None
    value = build.get(key)
    return value if isinstance(value, str) else None


def dir_build_value(dot_cargo: str, key: str) -> tuple[str, str] | None:
    """One directory's Cargo config value for a `[build]` key, if any.

    In one directory the legacy `config` file shadows `config.toml`
    entirely (observed stable-cargo behavior: cargo warns it is using
    `config` and never consults the sibling file).
    """
    legacy = os.path.join(dot_cargo, "config")
    if os.path.isfile(legacy):
        value = read_build_value(legacy, key)
        return (value, legacy) if value is not None else None
    current = os.path.join(dot_cargo, "config.toml")
    if os.path.isfile(current):
        value = read_build_value(current, key)
        return (value, current) if value is not None else None
    return None


def config_build_value(
    start: str, cargo_home: str, key: str
) -> tuple[str, str] | None:
    """First Cargo config value for a `[build]` key, highest precedence first.

    Walks `start` upward (nearest directory wins), then the CARGO_HOME global
    config last. Read-only: never writes, and never follows Cargo `--config`
    CLI overrides or `include` directives.
    """
    directory = os.path.abspath(start)
    while True:
        found = dir_build_value(os.path.join(directory, ".cargo"), key)
        if found is not None:
            return found
        parent = os.path.dirname(directory)
        if parent == directory:
            break
        directory = parent
    if cargo_home:
        found = dir_build_value(cargo_home, key)
        if found is not None:
            return found
    return None


def default_cargo_home(env: dict[str, str]) -> str:
    """CARGO_HOME from the child env, else the ambient default location."""
    if env.get("CARGO_HOME"):
        return env["CARGO_HOME"]
    return os.path.join(os.path.expanduser("~"), ".cargo")


def effective_wrapper(
    env: dict[str, str],
    cwd: str,
    cargo_home: str,
    env_order: tuple[str, str],
    config_key: str,
) -> tuple[str | None, str]:
    """Effective Cargo wrapper value and the source that provided it.

    The first set key in `env_order` wins (a set-but-empty value means the
    wrapper is already disabled); otherwise the Cargo config walk decides.
    Returns (None, "absent") when nothing sets it.
    """
    for key in env_order:
        if key in env:
            return env[key], f"env:{key}"
    found = config_build_value(cwd, cargo_home, config_key)
    if found is not None:
        return found[0], f"config:{found[1]}"
    return None, "absent"


def neutralize_sccache_wrappers(
    env: dict[str, str], cwd: str, cargo_home: str
) -> list[str]:
    """Empties wrapper keys in an owned child env when sccache is effective.

    Per the Cargo Book, setting the wrapper to the empty string overrides the
    config and disables the wrapper. The key is set empty, never deleted:
    deleting would fall back to a file-configured wrapper. An explicitly empty
    inherited value already disables the wrapper and is left alone; ordinary
    wrappers are preserved. Only the given dict is mutated: the parent
    environment, config files, and the shared daemon are never touched.
    """
    neutralized: list[str] = []
    pairs = (
        (
            ("RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"),
            "rustc-wrapper",
            "RUSTC_WRAPPER",
        ),
        (
            ("RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"),
            "rustc-workspace-wrapper",
            "RUSTC_WORKSPACE_WRAPPER",
        ),
    )
    for env_order, config_key, primary in pairs:
        value, _origin = effective_wrapper(
            env, cwd, cargo_home, env_order, config_key
        )
        if value and wrapper_names_sccache(value):
            eprint(
                f"disabling {primary} for the owned run "
                f"(the effective wrapper is sccache; empty overrides Cargo config)"
            )
            env[primary] = ""
            neutralized.append(primary)
    return neutralized


def run_owned_command(root: str, argv: list[str]) -> int:
    """Runs argv inside a fresh owned directory; returns the exit status.

    Exit statuses: the command's own code on failure; 0 on full success; 1
    when only cleanup failed. A command killed by a signal kills this helper
    with the same signal after cleanup, so callers still see the signal.
    """
    if not argv:
        eprint("usage: test_run_tmp.py -- <command> [args...]")
        return 2
    report_residuals(root)
    run_dir, token, _lock = create_run_dir(root)

    env = dict(os.environ)
    env["TMPDIR"] = run_dir
    env["PROMPT_FERRY_TEST_TMPDIR"] = run_dir
    env["PROMPT_FERRY_RUN_DIR"] = run_dir
    neutralize_sccache_wrappers(env, os.getcwd(), default_cargo_home(env))

    try:
        child = subprocess.Popen(argv, env=env, start_new_session=True)
    except OSError as error:
        eprint(f"cannot start the command: {error}")
        remove_run_dir(root, run_dir, token)
        return 127
    pgid = child.pid

    pending: list[int] = []

    def forward(signum: int, _frame: object) -> None:
        pending.append(signum)
        kill_group(pgid, signum)

    previous_int = signal.signal(signal.SIGINT, forward)
    previous_term = signal.signal(signal.SIGTERM, forward)
    try:
        # Drain forwarded signals: each one is re-sent to the owned group
        # until the child exits, then escalate inside the owned group only.
        while child.poll() is None:
            time.sleep(POLL_INTERVAL_SECONDS)
            if pending:
                pending.clear()
                if not wait_for_exit(child, time.monotonic() + SIGNAL_GRACE_SECONDS):
                    break
        if child.poll() is None:
            kill_group(pgid, signal.SIGTERM)
            if not wait_for_exit(child, time.monotonic() + SIGNAL_GRACE_SECONDS):
                kill_group(pgid, signal.SIGKILL)
                child.wait()
        exit_signal = None
        if child.returncode is not None and child.returncode < 0:
            exit_signal = -child.returncode

        # No cleanup before the owned group is gone: grandchildren that
        # outlive the child get a bounded drain, then group SIGTERM/SIGKILL.
        if group_alive(pgid):
            deadline = time.monotonic() + GROUP_DRAIN_SECONDS
            while group_alive(pgid) and time.monotonic() < deadline:
                time.sleep(POLL_INTERVAL_SECONDS)
        if group_alive(pgid):
            kill_group(pgid, signal.SIGTERM)
            deadline = time.monotonic() + KILL_GRACE_SECONDS
            while group_alive(pgid) and time.monotonic() < deadline:
                time.sleep(POLL_INTERVAL_SECONDS)
        group_gone = not group_alive(pgid)
        if not group_gone:
            kill_group(pgid, signal.SIGKILL)
            deadline = time.monotonic() + KILL_GRACE_SECONDS
            while group_alive(pgid) and time.monotonic() < deadline:
                time.sleep(POLL_INTERVAL_SECONDS)
            group_gone = not group_alive(pgid)

        status = child.returncode if child.returncode is not None else 127
        if not group_gone:
            eprint("the owned process group did not terminate; keeping the run directory")
            return status if isinstance(status, int) and status != 0 else 1
        cleanup_ok = remove_run_dir(root, run_dir, token)
        if exit_signal is not None:
            # Die by the same signal after cleanup so the caller sees it.
            # SIGKILL/SIGSTOP admit no handler call, but neither can be
            # caught, so killing ourselves with them always terminates.
            if exit_signal not in (signal.SIGKILL, signal.SIGSTOP):
                signal.signal(exit_signal, signal.SIG_DFL)
            os.kill(os.getpid(), exit_signal)
            return 128 + exit_signal
        if not cleanup_ok:
            # Preserve the command's own failure: 1 only when the command
            # itself succeeded but cleanup failed.
            return status if isinstance(status, int) and status != 0 else 1
        return status if isinstance(status, int) else 127
    finally:
        signal.signal(signal.SIGINT, previous_int)
        signal.signal(signal.SIGTERM, previous_term)


def remove_run_dir(root: str, run_dir: str, token: str) -> bool:
    """Removes exactly the owned run directory; True on success."""
    if not is_own_run_dir(root, run_dir, token):
        eprint("refusing to remove a directory that is not the owned run directory")
        return False
    try:
        shutil.rmtree(run_dir)
    except OSError as error:
        eprint(f"cannot remove the run directory: {error}")
        return False
    return True


def main(argv: list[str]) -> int:
    args = list(argv)
    if args[:1] == ["--"]:
        args = args[1:]
    if not args:
        eprint("usage: test_run_tmp.py -- <command> [args...]")
        return 2
    # A nested invocation (re-entered wrapper) must not mint a second
    # directory: replace this process with the command so the outer run keeps
    # owning the single directory for this process tree.
    if os.environ.get("PROMPT_FERRY_RUN_DIR"):
        try:
            os.execvp(args[0], args)
        except OSError as error:
            eprint(f"cannot start the command: {error}")
            return 127
    try:
        root = load_root()
    except LifecycleError as error:
        eprint(str(error))
        return 2
    try:
        return run_owned_command(root, args)
    except LifecycleError as error:
        eprint(str(error))
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
