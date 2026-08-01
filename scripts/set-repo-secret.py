#!/usr/bin/env python3
"""Set a GitHub Actions repository secret via the REST API.

Usage:
    GH_TOKEN=<pat> SECRET_NAME=<name> SECRET_VALUE=<value> \
        python scripts/set-repo-secret.py <owner>/<repo>

Encrypts SECRET_VALUE with the repo's Actions public key (libsodium sealed
box) and PUTs it as SECRET_NAME. Requires PyNaCl.
"""
import base64
import json
import os
import sys

import nacl.bindings
import requests

owner_repo = sys.argv[1] if len(sys.argv) > 1 else "mrpulor-gh/nuphus-mcp"
gh_token = os.environ["GH_TOKEN"]
name = os.environ["SECRET_NAME"]
value = os.environ["SECRET_VALUE"]

headers = {
    "Authorization": f"Bearer {gh_token}",
    "Accept": "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
}

# 1. Fetch the repository's Actions public key.
r = requests.get(
    f"https://api.github.com/repos/{owner_repo}/actions/secrets/public-key",
    headers=headers,
)
r.raise_for_status()
key = r.json()
print(f"pubkey id: {key['key_id']}")

# 2. Encrypt the secret value with a libsodium sealed box.
encrypted = nacl.bindings.crypto_box_seal(value.encode("utf-8"), base64.b64decode(key["key"]))

# 3. PUT the secret.
r = requests.put(
    f"https://api.github.com/repos/{owner_repo}/actions/secrets/{name}",
    headers=headers,
    json={
        "encrypted_value": base64.b64encode(encrypted).decode("ascii"),
        "key_id": key["key_id"],
    },
)
r.raise_for_status()
print(f"secret '{name}' set OK")
