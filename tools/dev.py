#!/usr/bin/env python3
"""Repository development checks. Python is never a production dependency."""

import argparse
import datetime
import json
import platform
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
        text=True, encoding="utf-8", capture_output=capture, env=env,
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
        "platform": {"system": platform.system(), "machine": platform.machine(), "release": platform.release()},
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
    versions["rustc_verbose"] = run(["rustc", "--version", "--verbose"], capture=True).stdout.strip()
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


def test_suite_counts(output):
    # Rustdoc runs merged no_run examples separately from compile-fail cases.
    # Keep every group nonempty/unskipped, and require the borrowing failure test.
    summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", output)
    if len(summaries) < 3 or any(int(row[0]) == 0 or any(int(n) for n in row[1:]) for row in summaries) or " - compile fail ... ok" not in output:
        raise CheckFailure("reader suites must execute unit, integration and borrowing compile-fail tests without skips")
    return [int(row[0]) for row in summaries]


def reader_check():
    bootstrap = check_bootstrap()
    run(["cargo", "clippy", "--locked", "--all-targets", "--no-default-features", "--", "-D", "warnings"])
    minimal = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1", "--offline", "--no-default-features"], capture=True).stdout)
    audit_dependencies(minimal, compression=False)
    suites = []
    for features in ([], ["--no-default-features"], ["--all-features"]):
        args = ["cargo", "test", "--locked", *features]
        result = run(args, capture=True)
        counts = test_suite_counts(result.stdout)
        suites.append({"command": args, "unit_integration_doctest_groups_passed": counts})
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
            "exact_id_slicing":"executed in Rust suites (task 012)",
            "small_exact_labels_and_pages":"executed in Rust suites (task 013)",
            "bounded_spill_primitives":"executed in Rust suites (task 014)",
            "large_lookup_and_unique_domains":"executed in Rust suites (task 015)",
            "m2_scale_qualification":"requires separate m2-scale-check --profile scale gate"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check-bootstrap", "oracle-build", "oracle-self-test", "fixtures-generate", "fixtures-check", "fixtures-regenerate-check", "spec-check", "contracts-check", "benchmark-oracle", "m0-check", "reader-check", "label-pages-check", "m2-scale-check"])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--baseline", type=Path, help="Explicit validated baseline report for m0-check")
    parser.add_argument("--profile", choices=['small','smoke','scale'], default='small')
    args = parser.parse_args()
    if args.baseline is not None and args.command != 'm0-check':
        parser.error('--baseline only applies to m0-check')
    if args.command == 'fixtures-generate' and args.output is None:
        parser.error('fixtures-generate requires --output NEW_DIRECTORY')
    if args.output is not None and args.command != 'fixtures-generate':
        parser.error('--output only applies to fixtures-generate')
    if '--profile' in sys.argv and args.command not in ['benchmark-oracle','m2-scale-check']:
        parser.error('--profile only applies to benchmark-oracle or m2-scale-check')
    if args.command == 'benchmark-oracle' and args.profile != 'small':
        parser.error('benchmark-oracle requires profile small')
    def external_command():
        # Reader validation must not import Unix resource/exec-only development
        # tools on Windows. Import only the explicitly selected command module.
        if sys.platform == 'win32' and args.command not in ['contracts-check', 'm2-scale-check']:
            raise CheckFailure('native oracle development commands are not qualified on Windows; use reader-check for the Rust reader')
        if args.command.startswith('oracle-'):
            import oracle
            return oracle.build(run) if args.command == 'oracle-build' else oracle.self_test(run)
        if args.command.startswith('fixtures-'):
            import fixtures
            if args.command == 'fixtures-generate': return fixtures.generate(run, args.output)
            if args.command == 'fixtures-regenerate-check': return fixtures.regeneration_check(run)
            return fixtures.check(run)
        if args.command == 'spec-check':
            import specification
            return specification.check(run)
        if args.command == 'contracts-check':
            import contracts
            return contracts.check()
        if args.command == 'benchmark-oracle':
            import benchmark
            return benchmark.execute(run, args.profile)
        if args.command == 'label-pages-check':
            import label_pages
            return label_pages.check(run)
        if args.command == 'm2-scale-check':
            if sys.platform != 'linux':
                raise CheckFailure('m2-scale-check requires Linux OS containment; reader-check is portable')
            import scale
            return scale.check(run, "smoke" if args.profile == "small" else args.profile)
        if args.command == 'm0-check':
            import m0
            result = m0.check(run, check_bootstrap, args.baseline)
            if result.get('errors'):
                raise CheckFailure('; '.join(result['errors']), result)
            if not result['m1_eligible']:
                raise CheckFailure('independent M0 review pending; automated checks passed', result, 2)
            return result
        raise CheckFailure('unimplemented development command')
    actions = {"check-bootstrap": check_bootstrap, "reader-check": reader_check}
    return report_check(args.command, actions.get(args.command, external_command))


if __name__ == "__main__":
    sys.exit(main())
