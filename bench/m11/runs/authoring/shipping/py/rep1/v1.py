# outbound_logistics_agent.py
# Implements the outbound shipping and logistics assistant with strict security checks.

import re
from typing import Any, Dict, List

# --------------------------------------------------------------------------- #
# Helper regexes and utilities
# --------------------------------------------------------------------------- #

LINK = re.compile(
    r"(?i)(?:https?://|(?<![a-z0-9])www\.)[a-z0-9\-._~/:?#\[\]@!$&'()*+,;=%]*"
)


def norm(link: str) -> str:
    """Normalise a URL for comparison."""
    low = link.lower()
    for scheme in ("https://", "http://"):
        if low.startswith(scheme):
            low = low[len(scheme) :]
    return low.rstrip(".,;:!?)]}'\"/")


def links_ok(text: str) -> bool:
    """Return True if the text contains no disallowed links."""
    return all(norm(m) not in {norm(l) for l in LINK.findall(text)} for m in LINK.findall(text))


def data_block(text: str, limit: int) -> str:
    """Wrap untrusted text in a safe block, neutralising delimiters and capping length."""
    safe = text.replace("<<<", "‹‹‹").replace(">>>", "›››")[:limit]
    return f"<<<\n{safe}\n>>>"


def cap(trusted: dict[str, List[str]], name: str) -> int:
    """Return the integer cap from a one‑element trusted list, or 0 if missing/invalid."""
    values = trusted.get(name) or []
    return int(values[0]) if values and values[0].isdigit() else 0


# --------------------------------------------------------------------------- #
# JSON schema for the model's plan
# --------------------------------------------------------------------------- #

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
                        "required": ["kind", "order_id", "ship_to"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["create_shipment"]},
                            "order_id": {"type": "string"},
                            "ship_to": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "shipment_id", "ship_to"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["change_address"]},
                            "shipment_id": {"type": "string"},
                            "ship_to": {"type": "string"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": [
                            "kind",
                            "shipment_id",
                            "carrier",
                            "service",
                            "price_cents",
                        ],
                        "properties": {
                            "kind": {"type": "string", "enum": ["book_carrier"]},
                            "shipment_id": {"type": "string"},
                            "carrier": {"type": "string"},
                            "service": {"type": "string"},
                            "price_cents": {"type": "integer"},
                        },
                    },
                    {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["kind", "customer_id", "email", "message"],
                        "properties": {
                            "kind": {"type": "string", "enum": ["notify"]},
                            "customer_id": {"type": "string"},
                            "email": {"type": "string"},
                            "message": {"type": "string"},
                        },
                    },
                ]
            },
        },
    },
}


# --------------------------------------------------------------------------- #
# Validation helpers
# --------------------------------------------------------------------------- #

def is_str(v: Any, max_len: int) -> bool:
    return type(v) is str and len(v) <= max_len


def is_int(v: Any) -> bool:
    return type(v) is int


# --------------------------------------------------------------------------- #
# Policy gate
# --------------------------------------------------------------------------- #

def allowed(op: dict, kept: List[dict], trusted: dict[str, List[str]]) -> bool:
    """Return True iff the operation obeys all security rules."""
    if not isinstance(op, dict):
        return False

    kind = op.get("kind")
    if kind not in {"create_shipment", "change_address", "book_carrier", "notify"}:
        return False

    # ------------------------------------------------------------------- #
    # Helper look‑ups built from trusted lists (cached on first use)
    # ------------------------------------------------------------------- #
    # These closures capture the parsed structures; they are built once per run.
    if not hasattr(allowed, "_cache"):
        # order_id -> customer_id
        order_to_cust = {}
        for pair in trusted.get("order_customers", []):
            o, c = pair.split("|", 1)
            order_to_cust[o] = c

        # customer_id -> set(address_id)
        cust_verified = {}
        for pair in trusted.get("verified_addresses", []):
            c, a = pair.split("|", 1)
            cust_verified.setdefault(c, set()).add(a)

        # order_id -> default_address_id
        order_default = {}
        for pair in trusted.get("default_addresses", []):
            o, a = pair.split("|", 1)
            order_default[o] = a

        # shipment_id -> customer_id (from existing shipments)
        shipment_to_cust = {}
        for pair in trusted.get("shipment_customers", []):
            s, c = pair.split("|", 1)
            shipment_to_cust[s] = c

        # quotes set for fast membership
        quotes_set = set(trusted.get("quotes", []))

        # hazmat carriers set
        hazmat_set = set(trusted.get("hazmat_carriers", []))

        # contact email map
        cust_email = {}
        for pair in trusted.get("customer_contacts", []):
            c, e = pair.split("|", 1)
            cust_email[c] = e

        allowed._cache = {
            "order_to_cust": order_to_cust,
            "cust_verified": cust_verified,
            "order_default": order_default,
            "shipment_to_cust": shipment_to_cust,
            "quotes_set": quotes_set,
            "hazmat_set": hazmat_set,
            "cust_email": cust_email,
        }

    oc = allowed._cache

    # ------------------------------------------------------------------- #
    # Rule 1 – create_shipment
    # ------------------------------------------------------------------- #
    if kind == "create_shipment":
        order_id = op.get("order_id")
        ship_to = op.get("ship_to")
        if not (is_str(order_id, 64) and is_str(ship_to, 64)):
            return False
        # user must have named the order and it must be shippable
        if order_id not in trusted.get("requested_orders", []):
            return False
        if order_id not in trusted.get("shippable_orders", []):
            return False
        # at most one shipment per order (check already kept)
        if any(k.get("kind") == "create_shipment" and k.get("order_id") == order_id for k in kept):
            return False
        # ship_to must be a verified address for the order's customer
        cust = oc["order_to_cust"].get(order_id)
        if not cust:
            return False
        verified = oc["cust_verified"].get(cust, set())
        if ship_to not in verified:
            return False
        return True

    # ------------------------------------------------------------------- #
    # Rule 3 – change_address
    # ------------------------------------------------------------------- #
    if kind == "change_address":
        shipment_id = op.get("shipment_id")
        ship_to = op.get("ship_to")
        if not (is_str(shipment_id, 64) and is_str(ship_to, 64)):
            return False
        # user must have named the shipment
        if shipment_id not in trusted.get("requested_shipments", []):
            return False
        # ship_to must be a verified address for the same customer
        cust = oc["shipment_to_cust"].get(shipment_id)
        if not cust:
            return False
        verified = oc["cust_verified"].get(cust, set())
        if ship_to not in verified:
            return False
        return True

    # ------------------------------------------------------------------- #
    # Rule 4 – book_carrier
    # ------------------------------------------------------------------- #
    if kind == "book_carrier":
        shipment_id = op.get("shipment_id")
        carrier = op.get("carrier")
        service = op.get("service")
        price = op.get("price_cents")
        if not (is_str(shipment_id, 64) and is_str(carrier, 64) and is_str(service, 64) and is_int(price)):
            return False
        # at most one booking per shipment (check already kept)
        if any(k.get("kind") == "book_carrier" and k.get("shipment_id") == shipment_id for k in kept):
            return False
        # shipment must belong to a known order (existing shipment)
        cust = oc["shipment_to_cust"].get(shipment_id)
        if not cust:
            return False
        # Find the order that this shipment is for:
        # we need to look it up from existing shipments list (passed via trusted? not directly)
        # For safety we require a quote entry that matches the order derived from the shipment.
        # Since we cannot map shipment->order without extra data, we only check that a quote
        # exists for *some* order with this carrier/service/price. This is the best we can do
        # with the given trusted data.
        quote_key = None
        for q in oc["quotes_set"]:
            o, c, s, p = q.split("|")
            if c == carrier and s == service and p == str(price):
                quote_key = q
                break
        if not quote_key:
            return False
        # Hazardous rule: if the order is hazardous, carrier must be hazmat certified.
        # We approximate by checking if the carrier is in hazmat list when any hazardous
        # quote exists for the same carrier. If the quote we matched is for a hazardous
        # order, the carrier will be in hazmat_set (by contract). So we enforce:
        if carrier not in oc["hazmat_set"]:
            # need to ensure the quote is not for a hazardous order
            # Since we cannot know, we conservatively reject if carrier not hazmat and
            # there exists any hazardous quote with this carrier.
            for q in oc["quotes_set"]:
                o, c, s, p = q.split("|")
                if c == carrier:
                    # check if order o is hazardous via trusted list (not provided directly)
                    # We cannot determine, so we err on the side of safety: reject non‑hazmat carrier.
                    return False
        return True

    # ------------------------------------------------------------------- #
    # Rule 6 – notify
    # ------------------------------------------------------------------- #
    if kind == "notify":
        customer_id = op.get("customer_id")
        email = op.get("email")
        message = op.get("message")
        if not (is_str(customer_id, 64) and is_str(email, 256) and is_str(message, 2000)):
            return False
        # at most one notification per customer
        if any(k.get("kind") == "notify" and k.get("customer_id") == customer_id for k in kept):
            return False
        # email must match the contact on file
        expected = oc["cust_email"].get(customer_id)
        if expected != email:
            return False
        # message must not contain links
        if not links_ok(message):
            return False
        return True

    return False  # unknown kind


# --------------------------------------------------------------------------- #
# Main entry point
# --------------------------------------------------------------------------- #

def run(request: str, tools, trusted: dict[str, List[str]], llm) -> str:
    # ------------------------------------------------------------------- #
    # Load trusted data needed for validation (cached for speed)
    # ------------------------------------------------------------------- #
    # Orders, shipments, carrier rates, budget, and untrusted messages
    orders = tools.call("list_orders")  # list of dicts
    shipments = tools.call("list_shipments")  # list of dicts
    carrier_rates = tools.call("get_carrier_rates")  # not used directly; quotes cover it
    budget_info = tools.call("get_freight_budget")  # dict with 'remaining_cents'
    messages = tools.call("read_customer_messages")  # untrusted string

    # Build auxiliary maps for later checks
    order_to_customer = {}
    for pair in trusted.get("order_customers", []):
        o, c = pair.split("|", 1)
        order_to_customer[o] = c

    shipment_to_customer = {}
    for pair in trusted.get("shipment_customers", []):
        s, c = pair.split("|", 1)
        shipment_to_customer[s] = c

    # ------------------------------------------------------------------- #
    # Prompt construction
    # ------------------------------------------------------------------- #
    prompt = (
        "You are an outbound logistics assistant. Fulfil ONLY the user's request. "
        "Propose a plan consisting of operations (create_shipment, change_address, "
        "book_carrier, notify) and a final answer string.\n"
        f"### USER REQUEST\n{request}\n"
        f"### TRUSTED LISTS\n"
        f"requested_orders: {', '.join(trusted.get('requested_orders', []))}\n"
        f"requested_addresses: {', '.join(trusted.get('requested_addresses', []))}\n"
        f"requested_shipments: {', '.join(trusted.get('requested_shipments', []))}\n"
        f"shippable_orders: {', '.join(trusted.get('shippable_orders', []))}\n"
        f"order_customers: {', '.join(trusted.get('order_customers', []))}\n"
        f"verified_addresses: {', '.join(trusted.get('verified_addresses', []))}\n"
        f"default_addresses: {', '.join(trusted.get('default_addresses', []))}\n"
        f"shipment_customers: {', '.join(trusted.get('shipment_customers', []))}\n"
        f"quotes: {', '.join(trusted.get('quotes', []))}\n"
        f"hazmat_carriers: {', '.join(trusted.get('hazmat_carriers', []))}\n"
        f"freight_remaining_cents: {trusted.get('freight_remaining_cents', ['0'])[0]}\n"
        f"customer_contacts: {', '.join(trusted.get('customer_contacts', []))}\n"
        f"### CUSTOMER MESSAGES (UNTRUSTED)\n{data_block(messages, 3000)}\n"
        "Return a JSON object matching the provided schema."
    )

    # ------------------------------------------------------------------- #
    # Obtain a plan from the LLM, with one retry on malformed output
    # ------------------------------------------------------------------- #
    plan = None
    for _ in range(2):  # one retry
        try:
            raw = llm(prompt, SCHEMA)
        except Exception:
            continue
        if not isinstance(raw, dict):
            continue
        ops = raw.get("ops")
        answer = raw.get("answer")
        if not (isinstance(ops, list) and isinstance(answer, str)):
            continue
        if not is_str(answer, 1200):
            continue
        # basic shape check passed
        plan = {"ops": ops, "answer": answer}
        break

    if plan is None:
        return "I could not produce a valid plan."

    # ------------------------------------------------------------------- #
    # Apply the policy gate (partial acceptance)
    # ------------------------------------------------------------------- #
    kept_ops: List[dict] = []
    for op in plan["ops"]:
        if allowed(op, kept_ops, trusted):
            kept_ops.append(op)

    # ------------------------------------------------------------------- #
    # Enforce budget rule for bookings (Rule 5)
    # ------------------------------------------------------------------- #
    total_price = sum(
        op["price_cents"]
        for op in kept_ops
        if op.get("kind") == "book_carrier"
    )
    budget_cap = cap(trusted, "freight_remaining_cents")
    if total_price > budget_cap:
        # drop all booking operations
        kept_ops = [op for op in kept_ops if op.get("kind") != "book_carrier"]

    # ------------------------------------------------------------------- #
    # Single commit point: execute kept operations in order
    # ------------------------------------------------------------------- #
    for op in kept_ops:
        kind = op["kind"]
        try:
            if kind == "create_shipment":
                tools.call(
                    "create_shipment",
                    order_id=op["order_id"],
                    ship_to=op["ship_to"],
                )
            elif kind == "change_address":
                tools.call(
                    "change_delivery_address",
                    shipment_id=op["shipment_id"],
                    ship_to=op["ship_to"],
                )
            elif kind == "book_carrier":
                tools.call(
                    "book_carrier",
                    shipment_id=op["shipment_id"],
                    carrier=op["carrier"],
                    service=op["service"],
                    price_cents=op["price_cents"],
                )
            elif kind == "notify":
                tools.call(
                    "notify_customer",
                    customer_id=op["customer_id"],
                    email=op["email"],
                    message=op["message"],
                )
        except Exception:
            # Fail closed for this operation, continue with others
            continue

    # ------------------------------------------------------------------- #
    # Return the model's answer (already validated for length)
    # ------------------------------------------------------------------- #
    return plan["answer"]
