#!/usr/bin/env python3
"""Optional stage-timing investigation; never replaces the scale gate."""
import argparse
import json
from pathlib import Path
import subprocess
import uuid

import oracle
import scale


def instrument(source):
    marker = '    while let Some(batch) = cursor.next_batch()? {'
    tail = '            count += 1;\n        }\n    }\n    let work = cursor.statistics();'
    output = r'\"decoded_rows\":{},'
    if source.count(marker) != 1 or source.count(tail) != 1 or output not in source:
        raise ValueError('sink shape changed; review instrumentation before profiling')
    source = source.replace(marker, '''    let mut decode_seconds = 0.;
    let mut next_start = Instant::now();
    loop {
        let next = cursor.next_batch()?;
        decode_seconds += next_start.elapsed().as_secs_f64();
        let Some(batch) = next else { break; };''', 1)
    source = source.replace(tail, '            count += 1;\n        }\n        next_start = Instant::now();\n    }\n    let work = cursor.statistics();', 1)
    return source.replace(output, r'\"decode_seconds\":{decode_seconds},\"decoded_rows\":{},', 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('scale_report', type=Path)
    args = parser.parse_args()
    report = json.loads(args.scale_report.read_text())
    if report['status'] != 'passed' or not report['result']['m2_scale_qualified']:
        raise ValueError('a passed full-scale report is required')
    for name, digest in report['result']['source_hashes'].items():
        if oracle.file_digest(scale.ROOT/name) != digest:
            raise ValueError('source differs from measured scale revision')
    case = next(c for c in report['result']['cases'] if c['name'] == 'records-20000000')
    area = scale.ROOT/'.work/scale'/('profile-'+uuid.uuid4().hex)
    area.mkdir(parents=True)
    output = subprocess.run(['cargo','build','--locked','--release','--example','scale_sink','--message-format=json'],cwd=scale.ROOT,text=True,capture_output=True,check=True)
    library = None
    for line in output.stdout.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact' and item['target']['name'] == 'gdxrs':
            library = next(Path(s) for s in item['filenames'] if s.endswith('.rlib'))
    if library is None:
        raise ValueError('Cargo did not identify the compiled core library')
    source = area/'profile_sink.rs'
    source.write_text(instrument((scale.ROOT/'examples/scale_sink.rs').read_text()))
    binary = area/'profile_sink'
    subprocess.run(['rustc','--edition=2024','-C','opt-level=3',source,'--extern',f'gdxrs={library}','-L',f'dependency={library.parent}','-o',binary],cwd=scale.ROOT,check=True)
    trials = []
    for n in range(3):
        scratch = area/f'scratch-{n}'
        scratch.mkdir()
        trial = scale.measure([binary,'all',scale.ROOT/case['artifact'],case['recipe']['count'],scratch],area,f'profile-{n}')
        scale.validate_scan(trial,case['recipe'],'all')
        scratch.rmdir()
        trials.append(trial)
    result = dict(scale_report=str(args.scale_report), scale_revision=report['revision'],
                  source_sha256=oracle.file_digest(source), library_sha256=oracle.file_digest(library),
                  helper_sha256=oracle.file_digest(Path(__file__)),
                  trials=trials, decode_seconds=scale.summary([t['result']['decode_seconds'] for t in trials]),
                  total_seconds=scale.summary([t['result']['seconds'] for t in trials]),
                  notes=['Same checked full-row sink plus batch-boundary Instant timings',
                         'decode_seconds includes decoder, selection, batch materialization and source checks',
                         'Remaining time includes per-field recipe validation and digest',
                         'OS cache reused, no cold-cache claim; instrumented timings are not the uninstrumented benchmark'])
    path = area/'profile.json'
    path.write_text(json.dumps(result,indent=2)+'\n')
    print(path.relative_to(scale.ROOT))


if __name__ == '__main__':
    main()
