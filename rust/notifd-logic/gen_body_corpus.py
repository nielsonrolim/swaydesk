"""Gera tests/body_corpus.json: entradas hostis e a saída do clean_body do Python, para conferir o porte.

Rodar com o /usr/bin/python3 do sistema (o HTMLParser muda entre versões do Python; o porte segue o do
3.12 com as correções de segurança):  /usr/bin/python3 rust/notifd-logic/gen_body_corpus.py
"""
import json
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "home", ".local", "bin"))
import waybar_notifications as wn  # noqa: E402

FIXED = [
    "", "   ", "texto simples", "<b>a</b> <i>b</i> <u>c</u>", '<a href="http://x">site</a>',
    '<img src="/etc/passwd" alt="foto"/>', '<span foreground="red">x</span>', '<b size="99999">x</b>',
    "1 < 2 & 3 &amp; 4", "<b><i>x", "<b><i>x</b>y</i>", "x</b>", "a <b", "a\x01b\nc", "a<br/>b",
    "x" * 5000, "&eacute;t&eacutee &notit; &notin; &amp &AMP; &#60; &#x3C &#0; &#128; &#xD800; &#99999999999;",
    "&#xFFFF; &#x1F600; &#; &#x; &", "fim &amp", "fim &amp;", "fim & x", "<script>alert(1)<b></script>x",
    "<SCRIPT>a</script >b", "<style>a</STYLE>", "<textarea>&amp;<b></textarea>", "<title>t</title>",
    "<plaintext><b>x</b>", "<xmp><b></xmp>", "<!-- c -->d", "<!-- c --!>d", "<!-->x", "<!--->x", "<!-- sem fim",
    "<?pi x>y", "<!doctype html>z", "<!DOCTYPE>", "<![CDATA[x]]>y", "<![if x]>y", "<![x>y", "<!x>y", "</ x>y",
    "</>y", "</", "<", "<<b>>", "<b/>x", "<a/>x", "<img/>", "<img alt=a alt=b>", "<img alt>", "<img alt='&lt;b&gt;'>",
    '<img alt="x>y">', "<b class='x>y'>z</b>", "<b a='unclosed>z", '<b =x>y</b>', "<b/ >y", "<b / x>y</b>",
    "<b\nclass=1>y</b>", "<b\x0cx>", "<İ>x</İ>", "<B>x</B>", "<b> </b>", " a ", "　x　",
    "</b\n>", "</b x='>'>", "<br>a<br>", "<img src=x onerror=alert(1)>", "<b>a<i>b<u>c</b>d</i>e</u>",
    "<a href='javascript:x'><b>link</b></a>", "a\tb\r\nc", '<img alt= "--><b>x', "<img alt=\n'</b>y", '<b a= "z', "\x85\x9f\x7f", "< b>", "<1>", "<b>&lt;i&gt;</b>",
]

TOKENS = [
    "<", ">", "</", "/", "/>", "=", '"', "'", " ", "\n", "\t", "a", "B", "x", "é", "&", ";", "#",
    "<b>", "</b>", "<i>", "</i>", "<u>", "</u>", "<b", "<i ", "<a href=", "</a>", "<br>", "<br/>", "<img alt=",
    "<span>", "</span>", "&amp", "&amp;", "&lt;", "&#60;", "&#x3c", "&eacute", "&notit;", "&#0;", "&#128;",
    "<!--", "-->", "--!>", "-", "<!", "<?", "<![CDATA[", "]]>", "<!doctype", "<script>", "</script>",
    "<style>", "</style", "<textarea>", "</textarea>", "<title>", "<plaintext>", "\x01", " ", " ",
]


def main():
    rng = random.Random(20261112)
    inputs = list(FIXED)
    for _ in range(2500):
        inputs.append("".join(rng.choice(TOKENS) for _ in range(rng.randint(1, 14))))
    corpus = [{"input": text, "output": wn.clean_body(text)} for text in inputs]
    out = os.path.join(HERE, "tests", "body_corpus.json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(corpus, f, ensure_ascii=False, indent=0)
        f.write("\n")


main()
