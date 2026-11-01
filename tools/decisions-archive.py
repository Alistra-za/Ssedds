"""decisions-archive.py: moves superseded rulings from .claude/DECISIONS.md verbatim (docs/DOCUMENTATION.md §7).

  py -3.13 -I tools\\decisions-archive.py --table <tsv> --apply [--root <repo>]
  py -3.13 -I tools\\decisions-archive.py --check --base <git-ref> [--table <tsv>] [--root <repo>]
  py -3.13 -I tools\\decisions-archive.py --comments-only --base <git-ref> [--exclude <path>...] [--root <repo>]
  py -3.13 -I tools\\decisions-archive.py --self-test
"""
import argparse, csv, pathlib, re, subprocess, sys, tempfile

DEC = ".claude/DECISIONS.md"
ARC = ".claude/DECISIONS-ARCHIVE.md"
DEC_BEFORE_MOVE = "docs/" + "DECISIONS.md"  # where a --base before the move into .claude/ holds it
RES = "docs/research/2026-10-early-spikes.md"


MOVED = ("archive-item", "archive-note")
BULLET = re.compile(r"\s*(?:[-*+]|\d+\.)\s")


class LookupError2(Exception):
    pass


def indent(l):
    return len(l) - len(l.lstrip())


def is_section(row):
    return "¶" not in row["id"] and not re.fullmatch(r"[A-Z]{1,2}\d{1,3}[a-z]?|[A-Z]{1,2}-[A-Za-z0-9-]+", row["id"])


def heading_re(name):
    """`## Note XX …` for a note letter, `## <name>` exactly for a section heading."""
    if re.fullmatch(r"[A-Z]{1,2}\??", name):
        return re.compile(rf"^## Note {re.escape(name)}(?=\s|$)")
    return re.compile(rf"^## {re.escape(name)}\s*$")


def section_range(lines, name):
    """(heading index, end) of the one `## ` section `name` names, or None."""
    hr = heading_re(name)
    hits = [i for i, l in enumerate(lines) if hr.match(l)]
    if len(hits) > 1:
        raise LookupError2(f"heading {name!r} appears {len(hits)} times")
    if not hits:
        return None
    j = hits[0] + 1
    while j < len(lines) and not lines[j].startswith("## "):
        j += 1
    return hits[0], j


def item_end(lines, i):
    """An item is its header plus every following line until a line at the same or lower indent that
    starts a bullet, a `## ` heading, or a blank line followed by an unindented line."""
    ind, j = indent(lines[i]), i + 1
    while j < len(lines):
        l = lines[j]
        if l.startswith("## "):
            break
        if not l.strip():
            if j + 1 >= len(lines) or (lines[j + 1].strip() and indent(lines[j + 1]) == 0):
                break
        elif indent(l) <= ind and BULLET.match(l):
            break
        j += 1
    while j > i + 1 and not lines[j - 1].strip():
        j -= 1
    return j


def find_span(lines, row):
    """[a, b) of the row's text in `lines`, or None when it is not there; LookupError2 when ambiguous."""
    rid = row["id"]
    if is_section(row):
        return section_range(lines, rid)
    note, _, prefix = rid.partition("¶")
    if not prefix:
        note = row["note"]
    rng = section_range(lines, note)
    if rng is None:
        return None
    h, e = rng
    if prefix:
        hits = [i for i in range(h + 1, e) if lines[i].strip().startswith(prefix)]
    else:
        pat = re.compile(rf"^\s*- (?:\*\*{re.escape(rid)}(?![0-9A-Za-z-])|{re.escape(rid)}\.)")
        hits = [i for i in range(h + 1, e) if pat.match(lines[i])]
    if len(hits) > 1:
        raise LookupError2(f"{rid}: {len(hits)} matches under {lines[h]!r}")
    if not hits:
        return None
    i = hits[0]
    if prefix:
        j = i + 1
        while j < e and lines[j].strip() and indent(lines[j]) > indent(lines[i]):
            j += 1
        return i, j
    return i, item_end(lines, i)


def archived_line(row):
    sup = " and ".join(f"Note {s.strip()}" for s in row.get("superseder", "").split(",") if s.strip())
    date = row.get("date") or "2026-10-31"
    if row["class"] == "partial":
        return f"ARCHIVED (wording before the edit; superseded in part by {sup}, {date})"
    if not sup:
        return f"ARCHIVED (process record, no live ruling, {date})"
    return f"ARCHIVED (superseded by {sup}, {date})"


def read_doc(p):
    if not p.exists():
        return [], "\n"
    raw = p.read_bytes().decode("utf-8")
    return raw.replace("\r\n", "\n").split("\n"), ("\r\n" if "\r\n" in raw else "\n")


def write_doc(p, lines, nl):
    p.parent.mkdir(parents=True, exist_ok=True)
    data = "\n".join(lines).replace("\n", nl).encode("utf-8")
    if not p.exists() or p.read_bytes() != data:
        p.write_bytes(data)


ARC_HEAD = ["# DECISIONS archive", "",
            "Rulings here are **not in force**. They moved verbatim from DECISIONS.md under their original IDs "
            "(docs/DOCUMENTATION.md §7). Never cite one as a reason for current behaviour.", ""]
RES_HEAD = ["# Early spikes and phase records (2026-10)", "",
            "Moved verbatim from DECISIONS.md on 2026-10-31 (docs/DOCUMENTATION.md §7): measurement and process "
            "records of 2026-10-08 to 2026-10-13, not rulings.", ""]


def add_block(doc, heading, block):
    """Append `block` (lines) under `heading` in doc (lines), adding the heading at the end when it is new."""
    rng = section_range(doc, heading[3:]) if heading.startswith("## ") else None
    if rng is None:
        if doc and doc[-1].strip():
            doc.append("")
        doc.extend([heading, "", *block, ""])
        return
    _, e = rng
    k = e
    while k > 0 and not doc[k - 1].strip():
        k -= 1
    doc[k:k] = ["", *block]


def contains(doc, block):
    n = len(block)
    return any(doc[i:i + n] == block for i in range(len(doc) - n + 1))


def apply(root, rows):
    root = pathlib.Path(root)
    live, nl = read_doc(root / DEC)
    arc, anl = read_doc(root / ARC)
    res, rnl = read_doc(root / RES)
    if not arc or arc == [""]:
        arc, anl = list(ARC_HEAD), nl
    if not res or res == [""]:
        res, rnl = list(RES_HEAD), nl
    moved_sections = {r["id"] for r in rows if r["class"] in ("research", "archive-note") and is_section(r)}
    found = []
    for r in rows:
        if r["class"] not in ("archive-item", "archive-note", "partial", "research"):
            continue
        if "¶" in r["id"] and r["id"].split("¶")[0] in moved_sections:
            raise LookupError2(f"{r['id']}: its section is moved whole ({r['id'].split('¶')[0]})")
        sp = find_span(live, r)
        if sp is None:
            if r["class"] == "research":
                if section_range(res, r["id"]) is None:
                    raise LookupError2(f"{r['id']}: not in DECISIONS.md nor in {RES}")
            elif not archived_already(arc, r):
                raise LookupError2(f"{r['id']}: not found in DECISIONS.md and not archived")
            continue
        found.append((sp, r))
    found.sort(key=lambda x: x[0][0])
    for (a, b), (c, d) in zip([f[0] for f in found], [f[0] for f in found][1:]):
        if c < b:
            raise LookupError2(f"overlapping spans at lines {a + 1} and {c + 1}")
    for (a, b), r in found:
        if r["class"] == "research":
            block = live[a:b]
            while block and not block[-1].strip():
                block.pop()
            if section_range(res, r["id"]) is None:
                if res[-1].strip():
                    res.append("")
                res.extend([*block, ""])
            continue
        if is_section(r):  # archive-note: heading, ARCHIVED line, body
            body = live[a + 1:b]
            while body and not body[0].strip():
                body.pop(0)
            while body and not body[-1].strip():
                body.pop()
            if section_range(arc, live[a][3:]) is None:
                if arc[-1].strip():
                    arc.append("")
                arc.extend([live[a], "", archived_line(r), *body, ""])
            continue
        heading = live[next(i for i in range(a, -1, -1) if live[i].startswith("## "))]
        block = [archived_line(r), *live[a:b]]
        if not contains(arc, live[a:b]):
            add_block(arc, heading, block)
    for (a, b), r in sorted(found, key=lambda x: -x[0][0]):
        if r["class"] != "partial":
            del live[a:b]
    write_doc(root / DEC, live, nl)
    write_doc(root / ARC, arc, anl)
    if len(res) > len(RES_HEAD):
        write_doc(root / RES, res, rnl)


def archived_already(arc, r):
    if is_section(r):
        return section_range(arc, r["id"]) is not None
    return find_span(arc, r) is not None


def deleted_lines(old, new):
    """Lines `git diff -U0` shows as removed from old to new."""
    with tempfile.TemporaryDirectory() as td:
        a, b = pathlib.Path(td, "a"), pathlib.Path(td, "b")
        a.write_text(old, encoding="utf-8", newline="\n")
        b.write_text(new, encoding="utf-8", newline="\n")
        out = subprocess.run(["git", "diff", "--no-index", "--unified=0", "--no-color", str(a), str(b)],
                             capture_output=True).stdout.decode("utf-8")
    return [l[1:] for l in out.split("\n") if l.startswith("-") and not l.startswith("---")]


def check(base_text, live_text, archive_text, research_text, rows):
    fails = []
    live = live_text.replace("\r\n", "\n").split("\n")
    arc = archive_text.replace("\r\n", "\n").split("\n")
    for name, doc in (("DECISIONS.md", live), ("DECISIONS-ARCHIVE.md", arc)):
        seen = {}
        for l in doc:
            if l.startswith("## "):
                seen[l] = seen.get(l, 0) + 1
        fails += [f"{name}: heading twice: {h}" for h, n in seen.items() if n > 1]
    for r in rows:
        try:
            if r["class"] == "archive-item" and not is_section(r) and find_span(live, r) is not None:
                fails.append(f"DECISIONS.md: archive-item {r['id']} is still live")
            if r["class"] == "archive-note" and section_range(live, r["id"] if is_section(r) else r["note"]) is not None:
                fails.append(f"DECISIONS.md: archive-note {r['id']} is still a heading")
        except LookupError2 as e:
            fails.append(str(e))
    kept = set(arc) | set(research_text.replace("\r\n", "\n").split("\n"))
    for l in deleted_lines(base_text.replace("\r\n", "\n"), "\n".join(live)):
        if l.strip() and l not in kept:
            fails.append(f"deleted and not archived: {l[:120]}")
    return fails


# ---------------------------------------------------------------- comments-only

MARK = {".rs": "//", ".js": "//", ".mjs": "//", ".cs": "//", ".cpp": "//", ".h": "//", ".ps1": "#", ".py": "#"}
BLOCK = {".rs": ("/*", "*/"), ".js": ("/*", "*/"), ".mjs": ("/*", "*/"), ".cs": ("/*", "*/"),
         ".cpp": ("/*", "*/"), ".h": ("/*", "*/"), ".ps1": ("<#", "#>"), ".py": ('"""', '"""')}


def code_part(line, ext):
    """Text before the first comment marker outside a string literal (None when there is no marker)."""
    mark = MARK[ext]
    quotes = '"' if ext == ".rs" else "\"'"
    q, i = None, 0
    while i < len(line):
        ch = line[i]
        if q:
            if ch == "\\" and ext != ".ps1":
                i += 2
                continue
            if ch == q:
                q = None
        elif ch in quotes:
            q = ch
        elif line.startswith(mark, i):
            return line[:i]
        i += 1
    return None


def comment_failures(diff_text, exclude=()):
    fails, path, ext, lineno = [], None, None, 0
    hunks = []  # (path, ext, [(sign, lineno, text)])
    for l in diff_text.replace("\r\n", "\n").split("\n"):
        if l.startswith("+++ "):
            p = l[4:]
            path = None if p == "/dev/null" else p[2:] if p.startswith("b/") else p
            if path is None:
                path = pending
            ext = pathlib.PurePosixPath(path).suffix.lower()
        elif l.startswith("--- "):
            p = l[4:]
            pending = p[2:] if p.startswith("a/") else p
        elif l.startswith("@@"):
            m = re.match(r"@@ -(\d+)(?:,\d+)? \+(\d+)", l)
            hunks.append((path, ext, []))
            old_no, new_no = int(m.group(1)), int(m.group(2))
        elif l and l[0] in "+-" and hunks and not l.startswith(("+++", "---")):
            if l[0] == "-":
                hunks[-1][2].append(("-", old_no, l[1:]))
                old_no += 1
            else:
                hunks[-1][2].append(("+", new_no, l[1:]))
                new_no += 1
    for path, ext, body in hunks:
        if any(path == e or path.startswith(e.rstrip("/") + "/") for e in exclude):
            continue
        if ext not in MARK:
            fails += [f"{path}:{n}: not a comment file type" for _, n, t in body if t.strip()]
            continue
        open_, close = BLOCK[ext]
        loose = {"-": [], "+": []}
        for sign in "-+":
            inside = False
            for s, n, t in body:
                if s != sign or not t.strip():
                    continue
                st = t.strip()
                if inside:
                    if close in st:
                        inside = False
                    continue
                if st.startswith(open_) and (ext != ".py" or st.count('"""') == 1):
                    inside = close not in st[len(open_):]
                    continue
                if ext == ".py" and st.startswith('"""') and st.endswith('"""') and len(st) >= 6:
                    continue
                if st.startswith(MARK[ext]):
                    continue
                cp = code_part(t, ext)
                loose[sign].append((n, t, cp))
        for sign, other in (("-", "+"), ("+", "-")):
            pool = [cp for _, _, cp in loose[other] if cp is not None]
            for n, t, cp in loose[sign]:
                if cp is not None and cp in pool:
                    pool.remove(cp)
                    continue
                fails.append(f"{path}:{n}: code changed: {t.strip()[:80]}")
    return fails


# ---------------------------------------------------------------- self-test

FIXTURE = """# Decisions

## Scope clarification

- A scope line.

## Note H — GUI (2026-10-09)

The GUI reads every set from the project. (superseded by HB1)
  - indented under the prose line.

- **H1 First.** One line.
- **H2 Multi.** Start
  - sub a
  - sub b

  - sub c after a blank
- **H3 Next.** x

## Note DU — style (2026-10-27)

- DU1. **One.** text
- DU2. **Two.** text
"""

ROWS = [
    {"id": "H2", "note": "H", "class": "archive-item", "superseder": "HB2", "date": "2026-10-29"},
    {"id": "H¶The GUI reads every set from", "note": "H", "class": "archive-item", "superseder": "HB1", "date": "2026-10-29"},
    {"id": "DU2", "note": "DU", "class": "partial", "superseder": "DV4", "date": "2026-10-28"},
]


def self_test():
    passed = 0

    def ok(name, cond):
        nonlocal passed
        if not cond:
            raise AssertionError(name)
        passed += 1

    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        (root / "docs/research").mkdir(parents=True)
        (root / DEC).parent.mkdir(parents=True)
        (root / DEC).write_bytes(FIXTURE.replace("\n", "\r\n").encode())
        lines = FIXTURE.split("\n")
        a, b = find_span(lines, ROWS[0])
        ok("span of the multi-line item", lines[a:b] == ["- **H2 Multi.** Start", "  - sub a", "  - sub b", "", "  - sub c after a blank"]
           and lines[slice(*find_span(lines, ROWS[2]))] == ["- DU2. **Two.** text"])
        apply(root, ROWS)
        raw = (root / DEC).read_bytes().decode()
        live = raw.replace("\r\n", "\n")
        ok("archiving removes those lines exactly and keeps CRLF", "\r\n" in raw and "H2 Multi" not in live and "sub c after a blank" not in live
           and "- **H1 First.** One line.\n\n- **H3 Next.** x" not in live and "- **H1 First.** One line.\n- **H3 Next.** x" in live
           and "- DU2. **Two.** text" in live)
        arc = (root / ARC).read_bytes().decode().replace("\r\n", "\n")
        entry = "\n\nARCHIVED (superseded by Note HB2, 2026-10-29)\n- **H2 Multi.** Start\n  - sub a"
        ok("archive has heading, ARCHIVED line, item (items in their original order under the heading)",
           entry in arc and arc.index("## Note H — GUI (2026-10-09)\n") < arc.index("The GUI reads") < arc.index(entry)
           and arc.count("## Note H ") == 1)
        before = ((root / DEC).read_bytes(), (root / ARC).read_bytes())
        apply(root, ROWS)
        ok("a second --apply changes nothing", before == ((root / DEC).read_bytes(), (root / ARC).read_bytes()))
        res = (root / RES).read_text(encoding="utf-8") if (root / RES).exists() else ""
        ok("--check passes", check(FIXTURE, live, arc, res, ROWS) == [])
        hand = live.replace("- **H3 Next.** x\n", "")
        ok("a hand-deleted live line fails --check", any("H3 Next" in f for f in check(FIXTURE, hand, arc, res, ROWS)))
        ok("a prose row moves under its note with the ARCHIVED line",
           "The GUI reads every set" not in live and "indented under the prose line" not in live
           and "ARCHIVED (superseded by Note HB1, 2026-10-29)\nThe GUI reads every set from the project. (superseded by HB1)\n  - indented under the prose line." in arc)

    def diff(path, *body):
        return f"diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,1 +1,1 @@\n" + "\n".join(body) + "\n"

    fails = [comment_failures(diff("x.rs", "+// FC5: x")), comment_failures(diff("x.rs", "-/// AG11 y"))]
    bad = [comment_failures(diff("x.rs", l)) for l in ("+#[test]", "+#![allow(x)]", "+    *x = 1;", "+# FC5")]
    ok("--comments-only: comment lines pass, code and foreign markers fail",
       fails == [[], []] and all(bad) and comment_failures(diff("x.ps1", "+# FC5")) == []
       and comment_failures(diff("x.rs", "+/* FC5", "+   still a comment */")) == [])
    ok("trailing comments: same code passes, other code fails, // in a string is not the marker",
       comment_failures(diff("x.rs", "-foo(); // AG11", "+foo(); // FC5")) == []
       and comment_failures(diff("x.rs", "-foo(); // AG11", "+bar(); // FC5")) != []
       and comment_failures(diff("x.rs", '-let s = "//"; // AG11', '+let s = "//"; // FC5')) == [])
    print(f"self-test: {passed} passed")


def read_table(p):
    with open(p, encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f, delimiter="\t", quoting=csv.QUOTE_NONE))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--table")
    ap.add_argument("--root", default=".")
    ap.add_argument("--base")
    ap.add_argument("--exclude", nargs="*", default=[])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--apply", action="store_true")
    g.add_argument("--check", action="store_true")
    g.add_argument("--comments-only", action="store_true")
    g.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    root = pathlib.Path(a.root)
    git = lambda *args: subprocess.run(["git", "-C", str(root), *args], capture_output=True, check=True).stdout.decode("utf-8")
    try:
        if a.apply:
            if not a.table:
                ap.error("--apply needs --table")
            apply(root, read_table(a.table))
            return 0
        if not a.base:
            ap.error("--check and --comments-only need --base")
        if a.check:
            table = a.table or max(map(str, (root / "tools/decisions-archive").glob("*.tsv")))
            text = lambda p: (root / p).read_text(encoding="utf-8") if (root / p).exists() else ""
            try:
                old = git("show", f"{a.base}:{DEC}")
            except subprocess.CalledProcessError:
                old = git("show", f"{a.base}:{DEC_BEFORE_MOVE}")
            fails = check(old, text(DEC), text(ARC), text(RES), read_table(table))
        else:
            # The DECISIONS files and the mistakes log are prose, not code with comments.
            diff = git("diff", "-U0", "--no-color", a.base, "--", "crates", "tools", ".claude",
                       "photoshop", "gimp", "paintdotnet", "chainner", f":!{DEC}", f":!{ARC}",
                       ":!.claude/mistakes", ":!.claude/scratch")
            fails = comment_failures(diff, a.exclude)
    except LookupError2 as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    for f in fails:
        print(f)
    print(f"{len(fails)} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
