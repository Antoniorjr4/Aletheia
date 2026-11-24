# Proposta: variantes homônimas em tipos diferentes

**Status:** proposta para decisão. Nenhum código. (Ciclo 1, 24/11/2026.)

## O caso

Na reanálise do M10, 4 dos 72 erros restantes são E0002 ("variante definida mais de uma vez"), em 2
programas (refunds rep3, support_email rep1). Nos dois o modelo declarou vários tipos de slot, cada um
com uma variante `Empty`:

```
type RefundSlot = | Empty | Refund { order: Text[..80], amount: Int[1..100000000] }
type CreditSlot = | Empty | Credit { customer: Text[..80], amount: Int[1..100000000] }
```

## Causa raiz

Não é o nome de variante ser global; é o idioma de slots fixos. A Aletheia não soma sobre uma lista
(D8), então o guia ensina a declarar um número fixo de slots opcionais com uma variante `Empty` e
somar slot a slot (`bench/m10/guides/aletheia.md`, linhas 78 a 90). Com mais de um tipo de operação,
o modelo repete o idioma por tipo e repete `Empty`. Os erros são sintoma de D8.

## Onde o nome de variante é global hoje

| camada | ponto | efeito de permitir homônimos |
| --- | --- | --- |
| verificador | `variants: HashMap<String, (TypeDecl, Variant)>` (check.rs) | construtor `Empty` sem tipo esperado fica ambíguo |
| ARX | `self.variants.get(nome)` na construção e na decodificação (lib.rs 555, 582) | escolheria o tipo errado |
| ARX e Python | igualdade de valores compara só a variante, não o tipo (value.rs 48; values.py 77) | `RefundSlot.Empty == CreditSlot.Empty` seria verdadeiro |
| Python | `to_json` codifica `{variante: campos}` sem o tipo (values.py 124) | perde a distinção na serialização |
| Lean | não verificado se a formalização assume variantes globais | precisa de checagem antes de qualquer mudança |

Permitir homônimos mexe em cinco lugares, dois deles semânticos (igualdade e decodificação) nos dois
runtimes. A igualdade só por variante hoje é correta porque os nomes são únicos; com homônimos,
deixaria de ser.

## Opções

**A. Manter nomes globais; diagnóstico que diz a correção.** E0002 passa a dizer de qual tipo é a
primeira `Empty` e a sugerir um nome distinto (`NoRefund`, `NoCredit`). Custo: só a mensagem.
Risco: nenhum. Não resolve a ergonomia, mas transforma um ciclo perdido em correção mecânica.

**B. Construtor qualificado quando ambíguo: `RefundSlot.Empty`.** Padrões resolvem pelo tipo do
`match`; construtores sem tipo esperado exigem qualificação. Exige: chave (tipo, variante) no
verificador e no ARX, igualdade por (tipo, variante) nos dois runtimes, tipo na serialização do
Python, revisão da formalização. Custo médio, risco médio, semântica explícita.

**C. Inferência pelo tipo esperado, sem qualificação.** Como B, mais tipagem bidirecional nos
construtores. Mais conveniente, mais regras implícitas, mais casos de erro difíceis de explicar.

**D. Atacar a causa: D8.** Com soma sobre lista (`sum`/`fold` delimitado) na política, o idioma de
slots some: o plano vira `List[Op, 0..3]` com um tipo `Op` e nenhuma `Empty`. Isso é P3 e é a
mudança que remove a classe de erro, não só esta instância.

## Recomendação

A agora (mensagem, dentro do ciclo); D dentro de P3, onde já está previsto; B só depois do ciclo e só se,
com D feito, homônimos continuarem aparecendo em dados novos. C não.

Não altero a resolução de nomes sem a sua decisão.
