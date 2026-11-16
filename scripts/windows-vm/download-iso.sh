#!/usr/bin/env bash
# Download the Windows 11 Enterprise evaluation ISO (x64, 90 days) from the Microsoft Evaluation Center.
# Usage: download-iso.sh [LANG]   LANG defaults to ja-JP (en-US, en-GB, ... as listed on the page)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

lang="${1:-ja-JP}"
page="https://www.microsoft.com/en-us/evalcenter/download-windows-11-enterprise"
ua="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
mkdir -p "$VM_DIR"

# The page links each language through a go.microsoft.com fwlink whose aria-label names the edition.
# Pick the non-LTSC 64-bit ISO for the language; the fwlink redirects to software-static.download.prss.microsoft.com.
link="$(curl -fsSL -A "$ua" "$page" | LANG_TAG="$lang" python3 -c '
import html, os, re, sys
s = sys.stdin.read()
want = "ISO 64-bit (" + os.environ["LANG_TAG"] + ")"
for m in re.finditer(r"<a[^>]*>", s):
    tag = m.group(0)
    label = re.search(r"aria-label=\"([^\"]*)\"", tag)
    href = re.search(r"href=\"(https://go\.microsoft\.com/fwlink/[^\"]*)\"", tag)
    if label and href and want in label.group(1) and "LTSC" not in label.group(1):
        print(html.unescape(href.group(1)))
        break
')"
[[ -n "$link" ]] || die "no $lang 64-bit ISO link on $page"

url="$(curl -fsS -o /dev/null -w '%{redirect_url}' -A "$ua" "$link")"
[[ "$url" == https://software-static.download.prss.microsoft.com/* ]] || die "unexpected download URL: $url"
echo "downloading $url"
curl -fL -C - -o "$ISO.part" "$url"
mv "$ISO.part" "$ISO"
echo "saved $ISO ($(basename "$url"))"
