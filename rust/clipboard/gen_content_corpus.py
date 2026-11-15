#!/usr/bin/python3
"""Gera tests/content_corpus.json: entradas e as respostas do read_content do Python (waybar_clipboard),
para o teste do porte em Rust conferir caso a caso. Rodar com o Python do sistema (o mesmo do popup):

    /usr/bin/python3 rust/clipboard/gen_content_corpus.py

As entradas: textos em UTF-8, Latin-1, UTF-16 (com e sem BOM, LE e BE), UTF-32, controles, CJK, emoji,
zeros soltos, binários conhecidos e bytes aleatórios (com e sem zeros), de vários tamanhos.
"""
import base64
import json
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "home/.local/bin"))
import waybar_clipboard as wc  # noqa: E402

TEXTS = ["hello", "https://x.com/a", "ação café AÇÃO", "中文 テキスト", "Привет мир", "👨‍👩‍👧 ok",
         "\x1b[31mred\x1b[0m", "tab\tand\nnewline", "一二三 中文", "ÿþ ok", " nbsp", "mix 中 a b c",
         "x" * 300, "Привет" * 40, "a\x1cb\x1dc", " private", "͸ unassigned", "﻿bom inside"]


def variants(text):
    out = [text.encode("utf-8"), text.encode("utf-16-le"), text.encode("utf-16"), b"\xfe\xff" + text.encode("utf-16-be"),
           text.encode("utf-32"), text.encode("utf-16-le") + b"\x00\x00", text.encode("utf-16-le") + b"\x00",
           text.encode("utf-8") + b"\x00", text.encode("utf-8").replace(b" ", b" \x00")]
    try:
        out.append(text.encode("latin-1"))
    except UnicodeEncodeError:
        pass
    return out


def main():
    rng = random.Random(20261112)
    inputs = []
    for text in TEXTS:
        inputs += variants(text)
    inputs += [b"", b"\x00", b"\x00\x00", b"\xff\xfe", b"\xfe\xff", b"\xff\xfe\x00\x00", b"\xff\xfe\x00\xd8\x00\x00lixo",
               b"%PDF-1.7\n%\xe2\xe3\n1 0 obj\x00\x01\x02 stream", b"PK\x03\x04\x14\x00\x00\x00", b"\x7fELF\x02\x01\x01\x00",
               bytes(range(256)) * 2, b"\xe9\x00", b"abc\x00de", bytes([0x89, 1, 0, 0x41, 0x42, 0x43, 0xe9, 0x20]) * 4]
    for _ in range(1500):  # bytes aleatórios, de 1 a 300
        n = rng.randrange(1, 300)
        if rng.random() < 0.5:
            inputs.append(bytes(rng.randrange(1, 256) for _ in range(n)))
        else:
            inputs.append(bytes(rng.randrange(256) for _ in range(n)))
    for _ in range(800):  # textos aleatórios de várias escritas, em codificações sortidas
        chars = "".join(chr(rng.choice([rng.randrange(32, 127), rng.randrange(0xa0, 0x250), rng.randrange(0x400, 0x500),
                                        rng.randrange(0x4e00, 0x4f00), rng.randrange(0x1f600, 0x1f650), rng.randrange(0, 32)]))
                        for _ in range(rng.randrange(1, 60)))
        inputs.append(rng.choice(variants(chars)))
    cases = []
    for data in inputs:
        c = wc.read_content(data)
        cases.append({"input": base64.b64encode(data).decode(), "kind": c.kind, "text": c.text, "label": c.label, "utf8": c.utf8})
    out = Path(__file__).resolve().parent / "tests" / "content_corpus.json"
    out.parent.mkdir(exist_ok=True)
    out.write_text(json.dumps({"python": sys.version.split()[0], "cases": cases}, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(cases)} casos em {out}")


if __name__ == "__main__":
    main()
