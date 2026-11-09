"""Cassete de replay (`arx::cassette`): programa, mundo, argumentos, configuração do provedor,
trocas HTTP cruas e o journal gravado."""

from __future__ import annotations

import json

from .providers import Exchange

FORMAT = "aletheia-cassette/1"


class Cassette:
    def __init__(self, provider: str, config: dict, manifest: dict, exchanges: list, journal: str):
        self.provider, self.config, self.manifest, self.exchanges, self.journal = (
            provider, config, manifest, exchanges, journal)

    @staticmethod
    def load(path: str) -> "Cassette":
        with open(path, encoding="utf-8") as f:
            j = json.load(f)
        if j.get("format") != FORMAT:
            raise ValueError(f"formato de cassete desconhecido (esperado `{FORMAT}`)")
        return Cassette(j.get("provider", ""), j["config"], j["manifest"],
                        [Exchange.from_json(x) for x in j["exchanges"]], j.get("journal", ""))

    def to_json(self) -> dict:
        return {"format": FORMAT, "provider": self.provider, "config": self.config, "manifest": self.manifest,
                "exchanges": [x.to_json() for x in self.exchanges], "journal": self.journal}

    def save(self, path: str) -> None:
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            json.dump(self.to_json(), f, ensure_ascii=False, indent=2)
            f.write("\n")


def first_divergence(a: str, b: str):
    if a == b:
        return None
    la, lb = a.splitlines(), b.splitlines()
    i = next((k for k, (x, y) in enumerate(zip(la, lb)) if x != y), min(len(la), len(lb)))
    return i, (la[i] if i < len(la) else "<fim>"), (lb[i] if i < len(lb) else "<fim>")
