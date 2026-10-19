"""The game's buildings and resource nodes, read from assets/data/**/*.ron.

The build needs three things per id: whether it is a building or a node,
its box (`size: (w, h, d)` in metres, what the engine fits the model to)
and, for nodes, the resource (trees are the nodes that give wood). It reads
them with a small RON reader (`parse`) instead of a full schema, so a new
field in the content never breaks the art build.
"""

import os
import re
from dataclasses import dataclass

KINDS = ("buildings", "nodes")  # the folder names, and the content's list names


@dataclass(frozen=True)
class Entry:
    kind: str          # "buildings" | "nodes"
    id: str
    size: tuple        # (w, h, d) metres: the box the engine fits the model to
    resource: str = ""  # nodes: what it gives ("wood", "stone"...)
    source: str = ""   # content file, for messages

    @property
    def is_tree(self):
        return self.kind == "nodes" and self.resource == "wood"


def load(data_dir):
    """{id: Entry} for every building and node under `data_dir` (recursive)."""
    out = {}
    for root, _, files in os.walk(data_dir):
        for name in sorted(files):
            if not name.endswith(".ron"):
                continue
            path = os.path.join(root, name)
            with open(path, encoding="utf-8") as f:
                doc = parse(f.read(), path)
            if not isinstance(doc, dict):
                continue
            for kind in KINDS:
                for item in doc.get(kind, ()) or ():
                    if not isinstance(item, dict) or not isinstance(item.get("id"), str):
                        continue
                    size = item.get("size")
                    size = tuple(float(v) for v in size) if isinstance(size, list) and len(size) == 3 else None
                    resource = str(item.get("resource") or "")
                    entry = Entry(kind, item["id"], size, resource, os.path.relpath(path, data_dir))
                    prev = out.get(entry.id)
                    if prev is None or (prev.size is None and size is not None):
                        out[entry.id] = entry
    return out


# --- a small RON reader ---------------------------------------------------
#
# Structs `Name(a: 1, b: "x")` and `(a: 1)` -> dict (an enum variant's name
# under "__variant"), tuples `(1, 2)` -> list, lists -> list, maps
# `{"k": v}` -> dict, strings -> str, numbers -> float/int, bare identifiers
# (true, false, None, enum variants) -> str. `Some(x)` -> x.

_TOKEN = re.compile(r"""
    (?P<ws>\s+|//[^\n]*|/\*.*?\*/)
  | (?P<str>"(?:[^"\\]|\\.)*")
  | (?P<num>[-+]?(?:\d[\d_]*\.?[\d_]*(?:[eE][-+]?\d+)?|\.\d+(?:[eE][-+]?\d+)?))
  | (?P<ident>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<punct>[()\[\]{}:,])
""", re.S | re.X)


class RonError(ValueError):
    pass


_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}


def _unescape(s):
    if "\\" not in s:
        return s
    return re.sub(r"\\(u\{[0-9a-fA-F]+\}|.)",
                  lambda m: chr(int(m.group(1)[2:-1], 16)) if m.group(1).startswith("u{")
                  else _ESCAPES.get(m.group(1), m.group(1)), s)


def _tokens(text, where):
    pos, out = 0, []
    while pos < len(text):
        m = _TOKEN.match(text, pos)
        if not m:
            line = text.count("\n", 0, pos) + 1
            raise RonError(f"{where}:{line}: unexpected {text[pos:pos + 12]!r}")
        pos = m.end()
        kind = m.lastgroup
        if kind == "ws":
            continue
        out.append((kind, m.group(kind)))
    out.append(("end", ""))
    return out


def parse(text, where="<ron>"):
    toks = _tokens(text, where)
    i = 0

    def peek(k=0):
        return toks[min(i + k, len(toks) - 1)]

    def take(expect=None):
        nonlocal i
        tok = toks[i]
        if expect is not None and tok[1] != expect:
            raise RonError(f"{where}: expected {expect!r}, found {tok[1]!r}")
        i += 1
        return tok

    def value():
        kind, text_ = peek()
        if kind == "str":
            take()
            return _unescape(text_[1:-1])
        if kind == "num":
            take()
            t = text_.replace("_", "")
            return float(t) if any(c in t for c in ".eE") else int(t)
        if text_ == "[":
            take()
            items = []
            while peek()[1] != "]":
                items.append(value())
                if peek()[1] == ",":
                    take()
            take("]")
            return items
        if text_ == "{":
            take()
            items = {}
            while peek()[1] != "}":
                k = value()
                take(":")
                items[k if isinstance(k, (str, int, float)) else repr(k)] = value()
                if peek()[1] == ",":
                    take()
            take("}")
            return items
        if text_ == "(":
            return paren(None)
        if kind == "ident":
            take()
            if peek()[1] == "(":
                if text_ == "Some":
                    take("(")
                    v = value()
                    take(")")
                    return v
                return paren(text_)
            return text_
        raise RonError(f"{where}: unexpected {text_!r}")

    def paren(variant):
        take("(")
        is_struct = peek()[0] == "ident" and peek(1)[1] == ":"
        fields, items = {}, []
        while peek()[1] != ")":
            if is_struct:
                name = take()[1]
                take(":")
                fields[name] = value()
            else:
                items.append(value())
            if peek()[1] == ",":
                take()
        take(")")
        if is_struct:
            if variant:
                fields["__variant"] = variant
            return fields
        if variant:
            return {"__variant": variant, "__items": items}
        return items

    out = value()
    if peek()[0] != "end":
        raise RonError(f"{where}: trailing {peek()[1]!r}")
    return out
