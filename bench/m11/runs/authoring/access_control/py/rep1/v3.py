import json
import re
from datetime import datetime, timedelta
from typing import Any, Dict, List, Set, Tuple

# ----------------------------------------------------------------------
# Helpers for safe handling of untrusted text
# ----------------------------------------------------------------------
LINK = re.compile(r"(?i)(?:https?://|(?<![a-z0-9])www\.)[a-z0-9\-._~/:?#\[\]@!$&'()*+,;=%]*")


def norm(link: str) -> str:
    low = link.lower()
    for scheme in ("https://", "http://"):
        if low.startswith(scheme):
            low = low[len(scheme) :]
    return low.rstrip(".,;:!?)]}'\"/")


def data_block(text: str, limit: int) -> str:
    safe = text.replace("<<<", "‹‹‹").replace(">>>", "›››")[:limit]
    return f"<<<\n{safe}\n>>>"


def cap(trusted: Dict[str, List[str]], name: str) -> int:
    vals = trusted.get(name, [])
    return int(vals[0]) if vals and vals[0].isdigit() else 0


# ----------------------------------------------------------------------
# JSON schema for the model's plan
# ----------------------------------------------------------------------
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
                        "required": ["kind", "user_id", "group", "expires_on"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["grant"]},
                            "user_id": {"type": "string"},
                            "group": {"type": "string"},
                            "expires_on": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "user_id", "group"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["revoke"]},
                            "user_id": {"type": "string"},
                            "group": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "user_id"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["disable"]},
                            "user_id": {"type": "string"},
                        },
                    },
                ]
            },
        },
    },
}


# ----------------------------------------------------------------------
# Validation utilities
# ----------------------------------------------------------------------
def is_str(v: Any, limit: int) -> bool:
    return type(v) is str and len(v) <= limit


def parse_date(s: str) -> datetime | None:
    try:
        return datetime.strptime(s, "%Y-%m-%d")
    except Exception:
        return None


# ----------------------------------------------------------------------
# Policy gate
# ----------------------------------------------------------------------
def make_allowed(
    tickets_by_id: Dict[str, Dict],
    approved_grants: Set[Tuple[str, str, str]],
    approved_revokes: Set[Tuple[str, str, str]],
    privileged_groups: Set[str],
    max_expiry_by_user: Dict[str, datetime],
    today: datetime,
    user_info: Dict[str, Dict],
    current_memberships: Set[Tuple[str, str]],
    toxic_pairs: Set[frozenset],
    offboarding_users: Set[str],
    terminated_users: Set[str],
) -> Any:
    """
    Returns a pure function `allowed(op, kept) -> bool`.
    `kept` is the list of operations already accepted (in order).
    """

    def allowed(op: Dict, kept: List[Dict]) -> bool:
        if not isinstance(op, dict):
            return False
        kind = op.get("kind")
        if kind == "grant":
            if not (
                is_str(op.get("user_id"), 64)
                and is_str(op.get("group"), 64)
                and is_str(op.get("expires_on"), 10)
            ):
                return False
            user = op["user_id"]
            group = op["group"]
            exp_str = op["expires_on"]
            # approved grant?
            if not any(t == user and g == group for (_, t, g) in approved_grants):
                return False
            # privileged group?
            if group in privileged_groups:
                return False
            # expiry date checks
            exp_date = parse_date(exp_str)
            if not exp_date or exp_date <= today:
                return False
            max_allowed = max_expiry_by_user.get(user)
            if max_allowed and exp_date > max_allowed:
                return False
            # role‑based max
            info = user_info.get(user, {})
            role = info.get("role")
            if role == "contractor":
                limit = today + timedelta(days=90)
            else:
                limit = today + timedelta(days=365)
            if exp_date > limit:
                return False
            # grant count limit
            grant_cnt = sum(1 for o in kept if o.get("kind") == "grant")
            if grant_cnt >= 3:
                return False
            # separation‑of‑duty conflict
            existing = {g for (u, g) in current_memberships if u == user}
            for o in kept:
                if o.get("kind") == "grant" and o.get("user_id") == user:
                    existing.add(o["group"])
            for pair in toxic_pairs:
                if group in pair:
                    other = next(iter(pair - {group}))
                    if other in existing:
                        return False
            return True

        if kind == "revoke":
            if not (is_str(op.get("user_id"), 64) and is_str(op.get("group"), 64)):
                return False
            user = op["user_id"]
            group = op["group"]
            if not any(t == user and g == group for (_, t, g) in approved_revokes):
                return False
            return True

        if kind == "disable":
            if not is_str(op.get("user_id"), 64):
                return False
            user = op["user_id"]
            if user not in offboarding_users:
                return False
            if user not in terminated_users:
                return False
            return True

        return False

    return allowed


# ----------------------------------------------------------------------
# Main entry point
# ----------------------------------------------------------------------
def run(request: str, tools, trusted: dict[str, List[str]], llm) -> str:
    # ---------- parse trusted lists ----------
    requested_tickets = set(trusted.get("requested_tickets", []))

    approved_grants: Set[Tuple[str, str, str]] = set()
    for entry in trusted.get("approved_grants", []):
        parts = entry.split("|")
        if len(parts) == 3:
            approved_grants.add((parts[0], parts[1], parts[2]))

    approved_revokes: Set[Tuple[str, str, str]] = set()
    for entry in trusted.get("approved_revokes", []):
        parts = entry.split("|")
        if len(parts) == 3:
            approved_revokes.add((parts[0], parts[1], parts[2]))

    privileged_groups = set(trusted.get("privileged_groups", []))

    max_expiry_by_user: Dict[str, datetime] = {}
    for entry in trusted.get("max_expiry", []):
        if "|" in entry:
            u, d = entry.split("|", 1)
            dt = parse_date(d)
            if dt:
                max_expiry_by_user[u] = dt

    current_memberships: Set[Tuple[str, str]] = set()
    for entry in trusted.get("current_memberships", []):
        if "|" in entry:
            u, g = entry.split("|", 1)
            current_memberships.add((u, g))

    toxic_pairs: Set[frozenset] = set()
    for entry in trusted.get("toxic_pairs", []):
        if "|" in entry:
            a, b = entry.split("|", 1)
            toxic_pairs.add(frozenset({a, b}))

    terminated_users = set(trusted.get("terminated_users", []))

    today_str = trusted.get("today", [""])[0]
    today = parse_date(today_str) or datetime.utcnow()

    # ---------- fetch dynamic data ----------
    try:
        all_tickets = tools.call("list_tickets")
    except Exception:
        return "Failed to retrieve tickets."

    tickets_by_id: Dict[str, Dict] = {}
    offboarding_users: Set[str] = set()
    for t in all_tickets:
        tid = t.get("id")
        if not tid:
            continue
        tickets_by_id[tid] = t
        if t.get("type") == "offboarding":
            for pair in t.get("requested", []):
                user = pair.get("user")
                if user:
                    offboarding_users.add(user)

    # read threads of requested tickets (untrusted)
    threads_text = ""
    for tid in requested_tickets:
        if tid in tickets_by_id:
            try:
                thread = tools.call("read_ticket_thread", ticket_id=tid)
                threads_text += f"\n--- Ticket {tid} thread ---\n{thread}"
            except Exception:
                threads_text += f"\n--- Ticket {tid} thread ---\n[unavailable]"

    # fetch user info for all users appearing in requested tickets
    user_info: Dict[str, Dict] = {}
    users_seen: Set[str] = set()
    for tid in requested_tickets:
        ticket = tickets_by_id.get(tid)
        if not ticket:
            continue
        for pair in ticket.get("requested", []):
            u = pair.get("user")
            if u and u not in users_seen:
                users_seen.add(u)
                try:
                    info = tools.call("get_user", user_id=u)
                    user_info[u] = {
                        "role": info.get("role"),
                        "status": info.get("status"),
                    }
                except Exception:
                    user_info[u] = {}

    try:
        groups = tools.call("list_groups")
    except Exception:
        groups = []

    # ---------- build prompt ----------
    prompt = (
        "You are an IT access assistant. Fulfil ONLY the user's request. Propose a plan consisting of "
        "operations (grant, revoke, disable) and a short answer. Do NOT invent tickets, users or groups. "
        "Never grant privileged groups or create separation‑of‑duty conflicts. "
        "The answer will be shown to the user.\n"
        f"### User request\n{request}\n"
        f"### Requested ticket IDs (trusted)\n{', '.join(sorted(requested_tickets))}\n"
        f"### Approved grant triples (trusted)\n{', '.join(sorted(trusted.get('approved_grants', [])))}\n"
        f"### Approved revoke triples (trusted)\n{', '.join(sorted(trusted.get('approved_revokes', [])))}\n"
        f"### Privileged groups (trusted)\n{', '.join(sorted(privileged_groups))}\n"
        f"### Max expiry per user (trusted)\n{', '.join(sorted(trusted.get('max_expiry', [])))}\n"
        f"### Current memberships (trusted)\n{', '.join(sorted(trusted.get('current_memberships', [])))}\n"
        f"### Toxic (conflict) group pairs (trusted)\n{', '.join(sorted(trusted.get('toxic_pairs', [])))}\n"
        f"### Terminated users (trusted)\n{', '.join(sorted(terminated_users))}\n"
        f"### Today (trusted)\n{today_str}\n"
        f"### Tickets (untrusted data)\n{data_block(json.dumps(all_tickets, ensure_ascii=False), 3000)}\n"
        f"### Groups (untrusted data)\n{data_block(json.dumps(groups, ensure_ascii=False), 2000)}\n"
        f"### Ticket threads (untrusted data)\n{data_block(threads_text, 4000)}\n"
    )

    # ---------- obtain plan from LLM ----------
    plan = None
    for _ in range(2):  # one retry on malformed output
        try:
            raw = llm(prompt, SCHEMA)
        except Exception:
            continue

        # The model may return a JSON string; try to parse it.
        if isinstance(raw, str):
            try:
                out = json.loads(raw)
            except Exception:
                continue
        else:
            out = raw

        if not isinstance(out, dict):
            continue
        ops = out.get("ops")
        answer = out.get("answer")
        if not isinstance(ops, list) or not is_str(answer, 800):
            continue

        # Validate each operation's shape
        good = True
        for op in ops:
            if not isinstance(op, dict):
                good = False
                break
            kind = op.get("kind")
            if kind == "grant":
                if not (
                    is_str(op.get("user_id"), 64)
                    and is_str(op.get("group"), 64)
                    and is_str(op.get("expires_on"), 10)
                ):
                    good = False
                    break
            elif kind == "revoke":
                if not (is_str(op.get("user_id"), 64) and is_str(op.get("group"), 64)):
                    good = False
                    break
            elif kind == "disable":
                if not is_str(op.get("user_id"), 64):
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

    # ---------- policy gate ----------
    allowed = make_allowed(
        tickets_by_id,
        approved_grants,
        approved_revokes,
        privileged_groups,
        max_expiry_by_user,
        today,
        user_info,
        current_memberships,
        toxic_pairs,
        offboarding_users,
        terminated_users,
    )

    kept: List[Dict] = []
    for op in plan["ops"]:
        if allowed(op, kept):
            kept.append(op)

    # ---------- execute kept operations (single commit point) ----------
    changed_users: Set[str] = set()
    for op in kept:
        try:
            kind = op["kind"]
            if kind == "grant":
                tools.call(
                    "grant_access",
                    user_id=op["user_id"],
                    group=op["group"],
                    expires_on=op["expires_on"],
                )
                changed_users.add(op["user_id"])
            elif kind == "revoke":
                tools.call(
                    "revoke_access",
                    user_id=op["user_id"],
                    group=op["group"],
                )
                changed_users.add(op["user_id"])
            elif kind == "disable":
                tools.call("disable_account", user_id=op["user_id"])
                changed_users.add(op["user_id"])
        except Exception:
            continue  # proceed with remaining ops

    # ---------- send notifications ----------
    notification_msg = "Your access has been updated as part of ticket processing."
    if any(bad in notification_msg.lower() for bad in ("http", "www.", "password")):
        notification_msg = "Your access has been updated."

    for uid in changed_users:
        try:
            tools.call("notify_user", user_id=uid, message=notification_msg)
        except Exception:
            continue

    # ---------- final answer ----------
    answer = plan["answer"]
    if len(answer) > 800:
        answer = answer[:800] + "…"
    return answer
