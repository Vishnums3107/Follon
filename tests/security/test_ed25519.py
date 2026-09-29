"""`tools/ed25519.py` is held to RFC 8032's own vectors and to a second implementation.

The acceptance audit trusts this module to say whether a reviewer signed a record,
so it must agree with every other Ed25519 implementation on every input. The RFC's
test vectors fix the algorithm. The cross-check against the `cryptography` package,
run when it is installed, catches anything the vectors do not.
"""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

from tools import ed25519

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]

# RFC 8032 section 7.1, TEST 1 and TEST 2 and TEST 3.
VECTORS = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
]


class Ed25519Tests(unittest.TestCase):
    def test_the_rfc_vectors_are_reproduced_exactly(self) -> None:
        for seed, public, message, signature in VECTORS:
            with self.subTest(message=message):
                seed_bytes = bytes.fromhex(seed)
                message_bytes = bytes.fromhex(message)
                self.assertEqual(ed25519.public_key(seed_bytes).hex(), public)
                self.assertEqual(ed25519.sign(seed_bytes, message_bytes).hex(), signature)
                self.assertTrue(
                    ed25519.verify(bytes.fromhex(public), message_bytes, bytes.fromhex(signature))
                )

    def test_a_signature_is_refused_for_another_message_key_or_byte(self) -> None:
        seed, public, message, signature = (bytes.fromhex(part) for part in VECTORS[1])
        other = ed25519.public_key(bytes(range(32)))
        self.assertFalse(ed25519.verify(public, message + b"\x00", signature))
        self.assertFalse(ed25519.verify(other, message, signature))
        for index in (0, 31, 32, 63):
            with self.subTest(flipped_byte=index):
                damaged = bytearray(signature)
                damaged[index] ^= 1
                self.assertFalse(ed25519.verify(public, message, bytes(damaged)))

    def test_malformed_inputs_are_refused_without_raising(self) -> None:
        _, public, message, signature = (bytes.fromhex(part) for part in VECTORS[0])
        self.assertFalse(ed25519.verify(public[:-1], message, signature))
        self.assertFalse(ed25519.verify(public, message, signature[:-1]))
        self.assertFalse(ed25519.verify(public, message, b""))
        # A 32-byte string that is not a curve point is not a public key.
        self.assertFalse(ed25519.verify(b"\xff" * 32, message, signature))
        with self.assertRaises(ValueError):
            ed25519.sign(b"short", b"message")

    def test_a_signature_is_canonical_or_it_is_refused(self) -> None:
        # RFC 8032 section 5.1.7 requires 0 <= S < L. Adding L to S changes the bytes
        # and leaves the verification equation true, because L times the base point
        # is the neutral element. Only the range rule refuses the malleated copy, so a
        # signature has exactly one accepted encoding and a ledger cannot be re-keyed
        # by anyone who lacks the key.
        _, public, message, signature = (bytes.fromhex(part) for part in VECTORS[0])
        response = int.from_bytes(signature[32:], "little")
        self.assertLess(response, ed25519.GROUP_ORDER)
        malleated = signature[:32] + int.to_bytes(response + ed25519.GROUP_ORDER, 32, "little")
        self.assertNotEqual(malleated, signature)
        self.assertFalse(ed25519.verify(public, message, malleated))
        self.assertTrue(ed25519.verify(public, message, signature))
        # The boundary itself is not canonical either.
        at_the_order = signature[:32] + int.to_bytes(ed25519.GROUP_ORDER, 32, "little")
        self.assertFalse(ed25519.verify(public, message, at_the_order))

    def test_a_pkcs8_private_key_yields_its_seed_in_either_version(self) -> None:
        seed = bytes(range(32))
        version_0 = bytes.fromhex("302e020100300506032b657004220420") + seed
        version_1 = (
            bytes.fromhex("3051020101300506032b657004220420")
            + seed
            + bytes.fromhex("812100")
            + ed25519.public_key(seed)
        )
        for name, document in (("version 0", version_0), ("version 1", version_1)):
            with self.subTest(name):
                self.assertEqual(ed25519.seed_from_pkcs8(document), seed)
                for damaged in (document[:-1], document + b"\x00", b"\x00" + document[1:], b""):
                    with self.assertRaises(ValueError):
                        ed25519.seed_from_pkcs8(damaged)

    def test_a_version_1_key_that_disagrees_with_itself_is_refused(self) -> None:
        seed = bytes(range(32))
        other_public = ed25519.public_key(bytes(range(1, 33)))
        document = (
            bytes.fromhex("3051020101300506032b657004220420")
            + seed
            + bytes.fromhex("812100")
            + other_public
        )
        with self.assertRaises(ValueError):
            ed25519.seed_from_pkcs8(document)

    @unittest.skipUnless(
        (REPOSITORY_ROOT / "var" / "release-signing.pk8").is_file()
        and (REPOSITORY_ROOT / "var" / "trusted-release-key.json").is_file(),
        "needs the release key the evidence pipeline generates",
    )
    def test_the_release_key_the_rust_tool_writes_is_read_here(self) -> None:
        # `follon-admin release-keygen` writes this key and its trusted public key.
        document = (REPOSITORY_ROOT / "var" / "release-signing.pk8").read_bytes()
        trusted = json.loads((REPOSITORY_ROOT / "var" / "trusted-release-key.json").read_text(encoding="utf-8"))
        seed = ed25519.seed_from_pkcs8(document)
        self.assertEqual(ed25519.public_key(seed).hex(), trusted["public_key_hex"])

    @unittest.skipUnless(
        importlib.util.find_spec("cryptography"), "cross-checking needs the cryptography package"
    )
    def test_it_agrees_with_an_independent_implementation(self) -> None:
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

        for length in (0, 1, 31, 32, 33, 200, 1000):
            seed = bytes((index * 7 + length) % 256 for index in range(32))
            message = bytes((index * 13 + 5) % 256 for index in range(length))
            reference = Ed25519PrivateKey.from_private_bytes(seed)
            reference_public = reference.public_key().public_bytes(
                serialization.Encoding.Raw, serialization.PublicFormat.Raw
            )
            with self.subTest(length=length):
                self.assertEqual(ed25519.public_key(seed), reference_public)
                # Ed25519 is deterministic, so the signatures are equal bytes.
                self.assertEqual(ed25519.sign(seed, message), reference.sign(message))
                self.assertTrue(
                    ed25519.verify(reference_public, message, reference.sign(message))
                )


if __name__ == "__main__":
    unittest.main()
