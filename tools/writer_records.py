"""Independent bounded recipe checks for task 021 (not a runtime dependency)."""
import hashlib
import struct
import sys
from pathlib import Path
import oracle
from oracle_limits import isolation_policy

ROOT = Path(__file__).resolve().parents[1]

def expected(compression, audit):
    zero='f:0000000000000000'
    one='f:3ff0000000000000'
    def number(n): return 'f:'+struct.pack('>d',n).hex()
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t45\t700\t6764787273\t{audit}']
    lines += [f'U\t{n}\t'+f'u{n}'.encode().hex() for n in range(1,701)]
    lines += ['T\t0\t0\t','T\t1\t0\t6d656d626572','A\t1\t7\t4163726f\t7479706564']
    members=[1,2,254,508,700]
    rows=[([n],[zero]) for n in range(1,701)]
    symbols=[('*',1,0,700,0,b'Universe',rows),
             ('I',1,0,5,0,b'',[([n],[one]) for n in members]),
             ('J',1,4,0,1,b'',[([n],[one]) for n in members])]
    pkeys=[(1,1),(1,2),(1,254),(1,508),(2,1),(254,2),(508,254),(700,700)]
    pvalues=['s:undef','s:na','s:pinf','s:minf','s:eps','a:7',number(-0.0),number(-3.25)]
    symbols += [('P',2,1,8,0,b'',[(k,[v]) for k,v in zip(pkeys,pvalues)]),
                ('Wide',20,1,2,0,b'',[([1]*20,[number(.5)]),([1]*19+[2],[number(2.)])]),
                ('Scalar',0,1,1,0,b'',[([],[one])]),('Empty',0,1,1,0,b'',[([],[zero])])]
    for kind,users,prefix in [(2,range(0,10),'V'),(3,range(53,60),'E')]:
        symbols += [(f'{prefix}{user}',0,kind,1,user,b'',[([],[zero,one,number(-1.),number(.5),number(2.)])]) for user in users]
    for kind,users,prefix in [(2,range(0,10),'DefaultV'),(3,range(53,60),'DefaultE')]:
        lows = [zero,zero,zero,zero,'s:minf','s:minf',zero,zero,one,one] if kind==2 else [zero,zero,'s:minf','s:minf',zero,zero,zero]
        highs = [zero,one,'s:pinf','s:pinf',zero,'s:pinf','s:pinf','s:pinf','s:pinf','s:pinf'] if kind==2 else [zero,'s:pinf',zero,'s:pinf',zero,'s:pinf',zero]
        for index,user in enumerate(users):
            symbols.append((f'{prefix}{user}',0,kind,1,user,b'',[([],[zero,zero,lows[index],highs[index],one])]))
    symbols += [(f'W{n}',1,1,3,0,b'',[([k],[zero]) for k in [1,2,254]]) for n in range(4)]
    symbols += [('Bulk',2,1,7000,0,b'',[((a,b),[number(-3.25)]) for a in range(1,11) for b in range(1,701)])]
    for n,(name,dim,kind,count,user,description,rows) in enumerate(symbols):
        lines.append(f'S\t{n}\t{name.encode().hex()}\t{dim}\t{kind}\t{count}\t{user}\t{description.hex()}')
        if n:
            domains=['2:4a','0:2a'] if n==3 else ['0:2a']*dim
            lines.append(f'D\t{n}\t'+('3' if n==3 else '1')+''.join('\t'+d for d in domains))
        for keys,values in rows: lines.append(f'R\t{n}\t'+','.join(map(str,keys))+'\t'+','.join(values))
        lines.append(f'E\t{n}\t{len(rows)}')
    return '\n'.join(lines)+'\n'

def wire_check(data, compression, decode_blocks):
    """Check authored preambles/record bytes without using Rust's reader."""
    start=struct.unpack_from('<q',data,50)[0]
    # Do not decode concatenated sections: only first section ends at first
    # physical offset after symbols, obtained from the major table.
    end=struct.unpack_from('<q',data,66)[0]
    catalog=data[start:end]
    if compression: catalog=decode_blocks(catalog)[0]
    at=0
    def string():
        nonlocal at
        n=catalog[at];at+=1
        value=catalog[at:at+n];at+=n
        return value
    assert string()==b'_SYMB_'
    count=struct.unpack_from('<i',catalog,at)[0];at+=4
    entries=[]
    for _ in range(count):
        name=string();pos,dim=struct.unpack_from('<qi',catalog,at);at+=12
        kind=catalog[at];at+=1
        user,rows,errors=struct.unpack_from('<iii',catalog,at);at+=12
        at+=1;string()
        compressed,domain=catalog[at:at+2];at+=2
        if domain: at+=dim*4
        comments=struct.unpack_from('<i',catalog,at)[0];at+=4
        for _ in range(comments):string()
        entries.append((name,pos,dim,rows))
    positions=sorted({p for _,p,_,_ in entries if p}|{start})
    sections={}
    for name,pos,dim,rows in entries:
        if not pos:continue
        end=positions[positions.index(pos)+1]
        section=data[pos:end]
        if compression:section=decode_blocks(section)[0]
        assert section[:7]==b'\x06_DATA_' and section[7]==dim
        assert struct.unpack_from('<i',section,8)[0]==(-1 if name!=b'Empty' and not name.startswith(b'Default') else 0)
        assert section[-1:]==b'\xff'
        sections[name]=section
    # P has a two-byte width (range 700). Delta 252 remains compact in dim 2;
    # delta 254 uses an explicit suffix, and a leading change writes both keys.
    wanted=bytes.fromhex('01 0000 0000 00 03 01 fe 02 02 fb01 03 01 0100 0000 04')
    assert sections[b'P'][28:28+len(wanted)]==wanted
    for n,width in enumerate([1,2,2,4]):
        section=sections[f'W{n}'.encode()]
        assert section[20:]==b'\x01'+(1).to_bytes(width,'little')+b'\x05\x02\x05\xfd\x05\xff'
    # Bulk alone crosses at least two decoded 32 KiB blocks.
    assert len(sections[b'Bulk'])>32768
    return len(entries)

def check(run, area, executable, decode_blocks):
    cases=[]
    for compression in [0,1]:
        features=[[],['--all-features']]
        if not compression:features.append(['--no-default-features'])
        for feature in features:
            path=area/f'records-{compression}-{len(cases)}.gdx'
            run(['cargo','run','--locked','--quiet',*feature,'--example','create_records','--',path,area,*(['zlib'] if compression else [])],capture=True)
            with path.open('rb') as source:data=source.read(2*1024*1024+1)
            if len(data)>2*1024*1024:raise ValueError('fixed record recipe exceeds 2 MiB bound')
            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
            if native!=expected(compression,'6764787273') or rust!=expected(compression,'<creation-system>'):
                (area/'records-native.tsv').write_text(native)
                (area/'records-rust.tsv').write_text(rust)
                (area/'records-expected.tsv').write_text(expected(compression,'6764787273'))
                raise ValueError('complete sorted record streams differ from independent recipe')
            symbols=wire_check(data,compression,decode_blocks)
            cases.append(dict(features=feature,compression=compression,bytes=len(data),sha256=hashlib.sha256(data).hexdigest(),symbols=symbols,complete_native_and_rust_stream_matches=True,independent_key_bytes_match=True))
    return dict(cases=cases,native_isolation=isolation_policy(),scope='task 021 sorted raw records; 700 UELs and 45 symbols per case')
