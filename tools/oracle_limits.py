"""POSIX native-oracle launcher; setup failures must not look like input rejection."""

import os
import resource
import sys


def apply_limits(platform=sys.platform):
    # Darwin does not reliably implement RLIMIT_AS. RLIMIT_DATA limits data/heap
    # growth there, not all virtual mappings; it is not an RSS or full AS cap.
    memory = resource.RLIMIT_DATA if platform == 'darwin' else resource.RLIMIT_AS
    for label, kind, requested in [('memory', memory, 256*1024*1024),
                                   ('CPU', resource.RLIMIT_CPU, 2),
                                   ('core', resource.RLIMIT_CORE, 0),
                                   ('output', resource.RLIMIT_FSIZE, 1024*1024)]:
        try:
            soft, hard = resource.getrlimit(kind)
            candidates = [requested] + [n for n in (soft, hard) if n != resource.RLIM_INFINITY]
            # Darwin may report an unlimited hard limit that it refuses to
            # accept again in setrlimit (EINVAL / Python ValueError). This
            # dedicated exec child never needs to raise its limits later.
            child_hard = min(requested, hard) if hard != resource.RLIM_INFINITY else requested
            resource.setrlimit(kind, (min(candidates), child_hard if platform == 'darwin' else hard))
        except (ValueError, OSError) as exc:
            raise RuntimeError(f'{label} limit setup failed: {exc}') from exc


def main():
    try:
        apply_limits()
        os.execv(sys.argv[1], sys.argv[1:])
    except Exception as exc:
        print(f'oracle isolation: {exc}', file=sys.stderr)
        return 125


if __name__ == '__main__':
    sys.exit(main())
