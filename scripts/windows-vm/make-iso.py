#!/usr/bin/env python3
"""Pack a directory into a small ISO 9660 + Joliet image (no xorriso / mkisofs needed).

Usage: make-iso.py SRC_DIR OUT.iso LABEL   (needs pycdlib: `uv run --with pycdlib` or `pip install --user pycdlib`)
Windows reads the Joliet names, so the original (mixed-case, long) file names are kept.
"""

import os
import re
import sys

import pycdlib


def iso9660_name(name, is_dir):
    stem, dot, ext = name.upper().partition(".")
    clean = lambda s: re.sub(r"[^A-Z0-9_]", "_", s)
    if is_dir:
        return clean(name.upper())[:31]
    return f"{clean(stem)[:26]}.{clean(ext)[:3]};1" if dot else f"{clean(stem)[:30]}.;1"


def main(src, out, label):
    iso = pycdlib.PyCdlib()
    iso.new(interchange_level=3, joliet=3, vol_ident=label)
    for root, dirs, files in os.walk(src):
        rel = os.path.relpath(root, src)
        parts = [] if rel == "." else rel.split(os.sep)
        iso_dir = "/" + "/".join(iso9660_name(p, True) for p in parts)
        joliet_dir = "/" + "/".join(parts)
        for d in sorted(dirs):
            iso.add_directory(
                iso_dir.rstrip("/") + "/" + iso9660_name(d, True),
                joliet_path=joliet_dir.rstrip("/") + "/" + d,
            )
        for f in sorted(files):
            iso.add_file(
                os.path.join(root, f),
                iso_dir.rstrip("/") + "/" + iso9660_name(f, False),
                joliet_path=joliet_dir.rstrip("/") + "/" + f,
            )
    iso.write(out)
    iso.close()


if __name__ == "__main__":
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    main(*sys.argv[1:])
