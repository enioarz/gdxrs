#!/usr/bin/env python3
"""Repository development checks. Python is never a production dependency."""

import argparse
import datetime
import json
from pathlib import Path
import subprocess
import sys
import time
import tomllib
import uuid

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / ".work"


class CheckFailure(Exception):
    pass


def run(argv, *, cwd=ROOT, timeout=180, capture=False, env=None):
    """Run without a shell; propagate errors, including missing tools/timeouts."""
    print("+", " ".join(map(str, argv)), flush=True)
    return subprocess.run(
        list(map(str, argv)), cwd=cwd, timeout=timeout, check=True,
        text=True, capture_output=capture, env=env,
    )


def revision():
    return subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()


def report_check(name, operation):
    """One exclusive report per invocation, failed until operation succeeds."""
    reports = WORK / "reports"
    reports.mkdir(parents=True, exist_ok=True)
    run_id = uuid.uuid4().hex
    path = reports / f"{name}-{run_id}.json"
    record = {
        "schema": 1, "command": name, "run_id": run_id,
        "revision": revision(), "cwd": str(ROOT),
        "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "status": "running", "result": None,
    }
    with path.open("x") as out:
        json.dump(record, out, indent=2)
    started = time.monotonic()
    exit_code = 1
    try:
        record["result"] = operation()
        record["status"] = "passed"
        exit_code = 0
    except (CheckFailure, OSError, ValueError, subprocess.SubprocessError) as exc:
        record["status"] = "failed"
        record["error"] = str(exc)
        print(f"ERROR: {exc}", file=sys.stderr)
        if isinstance(exc, subprocess.CalledProcessError):
            exit_code = exc.returncode if exc.returncode > 0 else 1
            if exc.stdout:
                print(exc.stdout, file=sys.stderr)
            if exc.stderr:
                print(exc.stderr, file=sys.stderr)
    except KeyboardInterrupt:
        record["status"] = "interrupted"
        exit_code = 130
    finally:
        record["elapsed_seconds"] = time.monotonic() - started
        record["exit_code"] = exit_code
        path.write_text(json.dumps(record, indent=2) + "\n")
        print(f"Report: {path.relative_to(ROOT)} ({record['status']})")
    return exit_code


def check_bootstrap():
    pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    if manifest["package"]["rust-version"] != pin:
        raise CheckFailure("MSRV and pinned toolchain must match at bootstrap")
    versions = {}
    for tool in ("rustc", "cargo"):
        value = run([tool, "--version"], capture=True).stdout.strip()
        if value.split()[1] != pin:
            raise CheckFailure(f"{tool} version differs from pin {pin}: {value}")
        versions[tool] = value
    for args in (
        ["fmt", "--all", "--", "--check"],
        ["check", "--locked", "--all-targets"],
        ["clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
    ):
        run(["cargo", *args])
    metadata = json.loads(run([
        "cargo", "metadata", "--locked", "--format-version", "1", "--offline",
    ], capture=True).stdout)
    if len(metadata["packages"]) != 1 or metadata["packages"][0]["dependencies"]:
        raise CheckFailure("bootstrap must have zero third-party dependencies")
    run(["cargo", "tree", "--locked"])
    return {"versions": versions, "runtime_dependencies": 0, "gdx_tests": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check-bootstrap", "oracle-build", "oracle-self-test", "fixtures-generate", "fixtures-check", "fixtures-regenerate-check"])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    from oracle import build, self_test
    import fixtures
    if args.command == 'fixtures-generate' and args.output is None:
        parser.error('fixtures-generate requires --output NEW_DIRECTORY')
    if args.output is not None and args.command != 'fixtures-generate':
        parser.error('--output only applies to fixtures-generate')
    actions = {
        "check-bootstrap": check_bootstrap,
        "oracle-build": lambda: build(run),
        "oracle-self-test": lambda: self_test(run),
        "fixtures-generate": lambda: fixtures.generate(run,args.output),
        "fixtures-check": lambda: fixtures.check(run),
        "fixtures-regenerate-check": lambda: fixtures.regeneration_check(run),
    }
    return report_check(args.command, actions[args.command])


if __name__ == "__main__":
    sys.exit(main())
