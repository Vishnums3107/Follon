"""Ed25519 (RFC 8032) in pure Python, with nothing to install.

The acceptance audit runs on whichever machine holds the ledgers, so it verifies
reviewer signatures with the standard library alone. Verification handles public
data only, which is why this needs none of a signer's constant-time care. Signing
is here for a reviewer's own machine and for tests. A reviewer who wants a hardened
signer can use any RFC 8032 implementation: a signature is the same bytes.

This is RFC 8032 section 6's reference algorithm. `tests/security/test_ed25519.py`
holds it to the RFC's own test vectors, and to a second implementation when one is
installed.
"""

from __future__ import annotations

import hashlib

FIELD = 2**255 - 19
GROUP_ORDER = 2**252 + 27742317777372353535851937790883648493
CURVE_D = -121665 * pow(121666, FIELD - 2, FIELD) % FIELD
SQRT_MINUS_ONE = pow(2, (FIELD - 1) // 4, FIELD)

PUBLIC_KEY_BYTES = 32
SEED_BYTES = 32
SIGNATURE_BYTES = 64

# A point is (X, Y, Z, T) in extended homogeneous coordinates.
Point = tuple[int, int, int, int]
NEUTRAL: Point = (0, 1, 1, 0)


def _inverse(value: int) -> int:
    return pow(value, FIELD - 2, FIELD)


def _add(first: Point, second: Point) -> Point:
    a = (first[1] - first[0]) * (second[1] - second[0]) % FIELD
    b = (first[1] + first[0]) * (second[1] + second[0]) % FIELD
    c = 2 * first[3] * second[3] * CURVE_D % FIELD
    d = 2 * first[2] * second[2] % FIELD
    e, f, g, h = b - a, d - c, d + c, b + a
    return (e * f % FIELD, g * h % FIELD, f * g % FIELD, e * h % FIELD)


def _multiply(scalar: int, point: Point) -> Point:
    result = NEUTRAL
    while scalar > 0:
        if scalar & 1:
            result = _add(result, point)
        point = _add(point, point)
        scalar >>= 1
    return result


def _equal(first: Point, second: Point) -> bool:
    # x1 / z1 == x2 / z2 exactly when x1 * z2 == x2 * z1, and the same for y.
    return (
        (first[0] * second[2] - second[0] * first[2]) % FIELD == 0
        and (first[1] * second[2] - second[1] * first[2]) % FIELD == 0
    )


def _recover_x(y: int, sign: int) -> int | None:
    if y >= FIELD:
        return None
    x_squared = (y * y - 1) * _inverse(CURVE_D * y * y + 1) % FIELD
    if x_squared == 0:
        return None if sign else 0
    x = pow(x_squared, (FIELD + 3) // 8, FIELD)
    if (x * x - x_squared) % FIELD != 0:
        x = x * SQRT_MINUS_ONE % FIELD
    if (x * x - x_squared) % FIELD != 0:
        return None
    if (x & 1) != sign:
        x = FIELD - x
    return x


_BASE_Y = 4 * _inverse(5) % FIELD
_BASE_X = _recover_x(_BASE_Y, 0)
assert _BASE_X is not None
BASE: Point = (_BASE_X, _BASE_Y, 1, _BASE_X * _BASE_Y % FIELD)


def _compress(point: Point) -> bytes:
    z_inverse = _inverse(point[2])
    x = point[0] * z_inverse % FIELD
    y = point[1] * z_inverse % FIELD
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decompress(encoded: bytes) -> Point | None:
    if len(encoded) != 32:
        return None
    y = int.from_bytes(encoded, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % FIELD)


def _hash_to_scalar(data: bytes) -> int:
    return int.from_bytes(hashlib.sha512(data).digest(), "little") % GROUP_ORDER


def _expand_seed(seed: bytes) -> tuple[int, bytes]:
    if len(seed) != SEED_BYTES:
        raise ValueError("an Ed25519 seed is 32 bytes")
    digest = hashlib.sha512(seed).digest()
    scalar = int.from_bytes(digest[:32], "little")
    scalar &= (1 << 254) - 8
    scalar |= 1 << 254
    return scalar, digest[32:]


def public_key(seed: bytes) -> bytes:
    """The 32-byte public key of a 32-byte seed."""
    scalar, _ = _expand_seed(seed)
    return _compress(_multiply(scalar, BASE))


def sign(seed: bytes, message: bytes) -> bytes:
    """The 64-byte signature of `message` under a 32-byte seed."""
    scalar, prefix = _expand_seed(seed)
    public = _compress(_multiply(scalar, BASE))
    nonce = _hash_to_scalar(prefix + message)
    commitment = _compress(_multiply(nonce, BASE))
    challenge = _hash_to_scalar(commitment + public + message)
    response = (nonce + challenge * scalar) % GROUP_ORDER
    return commitment + int.to_bytes(response, 32, "little")


def verify(public: bytes, message: bytes, signature: bytes) -> bool:
    """Whether `signature` is `public`'s signature of `message`. Never raises."""
    if len(public) != PUBLIC_KEY_BYTES or len(signature) != SIGNATURE_BYTES:
        return False
    key_point = _decompress(public)
    commitment_point = _decompress(signature[:32])
    if key_point is None or commitment_point is None:
        return False
    response = int.from_bytes(signature[32:], "little")
    if response >= GROUP_ORDER:
        return False
    challenge = _hash_to_scalar(signature[:32] + public + message)
    return _equal(
        _multiply(response, BASE), _add(commitment_point, _multiply(challenge, key_point))
    )


# The DER prefixes of an Ed25519 PKCS#8 private key. Both open with a sequence, a
# version, the Ed25519 algorithm identifier and an octet string that holds an octet
# string of the 32-byte seed. Version 0 ends there, 48 bytes in all. Version 1, which
# is what `follon-admin release-keygen` writes, follows the seed with the public key,
# 83 bytes in all.
_PKCS8_V0_PREFIX = bytes.fromhex("302e020100300506032b657004220420")
_PKCS8_V1_PREFIX = bytes.fromhex("3051020101300506032b657004220420")
_PKCS8_V1_PUBLIC_KEY_HEADER = bytes.fromhex("812100")


def seed_from_pkcs8(document: bytes) -> bytes:
    """The seed inside an Ed25519 PKCS#8 DER private key, in either version.

    A version 1 document that carries a public key must carry the seed's own, so
    a key file that disagrees with itself is refused rather than half-trusted.
    """
    if len(document) == len(_PKCS8_V0_PREFIX) + SEED_BYTES and document.startswith(_PKCS8_V0_PREFIX):
        return document[len(_PKCS8_V0_PREFIX):]
    if len(document) == (
        len(_PKCS8_V1_PREFIX) + SEED_BYTES + len(_PKCS8_V1_PUBLIC_KEY_HEADER) + PUBLIC_KEY_BYTES
    ) and document.startswith(_PKCS8_V1_PREFIX):
        seed_end = len(_PKCS8_V1_PREFIX) + SEED_BYTES
        seed = document[len(_PKCS8_V1_PREFIX):seed_end]
        header_end = seed_end + len(_PKCS8_V1_PUBLIC_KEY_HEADER)
        if document[seed_end:header_end] == _PKCS8_V1_PUBLIC_KEY_HEADER and document[header_end:] == public_key(seed):
            return seed
    raise ValueError("not an Ed25519 PKCS#8 private key")
