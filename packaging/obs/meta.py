#!/usr/bin/env python3
"""O meta do projeto do OBS com os alvos do meta/repositories.xml: lê o meta atual (osc meta prj) e troca
só os <repository>, sem mexer no título, nas pessoas nem nas flags. Os <repository> ficam no fim, onde o
OBS os espera.
uso: osc meta prj PROJETO | meta.py > novo.xml && osc meta prj PROJETO -F novo.xml"""
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

REPOSITORIES = Path(__file__).resolve().parent / "meta/repositories.xml"


def merge(current, repositories):
    project = ET.fromstring(current)
    if project.tag != "project":
        raise ValueError(f"not a project meta: <{project.tag}>")
    for old in project.findall("repository"):
        project.remove(old)
    for repository in ET.fromstring(repositories).findall("repository"):
        project.append(repository)
    ET.indent(project)
    return ET.tostring(project, encoding="unicode") + "\n"


def main():
    try:
        sys.stdout.write(merge(sys.stdin.read(), REPOSITORIES.read_text(encoding="utf-8")))
    except (ValueError, ET.ParseError) as error:
        sys.exit(f"meta.py: {error}")


if __name__ == "__main__":
    main()
