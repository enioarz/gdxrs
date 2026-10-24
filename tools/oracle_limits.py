"""POSIX native-oracle launcher; setup failures must not look like input rejection."""

import os
import resource
import sys


def isolation_policy(platform=None):
    platform = sys.platform if platform is None else platform
    return dict(platform=platform,
                memory_limit='unavailable' if platform == 'darwin' else '256 MiB address space or tighter inherited limit',
                cpu_seconds_max=2, output_bytes_max=1024*1024,
                core_bytes_max=0, wall_timeout_seconds=5)


def apply_limits(platform=sys.platform):
    # Both RLIMIT_AS and RLIMIT_DATA failed on the reported Darwin host.
    # Do not claim memory containment on macOS or silently substitute RLIMIT_RSS.
    limits = [('CPU', resource.RLIMIT_CPU, 2),
              ('core', resource.RLIMIT_CORE, 0),
              ('output', resource.RLIMIT_FSIZE, 1024*1024)]
    if platform != 'darwin':
        limits.insert(0, ('memory', resource.RLIMIT_AS, 256*1024*1024))
    for label, kind, requested in limits:
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
