#!/usr/bin/env python3
"""Turns the seed of PluginDB (https://github.com/blakebratcher/plugindb, data under CC0 1.0)
into the table of plugin families AudioStrada ships, crates/audiostrada-ui/assets/plugindb.tsv,
and tells what changed.

    scripts/update-plugindb.py                          # the latest seed on GitHub
    scripts/update-plugindb.py seed.json --commit SHA   # a local copy

Only names and families are kept. Corrections of single plugins go in
plugin_families_extra.tsv, next to the table.
"""

import argparse
import json
import sys
import urllib.request
from pathlib import Path

REPO = "blakebratcher/plugindb"
OUT = Path(__file__).resolve().parent.parent / "crates/audiostrada-ui/assets/plugindb.tsv"

# (category, subcategory) of PluginDB, and AudioStrada's family. Vague ones ("general",
# "creative") are left out: the plugin's own tags or its name decide.
FAMILIES = {
    **{("effect", s): "Dynamics" for s in [
        "compressor", "limiter", "gate", "de-esser", "dynamics", "multiband", "transient",
        "clipper", "channel-strip",
    ]},
    **{("effect", s): "EQ & Filter" for s in ["eq", "filter"]},
    **{("effect", s): "Space" for s in ["reverb", "delay"]},
    **{("effect", s): "Modulation" for s in ["modulation", "chorus", "pitch", "vocoder"]},
    **{("effect", s): "Color" for s in ["saturation", "distortion", "lo-fi", "enhancer", "preamp"]},
    **{("effect", s): "Amp & Cab" for s in ["amp-sim", "guitar"]},
    **{("effect", s): "Utility" for s in ["phase", "noise", "restoration"]},
    **{("utility", s): "Utility" for s in [
        "analyzer", "meter", "metering", "calibration", "dither", "monitoring", "routing",
        "phase", "recording", "networking", "general",
    ]},
}

# Amp simulators PluginDB files under distortion or general: their first tag tells.
AMP_TAGS = {"amp-sim", "cab-sim"}


def fetch(commit):
    def get(url):
        request = urllib.request.Request(url, headers={"User-Agent": "audiostrada-update-plugindb"})
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()

    if commit is None:
        commit = json.loads(get(f"https://api.github.com/repos/{REPO}/commits/main"))["sha"]
    seed = json.loads(get(f"https://raw.githubusercontent.com/{REPO}/{commit}/data/seed.json"))
    return seed, commit


def family(plugin):
    tags = plugin.get("tags") or []
    if plugin["category"] == "effect" and tags and tags[0] in AMP_TAGS:
        return "Amp & Cab"
    return FAMILIES.get((plugin["category"], plugin["subcategory"]))


def clean(text):
    return " ".join(text.replace("\t", " ").replace("|", " ").split())


def rows(seed):
    vendors = {m["slug"]: m["name"] for m in seed["manufacturers"]}
    out = {}
    for plugin in seed["plugins"]:
        fam = family(plugin)
        vendor = clean(vendors.get(plugin["manufacturer_slug"], ""))
        if fam is None or not vendor:
            continue
        names = []
        for name in [plugin["name"], *(plugin.get("aliases") or [])]:
            name = clean(name)
            if name and name not in names:
                names.append(name)
        out[(vendor, names[0])] = (fam, names)
    return out


def previous():
    old = {}
    if OUT.exists():
        for line in OUT.read_text(encoding="utf-8").splitlines():
            if line and not line.startswith("#"):
                vendor, fam, names = line.split("\t")
                old[(vendor, names.split("|")[0])] = fam
    return old


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("seed", nargs="?", help="a local seed.json instead of GitHub's")
    parser.add_argument("--commit", help="the PluginDB commit of the seed")
    args = parser.parse_args()
    if args.seed:
        if not args.commit:
            sys.exit("give the commit of the local seed with --commit")
        seed, commit = json.loads(Path(args.seed).read_text(encoding="utf-8")), args.commit
    else:
        seed, commit = fetch(args.commit)
    new = rows(seed)
    old = previous()
    lines = [
        f"# Plugin families from PluginDB (https://github.com/{REPO}), commit {commit[:12]},",
        "# data under CC0 1.0. Made by scripts/update-plugindb.py: do not edit by hand,",
        "# corrections go in plugin_families_extra.tsv.",
        "# vendor<TAB>family<TAB>name|alias|...",
    ]
    for (vendor, _), (fam, names) in sorted(new.items(), key=lambda r: (r[0][0].lower(), r[0][1].lower())):
        lines.append(f"{vendor}\t{fam}\t{'|'.join(names)}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    added = [k for k in new if k not in old]
    removed = [k for k in old if k not in new]
    changed = [(k, old[k], new[k][0]) for k in new if k in old and old[k] != new[k][0]]
    print(f"{len(new)} plugins from commit {commit[:12]} written to {OUT.name}")
    for vendor, name in added:
        print(f"  + {vendor}: {name} ({new[(vendor, name)][0]})")
    for vendor, name in removed:
        print(f"  - {vendor}: {name}")
    for (vendor, name), was, now in changed:
        print(f"  ~ {vendor}: {name} ({was} -> {now})")


if __name__ == "__main__":
    main()
