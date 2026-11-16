#!/usr/bin/env python3
"""Moves the game's own texts out of the rewrite's source: every Rust string literal whose text is an ldstr of the
player's Assembly-CSharp.dll (and reads as text: two or more words, or one lowercase word the C# method named in the comments above loads) becomes `crate::il!(str "Type::Method" k)`.

Needs the DLL's table as TSV first (it reads the installed game; nothing from it is committed):
    IL_DUMP=il.tsv cargo test --bin people-playground-bevy il_dump -- --ignored
    python scripts/il_strings.py il.tsv src/game/menu.rs [more files...]
Sites the compiler refuses (consts, patterns, format strings) go back to the literal; the script compiles until none is
left (at most 8 rounds) and prints the sites it kept and gave back.
"""
import collections, json, os, re, subprocess, sys

TYPE_METHOD = re.compile(r'\b([A-Z][A-Za-z0-9_]*(?:\+[A-Za-z0-9_]+)?)\.([A-Za-z_][A-Za-z0-9_<>]*)')
ESC = {'n': '\n', 'r': '\r', 't': '\t', '\\': '\\', '"': '"', "'": "'", '0': '\0'}


def decode(raw):
    out, i = [], 0
    while i < len(raw):
        c = raw[i]
        if c != '\\':
            out.append(c); i += 1; continue
        n = raw[i + 1]
        if n in ESC:
            out.append(ESC[n]); i += 2
        elif n == 'x':
            out.append(chr(int(raw[i + 2:i + 4], 16))); i += 4
        elif n == 'u':
            j = raw.index('}', i)
            out.append(chr(int(raw[i + 3:j], 16))); i = j + 1
        elif n in '\r\n':
            i += 1
            while i < len(raw) and raw[i] in ' \t\r\n':
                i += 1
        else:
            return None
    return ''.join(out)


def literals(text):
    """(start, end, raw) of each plain string literal outside comments, raw strings, byte strings and chars."""
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith('//', i):
            i = text.find('\n', i)
            i = n if i < 0 else i
        elif text.startswith('/*', i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith('/*', i):
                    depth, i = depth + 1, i + 2
                elif text.startswith('*/', i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif c == 'r' and re.match(r'r#*"', text[i:]) and not (i and (text[i - 1].isalnum() or text[i - 1] == '_')):
            hashes = re.match(r'r(#*)"', text[i:]).group(1)
            i = text.index('"' + hashes, i + len(hashes) + 2) + 1 + len(hashes)
        elif c == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == '\\' else 1
            prefix = text[i - 1] if i else ''
            if not (prefix.isalnum() or prefix == '_'):
                yield i, j + 1, text[i + 1:j]
            i = j + 1
        elif c == "'":
            m = re.match(r"'(\\u\{[0-9a-fA-F]+\}|\\x[0-9a-fA-F]{2}|\\.|[^\\'])'", text[i:])
            i += len(m.group(0)) if m else 1
        else:
            i += 1


def main():
    tsv, files = sys.argv[1], sys.argv[2:]
    where = collections.defaultdict(list)
    for line in open(tsv, encoding='utf-8'):
        m, kind, k, v = line.rstrip('\n').split('\t')
        if kind == 'str':
            where[json.loads(v)].append((m, int(k)))
    for locs in where.values():
        locs.sort(key=lambda x: (x[0], x[1]))
    originals, sites = {}, {}
    for f in files:
        text = open(f, 'rb').read().decode('utf-8')
        originals[f] = text
        found = []
        for start, end, raw in literals(text):
            v = decode(raw)
            if v is None or v not in where:
                continue
            single = len(v.split()) < 2
            line_start = text.rfind('\n', 0, start) + 1
            if text[line_start:start].lstrip().startswith('#'):
                continue
            after = text[end:end + 4].lstrip()
            if after.startswith('=>') or (after.startswith('|') and not after.startswith('||')):
                continue
            # The C# method the code names nearest above (its comments), else any that holds the text.
            above = text[max(0, start - 6000):start]
            named = [f'{t.replace("+", "/")}::{m}' for t, m in TYPE_METHOD.findall(above)][::-1]
            types = [n.split('::')[0] for n in named]
            locs = where[v]
            pick = next((l for n in named for l in locs if l[0] == n), None) \
                or next((l for t in types for l in locs if l[0].split('::')[0] == t), None) or locs[0]
            # One word: only a lowercase one that the named method itself loads (identifiers stay).
            if single and not (re.fullmatch(r'[a-z][a-z-]*', v) and any(l[0] == n for n in named for l in locs)):
                continue
            found.append([start, end, f'crate::il!(str "{pick[0]}" {pick[1]})', True])
        sites[f] = found
    for round in range(8):
        spans = {}
        for f in files:
            out, pos, b = [], 0, 0
            text = originals[f]
            for s in sites[f]:
                piece = text[pos:s[0]]
                out.append(piece); b += len(piece.encode())
                rep = s[2] if s[3] else text[s[0]:s[1]]
                if s[3]:
                    spans.setdefault(os.path.normpath(f), []).append((b, b + len(rep.encode()), s))
                out.append(rep); b += len(rep.encode()); pos = s[1]
            out.append(text[pos:])
            open(f, 'wb').write(''.join(out).encode())
        r = subprocess.run(['cargo', 'test', '--no-run', '-q', '--profile', 'fastrel', '--bin', 'people-playground-bevy',
                            '--message-format=json'], capture_output=True, text=True, encoding='utf-8')
        bad, unmapped = set(), []
        for line in r.stdout.splitlines():
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if msg.get('reason') != 'compiler-message' or msg['message']['level'] != 'error':
                continue
            hit = False
            stack = list(msg['message']['spans']) + [s for c in msg['message']['children'] for s in c['spans']]
            while stack:
                sp = stack.pop()
                for b0, b1, s in spans.get(os.path.normpath(sp['file_name']), []):
                    if b0 <= sp['byte_start'] < b1:
                        bad.add(id(s)); s[3] = False; hit = True
                if sp.get('expansion'):
                    stack.append(sp['expansion']['span'])
            if not hit:
                unmapped.append(msg['message']['rendered'])
        if unmapped and not bad:
            print('\n'.join(unmapped[:10]))
            sys.exit('errors outside the converted sites')
        print(f'round {round}: {len(bad)} sites given back')
        if not bad and r.returncode == 0:
            break
    kept = sum(s[3] for f in files for s in sites[f])
    back = sum(not s[3] for f in files for s in sites[f])
    for f in files:
        for s in sites[f]:
            if not s[3]:
                line = originals[f].count('\n', 0, s[0]) + 1
                print(f'given back: {f}:{line} {originals[f][s[0]:s[1]][:60]}')
    print(f'converted {kept}, given back {back}')


main()
