"""Gera rust/notifd-logic/src/entities.rs com as tabelas do html.unescape do Python."""
import html, sys
from html import entities

def lit(s):
    out = []
    for c in s:
        if c in '"\\':
            out.append('\\' + c)
        elif 0x20 <= ord(c) < 0x7f:
            out.append(c)
        else:
            out.append('\\u{%x}' % ord(c))
    return '"' + ''.join(out) + '"'

names = sorted(entities.html5)
lines = ["// Gerado por rust/notifd-logic/gen_entities.py a partir do html.entities.html5 do Python: não editar.",
         "// As referências nomeadas do HTML5 (com e sem o ponto e vírgula), como o html.unescape as conhece.",
         "pub(crate) const HTML5: &[(&str, &str)] = &["]
for n in names:
    lines.append(f"    ({lit(n)}, {lit(entities.html5[n])}),")
lines.append("];")
lines.append("")
lines.append("/// As referências numéricas que o html.unescape troca (as do windows-1252 e o nulo).")
lines.append("pub(crate) const INVALID_CHARREFS: &[(u32, &str)] = &[")
for k, v in sorted(html._invalid_charrefs.items()):
    lines.append(f"    ({k:#x}, {lit(v)}),")
lines.append("];")
lines.append("")
lines.append("/// Os pontos de código que o html.unescape apaga.")
lines.append("pub(crate) const INVALID_CODEPOINTS: &[u32] = &[")
for k in sorted(html._invalid_codepoints):
    lines.append(f"    {k:#x},")
lines.append("];")
sys.stdout.write("\n".join(lines) + "\n")
