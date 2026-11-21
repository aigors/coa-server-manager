"""Extract the CoA client's DBC tables for a Linux server's game data folder.

The world server needs the original CoA client's tables in DataDir/dbc (and an unpatched copy in DataDir/dbc_clientset);
they are not part of the server package. This runs the server fork's own extractor (apps/coa-dbc/client_dbc.py) with
mpqcli 0.13, whose `extract` takes `-j -o DIR ARCHIVE MEMBER` instead of the `-f MEMBER` the extractor passes.

    python extract-client-dbc.py CORE_CHECKOUT CLIENT_DATA_DIR OUT_DIR MPQCLI

CORE_CHECKOUT  a checkout of the server fork (Corfirean/azerothcore-wotlk-coa) at the commit of your package
CLIENT_DATA_DIR  the client's Data folder (the one with common.MPQ and patch-*.MPQ)
OUT_DIR  an empty folder for the tables
MPQCLI  the mpqcli executable (https://github.com/TheGrayDot/mpqcli/releases)

Afterwards check the set with: python CORE_CHECKOUT/apps/coa-dbc/client_dbc.py check OUT_DIR
"""
import sys
from pathlib import Path

if len(sys.argv) != 5:
    sys.exit(__doc__)
core, data, out, exe = sys.argv[1:]
sys.path.insert(0, str(Path(core) / "apps" / "coa-dbc"))
import client_dbc  # noqa: E402


class Mpq(client_dbc.MpqCli):
    def extract(self, archive, member, directory):
        self.run("extract", "-j", "-o", directory, archive, member)
        return Path(directory) / member.replace("/", "\\").rsplit("\\", 1)[-1]


# --original: read the launcher's untouched NAME.ORIGINAL copy of an archive it replaced locally.
manifest = client_dbc.extract(data, out, Mpq(exe), original=True, root=Path(core))
print(f"{len(manifest['files'])} tables in {out}")
for name in ("Spell.dbc", "SpellCustomAttr.dbc", "ChrRaces.dbc"):
    entry = manifest["files"].get(name)
    print(f"  {name}: " + (f"{entry['rows']} rows from {entry['archive']}" if entry else "MISSING"))
