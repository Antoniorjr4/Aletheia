"""Verificação Ed25519 (RFC 8032, §6: a implementação de referência), só com a biblioteca padrão.

O runtime Python é a referência semântica do ARX: precisa decidir do mesmo modo quais concessões
têm a assinatura da autoridade declarada (`arx::sign`). Só verifica; nunca assina. O ARX usa
`verify_strict`, que além disto recusa chaves e pontos de ordem pequena: para assinaturas
honestas as duas decisões coincidem, e onde diferem o ARX é o mais estrito.
"""
import hashlib

_p = 2**255 - 19
_L = 2**252 + 27742317777372353535851937790883648493
_d = -121665 * pow(121666, _p - 2, _p) % _p
_sqrt_m1 = pow(2, (_p - 1) // 4, _p)


def _add(P, Q):
    A, B = (P[1] - P[0]) * (Q[1] - Q[0]) % _p, (P[1] + P[0]) * (Q[1] + Q[0]) % _p
    C, D = 2 * P[3] * Q[3] * _d % _p, 2 * P[2] * Q[2] % _p
    E, F, G, H = B - A, D - C, D + C, B + A
    return (E * F % _p, G * H % _p, F * G % _p, E * H % _p)


def _mul(s, P):
    Q = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            Q = _add(Q, P)
        P = _add(P, P)
        s >>= 1
    return Q


def _equal(P, Q):
    return (P[0] * Q[2] - Q[0] * P[2]) % _p == 0 and (P[1] * Q[2] - Q[1] * P[2]) % _p == 0


def _recover_x(y, sign):
    if y >= _p:
        return None
    x2 = (y * y - 1) * pow(_d * y * y + 1, _p - 2, _p) % _p
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (_p + 3) // 8, _p)
    if (x * x - x2) % _p != 0:
        x = x * _sqrt_m1 % _p
    if (x * x - x2) % _p != 0:
        return None
    if (x & 1) != sign:
        x = _p - x
    return x


_gy = 4 * pow(5, _p - 2, _p) % _p
_gx = _recover_x(_gy, 0)
_G = (_gx, _gy, 1, _gx * _gy % _p)


def _decompress(s):
    y = int.from_bytes(s, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    return None if x is None else (x, y, 1, x * y % _p)


def verify(public: bytes, msg: bytes, signature: bytes) -> bool:
    if len(public) != 32 or len(signature) != 64:
        return False
    A, R = _decompress(public), _decompress(signature[:32])
    if A is None or R is None:
        return False
    s = int.from_bytes(signature[32:], "little")
    if s >= _L:
        return False
    h = int.from_bytes(hashlib.sha512(signature[:32] + public + msg).digest(), "little") % _L
    return _equal(_mul(s, _G), _add(R, _mul(h, A)))


PREFIX = "ed25519:"


def _unhex(s: str, n: int):
    try:
        b = bytes.fromhex(s)
    except ValueError:
        return None
    return b if len(b) == n and len(s) == 2 * n else None


def verify_tagged(public: str, kind: str, payload: str, sig) -> bool:
    """`arx::sign::verify`: chave e assinatura no formato `ed25519:<hex>`, mensagem com domínio."""
    if not isinstance(sig, str) or not sig.startswith(PREFIX) or not public.startswith(PREFIX):
        return False
    pk, sg = _unhex(public[len(PREFIX):], 32), _unhex(sig[len(PREFIX):], 64)
    if pk is None or sg is None:
        return False
    return verify(pk, f"aletheia/v1/{kind}\n{payload}".encode("utf-8"), sg)
