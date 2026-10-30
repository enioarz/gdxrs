"""Additional task-031 independent calibration across the 65,536-key boundary.

Development-only, fixed 131,073-row ceiling. No GDX output or core code changes.
Pass the acceptance run's snapshotted gdxrs-m4 executable as the sole argument.
"""
import functools
import json
from pathlib import Path
import subprocess
import sys

N = 131073
MASK = (1 << 64)-1
INITIAL = 14695981039346656037


def digest(h, words):
    for word in words:
        h = ((((h << 7) | (h >> 57)) & MASK) ^ word)*0x9e3779b185ebca87 & MASK
    return h


def key(row, dim):
    result = [row//65536+1]
    if dim > 1:
        result.append(row % 65536+1)
    for axis in range(2, dim):
        result.append((row*(2*axis+1)+5696) % 65536+1)
    return result


def values(row, fields):
    return [0x3ff0000000000000 | ((17*row+12345*f+5696) % (1 << 52)) for f in range(fields)]


@functools.lru_cache(maxsize=4)
def labels(count, short):
    h = INITIAL
    for raw in range(1, count+1):
        text = ('u'+str(1000000000+raw)).encode()
        if not short:
            text = text.ljust(63, b'x')
        h = digest(h, [raw])
        h = digest(h, text)
    return f'{h:016x}'


def expected(kind):
    dim = 1 if kind == 'dictionary' else 2 if kind == 'strings' else 20
    fields = 1 if kind == 'dictionary' else 5
    selected = [1, 2, N//2, N-1, N] if kind == 'dictionary' else range(N)
    h = INITIAL
    for i in selected:
        h = digest(h, ([i] if dim == 1 else key(i, dim))+values(i, fields))
    # Enumerate actual reverse-input events, then sort complete keys+ordinals.
    # This differs from the native closed-form ordinal and grouped-error loops.
    errors = []
    ordinal = 0
    if kind == 'errors':
        for i in reversed(range(N)):
            ordinal += 1
            if i % 2 == 0:
                ordinal += 1
                k = key(i, 20)
                k[0] = 65537
                errors.append((k, ordinal, 1, values(i, 5)))
            if i % 3 == 0:
                ordinal += 1
                errors.append((key(i, 20), ordinal, 2, values(i, 5)))
        errors.sort(key=lambda r: (r[0], r[1]))
    eh = INITIAL
    for k, ordinal, tag, v in errors:
        eh = digest(eh, [tag, ordinal, int(tag == 1)]+k+v)
    count = N if kind == 'dictionary' else 65536+int(kind == 'errors')
    return dict(count=5 if dim == 1 else N, digest=f'{h:016x}', labels=count,
                label_digest=labels(count, kind == 'strings'), errors=len(errors),
                error_bytes=159*len(errors), error_digest=f'{eh:016x}')


def main():
    if len(sys.argv) != 2:
        raise ValueError('pass the acceptance snapshot native executable')
    native = Path(sys.argv[1]).resolve(strict=True)
    results = []
    for kind in ['unsorted', 'strings', 'mapped', 'dictionary', 'errors', 'append']:
        actual = json.loads(subprocess.check_output([native, 'expected', kind, str(N)], text=True, timeout=180))
        want = expected(kind)
        if actual != want:
            raise ValueError('independent boundary calibration mismatch: '+kind)
        results.append(dict(kind=kind, input_count=N, expected=want))
    print(json.dumps(dict(native=str(native), cases=results)))


if __name__ == '__main__':
    main()
