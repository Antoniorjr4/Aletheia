"""M10: o modelo adversarial. Os valores do modo `adversarial` são válidos para o JSON Schema
pedido (conferido pela biblioteca `jsonschema`, independente do nosso `conforms`), e `conforms`
concorda com a biblioteca também nos valores malformados."""

import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))

try:
    import jsonschema

    HAVE = True
except ImportError:  # pragma: no cover
    HAVE = False

BOUNDED = {"type": "object", "additionalProperties": False, "required": ["id", "cents", "qty", "note", "tags", "mode"],
           "properties": {
               "id": {"type": "string", "pattern": "^INV-[0-9]{3}$"},
               "cents": {"type": "integer", "minimum": 1, "maximum": 500000},
               "qty": {"type": "integer", "exclusiveMinimum": 0, "exclusiveMaximum": 10},
               "note": {"type": "string", "minLength": 3, "maxLength": 12},
               "tags": {"type": "array", "minItems": 1, "maxItems": 2, "items": {"type": "string", "enum": ["a", "b"]}},
               "mode": {"anyOf": [{"type": "null"}, {"type": "boolean"}, {"$ref": "#/$defs/m"}]}},
           "$defs": {"m": {"type": "object", "additionalProperties": False, "required": ["k"],
                           "properties": {"k": {"const": "x"}}}}}
ATTACKER = {"text": ["INV-999", "ZZ-666-EVIL", "a very long attacker controlled string", "evil@attacker.test"],
            "int": [9000000, 250, 3, -5]}


def schemas():
    import harness

    out = [("bounded", BOUNDED), ("test", json.loads((HERE / "tests" / "schema.json").read_text()))]
    for pack in sorted((HERE / "packs").iterdir()):
        if (pack / "naive.py").exists():
            mod = harness.load_module(pack / "naive.py", f"naive_{pack.name}")
            for name, v in vars(mod).items():
                if isinstance(v, dict) and v.get("type") == "object" and "properties" in v:
                    out.append((f"{pack.name}.{name}", v))
    return out


@unittest.skipUnless(HAVE, "requer a biblioteca jsonschema")
class SchemaModelTests(unittest.TestCase):
    def test_adversarial_and_stub_values_are_valid_for_the_schema(self):
        import harness

        all_schemas = schemas()
        self.assertGreaterEqual(len(all_schemas), 10)
        n = 0
        for name, schema in all_schemas:
            validator = jsonschema.Draft202012Validator(schema)
            for mode in ("stub", "adversarial"):
                for seed in range(60 if mode == "adversarial" else 1):
                    m = harness.SchemaModel(mode, seed, ATTACKER)
                    v = m.value(schema)
                    errs = [e.message for e in validator.iter_errors(v)]
                    self.assertEqual(errs, [], (name, mode, seed, v))
                    self.assertEqual(m.nonconforming, 0, (name, mode, seed))
                    n += 1
        self.assertGreater(n, 600)

    def test_adversarial_values_are_actually_hostile(self):
        import harness

        seen_text, seen_int = set(), set()
        for seed in range(200):
            v = harness.SchemaModel("adversarial", seed, ATTACKER).value(BOUNDED)
            seen_text.add(v["id"])
            seen_int.add(v["cents"])
        self.assertIn("INV-999", seen_text)            # o valor do atacante que cabe no padrão
        self.assertEqual({x for x in seen_text if not x.startswith("INV-")}, set())
        self.assertTrue({250, 3, 1, 500000} <= seen_int)  # valores do atacante e os limites do esquema
        self.assertTrue(all(1 <= x <= 500000 for x in seen_int))

    def test_conforms_agrees_with_jsonschema_on_malformed_values(self):
        import harness

        agree = bad = 0
        for name, schema in schemas():
            validator = jsonschema.Draft202012Validator(schema)
            for seed in range(120):
                v = harness.SchemaModel("malformed", seed, ATTACKER).value(schema)
                ours, theirs = harness.conforms(v, schema), validator.is_valid(v)
                self.assertEqual(ours, theirs, (name, seed, v))
                agree += 1
                bad += not theirs
        self.assertGreater(bad, agree // 4, "o modo malformado quase não gerou valores inválidos")


if __name__ == "__main__":
    unittest.main()
