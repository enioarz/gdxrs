#!/usr/bin/env python3
"""Repository development checks. Python is never a production dependency."""

import argparse
import datetime
import json
import re
from pathlib import Path
import subprocess
import sys
import time
import tomllib
import uuid

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / ".work"


class CheckFailure(Exception):
    def __init__(self,message,result=None,exit_code=1):
        super().__init__(message)
        self.result=result
        self.exit_code=exit_code


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
    except Exception as exc:
        record["status"] = "failed"
        record["error"] = str(exc)
        if isinstance(exc,CheckFailure):
            record['result']=exc.result
            exit_code=exc.exit_code
            if exit_code==2: record['status']='blocked'
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
    audit_dependencies(metadata)
    run(["cargo", "tree", "--locked"])
    return {"versions": versions, "runtime_dependencies": 2, "runtime_tests_executed": 0,
            "dependency_audit": "docs/decisions/0004-compression.md"}


def audit_dependencies(metadata, compression=True):
    expected = {("gdxrs", "0.0.0")}
    if compression:
        expected |= {("miniz_oxide", "0.9.1"), ("adler2", "2.0.1")}
    actual = {(p["name"], p["version"]) for p in metadata["packages"]}
    if actual != expected:
        raise CheckFailure("resolved dependencies differ from audited codec graph")
    names = {p["id"]: p["name"] for p in metadata["packages"]}
    for node in metadata["resolve"]["nodes"]:
        if names[node["id"]] != "gdxrs" and node["features"]:
            raise CheckFailure("codec dependency features differ from bounded no-alloc audit")


def reader_check():
    bootstrap = check_bootstrap()
    run(["cargo", "clippy", "--locked", "--all-targets", "--no-default-features", "--", "-D", "warnings"])
    minimal = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1", "--offline", "--no-default-features"], capture=True).stdout)
    audit_dependencies(minimal, compression=False)
    suites = []
    for features in ([], ["--no-default-features"], ["--all-features"]):
        args = ["cargo", "test", "--locked", *features]
        result = run(args, capture=True)
        summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", result.stdout)
        if len(summaries) != 3 or any(int(row[0]) == 0 or any(int(n) for n in row[1:]) for row in summaries):
            raise CheckFailure("reader suites must execute unit, integration and compile-fail tests without skips")
        suites.append({"command": args, "unit_integration_doctest_passed": [int(row[0]) for row in summaries]})
    # Small frozen fixtures only: collecting the textual comparison here is
    # development tooling, never part of the production reader or dump example.
    paths = sorted((ROOT/"tests/fixtures/v7").glob("*.gdx"))
    if len(paths) != 10:
        raise CheckFailure("reader differential corpus must contain ten frozen fixtures")
    records = 0
    for path in paths:
        result = run(["cargo", "run", "--locked", "--quiet", "--example", "dump", "--", path], capture=True)
        expected = path.with_suffix(".expected.tsv").read_text()
        if result.stdout != expected:
            raise CheckFailure("Rust dump differs from authored expectation: " + path.name)
        records += sum(line.startswith("R\t") for line in result.stdout.splitlines())
    return {"bootstrap": bootstrap, "suites": suites, "differential_fixtures":len(paths),
            "public_records_compared":records, "no_compression_dependencies":0,
            "m2_scale_and_selection":"not implemented or measured"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check-bootstrap", "oracle-build", "oracle-self-test", "fixtures-generate", "fixtures-check", "fixtures-regenerate-check", "spec-check", "contracts-check", "benchmark-oracle", "m0-check", "reader-check"])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--profile", choices=['small'], default='small')
    args = parser.parse_args()
    from oracle import build, self_test
    import fixtures
    import specification
    import contracts
    import benchmark
    import m0
    if args.command == 'fixtures-generate' and args.output is None:
        parser.error('fixtures-generate requires --output NEW_DIRECTORY')
    if args.output is not None and args.command != 'fixtures-generate':
        parser.error('--output only applies to fixtures-generate')
    if '--profile' in sys.argv and args.command != 'benchmark-oracle':
        parser.error('--profile only applies to benchmark-oracle')
    def m0_check():
        result=m0.check(run,check_bootstrap)
        if not result['m1_eligible']:
            raise CheckFailure('independent M0 review pending; automated checks passed',result,2)
        return result
    actions = {
        "check-bootstrap": check_bootstrap,
        "reader-check": reader_check,
        "oracle-build": lambda: build(run),
        "oracle-self-test": lambda: self_test(run),
        "fixtures-generate": lambda: fixtures.generate(run,args.output),
        "fixtures-check": lambda: fixtures.check(run),
        "fixtures-regenerate-check": lambda: fixtures.regeneration_check(run),
        "spec-check": lambda: specification.check(run),
        "contracts-check": contracts.check,
        "benchmark-oracle": lambda: benchmark.execute(run,args.profile),
        "m0-check": m0_check,
    }
    return report_check(args.command, actions[args.command])


if __name__ == "__main__":
    sys.exit(main())
