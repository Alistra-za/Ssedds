"""Duplicate finder for the user-facing docs (docs/DOCUMENTATION.md §2: one home per fact).

  py -3.13 -I tools/doc-dupes.py [--root .] [--out <tsv>] [--allow tools/doc-dupes-allow.txt] [--files <glob>...]
      Prints counts, the top file pairs and every failing cluster; exits 1 when a cluster fails.
  py -3.13 -I tools/doc-dupes.py --lost <git-ref> --from <file>...
      Lists what <file> held at <ref> that the current tree no longer has (sentences, then lines); exits 0.
  py -3.13 -I tools/doc-dupes.py --self-test

A sentence (fenced and 4-space code skipped, markdown stripped, split at . ! ? : ;) keeps its content words (stop
words dropped); one with fewer than 7 is ignored. Two sentences are a pair when their 3-word-shingle Jaccard is at
least 0.35 or their word-set Jaccard at least 0.6 (shingles shared by more than 40 sentences are boilerplate; pairs
within 2 lines of each other in one file don't count). A side is a reference when its line links to the other
side's page, it has at most 25 words and no standalone number outside the link; otherwise it is a copy. Pairs join
into clusters; a cluster passes with at most one copy and two members, or when allowed (allow file) with at most
three members.
"""
import argparse
import collections
import glob
import hashlib
import itertools
import pathlib
import re
import subprocess
import sys

STOP = set("""a an the and or of to in on for with by is are be it its this that as at from into than then
when which who what if not no but so can each one all any you your their they them there here was were
has have had do does done will would should may might must also only more most such these those""".split())

LINK = re.compile(r"\[([^\]]*)\]\(([^)]*)\)")
NUMBER = re.compile(r"(?<![A-Za-z_])\d")


def default_files(root):
    r = pathlib.Path(root)
    out = [r / "README.md", r / "docs/gui.md"]
    out += sorted((r / "docs/wiki").glob("*.md"))
    out += sorted((r / "docs/research").glob("*.md"))
    out += sorted((r / "docs").glob("*-plugin.md"))
    return [p for p in out if p.exists() and p.name != "_Sidebar.md"]


def content_words(s):
    return [w for w in re.findall(r"[a-z0-9]+", s.lower()) if w not in STOP]


def shingles(ws, k=3):
    return {tuple(ws[i:i + k]) for i in range(len(ws) - k + 1)}


def jac(a, b):
    return len(a & b) / len(a | b) if a or b else 0.0


class Sent:
    __slots__ = ("file", "line", "text", "ws", "sh", "wset", "links", "nwords", "number", "table")

    def __init__(self, file, line, text, raw_line):
        self.file, self.line, self.text = file, line, text
        self.table = None
        self.ws = content_words(text)
        self.sh = shingles(self.ws)
        self.wset = set(self.ws)
        self.links = [pathlib.PurePosixPath(t.split("#")[0]).stem.lower() for _, t in LINK.findall(raw_line)]
        self.nwords = len(re.findall(r"[A-Za-z0-9]+", text))
        outside = text
        for label, _ in LINK.findall(raw_line):
            if label:
                outside = outside.replace(label, " ")
        self.number = bool(NUMBER.search(outside))


def sentences(file, text):
    """(line number, sentence, raw line) for prose lines."""
    in_code = False
    for n, line in enumerate(text.splitlines(), 1):
        if line.strip().startswith("```"):
            in_code = not in_code
            continue
        if in_code or line.startswith("    "):
            continue
        t = re.sub(r"!\[[^\]]*\]\([^)]*\)", " ", line)
        t = LINK.sub(r"\1", t)
        t = re.sub(r"[*_`#>|]", " ", t)
        for s in re.split(r"(?<=[.!?:;])\s+", t):
            s = s.strip()
            if s:
                yield n, s, line


def tables(text):
    """({line: table id} for table rows, {header lines}): a header is the row right above a |---| separator."""
    block, heads, cur = {}, set(), 0
    lines = text.splitlines()
    for n, line in enumerate(lines, 1):
        if line.lstrip().startswith("|"):
            if n == 1 or not lines[n - 2].lstrip().startswith("|"):
                cur += 1
            block[n] = cur
            if n < len(lines) and re.match(r"^\s*\|?[\s:|-]*-{3,}[\s:|-]*$", lines[n]):
                heads.add(n)
    return block, heads


def records(texts):
    """texts: {relpath: text} -> [Sent] with at least 7 content words. Table header rows are labels, not facts."""
    out = []
    for f, t in texts.items():
        block, heads = tables(t)
        for n, s, raw in sentences(f, t):
            if n in heads:
                continue
            r = Sent(f, n, s, raw)
            r.table = block.get(n)
            if len(r.ws) >= 7:
                out.append(r)
    return out


def is_pair(a, b):
    return jac(a.sh, b.sh) >= 0.35 or jac(a.wset, b.wset) >= 0.6


def is_reference(side, other):
    page = pathlib.PurePosixPath(other.file).stem.lower()
    return page in side.links and side.nwords <= 25 and not side.number


def find_pairs(recs):
    index = collections.defaultdict(list)
    for i, r in enumerate(recs):
        for sh in r.sh:
            index[sh].append(i)
    cand = set()
    for ids in index.values():
        if len(ids) > 40:
            continue
        cand.update(itertools.combinations(ids, 2))
    pairs = []
    for a, b in cand:
        ra, rb = recs[a], recs[b]
        if ra.file == rb.file and (abs(ra.line - rb.line) < 3 or (ra.table and ra.table == rb.table)):
            continue  # rows of one table are a record, not repetition
        if is_pair(ra, rb):
            pairs.append((a, b))
    return sorted(pairs)


def sent_hash(r):
    return hashlib.sha1(" ".join(r.ws).encode("utf-8")).hexdigest()[:12]


def load_allow(path):
    allowed = set()
    p = pathlib.Path(path)
    if p.exists():
        for line in p.read_text(encoding="utf-8").splitlines():
            if line.strip() and not line.startswith("#"):
                allowed.add(line.split("\t")[0].strip())
    return allowed


def clusters(recs, pairs, allowed):
    """[(members, copies, ok)] where members are (file, line) nodes and copies the nodes that are not references."""
    parent = {}

    def find(x):
        parent.setdefault(x, x)
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    ref_only = {}
    hashes = collections.defaultdict(set)
    for a, b in pairs:
        ra, rb = recs[a], recs[b]
        na, nb = (ra.file, ra.line), (rb.file, rb.line)
        parent[find(na)] = find(nb)
        for node, side, other in ((na, ra, rb), (nb, rb, ra)):
            ref_only[node] = ref_only.get(node, True) and is_reference(side, other)
            hashes[node].add(sent_hash(side))
    groups = collections.defaultdict(set)
    for k in list(parent):
        groups[find(k)].add(k)
    out = []
    for g in groups.values():
        copies = sorted(n for n in g if not ref_only[n])
        allow = any(h in allowed for n in g for h in hashes[n])
        ok = (len(copies) <= 1 and len(g) <= 2) or (allow and len(g) <= 3)
        out.append((sorted(g), copies, ok))
    return sorted(out, key=lambda c: (-len(c[0]), c[0]))


def read_texts(root, paths):
    r = pathlib.Path(root).resolve()
    return {p.resolve().relative_to(r).as_posix(): p.read_text(encoding="utf-8") for p in paths}


def report(root, paths, allow_path, out_tsv):
    texts = read_texts(root, paths)
    recs = records(texts)
    pairs = find_pairs(recs)
    cl = clusters(recs, pairs, load_allow(allow_path))
    linked = sum(1 for a, b in pairs if is_reference(recs[a], recs[b]) or is_reference(recs[b], recs[a]))
    sizes = collections.Counter(len(c[0]) for c in cl)
    failing = [c for c in cl if not c[2]]
    print(f"sentences {len(recs)} pairs {len(pairs)} linked {linked} clusters {len(cl)} "
          f"three-or-more {sum(v for k, v in sizes.items() if k >= 3)} failing {len(failing)}")
    print("cluster sizes", sorted(sizes.items()))
    byfile = collections.Counter(tuple(sorted((recs[a].file, recs[b].file))) for a, b in pairs)
    for (fa, fb), v in byfile.most_common(25):
        print(f"{v:4} {fa} <-> {fb}")
    text_of = {(r.file, r.line): r.text for r in recs}
    for members, copies, _ in failing:
        print(f"\ncluster of {len(members)} ({len(copies)} copies):")
        for n in members:
            mark = "copy" if n in copies else "ref "
            print(f"  {mark} {n[0]}:{n[1]}  {text_of.get(n, '')[:120]}")
    if out_tsv:
        with open(out_tsv, "w", encoding="utf-8") as f:
            f.write("file_a\tline_a\tfile_b\tline_b\thash_a\thash_b\ttext_a\ttext_b\n")
            for a, b in pairs:
                ra, rb = recs[a], recs[b]
                f.write(f"{ra.file}\t{ra.line}\t{rb.file}\t{rb.line}\t{sent_hash(ra)}\t{sent_hash(rb)}\t"
                        f"{ra.text[:160]}\t{rb.text[:160]}\n")
    return 1 if failing else 0


def norm_line(s):
    return re.sub(r"\s+", " ", re.sub(r"[*_`]", "", s)).strip().casefold()


MARKUP_ONLY = re.compile(r"^[\s|:\-`~#*>=_+]*$")


def lost_sentences(old_name, old_text, current):
    """Sentences of old_text with >= 7 content words that pair with nothing in current ({name: text})."""
    cur = records(current)
    words = collections.defaultdict(list)
    for i, r in enumerate(cur):
        for w in r.wset:
            words[w].append(i)
    out = []
    _, heads = tables(old_text)  # header rows are not sentences on either side; line mode still lists them
    for n, s, raw in sentences(old_name, old_text):
        if n in heads:
            continue
        r = Sent(old_name, n, s, raw)
        if len(r.ws) < 7:
            continue
        cand = {i for w in r.wset for i in words.get(w, ())}
        if not any(is_pair(r, cur[i]) for i in cand):
            out.append((n, s))
    return out


def lost_lines(old_text, current):
    """Non-blank lines of old_text with no exact (normalised) or word-set (>= 4 words, Jaccard >= 0.6) match."""
    blobs = [norm_line(t) for t in current.values()]
    cur_lines = [set(re.findall(r"[a-z0-9]+", l.lower())) for t in current.values() for l in t.splitlines()]
    out = []
    for n, line in enumerate(old_text.splitlines(), 1):
        if not line.strip() or MARKUP_ONLY.match(line):
            continue
        key = norm_line(line)
        if any(key in b for b in blobs):
            continue
        ws = set(re.findall(r"[a-z0-9]+", line.lower()))
        if len(ws) >= 4 and any(jac(ws, c) >= 0.6 for c in cur_lines if c):
            continue
        out.append((n, line))
    return out


def lost(root, ref, files):
    paths = default_files(root) + [pathlib.Path(root) / f for f in files if (pathlib.Path(root) / f).exists()]
    current = read_texts(root, list(dict.fromkeys(p.resolve() for p in paths)))
    for f in files:
        f = f.replace("\\", "/")
        try:
            old = subprocess.run(["git", "-C", str(root), "show", f"{ref}:{f}"], capture_output=True, check=True,
                                 encoding="utf-8").stdout
        except subprocess.CalledProcessError:
            print(f"{f}: not in {ref}")
            continue
        s = lost_sentences(f, old, current)
        ln = lost_lines(old, current)
        print(f"== {f} at {ref}: {len(s)} lost sentences, {len(ln)} lost lines")
        for n, t in s:
            print(f"sentence {f}:{n}: {t[:200]}")
        for n, t in ln:
            print(f"line     {f}:{n}: {t[:200]}")
    return 0


def self_test():
    passed = 0

    def check(name, cond):
        nonlocal passed
        if not cond:
            raise SystemExit(f"self-test FAILED: {name}")
        passed += 1

    def run(texts):
        recs = records(texts)
        pairs = find_pairs(recs)
        return recs, pairs, clusters(recs, pairs, set())

    nine = "The exporter writes every converted texture into the chosen output folder tree."
    _, p, _ = run({"A.md": nine + "\n", "B.md": "Intro line here.\n\n" + nine + "\n"})
    check("identical 9-word sentence found", len(p) == 1)

    a = "Tessalith reads the folder recursively, analyses every image and plans the conversion quickly."
    b = "Tessalith reads the folder recursively, analyses every image and plans its conversion."
    _, p, _ = run({"A.md": a + "\n", "B.md": b + "\n"})
    check("reworded pair found", len(p) == 1)

    six = "Export writes textures into output folders."
    _, p, _ = run({"A.md": six + "\n", "B.md": six + "\n"})
    check("6 content words ignored", len(p) == 0)

    home = "The data folders and what each one holds are listed in the files table of the installation guide."
    ref = "The data folders and what each one holds are listed in the table on the [Installation page](Installation.md#folders)."
    recs, p, cl = run({"Installation.md": home + "\n", "FAQ.md": ref + "\n"})
    check("linked short side is a reference and its pair passes", len(p) == 1 and all(c[2] for c in cl))

    home = "How BC7 weighting trades alpha against colour detail is explained with measured examples here."
    ref = "How BC7 weighting trades alpha against colour detail is explained on the [Texture panel page](GUI-Texture-Panel.md#bc7-weighting)."
    recs, p, cl = run({"GUI-Texture-Panel.md": home + "\n", "README.md": ref + "\n"})
    r = [x for x in recs if x.file == "README.md"][0]
    o = [x for x in recs if x.file != "README.md"][0]
    check("BC7 is not a standalone number", is_reference(r, o) and all(c[2] for c in cl))

    long_home = ("Each output larger than 16 MB is split into parts so that the archive tool accepts it, and the "
                 "manifest records every part with its own hash and size for the later checks.")
    long_ref = ("Each output larger than 16 MB is split into parts so that the archive tool accepts it, and the "
                "manifest records every part with its hash, see [Validation](Validation.md#parts).")
    recs, p, cl = run({"Validation.md": long_home + "\n", "FAQ.md": long_ref + "\n"})
    check("linked 30-word restatement with 16 MB is a copy and fails", len(p) == 1 and not all(c[2] for c in cl))

    _, p, _ = run({"A.md": "```\n" + nine + "\n```\n", "B.md": nine + "\n"})
    check("fenced text ignored", len(p) == 0)

    hdr = "| Setting | RGB PSNR | premul PSNR | alpha PSNR | alpha SSIM | alpha max err | exact share |\n|---|---|---|---|---|---|---|\n"
    row = "| BC7 weight one today | 0/30 of 30 | 0/30 of 30 | 0/14 of 14 | 0/15 of 15 | +6.08 dB more | 30/30 |\n"
    row2 = "| BC7 weight four tried | 2/30 of 30 | 1/30 of 30 | 1/14 of 14 | 0/15 of 15 | +4.75 dB more | 29/30 |\n"
    _, p, _ = run({"R.md": hdr + row + "| a |\n| b |\n| c |\n" + row2 + "\nText.\n\n" + hdr + "| x |\n"})
    check("table header rows and rows of one table do not pair", len(p) == 0)

    old = "Intro.\n\n" + nine + "\n\nThe preview shows every mip level of the encoded texture beside the source.\n"
    cur = {"A.md": "Intro.\n", "B.md": nine + "\n"}
    ls = [t for _, t in lost_sentences("A.md", old, cur)]
    check("--lost lists a deleted sentence and not a moved one",
          any("preview shows every mip" in t for t in ls) and not any("exporter writes" in t for t in ls))

    old = "| Key | x | y |\n|---|---|---|\n| fill alpha |\n\n```toml\nmip_filter = \"kaiser\"\nlevels = 9\n```\n"
    cur = {"A.md": "Other text.\n", "B.md": "```toml\nlevels = 9\n```\n"}
    ll = [t for _, t in lost_lines(old, cur)]
    check("--lost line mode: table row and TOML line listed, moved code line not",
          "| fill alpha |" in ll and 'mip_filter = "kaiser"' in ll and "levels = 9" not in ll)
    print(f"self-test: {passed} passed")
    return 0


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    ap =argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default=".")
    ap.add_argument("--out")
    ap.add_argument("--allow", default="tools/doc-dupes-allow.txt")
    ap.add_argument("--files", nargs="+")
    ap.add_argument("--lost")
    ap.add_argument("--from", dest="from_files", nargs="+")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    root = pathlib.Path(a.root)
    if a.lost:
        if not a.from_files:
            ap.error("--lost needs --from <file>...")
        return lost(root, a.lost, a.from_files)
    if a.files:
        paths = []
        for g in a.files:
            hits = sorted(glob.glob(str(root / g)))
            paths += [pathlib.Path(h) for h in hits if not h.endswith("_Sidebar.md")]
        paths = list(dict.fromkeys(paths))
    else:
        paths = default_files(root)
    allow = a.allow if pathlib.Path(a.allow).is_absolute() else root / a.allow
    return report(root, paths, allow, a.out)


if __name__ == "__main__":
    sys.exit(main())
