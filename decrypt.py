
import os
import sys
from pathlib import Path
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

key = bytes.fromhex(os.environ["ASSET_ENCRYPTION_KEY"])
if len(key) != 32:
    raise ValueError("Key must be 32 bytes")

for arg in sys.argv[1:]:
    path = Path(arg)
    data = path.read_bytes()
    plaintext = AESGCM(key).decrypt(data[:12], data[12:], None)
    output = path.with_name(path.name.removesuffix(".enc"))
    output.write_bytes(plaintext)
    print(f"Decrypted: {output}")
