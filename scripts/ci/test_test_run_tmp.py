#!/usr/bin/env python3
"""Unit checks for the per-run test-directory lifecycle helper.

Synthetic temporary roots only: no services, no repository `.env`, no real
credentials, and no object-store or database contact. Signal and process-group
cases target only processes this suite spawned. Run with `uv run --no-project
python scripts/ci/test_test_run_tmp.py` from the repository root.
"""

from __future__ import annotations

import contextlib
import io
import os
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import test_run_tmp as helper

PASS = 0
FAIL = 0


def check(label: str, ok: bool, detail: str = "") -> None:
    global PASS, FAIL
    if ok:
        PASS += 1
        print(f"ok   {label}")
    else:
        FAIL += 1
        print(f"FAIL {label}")
        if detail:
            print(f"     {detail}")


def make_root() -> str:
    root = tempfile.mkdtemp(prefix="run-tmp-test-root-")
    with open(os.path.join(root, "legacy-canary.txt"), "w", encoding="utf-8") as handle:
        handle.write("untouched\n")
    return root


def run_dirs(root: str) -> list[str]:
    return sorted(
        entry
        for entry in os.listdir(root)
        if entry.startswith(helper.RUN_PREFIX)
        and os.path.isdir(os.path.join(root, entry))
        and not os.path.islink(os.path.join(root, entry))
    )


def test_normal_exit_cleans_sqlite_sidecars() -> None:
    root = make_root()
    try:
        status = helper.run_owned_command(root, [
            "bash", "-c",
            'touch "$TMPDIR/normal.sqlite" "$TMPDIR/normal.sqlite-wal" '
            '"$TMPDIR/normal.sqlite-shm"; exit 0',
        ])
        check("a passing command exits zero", status == 0, f"got {status}")
        check("the owned directory is removed", run_dirs(root) == [], str(run_dirs(root)))
        check("the legacy canary is untouched",
              os.path.isfile(os.path.join(root, "legacy-canary.txt")))
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_failure_exit_is_preserved_and_cleaned() -> None:
    root = make_root()
    try:
        status = helper.run_owned_command(root, ["bash", "-c", "exit 7"])
        check("a failing command keeps its exit code", status == 7, f"got {status}")
        check("the owned directory is removed after failure", run_dirs(root) == [])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_argv_with_spaces_survives() -> None:
    base = tempfile.mkdtemp(prefix="run-tmp-test-root with space-")
    root = os.path.join(base, "nested root")
    os.makedirs(root)
    with open(os.path.join(root, "legacy-canary.txt"), "w", encoding="utf-8") as handle:
        handle.write("untouched\n")
    try:
        marker = os.path.join(root, "argv probe.txt")
        status = helper.run_owned_command(root, ["bash", "-c", 'echo ok > "$0"', marker])
        check("argv with spaces runs", status == 0, f"got {status}")
        check("the spaced side effect is outside the removed directory",
              os.path.isfile(marker))
        check("the owned directory is removed", run_dirs(root) == [])
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_lingering_grandchild_is_reaped_before_cleanup() -> None:
    root = make_root()
    pid_file = os.path.join(root, "grandchild.pid")
    try:
        previous, helper.GROUP_DRAIN_SECONDS = helper.GROUP_DRAIN_SECONDS, 1.0
        try:
            status = helper.run_owned_command(root, [
                "bash", "-c", 'sleep 60 & echo $! > "$0"; exit 0', pid_file,
            ])
        finally:
            helper.GROUP_DRAIN_SECONDS = previous
        check("the command exits zero", status == 0, f"got {status}")
        with open(pid_file, encoding="utf-8") as handle:
            grandchild = int(handle.read().strip())
        try:
            os.kill(grandchild, 0)
            alive = True
        except ProcessLookupError:
            alive = False
        check("the lingering grandchild is gone before cleanup", not alive)
        check("the owned directory is removed", run_dirs(root) == [])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def cli(args: list[str], root: str, extra_env: dict[str, str] | None = None,
        timeout: float = 90.0, cwd: str | None = None) -> subprocess.CompletedProcess[str]:
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "PROMPT_FERRY_TEST_TMPDIR": root,
    }
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        [sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                      "test_run_tmp.py"), "--", *args],
        env=env, capture_output=True, text=True, timeout=timeout, cwd=cwd,
    )


def test_signal_is_forwarded_and_exit_code_preserved() -> None:
    root = make_root()
    try:
        child = subprocess.Popen(
            [sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                          "test_run_tmp.py"), "--",
             "bash", "-c", 'trap \'kill $SLEEP_PID 2>/dev/null; exit 42\' TERM; '
                           'sleep 30 & SLEEP_PID=$!; wait'],
            env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                 "PROMPT_FERRY_TEST_TMPDIR": root},
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
        )
        try:
            # Give the helper time to create the run directory and spawn.
            deadline = time.monotonic() + 15.0
            while not run_dirs(root) and time.monotonic() < deadline:
                time.sleep(0.05)
            child.send_signal(signal.SIGTERM)
            _, stderr = child.communicate(timeout=60.0)
        except Exception:
            child.kill()
            raise
        check("a TERM-forwarded command keeps its exit code",
              child.returncode == 42, f"got {child.returncode}: {stderr[-500:]}")
        check("the owned directory is removed after the signal", run_dirs(root) == [])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_unresponsive_group_is_escalated_inside_itself() -> None:
    root = make_root()
    try:
        child = subprocess.Popen(
            [sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                          "test_run_tmp.py"), "--",
             "bash", "-c", 'trap "" TERM; sleep 30'],
            env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                 "PROMPT_FERRY_TEST_TMPDIR": root},
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
        )
        try:
            deadline = time.monotonic() + 15.0
            while not run_dirs(root) and time.monotonic() < deadline:
                time.sleep(0.05)
            child.send_signal(signal.SIGTERM)
            _, stderr = child.communicate(timeout=120.0)
        except Exception:
            child.kill()
            raise
        # TERM is ignored, so the helper escalates inside the owned group and
        # then dies by the same SIGKILL it used last.
        check("an ignoring group ends killed", child.returncode == -signal.SIGKILL,
              f"got {child.returncode}: {stderr[-500:]}")
        check("the owned directory is removed after escalation", run_dirs(root) == [])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_concurrent_runs_stay_isolated() -> None:
    root = make_root()
    helper_path = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                               "test_run_tmp.py")
    base_env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                "PROMPT_FERRY_TEST_TMPDIR": root}
    first = subprocess.Popen(
        [sys.executable, helper_path, "--",
         "bash", "-c", 'sleep 0.2; touch "$TMPDIR/job-1.tmp"; exit 0'],
        env=base_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    second = subprocess.Popen(
        [sys.executable, helper_path, "--",
         "bash", "-c", 'sleep 0.2; touch "$TMPDIR/job-2.tmp"; exit 0'],
        env=base_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        check("concurrent runs both succeed",
              first.wait(timeout=90.0) == 0 and second.wait(timeout=90.0) == 0,
              f"got {(first.returncode, second.returncode)}")
        check("neither run deletes the other or the root", run_dirs(root) == [])
        check("the legacy canary is untouched",
              os.path.isfile(os.path.join(root, "legacy-canary.txt")))
    finally:
        for child in (first, second):
            try:
                child.kill()
            except (ProcessLookupError, OSError):
                pass
        shutil.rmtree(root, ignore_errors=True)


def test_removal_refuses_foreign_paths() -> None:
    root = make_root()
    try:
        owned, token, descriptor = helper.create_run_dir(root)
        try:
            check("a symlink is never removed", not helper.remove_run_dir(
                root, os.path.join(root, "link"), "x"))
            check("an outside path is never removed",
                  not helper.remove_run_dir(root, tempfile.gettempdir(), token))
            check("a wrong token is never removed",
                  not helper.remove_run_dir(root, owned, "wrong-token"))
            check("the root itself is never removed",
                  not helper.remove_run_dir(root, root, token))
            check("the owned directory is removed",
                  helper.remove_run_dir(root, owned, token))
            check("the owned directory is gone", not os.path.exists(owned))
        finally:
            try:
                os.close(descriptor)
            except OSError:
                pass
        link = os.path.join(root, "run-evil")
        try:
            os.symlink("/", link)
        except OSError:
            pass
        if os.path.islink(link):
            check("a run- symlink is never removed",
                  not helper.remove_run_dir(root, link, token))
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_cleanup_failure_is_nonzero() -> None:
    if hasattr(os, "geteuid") and os.geteuid() == 0:
        print("skip cleanup-failure case: running as root ignores permission bits")
        return
    root = make_root()
    try:
        owned, token, descriptor = helper.create_run_dir(root)
        try:
            os.chmod(owned, 0o555)
            ok = helper.remove_run_dir(root, owned, token)
            check("an undeletable directory fails instead of pretending",
                  not ok)
        finally:
            try:
                os.close(descriptor)
            except OSError:
                pass
            os.chmod(owned, 0o700)
            shutil.rmtree(owned, ignore_errors=True)
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_sigkill_residual_is_reported_not_deleted() -> None:
    root = make_root()
    helper_path = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                               "test_run_tmp.py")
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"),
           "PROMPT_FERRY_TEST_TMPDIR": root}
    child = subprocess.Popen([sys.executable, helper_path, "--", "sleep", "5"],
                             env=env, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 15.0
        dirs = run_dirs(root)
        while not dirs and time.monotonic() < deadline:
            time.sleep(0.05)
            dirs = run_dirs(root)
        check("the run directory exists while owned", len(dirs) == 1, str(dirs))
        if not dirs:
            return
        child.kill()  # SIGKILL the helper: no cleanup can run.
        child.wait(timeout=15.0)
        check("the residual survives its killed owner", run_dirs(root) == dirs)
        # Let the orphaned sleep exit on its own before the next invocation,
        # so only the residual directory (not a live group) is reported.
        time.sleep(6.0)
        completed = cli(["true"], root)
        check("the next run still succeeds", completed.returncode == 0,
              completed.stderr[-500:])
        check("the next run reports the residual",
              "residual run directory" in completed.stderr, completed.stderr[-500:])
        check("the residual is not auto-deleted", run_dirs(root) == dirs)
    finally:
        try:
            child.kill()
        except (ProcessLookupError, OSError):
            pass
        shutil.rmtree(root, ignore_errors=True)


def test_nested_invocation_execs_without_a_new_directory() -> None:
    root = make_root()
    try:
        completed = cli(["true"], root,
                        extra_env={"PROMPT_FERRY_RUN_DIR": "/nonexistent-outer-run"})
        check("a nested invocation runs the command", completed.returncode == 0,
              completed.stderr[-300:])
        check("a nested invocation mints no directory", run_dirs(root) == [])
        empty = cli([], root, extra_env={"PROMPT_FERRY_RUN_DIR": "x"})
        check("an empty nested command fails closed", empty.returncode == 2)
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_missing_root_fails_closed() -> None:
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin")}
    completed = subprocess.run(
        [sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                      "test_run_tmp.py"), "--", "true"],
        env=env, capture_output=True, text=True, timeout=30.0,
    )
    check("a missing root fails closed", completed.returncode == 2,
          completed.stderr[-300:])
    check("the failure names the setting",
          "PROMPT_FERRY_TEST_TMPDIR" in completed.stderr)


def test_output_never_carries_environment_secrets() -> None:
    root = make_root()
    try:
        completed = cli(["bash", "-c", "exit 3"], root,
                        extra_env={"PROMPT_FERRY_TEST_S3_SECRET_KEY": "s3-fixture-secret"})
        check("a failing command keeps its code", completed.returncode == 3)
        combined = completed.stdout + completed.stderr
        check("no secret reaches the output", "s3-fixture-secret" not in combined,
              combined[-500:])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def probe_wrappers() -> list[str]:
    return ["bash", "-c",
            'echo \"wrapper=${RUSTC_WRAPPER-unset} workspace=${RUSTC_WORKSPACE_WRAPPER-unset}\"']


def test_sccache_env_wrapper_is_emptied_not_removed() -> None:
    root = make_root()
    home = tempfile.mkdtemp(prefix="sccache-cli-home-")
    try:
        # A hermetic CARGO_HOME (no config inside) proves the outcome comes
        # from the env keys, not ambient Cargo config.
        completed = cli(probe_wrappers(), root,
                        extra_env={"RUSTC_WRAPPER": "/usr/bin/sccache",
                                   "RUSTC_WORKSPACE_WRAPPER": "sccache",
                                   "CARGO_HOME": home})
        check("the command still succeeds", completed.returncode == 0,
              completed.stderr[-300:])
        # Set-but-empty expands to "" under `${VAR-unset}`; "unset" would mean
        # the key was deleted (which would fall back to a file wrapper).
        check("an sccache wrapper is emptied, not removed",
              "wrapper= workspace=" in completed.stdout,
              completed.stdout[-300:])
        check("the parent environment is unchanged",
              "RUSTC_WRAPPER" not in os.environ
              and "RUSTC_WORKSPACE_WRAPPER" not in os.environ)
        check("the owned directory is removed", run_dirs(root) == [])
    finally:
        shutil.rmtree(root, ignore_errors=True)
        shutil.rmtree(home, ignore_errors=True)


def test_global_config_sccache_is_overridden_empty() -> None:
    base = tempfile.mkdtemp(prefix="sccache-cli-config-")
    try:
        home = os.path.join(base, "home")
        work = os.path.join(base, "work")
        root = os.path.join(base, "runroot")
        os.makedirs(home)
        os.makedirs(work)
        os.makedirs(root)
        with open(os.path.join(home, "config.toml"),
                  "w", encoding="utf-8") as handle:
            handle.write('[build]\nrustc-wrapper = "sccache"\n')
        completed = cli(probe_wrappers(), root,
                        extra_env={"CARGO_HOME": home}, cwd=work)
        check("the command still succeeds", completed.returncode == 0,
              completed.stderr[-300:])
        check("a global-config sccache is overridden empty in the owned child",
              "wrapper= workspace=unset" in completed.stdout,
              completed.stdout[-300:])
        check("the parent environment is unchanged",
              "RUSTC_WRAPPER" not in os.environ)
        check("the owned directory is removed", run_dirs(root) == [])
    finally:
        shutil.rmtree(base, ignore_errors=True)


def write_home_config(home: str, pairs: dict[str, str],
                    legacy: bool = False) -> None:
    # The global config lives directly in CARGO_HOME (config.toml), never
    # under a nested .cargo directory.
    os.makedirs(home, exist_ok=True)
    lines = ["[build]"]
    for key, value in pairs.items():
        lines.append(f'{key} = "{value}"')
    with open(os.path.join(home, "config" if legacy else "config.toml"),
              "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def write_build_config(directory: str, pairs: dict[str, str],
                       legacy: bool = False) -> None:
    dot = os.path.join(directory, ".cargo")
    os.makedirs(dot, exist_ok=True)
    lines = ["[build]"]
    for key, value in pairs.items():
        lines.append(f'{key} = "{value}"')
    with open(os.path.join(dot, "config" if legacy else "config.toml"),
              "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def neutralize(env: dict[str, str], cwd: str, home: str) -> dict[str, str]:
    owned = dict(env)
    helper.neutralize_sccache_wrappers(owned, cwd, home)
    return owned


def test_effective_wrapper_prefers_env_over_config() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        write_build_config(cwd, {"rustc-wrapper": "sccache"})
        owned = neutralize({"RUSTC_WRAPPER": "/opt/custom-wrapper"}, cwd, home)
        check("an env ordinary wrapper beats a config sccache",
              owned == {"RUSTC_WRAPPER": "/opt/custom-wrapper"}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_bare_env_beats_cargo_build_env() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        owned = neutralize({"RUSTC_WRAPPER": "/opt/custom-wrapper",
                            "CARGO_BUILD_RUSTC_WRAPPER": "sccache"}, cwd, home)
        check("a shadowed CARGO_BUILD sccache stays untouched",
              owned == {"RUSTC_WRAPPER": "/opt/custom-wrapper",
                        "CARGO_BUILD_RUSTC_WRAPPER": "sccache"}, str(owned))
        owned = neutralize({"RUSTC_WRAPPER": "sccache",
                            "CARGO_BUILD_RUSTC_WRAPPER": "/opt/custom-wrapper"},
                           cwd, home)
        check("a winning bare sccache is emptied, the other key kept",
              owned == {"RUSTC_WRAPPER": "",
                        "CARGO_BUILD_RUSTC_WRAPPER": "/opt/custom-wrapper"},
              str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_explicit_empty_env_is_already_disabled() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        write_home_config(home, {"rustc-wrapper": "sccache"})
        owned = neutralize({"RUSTC_WRAPPER": ""}, cwd, home)
        check("an explicit empty wrapper is left alone",
              owned == {"RUSTC_WRAPPER": ""}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_legacy_config_shadows_toml() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        write_build_config(cwd, {"rustc-wrapper": "/opt/custom-wrapper"},
                           legacy=True)
        write_build_config(cwd, {"rustc-wrapper": "sccache"})
        owned = neutralize({}, cwd, home)
        check("a legacy ordinary config shadows a toml sccache",
              owned == {}, str(owned))
        shutil.rmtree(os.path.join(cwd, ".cargo"))
        write_build_config(cwd, {"rustc-wrapper": "sccache"}, legacy=True)
        write_build_config(cwd, {"rustc-wrapper": "/opt/custom-wrapper"})
        owned = neutralize({}, cwd, home)
        check("a legacy sccache is emptied despite a toml ordinary",
              owned == {"RUSTC_WRAPPER": ""}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_project_config_beats_global() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        write_build_config(cwd, {"rustc-wrapper": "/opt/custom-wrapper"})
        write_home_config(home, {"rustc-wrapper": "sccache"})
        owned = neutralize({}, cwd, home)
        check("an ordinary project wrapper beats a global sccache",
              owned == {}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_ancestor_config_beats_global() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        proj = os.path.join(base, "proj")
        cwd = os.path.join(proj, "sub")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        write_build_config(proj, {"rustc-wrapper": "/opt/custom-wrapper"})
        write_home_config(home, {"rustc-wrapper": "sccache"})
        owned = neutralize({}, cwd, home)
        check("an ancestor ordinary wrapper beats a global sccache",
              owned == {}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_workspace_keys_neutralized_together() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        write_home_config(home, {"rustc-wrapper": "sccache",
                                  "rustc-workspace-wrapper": "sccache"})
        owned = neutralize({}, cwd, home)
        check("both sccache keys are emptied",
              owned == {"RUSTC_WRAPPER": "",
                        "RUSTC_WORKSPACE_WRAPPER": ""}, str(owned))
        os.remove(os.path.join(home, "config.toml"))
        write_home_config(home, {"rustc-wrapper": "sccache",
                                  "rustc-workspace-wrapper": "/opt/custom-wrapper"})
        owned = neutralize({}, cwd, home)
        check("an ordinary workspace wrapper is preserved",
              owned == {"RUSTC_WRAPPER": ""}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_wrapper_paths_with_spaces() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        owned = neutralize({"RUSTC_WRAPPER": "/opt/my dir/wrap"}, cwd, home)
        check("a spaced ordinary path is preserved",
              owned == {"RUSTC_WRAPPER": "/opt/my dir/wrap"}, str(owned))
        owned = neutralize({"RUSTC_WRAPPER": "/opt/my dir/sccache"}, cwd, home)
        check("a spaced sccache path is emptied",
              owned == {"RUSTC_WRAPPER": ""}, str(owned))
        owned = neutralize({"RUSTC_WRAPPER": "sccache --flag"}, cwd, home)
        check("a flag-bearing value is a literal path, preserved",
              owned == {"RUSTC_WRAPPER": "sccache --flag"}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_absent_everywhere_changes_nothing() -> None:
    base = tempfile.mkdtemp(prefix="sccache-resolve-")
    try:
        cwd = os.path.join(base, "proj")
        home = os.path.join(base, "home")
        os.makedirs(cwd)
        os.makedirs(home)
        owned = neutralize({"PATH": "/usr/bin:/bin"}, cwd, home)
        check("no wrapper anywhere leaves the env alone",
              owned == {"PATH": "/usr/bin:/bin"}, str(owned))
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_empty_wrapper_disables_configured_sccache_shim() -> None:
    if shutil.which("cargo") is None:
        print("skip sccache-shim probe: cargo not on PATH")
        return
    base = tempfile.mkdtemp(prefix="sccache-shim-probe-")
    try:
        crate = os.path.join(base, "crate")
        runroot = os.path.join(base, "runroot")
        home = os.path.join(base, "home")
        os.makedirs(os.path.join(crate, "src"))
        os.makedirs(os.path.join(crate, ".cargo"))
        os.makedirs(runroot)
        os.makedirs(home)
        with open(os.path.join(crate, "Cargo.toml"), "w", encoding="utf-8") as handle:
            handle.write('[package]\nname = "shimprobe"\nversion = "0.0.0"\n'
                         'edition = "2021"\n')
        with open(os.path.join(crate, "src", "main.rs"), "w", encoding="utf-8") as handle:
            handle.write("fn main() {}\n")
        shim = os.path.join(base, "sccache")
        marker = os.path.join(base, "shim-marker")
        with open(shim, "w", encoding="utf-8") as handle:
            handle.write(f'#!/bin/sh\ntouch "{marker}"\nexit 1\n')
        os.chmod(shim, 0o755)
        with open(os.path.join(crate, ".cargo", "config.toml"),
                  "w", encoding="utf-8") as handle:
            handle.write(f'[build]\nrustc-wrapper = "{shim}"\n')
        cargo_env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": os.environ.get("HOME", "/root"),
            "CARGO_HOME": home,
        }
        # Control without the helper: the configured shim runs and fails.
        control = subprocess.run(
            ["cargo", "build", "--offline"], cwd=crate, env=cargo_env,
            capture_output=True, text=True, timeout=120.0,
        )
        check("the shim fails a build when invoked (control)",
              control.returncode != 0 and os.path.exists(marker),
              control.stderr[-300:])
        os.remove(marker)
        # Probe through the owned helper: empty disables the configured shim.
        probed = cli(["cargo", "build", "--offline"], runroot,
                     extra_env=cargo_env, timeout=240.0, cwd=crate)
        check("the owned build succeeds with the shim shadowed",
              probed.returncode == 0, probed.stderr[-500:])
        check("the shim was never invoked",
              not os.path.exists(marker),
              (probed.stdout + probed.stderr)[-300:])
        check("the owned directory is removed", run_dirs(runroot) == [])
    finally:
        shutil.rmtree(base, ignore_errors=True)


def test_ordinary_compiler_wrapper_is_preserved() -> None:
    root = make_root()
    try:
        completed = cli(probe_wrappers(), root,
                        extra_env={"RUSTC_WRAPPER": "/opt/custom-wrapper",
                                   "RUSTC_WORKSPACE_WRAPPER": "/opt/other-wrapper"})
        check("the command still succeeds", completed.returncode == 0,
              completed.stderr[-300:])
        check("a non-sccache wrapper is preserved",
              "wrapper=/opt/custom-wrapper workspace=/opt/other-wrapper"
              in completed.stdout,
              completed.stdout[-300:])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def test_sccache_name_matching_is_exact() -> None:
    check("a bare sccache name matches", helper.wrapper_names_sccache("sccache"))
    check("an absolute sccache path matches",
          helper.wrapper_names_sccache("/usr/bin/sccache"))
    check("a spaced sccache path matches",
          helper.wrapper_names_sccache("/opt/my dir/sccache"))
    check("a blank value never matches",
          not helper.wrapper_names_sccache(""))
    check("a similarly named binary never matches",
          not helper.wrapper_names_sccache("/usr/bin/sccache-foo"))
    check("a flag-bearing value is a literal path, never matches",
          not helper.wrapper_names_sccache("sccache --show-stats"))
    check("an ordinary wrapper never matches",
          not helper.wrapper_names_sccache("/opt/custom-wrapper"))


def test_command_failure_survives_cleanup_failure() -> None:
    root = make_root()
    real_rmtree = shutil.rmtree

    def failing_rmtree(path: str, *args: object, **kwargs: object) -> None:
        raise PermissionError("synthetic cleanup failure")

    shutil.rmtree = failing_rmtree
    try:
        captured = io.StringIO()
        with contextlib.redirect_stderr(captured):
            failed = helper.run_owned_command(root, ["bash", "-c", "exit 7"])
        check("a failing command keeps its code when cleanup also fails",
              failed == 7, f"got {failed}")
        check("the cleanup diagnostic is retained",
              "cannot remove the run directory" in captured.getvalue(),
              captured.getvalue()[-300:])
        check("the owned directory is left in place", len(run_dirs(root)) == 1,
              str(run_dirs(root)))
        captured = io.StringIO()
        with contextlib.redirect_stderr(captured):
            succeeded = helper.run_owned_command(root, ["true"])
        check("a passing command reports cleanup failure as 1",
              succeeded == 1, f"got {succeeded}")
    finally:
        shutil.rmtree = real_rmtree
        shutil.rmtree(root, ignore_errors=True)


def main() -> int:
    test_normal_exit_cleans_sqlite_sidecars()
    test_failure_exit_is_preserved_and_cleaned()
    test_argv_with_spaces_survives()
    test_lingering_grandchild_is_reaped_before_cleanup()
    test_signal_is_forwarded_and_exit_code_preserved()
    test_unresponsive_group_is_escalated_inside_itself()
    test_concurrent_runs_stay_isolated()
    test_removal_refuses_foreign_paths()
    test_cleanup_failure_is_nonzero()
    test_command_failure_survives_cleanup_failure()
    test_sigkill_residual_is_reported_not_deleted()
    test_nested_invocation_execs_without_a_new_directory()
    test_missing_root_fails_closed()
    test_output_never_carries_environment_secrets()
    test_sccache_env_wrapper_is_emptied_not_removed()
    test_global_config_sccache_is_overridden_empty()
    test_ordinary_compiler_wrapper_is_preserved()
    test_sccache_name_matching_is_exact()
    test_effective_wrapper_prefers_env_over_config()
    test_bare_env_beats_cargo_build_env()
    test_explicit_empty_env_is_already_disabled()
    test_legacy_config_shadows_toml()
    test_project_config_beats_global()
    test_ancestor_config_beats_global()
    test_workspace_keys_neutralized_together()
    test_wrapper_paths_with_spaces()
    test_absent_everywhere_changes_nothing()
    test_empty_wrapper_disables_configured_sccache_shim()
    print(f"\n{PASS} passed, {FAIL} failed")
    return 1 if FAIL else 0


if __name__ == "__main__":
    raise SystemExit(main())
