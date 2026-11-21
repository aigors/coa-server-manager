"""Sign a Linux server package with your own key, for a coa-web that trusts it (COA_TRUSTED_KEY).

The Manager installs a package only when `manifest.json.sig` (Ed25519 over the exact bytes of `manifest.json`, base64)
matches the key it trusts. The packages built by a fork are published unsigned; sign them yourself:

    python3 sign-package.py keygen KEY_DIR          # once: KEY_DIR/private.pem (keep it secret) and KEY_DIR/public.b64
    python3 sign-package.py sign KEY_DIR PKG_DIR    # writes PKG_DIR/manifest.json.sig

Then give coa-web the content of public.b64 as COA_TRUSTED_KEY. Needs the `cryptography` package
(Debian/Ubuntu: apt install python3-cryptography).
"""
import base64
import os
import sys
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def keygen(key_dir: Path) -> None:
    key_dir.mkdir(parents=True, exist_ok=True)
    private = key_dir / "private.pem"
    if private.exists():
        sys.exit(f"{private} already exists; a new key would orphan every package signed with it")
    key = Ed25519PrivateKey.generate()
    pem = key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
    fd = os.open(private, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as f:
        f.write(pem)
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    (key_dir / "public.b64").write_text(base64.b64encode(public).decode() + "\n")
    print(f"private key: {private} (keep it secret)")
    print(f"COA_TRUSTED_KEY={base64.b64encode(public).decode()}")


def sign(key_dir: Path, pkg_dir: Path) -> None:
    key = serialization.load_pem_private_key((key_dir / "private.pem").read_bytes(), None)
    manifest = (pkg_dir / "manifest.json").read_bytes()
    signature = key.sign(manifest)
    key.public_key().verify(signature, manifest)
    (pkg_dir / "manifest.json.sig").write_text(base64.b64encode(signature).decode())
    print(f"signed {pkg_dir / 'manifest.json'}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "keygen":
        keygen(Path(sys.argv[2]))
    elif len(sys.argv) == 4 and sys.argv[1] == "sign":
        sign(Path(sys.argv[2]), Path(sys.argv[3]))
    else:
        sys.exit(__doc__)
