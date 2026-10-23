"""Authored fixture inputs and independent public-API expectations.

No bytes are decoded here and no oracle output is learned as an expectation.
The input/expected model is deliberately limited to the small fixture corpus.
"""

from oracle import finite


def hx(value):
    return value.hex() if isinstance(value, bytes) else value.encode().hex()


class Case:
    def __init__(self, name, compression, labels):
        self.name, self.compression, self.labels = name, compression, labels
        self.recipe = ["U\t" + hx(label) for label in labels]
        self.symbols, self.texts, self.acronyms, self.events = [], [""], [], []

    def symbol(self, name, dim, kind=1, user=0, description="", mode="raw", bounds=None):
        line = ["S", hx(name), str(dim), str(kind), str(user), hx(description), mode]
        if bounds:
            line += [','.join(map(str, bounds[0])), ','.join(map(str, bounds[1]))]
        self.recipe.append('\t'.join(line))
        item = dict(name=name, dim=dim, kind=kind, user=user, description=description,
                    records=[], comments=[], domains=[(0, '*')]*dim, domain_mode=1, mode=mode)
        self.symbols.append(item)
        return item

    def row(self, keys, values, input_keys=None, expected_values=None):
        item = self.symbols[-1]
        encoded = keys if input_keys is None else input_keys
        if item['mode'] == 'str':
            encoded = [hx(v) for v in encoded]
        self.recipe.append('K\t' + ','.join(map(str, encoded)) + '\t' + ','.join(values))
        item['records'].append((keys, values if expected_values is None else expected_values))

    def end(self):
        self.recipe.append('END')
        self.symbols[-1]['records'].sort(key=lambda row: row[0])

    def alias(self, target, name):
        self.recipe.append('ALIAS\t' + hx(target) + '\t' + hx(name))
        source = next((i for i, s in enumerate(self.symbols, 1) if s['name'] == target), 0)
        dim = self.symbols[source-1]['dim'] if source else 1
        records = self.symbols[source-1]['records'] if source else [([i], [finite(0)]) for i in range(1, len(self.labels)+1)]
        self.symbols.append(dict(name=name, dim=dim, kind=4, user=source, description='Aliased with '+target,
                                 records=records, comments=[], domains=[(0, '*')]*dim, domain_mode=1, mode='alias'))

    def expectation(self):
        lines = ['GDXRS-ORACLE\t1', '\t'.join(['F', '7', str(self.compression), str(len(self.symbols)), str(len(self.labels)), hx('gdxrs-fixtures'), '<creation-system>'])]
        lines += [f'U\t{i}\t{hx(label)}' for i, label in enumerate(self.labels, 1)]
        lines += [f'T\t{i}\t0\t{hx(text)}' for i, text in enumerate(self.texts)]
        lines += [f'A\t{i}\t{idx}\t{hx(name)}\t{hx(text)}' for i, (name, text, idx) in enumerate(self.acronyms, 1)]
        universe = dict(name='*', dim=1, kind=0, user=0, description='Universe',
                        records=[([i], [finite(0)]) for i in range(1, len(self.labels)+1)])
        for n, item in enumerate([universe, *self.symbols]):
            rows = item['records']
            count = 0 if item['kind'] == 4 else len(rows)
            lines.append('\t'.join(map(str, ['S', n, hx(item['name']), item['dim'], item['kind'], count, item['user'], hx(item['description'])])))
            if n:
                lines.append('\t'.join(map(str, ['D', n, item['domain_mode'], *[str(i)+':'+hx(label) for i, label in item['domains']]])))
                lines += [f'C\t{n}\t{i}\t{hx(text)}' for i, text in enumerate(item['comments'], 1)]
            lines += [f"R\t{n}\t{','.join(map(str, keys))}\t{','.join(values)}" for keys, values in rows]
            lines.append(f'E\t{n}\t{len(rows)}')
        return '\n'.join(lines) + '\n'


def semantic(compression):
    c = Case(f'semantics-{compression}', compression, ['z', 'a', 'C', b'\xff', 'l'*63])
    # The pinned native registration table is case-insensitive: adding A after
    # a succeeds but must NOT introduce another ID. Case-distinct stored labels
    # are tested separately via a narrowly annotated, oracle-verified transform.
    c.recipe += ['U\t41']
    c.recipe += ['T\t'+hx('element text'), 'A\t'+hx('ac')+'\t'+hx('acronym')+'\t7']
    c.texts += ['element text']; c.acronyms += [('ac', 'acronym', 7)]
    i = c.symbol('i', 1, 0, description='set')
    for key in range(1, 6): c.row([key], [finite(1 if key == 1 else 0)])
    c.end()
    i['comments'] = ['c'*255]; c.recipe += ['C\t'+hx('c'*255)]
    c.alias('i', 'ii'); c.alias('*', 'all')
    c.symbol('empty', 1, 0); c.end()
    c.symbol('scalar', 0); c.row([], [finite(2.5)]); c.end()
    empty = c.symbol('default_scalar', 0); c.end(); empty['records'] = [([], [finite(0)])]
    p = c.symbol('n'+'s'*62, 2, description='d'*255)
    p['domains'] = [(1, 'i'), (0, '*')]; p['domain_mode'] = 3
    c.recipe += ['D\t'+hx('i')+'\t'+hx('*')]
    values = ['s:undef', 's:na', 's:pinf', 's:minf', 's:eps', finite(0), finite(1), finite(-1), finite(.5), finite(2), finite(2.5), 'a:7', finite(-0.0), 'f:7ff8000000000001', 'f:7ff0000000000000', 'f:fff0000000000000']
    normalized = {12: finite(0), 13:'s:na', 14:'s:pinf', 15:'s:minf'}
    for k, value in enumerate(values): c.row([k//4+1, k%4+1], [value], expected_values=[normalized.get(k, value)])
    c.end()
    p = c.symbol('relaxed', 1); c.row([1], [finite(3.5)]); c.end()
    p['domain_mode'] = 2; p['domains'] = [(0, 'not_registered')]
    c.recipe += ['X\t'+hx('not_registered')]
    c.symbol('twenty', 20, 2, 5); c.row([1]*20, [finite(1.25), finite(-2.5), 's:minf', 's:pinf', finite(4)]); c.end()
    for user in range(1, 10):
        low = 's:minf' if user in (4,5) else finite(1 if user in (8,9) else 0)
        high = finite(1) if user == 1 else finite(0) if user == 4 else 's:pinf'
        item = c.symbol('v'+str(user), 0, 2, user); c.end()
        item['records'] = [([], [finite(0), finite(0), low, high, finite(1)])]
    for subtype in range(7):
        low = 's:minf' if subtype in (2,3) else finite(0)
        high = 's:pinf' if subtype in (1,3,5) else finite(0)
        item = c.symbol('e'+str(subtype), 0, 3, 53+subtype); c.end()
        item['records'] = [([], [finite(0), finite(0), low, high, finite(1)])]
    return c


def keys_case():
    c = Case('key-widths', 0, ['u'+str(i) for i in range(1, 65537)])
    c.recipe = ['URANGE\t65536']
    for high in (255, 256, 65535, 65536):
        c.symbol('w'+str(high), 1, mode='bound', bounds=([1],[high]))
        for key in (1, 2, 254, high): c.row([key], [finite(2.5)])
        c.end()
    c.symbol('prefix', 2, mode='bound', bounds=([1,1],[65536,65536]))
    for key in ([1,1],[1,2],[1,254],[1,508],[2,1],[65536,65536]): c.row(key, [finite(-3.25)])
    c.end()
    return c


def blocks_case():
    c = Case('blocks', 1, ['u'+str(i) for i in range(1, 5001)])
    c.recipe = ['URANGE\t5000']
    c.symbol('data', 1)
    # Reproducible nonconstant binary64 payloads, always finite and below acronym threshold.
    state = 0x123456789abcdef
    for key in range(1, 5001):
        state = (state * 6364136223846793005 + 1442695040888963407) & ((1 << 64)-1)
        value = 'f:' + format(0x3ff0000000000000 | (state & ((1 << 52)-1)), '016x')
        c.row([key], [value])
    c.end()
    return c


def operations_case():
    c = Case('operations', 0, ['z', 'a', 'b'])
    c.recipe += ['M\t10\t'+hx('z'), 'M\t20\t'+hx('a')]
    c.symbol('mapped', 1, mode='map')
    c.row([2], [finite(8)], input_keys=[20]); c.row([1], [finite(9)], input_keys=[10]); c.end()
    c.symbol('strings', 1, mode='str')
    c.row([3], [finite(2)], input_keys=['b']); c.row([1], [finite(3)], input_keys=['z']); c.end()
    c.symbol('i', 1, 0); c.row([1], [finite(0)]); c.end()
    c.symbol('errors', 1)
    c.recipe += ['D\t'+hx('i')]
    c.symbols[-1].update(domain_mode=3, domains=[(3,'i')])
    c.row([1], [finite(2.5)])
    c.recipe += ['!K\t1\t'+finite(3), '!K\t2\t'+finite(4)]
    c.events += ['REJECT\tK\t-100009\t1', 'REJECT\tK\t-100058\t2']
    c.end()
    c.recipe += ['!S\t'+hx('errors')+'\t1\t1\t0\t\traw']
    c.events += ['REJECT\tS\t-100007\t0']
    c.recipe += ['APPEND']
    c.symbol('appended', 0); c.row([], [finite(42)]); c.end()
    return c


def optional_case():
    c = Case('optional-domains', 0, ['one'])
    c.symbol('p', 1); c.row([1], [finite(2.5)]); c.end()
    return c


def cases():
    return [semantic(0), semantic(1), keys_case(), blocks_case(), operations_case(), optional_case()]
