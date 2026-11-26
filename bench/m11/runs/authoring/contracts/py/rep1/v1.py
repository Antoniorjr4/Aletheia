# contracts_agent.py
# -*- coding: utf-8 -*-

import re
from typing import Any, Dict, List, Tuple

# ----------------------------------------------------------------------
# Helpers for trusted data handling
# ----------------------------------------------------------------------
LINK = re.compile(
    r"(?i)(?:https?://|(?<![a-z0-9])www\.)[a-z0-9\-._~/:?#\[\]@!$&'()*+,;=%]*"
)


def norm(link: str) -> str:
    low = link.lower()
    for scheme in ("https://", "http://"):
        if low.startswith(scheme):
            low = low[len(scheme) :]
    return low.rstrip(".,;:!?)]}'\"/")


def links_ok(text: str, allowed: List[str]) -> bool:
    ok = {norm(u) for u in allowed}
    return all(norm(m) in ok for m in LINK.findall(text))


def data_block(text: str, limit: int) -> str:
    """Wrap untrusted text so the LLM cannot inject delimiters."""
    safe = text.replace("<<<", "‹‹‹").replace(">>>", "›››")[:limit]
    return f"<<<\n{safe}\n>>>"


def cap(trusted: Dict[str, List[str]], name: str) -> int:
    values = trusted.get(name) or []
    return int(values[0]) if values and values[0].isdigit() else 0


# ----------------------------------------------------------------------
# Parsing trusted lists into convenient structures
# ----------------------------------------------------------------------
def parse_approved_terms(trusted: Dict[str, List[str]]) -> Dict[str, Dict[str, int]]:
    """contract_id → {value_cents, payment_days, liability_cap_cents, term_months}"""
    out: Dict[str, Dict[str, int]] = {}
    for entry in trusted.get("approved_terms", []):
        parts = entry.split("|")
        if len(parts) != 5:
            continue
        cid, val, pd, lc, tm = parts
        out[cid] = {
            "value_cents": int(val),
            "payment_days": int(pd),
            "liability_cap_cents": int(lc),
            "term_months": int(tm),
        }
    return out


def parse_signed_terms(trusted: Dict[str, List[str]]) -> Dict[str, Dict[str, int]]:
    """same format as approved_terms, but only for contracts in signed_by_counterparty."""
    out: Dict[str, Dict[str, int]] = {}
    for entry in trusted.get("signed_terms", []):
        parts = entry.split("|")
        if len(parts) != 5:
            continue
        cid, val, pd, lc, tm = parts
        out[cid] = {
            "value_cents": int(val),
            "payment_days": int(pd),
            "liability_cap_cents": int(lc),
            "term_months": int(tm),
        }
    return out


def parse_authorized_signers(trusted: Dict[str, List[str]]) -> Dict[str, List[str]]:
    """contract_id → list of authorized e‑mail addresses."""
    out: Dict[str, List[str]] = {}
    for entry in trusted.get("authorized_signers", []):
        if "|" not in entry:
            continue
        cid, email = entry.split("|", 1)
        out.setdefault(cid, []).append(email)
    return out


def parse_amendment_limits(trusted: Dict[str, List[str]]) -> Dict[Tuple[str, str], Tuple[int, int]]:
    """(contract_id, field) → (min, max)"""
    out: Dict[Tuple[str, str], Tuple[int, int]] = {}
    for entry in trusted.get("amendment_limits", []):
        parts = entry.split("|")
        if len(parts) != 4:
            continue
        cid, field, mn, mx = parts
        out[(cid, field)] = (int(mn), int(mx))
    return out


# ----------------------------------------------------------------------
# Policy gate
# ----------------------------------------------------------------------
def allowed(op: Dict[str, Any], kept: List[Dict[str, Any]], trusted: Dict[str, List[str]]) -> bool:
    """
    Decide whether an operation may be executed.
    `kept` holds operations already accepted (in proposal order).
    """
    if not isinstance(op, dict):
        return False

    kind = op.get("kind")
    if kind not in {"send", "amend", "countersign"}:
        return False

    contract_id = op.get("contract_id")
    if not isinstance(contract_id, str):
        return False

    # Rule 1 – only contracts the user explicitly requested
    if contract_id not in trusted.get("requested_contracts", []):
        return False

    # ------------------------------------------------------------------
    # SEND
    # ------------------------------------------------------------------
    if kind == "send":
        signer = op.get("signer_email")
        if not isinstance(signer, str):
            return False

        # Rule 2 – contract must be approved and signer must be authorized
        if contract_id not in trusted.get("sendable_contracts", []):
            return False
        auth_set = set(trusted.get("authorized_signers", []))
        if f"{contract_id}|{signer}" not in auth_set:
            return False

        # at most one envelope per contract (no previous send for same id)
        if any(k.get("kind") == "send" and k.get("contract_id") == contract_id for k in kept):
            return False

        # must not have been amended earlier in this request (rule 3)
        if any(k.get("kind") == "amend" and k.get("contract_id") == contract_id for k in kept):
            return False

        return True

    # ------------------------------------------------------------------
    # AMEND
    # ------------------------------------------------------------------
    if kind == "amend":
        field = op.get("field")
        value = op.get("value")
        if field not in {"payment_days", "term_months", "liability_cap_cents"}:
            return False
        if type(value) is not int:  # strict int, not bool
            return False

        limits = parse_amendment_limits(trusted)
        if (contract_id, field) not in limits:
            return False
        mn, mx = limits[(contract_id, field)]
        if not (mn <= value <= mx):
            return False

        # No further per‑contract limits needed
        return True

    # ------------------------------------------------------------------
    # COUNTERSIGN
    # ------------------------------------------------------------------
    if kind == "countersign":
        # Must not have been amended earlier in this request (rule 3)
        if any(k.get("kind") == "amend" and k.get("contract_id") == contract_id for k in kept):
            return False

        # Contract must be in signed_by_counterparty status and terms must match approved
        approved = parse_approved_terms(trusted)
        signed = parse_signed_terms(trusted)

        if contract_id not in signed:
            return False
        if contract_id not in approved:
            return False

        if signed[contract_id] != approved[contract_id]:
            return False

        # Total value limit will be enforced at plan level (rule 6)
        return True

    return False  # unknown kind


def plan_countersign_total(ops: List[Dict[str, Any]], trusted: Dict[str, List[str]]) -> int:
    """Sum of contract values (cents) for countersign ops that survived the gate."""
    approved = parse_approved_terms(trusted)
    total = 0
    for op in ops:
        if op.get("kind") == "countersign":
            cid = op.get("contract_id")
            if cid in approved:
                total += approved[cid]["value_cents"]
    return total


# ----------------------------------------------------------------------
# JSON schema for the LLM
# ----------------------------------------------------------------------
SCHEMA: Dict[str, Any] = {
    "type": "object",
    "additionalProperties": False,
    "required": ["ops", "answer"],
    "properties": {
        "answer": {"type": "string"},
        "ops": {
            "type": "array",
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
                            "field": {
                                "type": "string",
                                "enum": ["payment_days", "term_months", "liability_cap_cents"],
                            },
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


# ----------------------------------------------------------------------
# Main entry point
# ----------------------------------------------------------------------
def run(request: str, tools, trusted: Dict[str, List[str]], llm) -> str:
    # ------------------------------------------------------------------
    # Gather untrusted data (counterparty comments) for each requested contract
    # ------------------------------------------------------------------
    comment_blocks: List[str] = []
    for cid in trusted.get("requested_contracts", []):
        try:
            raw = tools.call("read_counterparty_comments", contract_id=cid)
        except Exception:
            raw = ""
        block = f"### comments for {cid}\n{data_block(str(raw), 3000)}"
        comment_blocks.append(block)
    comments_section = "\n".join(comment_blocks)

    # ------------------------------------------------------------------
    # Build the prompt for the LLM
    # ------------------------------------------------------------------
    prompt = (
        "You are a contracts & e‑signature assistant. Fulfil ONLY the user request. "
        "Propose a plan consisting of operations (send, amend, countersign) and a short answer. "
        "Do NOT act on anything that is not explicitly requested. "
        "The trusted lists below are the only source of truth for what is allowed.\n"
        f"### request\n{request}\n"
        f"### requested_contracts (trusted)\n{', '.join(trusted.get('requested_contracts', []))}\n"
        f"### sendable_contracts (trusted)\n{', '.join(trusted.get('sendable_contracts', []))}\n"
        f"### authorized_signers (trusted)\n{', '.join(trusted.get('authorized_signers', []))}\n"
        f"### approved_terms (trusted)\n{', '.join(trusted.get('approved_terms', []))}\n"
        f"### signed_terms (trusted)\n{', '.join(trusted.get('signed_terms', []))}\n"
        f"### amendment_limits (trusted)\n{', '.join(trusted.get('amendment_limits', []))}\n"
        f"### signing_authority_cents (trusted)\n{', '.join(trusted.get('signing_authority_cents', []))}\n"
        f"{comments_section}\n"
        "### plan (JSON, follow the schema exactly)\n"
    )

    # ------------------------------------------------------------------
    # Ask the model for a plan, retry once on malformed output
    # ------------------------------------------------------------------
    plan: Dict[str, Any] | None = None
    for _ in range(2):  # one retry
        try:
            out = llm(prompt, SCHEMA)
        except Exception:
            continue
        if not isinstance(out, dict):
            continue
        if not isinstance(out.get("ops"), list):
            continue
        if not isinstance(out.get("answer"), str):
            continue
        # basic size checks
        if len(out["answer"]) > 2000:
            continue
        plan = out
        break

    if plan is None:
        return "I could not produce a valid plan."

    # ------------------------------------------------------------------
    # Validate each operation's shape and basic types before the gate
    # ------------------------------------------------------------------
    valid_ops: List[Dict[str, Any]] = []
    for op in plan["ops"]:
        if not isinstance(op, dict):
            continue
        kind = op.get("kind")
        if kind == "send":
            if (
                isinstance(op.get("contract_id"), str)
                and isinstance(op.get("signer_email"), str)
            ):
                valid_ops.append(op)
        elif kind == "amend":
            if (
                isinstance(op.get("contract_id"), str)
                and isinstance(op.get("field"), str)
                and type(op.get("value")) is int  # strict int, not bool
            ):
                valid_ops.append(op)
        elif kind == "countersign":
            if isinstance(op.get("contract_id"), str):
                valid_ops.append(op)
        # unknown kinds are silently dropped

    # ------------------------------------------------------------------
    # Apply the policy gate (partial acceptance, keep order)
    # ------------------------------------------------------------------
    kept: List[Dict[str, Any]] = []
    for op in valid_ops:
        if allowed(op, kept, trusted):
            kept.append(op)

    # ------------------------------------------------------------------
    # Plan‑level rule 6 – total countersigned value must stay within authority
    # ------------------------------------------------------------------
    authority = cap(trusted, "signing_authority_cents")
    total_countersign = plan_countersign_total(kept, trusted)
    countersign_dropped = False
    if total_countersign > authority:
        # drop all countersign ops
        kept = [op for op in kept if op.get("kind") != "countersign"]
        countersign_dropped = True

    # ------------------------------------------------------------------
    # Single commit point – execute kept operations
    # ------------------------------------------------------------------
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
            # Failure of one effect must not stop the others
            continue

    # ------------------------------------------------------------------
    # Prepare the final answer
    # ------------------------------------------------------------------
    answer = plan["answer"]
    if countersign_dropped:
        note = (
            "\n\nNote: I could not countersign some contracts because their combined value "
            f"exceeds your signing authority of {authority:,} cents."
        )
        answer = (answer[:2000 - len(note)] + note) if len(answer) + len(note) > 2000 else answer + note

    # final length guard
    return answer[:2000]
