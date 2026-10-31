"""Fail-closed optional SHA-256 dependency/feature audit for task 033."""
PACKAGES={('sha2','0.10.9'),('digest','0.10.7'),('block-buffer','0.10.4'),('crypto-common','0.1.7'),('generic-array','0.14.7'),('typenum','1.20.1'),('version_check','0.9.5'),('cfg-if','1.0.5'),('cpufeatures','0.2.17'),('libc','0.2.191')}
FEATURES={'sha2':['force-soft'],'digest':['block-buffer','core-api','default'],'generic-array':['more_lengths']}
def audit(metadata,compression):
    expected={('gdxrs','0.0.0')}|PACKAGES
    if compression:expected|={('miniz_oxide','0.9.1'),('adler2','2.0.1')}
    if {(p['name'],p['version']) for p in metadata['packages']}!=expected:raise ValueError('index dependency graph differs from audited pins')
    names={p['id']:p['name'] for p in metadata['packages']}
    for node in metadata['resolve']['nodes']:
        name=names[node['id']]
        if name=='gdxrs':continue
        features=['with-alloc'] if name=='miniz_oxide' else FEATURES.get(name,[])
        if sorted(node['features'])!=features:raise ValueError('index features must retain software-only fixed-buffer digest')
    return {'additional_optional_packages':len(PACKAGES),'digest':'sha2 0.10.9, force-soft, no default features','default_runtime_graph_unchanged':True}
