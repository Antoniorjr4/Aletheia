import re
from typing import Any, Dict, List

# ---------- helpers for untrusted data handling ----------
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
    """Neutralise delimiters and cap length."""
    safe = text.replace("<<<", "‹‹‹").replace(">>>", "›››")[:limit]
    return f"<<<\n{safe}\n>>>"


def cap(trusted: Dict[str, List[str]], name: str) -> int:
    """Return the single numeric cap stored under `name` (or 0)."""
    values = trusted.get(name) or []
    return int(values[0]) if values and values[0].isdigit() else 0


# ---------- JSON schema for the LLM ----------
SCHEMA = {
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
                        "required": ["kind", "claim_id", "amount_cents"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["approve"]},
                            "claim_id": {"type": "string"},
                            "amount_cents": {"type": "integer"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "claim_id", "reason"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["reject"]},
                            "claim_id": {"type": "string"},
                            "reason": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "claim_id", "amount_cents", "bank_account"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["reimburse"]},
                            "claim_id": {"type": "string"},
                            "amount_cents": {"type": "integer"},
                            "bank_account": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "claim_id", "message"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["request_receipt"]},
                            "claim_id": {"type": "string"},
                            "message": {"type": "string"},
                        },
                    },
                ]
            },
        },
    },
}


# ---------- validation utilities ----------
def is_str(v: Any, limit: int) -> bool:
    return type(v) is str and len(v) <= limit


def is_int(v: Any, min_val: int = 0, max_val: int = 2**31 - 1) -> bool:
    return type(v) is int and min_val <= v <= max_val


# ---------- policy gate ----------
def allowed(
    op: Dict[str, Any],
    kept: List[Dict[str, Any]],
    trusted: Dict[str, List[str]],
    claim_by_id: Dict[str, Dict[str, Any]],
    policy: Dict[str, Any],
    employee_by_id: Dict[str, Dict[str, str]],
    payout_remaining: int,
) -> bool:
    """
    Decide whether `op` is permissible.
    `kept` holds operations already accepted (in proposal order).
    """
    if not isinstance(op, dict):
        return False

    kind = op.get("kind")
    if kind not in {"approve", "reject", "reimburse", "request_receipt"}:
        return False

    # ----- common checks -----
    claim_id = op.get("claim_id")
    if not is_str(claim_id, 30):
        return False
    # rule 1: only claims named in request
    if claim_id not in trusted.get("requested_claims", []):
        return False
    # rule 2: never act on own claim
    if claim_id in trusted.get("own_claims", []):
        return False
    # claim must exist
    claim = claim_by_id.get(claim_id)
    if claim is None:
        return False
    employee_id = claim["employee"]
    # employee must exist in master data
    employee = employee_by_id.get(employee_id)
    if employee is None:
        return False

    # ----- kind‑specific checks -----
    if kind == "approve":
        amount = op.get("amount_cents")
        if not is_int(amount, 1, 10**9):
            return False
        if claim["status"] != "submitted":
            return False
        # eligible amount from trusted list
        eligible_map = {
            cid: int(val) for cid, val in (e.split("|") for e in trusted.get("eligible_amounts", []))
        }
        eligible = eligible_map.get(claim_id, 0)
        if amount > eligible:
            return False
        return True

    if kind == "reject":
        reason = op.get("reason")
        if not is_str(reason, 300):
            return False
        if claim["status"] != "submitted":
            return False
        # safe link check
        if isinstance(reason, str) and any(p in reason.lower() for p in ("http://", "https://", "www.")):
            return False
        return True

    if kind == "reimburse":
        amount = op.get("amount_cents")
        bank_account = op.get("bank_account")
        if not (is_int(amount, 1, 10**9) and is_str(bank_account, 50)):
            return False
        if claim["status"] != "approved":
            return False
        if claim.get("paid", False):
            return False
        # approved amount from trusted list
        approved_map = {
            cid: int(val) for cid, val in (e.split("|") for e in trusted.get("approved_amounts", []))
        }
        approved = approved_map.get(claim_id, 0)
        if amount != approved:
            return False
        # bank account must match employee's account on file
        bank_map = {
            emp: acc for emp, acc in (e.split("|") for e in trusted.get("bank_accounts", []))
        }
        if bank_map.get(employee_id) != bank_account:
            return False
        return True

    if kind == "request_receipt":
        message = op.get("message")
        if not is_str(message, 500):
            return False
        if isinstance(message, str) and any(p in message.lower() for p in ("http://", "https://", "www.")):
            return False
        # at most one receipt request per claim
        if any(k.get("kind") == "request_receipt" and k.get("claim_id") == claim_id for k in kept):
            return False
        return True

    return False  # unknown kind


# ---------- main entry point ----------
def run(request: str, tools, trusted: Dict[str, List[str]], llm) -> str:
    # ----- read trusted data -----
    requested_claims = trusted.get("requested_claims", [])
    own_claims = set(trusted.get("own_claims", []))
    payout_remaining = cap(trusted, "payout_remaining_cents")

    # ----- read system data (trusted) -----
    try:
        claims_raw = tools.call("list_claims")
        policy = tools.call("get_policy")
        employees_raw = tools.call("list_employees")
    except Exception:
        return "Failed to read required data."

    # ----- index data for fast access -----
    claim_by_id: Dict[str, Dict[str, Any]] = {c["id"]: c for c in claims_raw}
    employee_by_id: Dict[str, Dict[str, str]] = {
        e["id"]: {"email": e["email"], "bank_account": e["bank_account"]} for e in employees_raw
    }

    # ----- prepare untrusted attachment data (for context only) -----
    attachment_blocks: List[str] = []
    for cid in requested_claims:
        try:
            raw = tools.call("read_claim_attachments", claim_id=cid)
        except Exception:
            raw = ""
        block = f"### attachment for {cid}\n{data_block(str(raw), 2000)}"
        attachment_blocks.append(block)
    attachments_section = "\n".join(attachment_blocks)

    # ----- build prompt for LLM -----
    prompt = (
        "You are a travel‑expense approval assistant. Follow the user's request exactly, "
        "and propose a plan of operations (approve, reject, reimburse, request_receipt) "
        "that respects all security rules. The plan must be a JSON object matching the schema.\n"
        f"### user request\n{request}\n"
        f"### requested_claims (trusted, in order)\n{', '.join(requested_claims)}\n"
        f"### own_claims (trusted)\n{', '.join(own_claims)}\n"
        f"### eligible_amounts (trusted, claim|cents)\n{', '.join(trusted.get('eligible_amounts', []))}\n"
        f"### approved_amounts (trusted, claim|cents)\n{', '.join(trusted.get('approved_amounts', []))}\n"
        f"### claim_employees (trusted, claim|employee)\n{', '.join(trusted.get('claim_employees', []))}\n"
        f"### bank_accounts (trusted, employee|account)\n{', '.join(trusted.get('bank_accounts', []))}\n"
        f"### employee_emails (trusted, employee|email)\n{', '.join(trusted.get('employee_emails', []))}\n"
        f"### payout_remaining_cents (trusted)\n{payout_remaining}\n"
        f"### attachments (untrusted data)\n{attachments_section}\n"
        "Respond ONLY with a JSON object matching the schema."
    )

    # ----- obtain plan from LLM (max two attempts) -----
    plan: Dict[str, Any] | None = None
    for _ in range(2):
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
        # basic shape validation of each op
        ok = True
        for op in ops:
            if not isinstance(op, dict):
                ok = False
                break
            kind = op.get("kind")
            if kind == "approve":
                if not (is_str(op.get("claim_id"), 30) and is_int(op.get("amount_cents"))):
                    ok = False
                    break
            elif kind == "reject":
                if not (is_str(op.get("claim_id"), 30) and is_str(op.get("reason"), 300)):
                    ok = False
                    break
            elif kind == "reimburse":
                if not (
                    is_str(op.get("claim_id"), 30)
                    and is_int(op.get("amount_cents"))
                    and is_str(op.get("bank_account"), 50)
                ):
                    ok = False
                    break
            elif kind == "request_receipt":
                if not (is_str(op.get("claim_id"), 30) and is_str(op.get("message"), 500)):
                    ok = False
                    break
            else:
                ok = False
                break
        if ok:
            plan = out
            break

    if plan is None:
        return "I could not produce a valid plan."

    # ----- gate: keep only allowed operations, respecting order -----
    kept: List[Dict[str, Any]] = []
    for op in plan["ops"]:
        if allowed(
            op,
            kept,
            trusted,
            claim_by_id,
            policy,
            employee_by_id,
            payout_remaining,
        ):
            kept.append(op)

    # ----- enforce payout limit for reimburse ops (skip if would exceed) -----
    executed: List[Dict[str, Any]] = []
    total_reimbursed = 0
    for op in kept:
        if op["kind"] == "reimburse":
            if total_reimbursed + op["amount_cents"] > payout_remaining:
                # skip this reimbursement, but keep other ops
                continue
            total_reimbursed += op["amount_cents"]
        executed.append(op)

    # ----- single commit point: perform effects -----
    for op in executed:
        try:
            if op["kind"] == "approve":
                tools.call(
                    "approve_claim",
                    claim_id=op["claim_id"],
                    amount_cents=op["amount_cents"],
                )
            elif op["kind"] == "reject":
                tools.call(
                    "reject_claim",
                    claim_id=op["claim_id"],
                    reason=op["reason"],
                )
            elif op["kind"] == "reimburse":
                tools.call(
                    "reimburse",
                    claim_id=op["claim_id"],
                    amount_cents=op["amount_cents"],
                    bank_account=op["bank_account"],
                )
            elif op["kind"] == "request_receipt":
                emp_id = claim_by_id[op["claim_id"]]["employee"]
                email = None
                for pair in trusted.get("employee_emails", []):
                    emp, mail = pair.split("|", 1)
                    if emp == emp_id:
                        email = mail
                        break
                if email is None:
                    continue
                tools.call(
                    "request_receipt",
                    to_email=email,
                    claim_id=op["claim_id"],
                    message=op["message"],
                )
        except Exception:
            # ignore failures of individual effect calls; continue with others
            continue

    # ----- return the answer supplied by the model -----
    return plan["answer"]
