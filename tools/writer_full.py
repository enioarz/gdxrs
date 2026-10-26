"""Task 022: independent complete small recipe, in both native directions."""
import hashlib
import struct
import sys
from pathlib import Path
import fixtures
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]

def specification():
    f=lambda n:'f:'+struct.pack('>d',n).hex()
    z=f(0.);o=f(1.)
    # (name, dim, kind, user, description, domains mode, domains, rows, comments)
    member=[([1],[o]),([3],[z])]
    return [(b'I',1,0,0,b'd'*255,1,['0:2a'],member,[b'c'*255,b'second']),
      (b'J',1,4,1,b'Aliased with I',1,['0:2a'],member,[]),
      (b'K',1,4,2,b'Aliased with J',1,['0:2a'],member,[]),
      (b'P',2,1,0,b'strict',3,['2:4a','0:2a'],[([1,4],['s:undef']),([3,2],['a:7'])],[b'param']),
      (b'R',2,1,0,b'',2,['0:667574757265','0:2a'],[([2,1],['s:eps'])],[]),
      (b'V',20,2,5,b'',1,['0:2a']*20,[([1]*20,[o,f(-1.),'s:minf','s:pinf',f(2.5)])],[]),
      (b'E',0,3,53,b'',1,[],[([],['s:na',z,z,z,o])],[]),
      (b'Empty',0,1,0,b'',1,[],[([],[z])],[]),
      (b'All',1,4,0,b'Aliased with *',1,['0:2a'],[([n],[z]) for n in range(1,5)],[])]

def expected(compression,producer,audit):
    z='f:0000000000000000'
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t9\t4\t{producer}\t{audit}']
    lines += [f'U\t{n}\t'+label.hex() for n,label in enumerate([b'z',b'a',b'C',b'\x80'],1)]
    lines += ['T\t0\t0\t','T\t1\t0\t6d656d626572','A\t1\t7\t4163726f\t7479706564']
    lines += ['S\t0\t2a\t1\t0\t4\t0\t556e697665727365']
    lines += [f'R\t0\t{n}\t{z}' for n in range(1,5)]+['E\t0\t4']
    for n,(name,dim,kind,user,description,mode,domains,rows,comments) in enumerate(specification(),1):
        count=0 if kind==4 else len(rows)
        lines.append(f'S\t{n}\t{name.hex()}\t{dim}\t{kind}\t{count}\t{user}\t{description.hex()}')
        lines.append(f'D\t{n}\t{mode}'+''.join('\t'+d for d in domains))
        lines += [f'C\t{n}\t{k}\t'+c.hex() for k,c in enumerate(comments,1)]
        for keys,values in rows:lines.append(f'R\t{n}\t'+','.join(map(str,keys))+'\t'+','.join(values))
        lines.append(f'E\t{n}\t{len(rows)}')
    return '\n'.join(lines)+'\n'

def recipe():
    lines=['U\t'+b.hex() for b in [b'z',b'a',b'C',b'\x80']]
    lines += ['T\t6d656d626572','A\t4163726f\t7479706564\t7']
    for name,dim,kind,user,description,mode,domains,rows,comments in specification():
        if kind==4:
            target=b'*' if user==0 else b'I' if name==b'J' else b'J'
            lines.append('ALIAS\t'+target.hex()+'\t'+name.hex());continue
        lines.append(f'S\t{name.hex()}\t{dim}\t{kind}\t{user}\t{description.hex()}\traw')
        if mode==3:lines.append('D\t4a\t2a')
        for keys,values in rows if name!=b'Empty' else []:lines.append('K\t'+','.join(map(str,keys))+'\t'+','.join(values))
        lines.append('END')
        if mode==2:lines.append('X\t667574757265\t2a')
        lines += ['C\t'+c.hex() for c in comments]
    return '\n'.join(lines)+'\n'

def check(run,area,executable,decode_blocks):
    cases=[]
    unsorted_cases=[]
    recipe_path=area/'full.recipe.tsv';recipe_path.write_text(recipe())
    for compression in [0,1]:
        native_path=area/f'full-native-{compression}.gdx'
        run([executable,'recipe',native_path,recipe_path,str(compression)],capture=True)
        producer=b'gdxrs-fixtures'.hex()
        native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',native_path],capture=True,timeout=5).stdout
        if fixtures.normalize_dump(native)!=expected(compression,producer,'<creation-system>'):
            (area/'full-native-actual.tsv').write_text(fixtures.normalize_dump(native));(area/'full-native-expected.tsv').write_text(expected(compression,producer,'<creation-system>'))
            raise ValueError('native complete recipe differs from independent expectation')
        features=[[],['--all-features']]
        if not compression:features.append(['--no-default-features'])
        for feature in features:
            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',native_path],capture=True).stdout
            if rust!=expected(compression,producer,'<creation-system>'):raise ValueError('native writes → Rust reads complete metadata/record mismatch')
            path=area/f'full-rust-{compression}-{len(cases)}.gdx'
            run(['cargo','run','--locked','--quiet',*feature,'--example','create_full','--',path,area,*(['zlib'] if compression else [])],capture=True)
            with path.open('rb') as src:data=src.read(16385)
            if len(data)>16384:raise ValueError('full fixed fixture exceeds 16 KiB bound')
            offsets=struct.unpack_from('<6q',data,50)
            symbols,uels,texts,acronyms,next_write,domains=offsets
            if not (126<=symbols==next_write<texts<uels<acronyms<domains<len(data)):
                raise ValueError('canonical metadata physical offsets/order mismatch')
            for begin,end,marker in [(symbols,texts,b'_SYMB_'),(texts,uels,b'_SETT_'),(uels,acronyms,b'_UEL_'),(acronyms,domains,b'_ACRO_'),(domains,len(data),b'_DOMS_')]:
                section=data[begin:end]
                if compression:section=decode_blocks(section)[0]
                token=bytes([len(marker)])+marker
                if not section.startswith(token) or not section.endswith(token):raise ValueError('metadata section framing differs')
            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
            if native!=expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex()) or rust!=expected(compression,b'gdxrs'.hex(),'<creation-system>'):
                raise ValueError('Rust writes → native reads complete metadata/record mismatch')
            cases.append(dict(features=feature,compression=compression,bytes=len(data),sha256=hashlib.sha256(data).hexdigest(),native_sha256=hashlib.sha256(native_path.read_bytes()).hexdigest(),both_directions_complete_stream_match=True,canonical_offsets_and_section_markers_match=True))
            unsorted_path=area/f'full-unsorted-{compression}-{len(cases)}.gdx'
            created=run(['cargo','run','--locked','--quiet',*feature,'--example','create_full','--',unsorted_path,area,*(['zlib'] if compression else []),'unsorted'],capture=True)
            if 'unsorted_records=2 merge_passes=1' not in created.stderr:raise ValueError('unsorted author did not exercise forced external merging')
            with unsorted_path.open('rb') as source:unsorted=source.read(16385)
            if unsorted!=data:raise ValueError('reversed unsorted input differs from independently qualified full output bytes')
            native_unsorted=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',unsorted_path],capture=True,timeout=5).stdout
            rust_unsorted=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',unsorted_path],capture=True).stdout
            if native_unsorted!=expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex()) or rust_unsorted!=expected(compression,b'gdxrs'.hex(),'<creation-system>'):raise ValueError('unsorted complete metadata/record streams differ from independent expectation')
            unsorted_cases.append(dict(features=feature,compression=compression,bytes=len(unsorted),sha256=hashlib.sha256(unsorted).hexdigest(),forced_external_merge=True,complete_native_and_rust_stream_match=True))
    return dict(unsorted_cases=unsorted_cases,cases=cases,scope='complete supported metadata and records; 9 symbols, 4 UELs, 20 public rows per case',recipe_sha256=hashlib.sha256(recipe_path.read_bytes()).hexdigest(),native_isolation=isolation_policy())
