"""Pinned development oracle build and checks. Never imported by Cargo."""

import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
AREA = ROOT / ".work" / "oracle"


def file_digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_fingerprint():
    digest = hashlib.sha256()
    for path in sorted((ROOT / "tools/oracle").glob("*")):
        if path.is_file():
            digest.update(path.name.encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def cmake():
    installed = shutil.which("cmake")
    if installed:
        return installed
    local = ROOT / ".work/python/cmake/data/bin/cmake"
    if local.is_file():
        return str(local)
    raise ValueError("CMake missing; see docs/oracle.md")


def source(run, path, url, commit):
    if not path.exists():
        run(["git", "clone", "--no-checkout", url, path], timeout=180)
        run(["git", "-C", path, "checkout", "--detach", commit])
    head = run(["git", "-C", path, "rev-parse", "HEAD"], capture=True).stdout.strip()
    if head != commit:
        raise ValueError(f"wrong source revision in {path}; refusing to reset existing files")
    dirty = run(["git", "-C", path, "status", "--porcelain", "--untracked-files=all"], capture=True).stdout
    if dirty:
        raise ValueError(f"modified oracle source: {path}")


def build(run):
    AREA.mkdir(parents=True, exist_ok=True)
    pins = json.loads((ROOT / "tools/oracle/sources.json").read_text())
    gdx = AREA / "gdx"
    source(run, gdx, pins["gdx"]["url"], pins["gdx"]["commit"])
    if not (gdx / "zlib/.git").exists():
        run(["git", "-C", gdx, "submodule", "update", "--init", "zlib"], timeout=180)
    source(run, gdx / "zlib", pins["zlib"]["url"], pins["zlib"]["commit"])
    compiler = run(["g++", "--version"], capture=True).stdout.splitlines()[0]
    cmake_version = run([cmake(), "--version"], capture=True).stdout.splitlines()[0]
    digest = hashlib.sha256(source_fingerprint().encode())
    digest.update((compiler + cmake_version).encode())
    identity = digest.hexdigest()
    directory = AREA / "builds" / identity
    native = directory / "gdx"
    driver = directory / "driver"
    # Force bundled zlib; a system find must not silently alter the reference.
    run([cmake(), "-S", gdx, "-B", native, *pins["build_options"]], timeout=180)
    run([cmake(), "--build", native, "--target", "gdx-static", "--parallel", "2"], timeout=600)
    library = native / "libgdx-static.a"
    if not library.is_file():
        raise ValueError("expected Linux static library missing; platform not yet supported")
    run([cmake(), "-S", ROOT / "tools/oracle", "-B", driver,
         "-DCMAKE_BUILD_TYPE=Release", f"-DGDX_ROOT={gdx}", f"-DGDX_LIBRARY={library}"])
    run([cmake(), "--build", driver, "--parallel", "2"], timeout=180)
    executable = driver / "gdxrs-oracle"
    if not executable.is_file():
        raise ValueError("oracle executable missing")
    result = {"identity": identity, "source_fingerprint": source_fingerprint(), "compiler": compiler, "cmake": cmake_version,
              "pins": pins, "executable": str(executable),
              "executable_sha256": file_digest(executable)}
    (AREA / "current.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def executable():
    identity = json.loads((AREA / "current.json").read_text())
    if identity.get("source_fingerprint") != source_fingerprint():
        raise ValueError("oracle sources changed; rerun oracle-build")
    path = Path(identity["executable"])
    if not path.is_relative_to(AREA.resolve()) or not path.is_file():
        raise ValueError("invalid oracle path; rerun oracle-build")
    if file_digest(path) != identity["executable_sha256"]:
        raise ValueError("oracle executable changed; rerun oracle-build")
    return path


def finite(number):
    return "f:" + struct.pack(">d", number).hex()


def smoke_expectation():
    """Authored from smoke inputs, not learned from a dump."""
    hx = lambda text: text.encode().hex()
    result = ["GDXRS-ORACLE\t1"]
    result += [f"U\t{i}\t{hx(label)}" for i, label in enumerate(["z", "a", "b"], 1)]
    result += ["T\t0\t0\t", f"T\t1\t0\t{hx('set element')}", f"A\t1\t7\t{hx('ac')}\t{hx('acronym')}"]
    # Universe and alias-independent records are part of the public raw view.
    result += [f"S\t0\t{hx('*')}\t1\t0\t3\t0\t{hx('Universe')}"]
    result += [f"R\t0\t{i}\t{finite(0)}" for i in range(1, 4)] + ["E\t0\t3"]
    result += [f"S\t1\t{hx('i')}\t1\t0\t2\t0\t{hx('set')}", "D\t1\t1\t0:2a", f"C\t1\t1\t{hx('smoke comment')}"]
    result += [f"R\t1\t1\t{finite(1)}", f"R\t1\t2\t{finite(0)}", "E\t1\t2"]
    result += [f"S\t2\t{hx('p')}\t2\t1\t3\t0\t{hx('parameter')}", "D\t2\t3\t1:69\t0:2a"]
    result += [f"R\t2\t1,2\t{finite(2.5)}", "R\t2\t1,3\ts:na", "R\t2\t2,1\ta:7", "E\t2\t3"]
    result += [f"S\t3\t{hx('x')}\t1\t2\t1\t5\t{hx('variable')}", "D\t3\t1\t0:2a"]
    result += ["R\t3\t3\t" + ",".join([finite(1.25), finite(-2.5), "s:minf", "s:pinf", finite(4)]), "E\t3\t1"]
    return result


def self_test(run):
    exe = executable()
    expected = smoke_expectation()
    cases = []
    with tempfile.TemporaryDirectory(prefix="smoke-", dir=AREA) as directory:
        for compression in (0, 1):
            path = Path(directory) / f"smoke-{compression}.gdx"
            run([exe, "smoke", path, str(compression)])
            lines = run([exe, "dump", path], capture=True).stdout.splitlines()
            file_line = lines.pop(1).split("\t")
            if file_line[:6] != ["F", "7", str(compression), "3", "3", "gdxrs-oracle".encode().hex()]:
                raise ValueError(f"file metadata mismatch: {file_line}")
            if lines != expected:
                import difflib
                raise ValueError("smoke semantic mismatch:\n" + "\n".join(difflib.unified_diff(expected, lines, fromfile="expected", tofile="observed")))
            cases.append({"compression": compression, "symbols": 3, "records": 6, "universe_records": 3})
        invalid = Path(directory) / "truncated.gdx"
        invalid.write_bytes(b"bad")
        for argv in ([exe, "dump", invalid], [exe, "dump", Path(directory) / "missing.gdx"], [exe, "unknown"]):
            result = subprocess.run(list(map(str, argv)), capture_output=True, text=True, timeout=15)
            if result.returncode == 0:
                raise ValueError(f"oracle accepted invalid invocation: {argv}")
    return {"cases": cases, "negative_invocations": 3, "expected_source": "tools/oracle.py:smoke_expectation"}
