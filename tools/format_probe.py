"""Independent v7 byte annotations for small development fixtures, not production I/O.

Bounded to 16 MiB input/decoded data and 100,000 entries. No Rust parser is used.
Numeric spans permit a narrowly defined opposite-endian fixture transformation.
"""

import struct
import zlib

LIMIT = 16 * 1024 * 1024


class Cursor:
    def __init__(self, data, order="little", base=0, spans=None):
        if len(data) > LIMIT:
            raise ValueError("probe data limit")
        self.data, self.at, self.order, self.base = data, 0, order, base
        self.spans = spans

    def take(self, n):
        if n < 0 or n > len(self.data) - self.at:
            raise ValueError("probe truncated field")
        value = self.data[self.at:self.at + n]
        self.at += n
        return value

    def number(self, n, signed=False):
        start = self.at
        value = int.from_bytes(self.take(n), self.order, signed=signed)
        if self.spans is not None and n > 1:
            self.spans.append((self.base + start, n))
        return value

    def byte(self):
        return self.number(1)

    def i32(self):
        return self.number(4, True)

    def count(self):
        n = self.i32()
        if not 0 <= n <= 100000:
            raise ValueError("probe count limit")
        return n

    def string(self):
        return self.take(self.byte())

    def marker(self, expected):
        if self.string() != expected:
            raise ValueError(f"probe bad marker {expected!r}")

    def done(self):
        if self.at != len(self.data):
            raise ValueError(f"probe trailing section data: {len(self.data)-self.at}")


class Probe:
    def __init__(self, data):
        if not data or len(data) > LIMIT:
            raise ValueError("probe input limit")
        self.data, self.spans, self.blocks, self.symbols = data, [], [], []
        h = Cursor(data, spans=self.spans)
        patterns = [(2, 0x1234), (4, 0x12345678), (8, int.from_bytes(struct.pack('<d', 3.141592653589793), 'little'))]
        orders = []
        for size, pattern in patterns:
            if h.byte() != size:
                raise ValueError("probe bad primitive size")
            start = h.at
            raw = h.take(size)
            self.spans.append((start, size))
            if raw == pattern.to_bytes(size, 'little'):
                orders.append('little')
            elif raw == pattern.to_bytes(size, 'big'):
                orders.append('big')
            else:
                raise ValueError("probe bad byte-order pattern")
        if len(set(orders)) != 1:
            raise ValueError("probe mixed primitive byte orders are not covered")
        self.order = h.order = orders[0]
        if h.byte() != 123:
            raise ValueError("probe bad magic")
        h.marker(b'GAMSGDX')
        self.version_offset = h.at
        if h.i32() != 7:
            raise ValueError("probe unsupported version")
        self.compression = h.i32()
        if self.compression not in (0, 1):
            raise ValueError("probe invalid compression")
        self.audit = h.string()
        self.producer = h.string()
        self.index_offset = h.at
        if h.i32() != 19510624:
            raise ValueError("probe bad major-index marker")
        self.offsets = [h.number(8, True) for _ in range(6)]
        self.data_start = self.index_offset + 80
        active = [o for o in self.offsets if o]
        if any(o < self.data_start or o >= len(data) for o in active):
            raise ValueError("probe section offset outside file")
        self._metadata()
        positions = sorted(s['position'] for s in self.symbols if s['type'] != 4)
        self.records = []
        for symbol in self.symbols:
            if symbol['type'] != 4:
                start = symbol['position']
                end = min([p for p in positions if p > start] + [self.offsets[0]])
                self.records.append(self._records(symbol, start, end))

    def section(self, start, end, compressed):
        if not 0 <= start <= end <= len(self.data):
            raise ValueError('probe section bounds')
        raw = self.data[start:end]
        if not compressed:
            return Cursor(raw, self.order, start, self.spans), []
        pieces, blocks, physical, decoded = [], [], start, 0
        while physical < end:
            if end - physical < 3:
                raise ValueError('probe truncated block header')
            tag = self.data[physical]
            length = int.from_bytes(self.data[physical+1:physical+3], 'big')
            if not length or length > end - physical - 3 or tag not in (0, 1):
                raise ValueError('probe invalid block framing')
            payload = self.data[physical+3:physical+3+length]
            if tag:
                decoder = zlib.decompressobj()
                try:
                    payload = decoder.decompress(payload, 32769)
                except zlib.error as exc:
                    raise ValueError('probe invalid zlib/checksum') from exc
                if not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
                    raise ValueError('probe invalid zlib framing/size')
            if not 0 < len(payload) <= 32768 or decoded + len(payload) > LIMIT:
                raise ValueError('probe decoded size limit')
            block = dict(physical=physical, tag=tag, encoded=length,
                         decoded_start=decoded, decoded_end=decoded + len(payload))
            blocks.append(block); self.blocks.append(block)
            pieces.append(payload); decoded += len(payload); physical += 3 + length
        return Cursor(b''.join(pieces), self.order), blocks

    def meta_section(self, index):
        start = self.offsets[index]
        end = min([o for o in self.offsets if o > start] + [len(self.data)])
        return self.section(start, end, self.compression)[0]

    def _metadata(self):
        s = self.meta_section(0)
        s.marker(b'_SYMB_')
        for n in range(s.count()):
            item = dict(id=n+1, name=s.string())
            item['position_offset'] = s.base + s.at if s.spans is not None else None
            item['position'] = s.number(8, True)
            item['dimension'] = s.i32(); item['type'] = s.byte()
            item['user'] = s.i32(); item['count'] = s.i32(); item['errors'] = s.i32()
            item['has_text'] = s.byte(); item['description'] = s.string()
            item['compressed'] = s.byte()
            if not 0 <= item['dimension'] <= 20 or not 0 <= item['type'] <= 4:
                raise ValueError('probe invalid symbol type/dimension')
            item['domains'] = [s.i32() for _ in range(item['dimension'])] if s.byte() else None
            item['comments'] = [s.string() for _ in range(s.count())]
            self.symbols.append(item)
        s.marker(b'_SYMB_'); s.done()
        self.tables = {}
        for index, name in [(2, b'_SETT_'), (1, b'_UEL_')]:
            s = self.meta_section(index); s.marker(name)
            self.tables[name.decode()] = [s.string() for _ in range(s.count())]
            s.marker(name); s.done()
        s = self.meta_section(3); s.marker(b'_ACRO_')
        self.acronyms = [(s.string(), s.string(), s.i32()) for _ in range(s.count())]
        s.marker(b'_ACRO_'); s.done()
        self.domain_strings, self.relaxed = [], []
        if self.offsets[5]:
            s = self.meta_section(5); s.marker(b'_DOMS_')
            self.domain_strings = [s.string() for _ in range(s.count())]
            s.marker(b'_DOMS_')
            while True:
                symbol = s.i32()
                if symbol <= 0:
                    break
                if symbol > len(self.symbols):
                    raise ValueError('probe invalid relaxed-domain symbol')
                self.relaxed.append((symbol, [s.i32() for _ in range(self.symbols[symbol-1]['dimension'])]))
            s.marker(b'_DOMS_'); s.done()

    def _records(self, item, start, end):
        s, blocks = self.section(start, end, item['compressed'])
        s.marker(b'_DATA_')
        dim_offset = s.at
        dim = s.byte()
        if dim != item['dimension']:
            raise ValueError('probe dimension marker mismatch')
        stored_count = s.i32()
        bounds = [(s.i32(), s.i32()) for _ in range(dim)]
        widths = [1 if 0 < hi-lo+1 <= 255 else 2 if 0 < hi-lo+1 <= 65535 else 4 for lo, hi in bounds]
        keys = [-256] * dim
        tags, rows, annotations, crossings, first_tags = set(), [], [], 0, set()
        while True:
            first = s.at
            tag = s.byte()
            if tag == 255:
                break
            first_tags.add(tag)
            if tag > dim:
                if dim:
                    keys[-1] += tag - dim
            else:
                if not 1 <= tag <= dim:
                    raise ValueError('probe invalid key tag')
                for d in range(tag - 1, dim):
                    keys[d] = s.number(widths[d], widths[d] == 4) + bounds[d][0]
            if any(not 1 <= key <= len(self.tables['_UEL_']) for key in keys):
                raise ValueError('probe invalid raw key')
            values = []
            for _ in range(5 if item['type'] in (2, 3) else 1):
                value_tag = s.byte(); tags.add(value_tag)
                if value_tag > 10:
                    raise ValueError('probe invalid value tag')
                bits = s.number(8) if value_tag == 10 else None
                values.append((value_tag, bits))
            rows.append((tuple(keys), values))
            if len(rows) > 100000:
                raise ValueError('probe record limit')
            crossings += any(first < b['decoded_end'] < s.at for b in blocks)
            if len(annotations) < 8:
                annotations.append(dict(start=first, end=s.at, hex=s.data[first:s.at].hex(), keys=list(keys)))
        s.done()
        if len(rows) != item['count']:
            raise ValueError('probe stored record count mismatch')
        return dict(symbol=item['id'], widths=widths, bounds=bounds, header_count=stored_count,
                    rows=rows, tags=sorted(tags), first_tags=sorted(first_tags),
                    crossing_records=crossings, blocks=blocks, annotations=annotations,
                    dimension_offset=start+dim_offset if not item['compressed'] else None,
                    record_offset=start+annotations[0]['start'] if annotations and not item['compressed'] else None)

    def summary(self):
        return dict(byte_order=self.order, compression=self.compression,
                    symbols=len(self.symbols), uels=len(self.tables['_UEL_']),
                    widths=sorted({w for r in self.records for w in r['widths']}),
                    tags=sorted({t for r in self.records for t in r['tags']}),
                    stored_records=sum(len(r['rows']) for r in self.records),
                    compressed_blocks=sum(b['tag'] == 1 for b in self.blocks),
                    stored_blocks=sum(b['tag'] == 0 for b in self.blocks),
                    crossing_records=sum(r['crossing_records'] for r in self.records),
                    multiple_records_per_block=any(len(r['rows']) > len(r['blocks']) and r['blocks'] for r in self.records))

    def opposite_endian(self):
        if self.compression:
            raise ValueError('endian transformation requires an uncompressed fixture')
        result = bytearray(self.data)
        previous_end = 0
        for at, size in sorted(self.spans):
            if at < previous_end:
                raise ValueError('overlapping numeric annotations')
            result[at:at+size] = result[at:at+size][::-1]
            previous_end = at + size
        return bytes(result)
