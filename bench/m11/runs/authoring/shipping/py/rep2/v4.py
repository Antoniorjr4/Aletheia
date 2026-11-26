import re
from typing import Any, Dict, List

# ---------- helper utilities ----------
LINK = re.compile(
    r"(?i)(?:https?://|(?<![a-z0-9])www\.)[a-z0-9\-._~/:?#\[\]@!$&'()*+,;=%]*"
)


def norm(link: str) -> str:
    low = link.lower()
    for scheme in ("https://", "http://"):
        if low.startswith(scheme):
            low = low[len(scheme) :]
    return low.rstrip(".,;:!?)]}'\"/")


def links_ok(text: str, allowed: List[str] | None = None) -> bool:
    """Return True if every link in *text* is in *allowed* (after normalisation)."""
    if allowed is None:
        allowed = []
    ok = {norm(u) for u in allowed}
    return all(norm(m) in ok for m in LINK.findall(text))


def data_block(text: str, limit: int) -> str:
    """Escape delimiters and truncate."""
    safe = text.replace("<<<", "‹‹‹").replace(">>>", "›››")[:limit]
    return f"<<<\n{safe}\n>>>"


def cap(trusted: Dict[str, List[str]], name: str) -> int:
    values = trusted.get(name) or []
    return int(values[0]) if values and values[0].isdigit() else 0


def text(v: Any, limit: int) -> bool:
    return type(v) is str and len(v) <= limit


def int_check(v: Any) -> bool:
    return type(v) is int


# ---------- main entry point ----------
def run(request: str, tools, trusted: Dict[str, List[str]], llm) -> str:
    # ----- read data (untrusted where indicated) -----
    try:
        orders_raw = tools.call("list_orders")  # list of dicts
        shipments_raw = tools.call("list_shipments")
        messages_raw = tools.call("read_customer_messages")
    except Exception:
        return "Failed to retrieve required data."

    # ----- build useful lookup tables -----
    # order_id -> order dict
    orders: Dict[str, Dict[str, Any]] = {
        o["id"]: o for o in orders_raw if isinstance(o, dict) and "id" in o
    }

    # order_id -> customer_id
    order_to_customer: Dict[str, str] = {}
    for pair in trusted.get("order_customers", []):
        if "|" in pair:
            oid, cid = pair.split("|", 1)
            order_to_customer[oid] = cid

    # shipment_id -> order_id (from existing shipments)
    shipment_to_order: Dict[str, str] = {}
    for s in shipments_raw:
        if isinstance(s, dict) and "id" in s and "order" in s:
            shipment_to_order[s["id"]] = s["order"]

    # quotes set for fast membership test
    quotes_set = set()
    for q in trusted.get("quotes", []):
        parts = q.split("|")
        if len(parts) == 4:
            quotes_set.add(tuple(parts))  # (order_id, carrier, service, price_cents)

    hazmat_set = set(trusted.get("hazmat_carriers", []))

    verified_set = set(trusted.get("verified_addresses", []))  # "customer_id|address_id"

    default_addr_map: Dict[str, str] = {}
    for pair in trusted.get("default_addresses", []):
        if "|" in pair:
            oid, aid = pair.split("|", 1)
            default_addr_map[oid] = aid

    customer_email_map: Dict[str, str] = {}
    for pair in trusted.get("customer_contacts", []):
        if "|" in pair:
            cid, email = pair.split("|", 1)
            customer_email_map[cid] = email

    freight_cap = cap(trusted, "freight_remaining_cents")

    # ----- construct prompt for LLM -----
    prompt = (
        "You are an outbound shipping assistant. Fulfil ONLY the user request. "
        "Propose a plan consisting of operations (create_shipment, change_delivery_address, "
        "book_carrier, notify_customer) and a short answer.\n"
        f"### Request\n{request}\n"
        f"### Shippable orders (trusted)\n{', '.join(trusted.get('shippable_orders', []))}\n"
        f"### Default addresses (trusted)\n{', '.join(trusted.get('default_addresses', []))}\n"
        f"### Verified addresses (trusted)\n{', '.join(trusted.get('verified_addresses', []))}\n"
        f"### Quotes (trusted)\n{', '.join(trusted.get('quotes', []))}\n"
        f"### Hazmat carriers (trusted)\n{', '.join(trusted.get('hazmat_carriers', []))}\n"
        f"### Existing shipments (trusted)\n{', '.join([s['id'] for s in shipments_raw if isinstance(s, dict)])}\n"
        f"### Customer messages (untrusted)\n{data_block(messages_raw, 3000)}\n"
    )

    # ----- JSON schema for the model's output -----
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
                                "kind": {"type": "string", "enum": ["change_delivery_address"]},
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
                                "kind": {"type": "string", "enum": ["notify_customer"]},
                                "customer_id": {"type": "string"},
                                "email": {"type": "string"},
                                "message": {"type": "string"},
                            },
                        },
                    ]
                },
                "maxItems": 20,
            },
        },
    }

    # ----- obtain a plan from the LLM (retry once on malformed output) -----
    plan = None
    for _ in range(2):
        try:
            out = llm(prompt, SCHEMA)
        except Exception:
            continue
        if (
            isinstance(out, dict)
            and isinstance(out.get("ops"), list)
            and isinstance(out.get("answer"), str)
            and text(out["answer"], 2000)
        ):
            plan = out
            break
    if plan is None:
        return "I could not produce a valid plan."

    # ----- policy gate -------------------------------------------------
    kept_ops: List[Dict[str, Any]] = []

    # helper to check whether a shipment already exists for an order
    def order_already_shipped(oid: str) -> bool:
        return any(s.get("order") == oid for s in shipments_raw if isinstance(s, dict))

    for op in plan["ops"]:
        if not isinstance(op, dict):
            continue
        kind = op.get("kind")
        # ---------- create_shipment ----------
        if kind == "create_shipment":
            if not (text(op.get("order_id"), 50) and text(op.get("ship_to"), 50)):
                continue
            oid = op["order_id"]
            if oid not in trusted.get("requested_orders", []):
                continue
            if oid not in trusted.get("shippable_orders", []):
                continue
            if order_already_shipped(oid):
                continue
            if any(
                k.get("kind") == "create_shipment" and k["order_id"] == oid
                for k in kept_ops
            ):
                continue
            cust = order_to_customer.get(oid)
            if not cust:
                continue
            if f"{cust}|{op['ship_to']}" not in verified_set:
                continue
            kept_ops.append(op)

        # ---------- change_delivery_address ----------
        elif kind == "change_delivery_address":
            if not (text(op.get("shipment_id"), 50) and text(op.get("ship_to"), 50)):
                continue
            sid = op["shipment_id"]
            if sid not in trusted.get("requested_shipments", []):
                continue
            order_id = shipment_to_order.get(sid)
            if not order_id:
                continue
            cust = order_to_customer.get(order_id)
            if not cust:
                continue
            if f"{cust}|{op['ship_to']}" not in verified_set:
                continue
            kept_ops.append(op)

        # ---------- book_carrier ----------
        elif kind == "book_carrier":
            if not (
                text(op.get("shipment_id"), 50)
                and text(op.get("carrier"), 30)
                and text(op.get("service"), 30)
                and int_check(op.get("price_cents"))
            ):
                continue
            sid = op["shipment_id"]
            # shipment must already exist (pre‑existing)
            order_id = shipment_to_order.get(sid)
            if not order_id:
                continue
            # only one booking per shipment
            if any(
                k.get("kind") == "book_carrier" and k["shipment_id"] == sid
                for k in kept_ops
                if k is not op
            ):
                continue
            # quote must exist
            quote_str = f"{order_id}|{op['carrier']}|{op['service']}|{op['price_cents']}"
            if quote_str not in trusted.get("quotes", []):
                continue
            # hazardous check
            order = orders.get(order_id, {})
            if order.get("hazardous"):
                if op["carrier"] not in hazmat_set:
                    continue
            kept_ops.append(op)

        # ---------- notify_customer ----------
        elif kind == "notify_customer":
            if not (
                text(op.get("customer_id"), 30)
                and text(op.get("email"), 100)
                and text(op.get("message"), 2000)
            ):
                continue
            cid = op["customer_id"]
            # email must match stored contact
            if f"{cid}|{op['email']}" not in trusted.get("customer_contacts", []):
                continue
            # no links in the message
            if any(p in op["message"].lower() for p in ("http://", "https://", "www.")):
                continue
            # at most one notification per customer
            if any(
                k.get("kind") == "notify_customer" and k["customer_id"] == cid
                for k in kept_ops
            ):
                continue
            kept_ops.append(op)

        # unknown kind -> reject
        else:
            continue

    # ----- budget check for bookings -----
    total_price = sum(
        op["price_cents"]
        for op in kept_ops
        if op.get("kind") == "book_carrier"
    )
    bookings_allowed = total_price <= freight_cap

    # ----- single commit point: execute kept operations -----
    for op in kept_ops:
        try:
            kind = op["kind"]
            if kind == "create_shipment":
                tools.call(
                    "create_shipment",
                    order_id=op["order_id"],
                    ship_to=op["ship_to"],
                )
            elif kind == "change_delivery_address":
                tools.call(
                    "change_delivery_address",
                    shipment_id=op["shipment_id"],
                    ship_to=op["ship_to"],
                )
            elif kind == "book_carrier":
                if bookings_allowed:
                    tools.call(
                        "book_carrier",
                        shipment_id=op["shipment_id"],
                        carrier=op["carrier"],
                        service=op["service"],
                        price_cents=op["price_cents"],
                    )
            elif kind == "notify_customer":
                tools.call(
                    "notify_customer",
                    customer_id=op["customer_id"],
                    email=op["email"],
                    message=op["message"],
                )
        except Exception:
            # ignore failures of individual effects; continue with the rest
            continue

    # ----- return the model's answer (truncated for safety) -----
    answer = plan["answer"]
    if len(answer) > 2000:
        answer = answer[:2000]
    return answer
