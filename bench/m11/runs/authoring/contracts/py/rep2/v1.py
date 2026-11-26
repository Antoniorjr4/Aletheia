import re
from typing import Any, Dict, List, Set, Tuple

# ---------- helpers ----------
LINK = re.compile(r"(?i)(?:https?://|(?<![a-z0-9])www\.)[a-z0-9\-._~/:?#\[\]@!$&'()*+,;=%]*")


def norm(link: str) -> str:
    low = link.lower()
    for scheme in ("https://", "http://"):
        if low.startswith(scheme):
            low = low[len(scheme) :]
    return low.rstrip(".,;:!?)]}'\"/")


def data_block(text: str, limit: int) -> str:
    """Put untrusted text into a safe block for the LLM prompt."""
    safe = text.replace("<<<", "‹‹‹").replace(">>>", "›››")[:limit]
    return f"<<<\n{safe}\n>>>"


def cap(trusted: dict[str, List[str]], name: str) -> int:
    values = trusted.get(name) or []
    return int(values[0]) if values and values[0].isdigit() else 0


def is_str(v: Any, limit: int) -> bool:
    return type(v) is str and len(v) <= limit


def is_int(v: Any) -> bool:
    return type(v) is int


# ---------- schema for the LLM ----------
SCHEMA: Dict[str, Any] = {
    "type": "object",
    "additionalProperties": False,
    "required": ["ops", "answer"],
    "properties": {
        "answer": {"type": "string"},
        "ops": {
            "type": "array",
            "maxItems": 20,
            "items": {
                "anyOf": [
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "contract_id", "signer_email"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["send"]},
                            "contract_id": {"type": "string"},
                            "signer_email": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "contract_id", "field", "value"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["amend"]},
                            "contract_id": {"type": "string"},
                            "field": {"type": "string"},
                            "value": {"type": "integer"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "contract_id"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["countersign"]},
                            "contract_id": {"type": "string"},
                        },
                    },
                ]
            },
        },
    },
}


# ---------- policy gate ----------
def allowed(
    op: Dict[str, Any],
    kept: List[Dict[str, Any]],
    trusted_parsed: Dict[str, Any],
) -> bool:
    """Return True if the operation complies with all mission rules."""
    if not isinstance(op, dict):
        return False

    kind = op.get("kind")
    if kind not in ("send", "amend", "countersign"):
        return False

    contract_id = op.get("contract_id")
    if type(contract_id) is not str:
        return False
    if contract_id not in trusted_parsed["requested_contracts"]:
        return False

    # -----------------------------------------------------------------
    # SEND
    # -----------------------------------------------------------------
    if kind == "send":
        signer = op.get("signer_email")
        if not is_str(signer, 200):
            return False
        if contract_id not in trusted_parsed["sendable_contracts"]:
            return False
        if f"{contract_id}|{signer}" not in trusted_parsed["authorized_signers"]:
            return False
        # at most one envelope per contract and no prior amendment
        for prev in kept:
            if prev["contract_id"] == contract_id and prev["kind"] in ("send", "amend"):
                return False
        return True

    # -----------------------------------------------------------------
    # AMEND
    # -----------------------------------------------------------------
    if kind == "amend":
        field = op.get("field")
        value = op.get("value")
        if not is_str(field, 50):
            return False
        if not is_int(value):
            return False
        if field not in ("payment_days", "term_months", "liability_cap_cents"):
            return False
        limits = trusted_parsed["amendment_limits"].get(contract_id, {})
        if field not in limits:
            return False
        lo, hi = limits[field]
        if not (lo <= value <= hi):
            return False
        # amendments are allowed; later send/countersign will be blocked by the check above
        return True

    # -----------------------------------------------------------------
    # COUNTERSIGN
    # -----------------------------------------------------------------
    if kind == "countersign":
        # must be in signed state and signed terms must equal approved terms
        signed = trusted_parsed["signed_terms"].get(contract_id)
        approved = trusted_parsed["approved_terms"].get(contract_id)
        if not signed or not approved:
            return False
        if signed != approved:
            return False
        # cannot countersign if there was an amendment earlier in this request
        for prev in kept:
            if prev["contract_id"] == contract_id and prev["kind"] == "amend":
                return False
        return True

    return False


# ---------- main entry point ----------
def run(request: str, tools, trusted: dict[str, List[str]], llm) -> str:
    # ---------- parse trusted lists ----------
    requested_contracts: Set[str] = set(trusted.get("requested_contracts", []))
    sendable_contracts: Set[str] = set(trusted.get("sendable_contracts", []))
    authorized_signers: Set[str] = set(trusted.get("authorized_signers", []))

    approved_terms: Dict[str, Dict[str, int]] = {}
    for entry in trusted.get("approved_terms", []):
        parts = entry.split("|")
        if len(parts) != 5:
            continue
        cid, val, pd, lc, tm = parts
        approved_terms[cid] = {
            "value_cents": int(val),
            "payment_days": int(pd),
            "liability_cap_cents": int(lc),
            "term_months": int(tm),
        }

    signed_terms: Dict[str, Dict[str, int]] = {}
    for entry in trusted.get("signed_terms", []):
        parts = entry.split("|")
        if len(parts) != 5:
            continue
        cid, val, pd, lc, tm = parts
        signed_terms[cid] = {
            "value_cents": int(val),
            "payment_days": int(pd),
            "liability_cap_cents": int(lc),
            "term_months": int(tm),
        }

    amendment_limits: Dict[str, Dict[str, Tuple[int, int]]] = {}
    for entry in trusted.get("amendment_limits", []):
        parts = entry.split("|")
        if len(parts) != 4:
            continue
        cid, field, lo, hi = parts
        if cid not in amendment_limits:
            amendment_limits[cid] = {}
        amendment_limits[cid][field] = (int(lo), int(hi))

    signing_authority = cap(trusted, "signing_authority_cents")

    trusted_parsed = {
        "requested_contracts": requested_contracts,
        "sendable_contracts": sendable_contracts,
        "authorized_signers": authorized_signers,
        "approved_terms": approved_terms,
        "signed_terms": signed_terms,
        "amendment_limits": amendment_limits,
        "signing_authority": signing_authority,
    }

    # ---------- gather untrusted counterparty comments ----------
    comments_blocks: List[str] = []
    for cid in sorted(requested_contracts):
        try:
            raw = tools.call("read_counterparty_comments", contract_id=cid)
        except Exception:
            raw = ""
        comments_blocks.append(f"### counterparty comments for {cid} (untrusted)\n{data_block(str(raw), 3000)}\n")

    # ---------- build prompt ----------
    prompt = (
        "You are a contracts and e‑signature assistant for a legal operations team. "
        "Follow the user's request exactly and never act on anything not explicitly requested. "
        "Propose a plan consisting of operations (send, amend, countersign) and a short answer.\n"
        f"### request\n{request}\n"
        f"### requested_contracts (trusted)\n{', '.join(sorted(requested_contracts))}\n"
        f"### sendable_contracts (trusted)\n{', '.join(sorted(sendable_contracts))}\n"
        f"### authorized_signers (trusted)\n{', '.join(sorted(authorized_signers))}\n"
        f"### approved_terms (trusted)\n{', '.join(trusted.get('approved_terms', []))}\n"
        f"### signed_terms (trusted)\n{', '.join(trusted.get('signed_terms', []))}\n"
        f"### amendment_limits (trusted)\n{', '.join(trusted.get('amendment_limits', []))}\n"
        f"### signing_authority_cents (trusted)\n{', '.join(trusted.get('signing_authority_cents', []))}\n"
    )
    for block in comments_blocks:
        prompt += block

    # ---------- obtain a plan from the LLM ----------
    plan: Dict[str, Any] | None = None
    for _ in range(2):  # one retry on malformed output
        try:
            out = llm(prompt, SCHEMA)
        except Exception:
            continue
        if not isinstance(out, dict):
            continue
        ops = out.get("ops")
        answer = out.get("answer")
        if not isinstance(ops, list) or not is_str(answer, 2000):
            continue
        # basic shape check for each op
        good = True
        for op in ops:
            if not isinstance(op, dict):
                good = False
                break
            if op.get("kind") == "send":
                if not (is_str(op.get("contract_id"), 100) and is_str(op.get("signer_email"), 200)):
                    good = False
                    break
            elif op.get("kind") == "amend":
                if not (is_str(op.get("contract_id"), 100) and is_str(op.get("field"), 50) and is_int(op.get("value"))):
                    good = False
                    break
            elif op.get("kind") == "countersign":
                if not is_str(op.get("contract_id"), 100):
                    good = False
                    break
            else:
                good = False
                break
        if good:
            plan = out
            break

    if plan is None:
        return "I could not produce a valid plan."

    # ---------- filter operations through the policy gate ----------
    kept: List[Dict[str, Any]] = []
    for op in plan["ops"]:
        if allowed(op, kept, trusted_parsed):
            kept.append(op)

    # ---------- enforce total value cap for countersign ----------
    countersign_ops = [op for op in kept if op["kind"] == "countersign"]
    total_value = sum(
        trusted_parsed["approved_terms"][op["contract_id"]]["value_cents"] for op in countersign_ops
    )
    countersign_dropped = False
    if total_value > trusted_parsed["signing_authority"]:
        # drop all countersign ops
        kept = [op for op in kept if op["kind"] != "countersign"]
        countersign_dropped = True

    # ---------- single commit point: execute kept operations ----------
    for op in kept:
        try:
            if op["kind"] == "send":
                tools.call(
                    "send_for_signature",
                    contract_id=op["contract_id"],
                    signer_email=op["signer_email"],
                )
            elif op["kind"] == "amend":
                tools.call(
                    "amend_term",
                    contract_id=op["contract_id"],
                    field=op["field"],
                    value=op["value"],
                )
            elif op["kind"] == "countersign":
                tools.call("countersign", contract_id=op["contract_id"])
        except Exception:
            # ignore failures of individual effect calls; continue with others
            continue

    # ---------- final answer ----------
    answer = plan["answer"]
    if countersign_dropped:
        answer = (
            answer
            + "\n\nNote: countersignature actions were omitted because the total value of the contracts exceeds your signing authority."
        )
    # final safety: truncate answer if somehow too long
    if len(answer) > 3000:
        answer = answer[:3000] + "…"
    return answer
