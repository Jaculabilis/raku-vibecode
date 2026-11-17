#!/usr/bin/env python3
"""Convert a restricted Python source file into Zaddy py.* production syntax.

Each indented block becomes py.Compound('head', [...]); other lines become
py.Statement('...').  Zaddy string literals cannot contain single quotes, and
their contents pass through one level of Python string-literal unescaping, so
backslashes are doubled here.
"""
import sys

def parse(lines):
    items = []  # (indent, text)
    for raw in lines:
        if not raw.strip() or raw.strip().startswith('#'):
            continue
        ind = len(raw) - len(raw.lstrip(' '))
        items.append((ind, raw.strip()))
    pos = 0
    def block(indent):
        nonlocal pos
        out = []
        while pos < len(items) and items[pos][0] == indent:
            ind, text = items[pos]
            pos += 1
            if pos < len(items) and items[pos][0] > indent:
                assert text.endswith(':'), text
                out.append(('C', text[:-1], block(items[pos][0])))
            else:
                out.append(('S', text))
        if pos < len(items) and items[pos][0] > indent:
            raise SystemExit('bad indentation near: %r' % (items[pos],))
        return out
    rv = block(0)
    assert pos == len(items)
    return rv

def lit(s):
    if "'" in s:
        raise SystemExit('single quote in line: ' + s)
    return "'" + s.replace('\\', '\\\\') + "'"

def emit(nodes, indent):
    pad = ' ' * indent
    parts = []
    for n in nodes:
        if n[0] == 'S':
            parts.append(pad + 'py.Statement(' + lit(n[1]) + ')')
        elif not n[2]:
            parts.append(pad + 'py.Compound(' + lit(n[1]) + ', [])')
        else:
            parts.append(pad + 'py.Compound(' + lit(n[1]) + ', [\n' + emit(n[2], indent + 4) + '\n' + pad + '])')
    return ',\n'.join(parts)

if __name__ == '__main__':
    print(emit(parse(open(sys.argv[1]).read().split('\n')), int(sys.argv[2]) if len(sys.argv) > 2 else 0))
