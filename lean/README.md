# Aletheia em Lean 4: o núcleo λ_Ath mecanizado

Esta pasta é a primeira prova mecanizada do Aletheia. Ela formaliza o núcleo λ_Ath, isto é,
o subconjunto que o `athc` verifica e o ARX executa, e demonstra dois teoremas.

- **T1**: preservação e progresso, incluindo capacidades lineares e orçamento graduado.
- **T3**: não-interferência com endosso robusto.

Não há `sorry`. Cada teorema principal tem o conjunto de axiomas fixado em
[`Aletheia/Axioms.lean`](Aletheia/Axioms.lean), que só aceita os três axiomas padrão do Lean
(`propext`, `Quot.sound`, `Classical.choice`). O CI roda `lake build --wfail`, que reprova
qualquer aviso, inclusive `declaration uses 'sorry'`.

```bash
cd lean
lake build --wfail          # toolchain fixado em lean-toolchain (Lean 4.20.0)
```

## O que está modelado

| arquivo | conteúdo | no Marco Zero |
|---|---|---|
| `Lattice.lean` | `Untrusted ⊑ Endorsed ⊑ Trusted`, encontro `⊓`, `low ℓ` (ℓ ⊒ Endorsed) | `ath_syntax::ast::Label`, E3 |
| `Budget.lean` | β sobre seis dimensões (tok, usd, wall, energy, bytes, risk), `+`, `⊔`, `n • β` | E6, contrato Δ |
| `Syntax.lean` | tipos `Int`/`Bool`/`Unit`/`List[T, 0..n]`, valores, capacidades, eventos, assinatura Σ (custos, tipos de argumento, políticas), oráculo | AST, Codex |
| `Typing.lean` | Γ; pc ⊢ e : τ ^ ι ▷ β, contagem de usos `uses`, `Linear` | E1, E3, E5 (políticas), E6 |
| `Eval.lean` | avaliador total de passo grande com quatro erros verificados em tempo de execução | ARX |
| `Soundness.lean` | T1 | |
| `Noninterference.lean` | T3 | |
| `Examples.lean` | o padrão de remediação: tipado, executado e com as variantes perigosas recusadas | `examples/remediate.ath` |

As expressões são estas, com índices de de Bruijn:

```
e ::= v | x | let x = e in e | if e then e else e | e ⊕ e
    | refine e by p else e        -- endosso pela política auditada p
    | act t k e                   -- ferramenta t com a capacidade no índice k
    | for (n) x in e { e }        -- laço sobre List[T, 0..n]
    | infer m -> T { e }          -- saída do modelo: sempre Untrusted
```

**Capacidades** só existem no ambiente inicial (as capacidades do agente). Elas não têm tipo
de valor: não podem ser copiadas por `let`, guardadas numa lista nem passadas como
argumento, e só servem como a autoridade de uma `act`. O estrato E1 é `uses e k ≤ 1` para
cada capacidade, com o laço multiplicando pelo teto: `uses (for n xs b) k = uses xs k + n ·
uses b (k+1)`.

**O avaliador não confia na tipagem.** Ele verifica tudo o que o ARX verifica e falha com um
erro explícito:

- `stuck`: operação sobre um valor de forma errada;
- `noCap`: efeito sem capacidade;
- `reuse`: capacidade linear já consumida;
- `budget`: o efeito estouraria o teto. A verificação vem antes do efeito, como a reserva
  do ARX.

**O oráculo** `O : ℕ → ℕ → Ty → Val → Val` é arbitrário, logo adversarial. A única hipótese
sobre ele é que devolve valores do tipo pedido, que é o que a validação de esquema do ARX
garante (decodificação restrita e análise; recusa → `refine`).

## T1: preservação e progresso

```lean
theorem soundness (hO : c.oracle.Ok) (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o, eval S c e ρ s = .ok o ∧ HasTy o.v τ ∧ Within s.spent o.st.spent β ∧
      Footprint ρ (uses e) s.used o.st.used

theorem progress     … : ∀ err, eval S c e ρ s ≠ .error err
theorem preservation … (he : eval S c e ρ s = .ok o) :
    HasTy o.v τ ∧ Within s.spent o.st.spent β ∧ o.st.spent ≤ c.limit
theorem eval_nodup   : eval S c e ρ s = .ok o → s.used.Nodup → o.st.used.Nodup
```

`Start Γ ρ s` diz que o ambiente realiza Γ, que as capacidades têm identidades distintas e
que nenhuma foi consumida. `Within s s' β` diz que o gasto real fica dentro de β **em cada
dimensão**. Daí as três garantias do julgamento central:

- um programa bem tipado **não executa efeito sem capacidade**;
- **não reusa capacidade linear**: o avaliador recusaria o reuso, e o progresso diz que ele
  nunca recusa; `eval_nodup` fecha o consumo linear;
- **não excede orçamento**: o laço gasta no máximo `n • β(corpo)`, como na regra E6.

## T3: não-interferência com endosso robusto

```lean
theorem noninterference (h : HasType S Γ pc e τ ι β) (hl : LowEq Γ ρ₁ ρ₂)
    (h₁ : eval S c₁ e ρ₁ s₁ = .ok o₁) (h₂ : eval S c₂ e ρ₂ s₂ = .ok o₂) :
    (o₁.tr = o₂.tr ∧ (low ι → o₁.v = o₂.v)) ∨ Diverge o₁.tr o₂.tr
```

`LowEq Γ ρ₁ ρ₂` deixa as duas execuções diferirem arbitrariamente em toda entrada
`Untrusted`. Os dois oráculos e os dois tetos (`c₁`, `c₂`) também são arbitrários e
independentes. `Diverge` diz que os traços coincidem até uma **decisão de endosso** e
divergem nela; não há terceira possibilidade. Consequências:

```lean
-- mesmas decisões de endosso ⇒ mesmas ações, mesmas capacidades, mesmos argumentos
theorem robust_endorsement … (hend : endorsements o₁.tr = endorsements o₂.tr) :
    o₁.tr = o₂.tr ∧ actions o₁.tr = actions o₂.tr ∧ (low ι → o₁.v = o₂.v)

-- todo valor aceito por `refine … by p` satisfaz p, em qualquer execução
theorem endorsed_sound : eval S c e ρ s = .ok o → PolicyOk S o.tr

-- sob pc Untrusted não há ação nem endosso
theorem confine (h : HasType S Γ pc e τ ι β) : ¬ low pc → eval S c e ρ s = .ok o → o.tr = []

-- com o T1, as duas execuções sempre terminam: o T3 vale sem hipótese sobre o resultado
theorem noninterference_total … : ∃ o₁ o₂, eval … = .ok o₁ ∧ eval … = .ok o₂ ∧ NI ι o₁ o₂
```

A robustez está na regra de tipagem do endosso: `refine` exige `pc ⊒ Endorsed`. A própria
decisão de endossar, portanto, não está sob controle de dado não confiável. Juntando tudo:
**entradas `Untrusted` não desviam o fluxo de execução para ações fora de políticas
comprovadas**. O único canal da entrada não confiável até uma ação é um endosso, e todo
endosso aceito satisfaz a sua política auditada.

## Exemplos verificados (`Examples.lean`)

- `remediate_typed`, `remediate_linear` e `remediate_runs` cobrem o caminho
  `infer → refine by SafeScale else 3 → scale`: o programa é bem tipado, linear e cabe no
  orçamento, e o T1 garante a execução para **qualquer** resposta do modelo.
- `injected_rejected`: usar a resposta do modelo direto como argumento da ação não tipa.
- `steered_rejected`: escolher a ação por um teste sobre a resposta do modelo não tipa.
- `twice_not_linear` e `looped_not_linear` mostram que consumir uma capacidade duas vezes,
  inclusive num laço com teto 2, não passa no E1. Com teto 1, o laço passa.
- Duas execuções concretas, por `rfl`, reproduzem a injeção que a Groq recebeu em
  `bench/replay/`:
  - com o modelo respondendo 0, o traço é `[endorse none, act scale 3]`;
  - com o modelo respondendo 5, o traço é `[endorse (some 5), act scale 5]`.

  Os dois divergem exatamente na decisão de endosso.

## Fora deste núcleo, por ora

A prova é do cálculo, não do código Rust ou Python. A correspondência com o ARX vem da
construção (os mesmos erros, a reserva antes do efeito, a mesma contagem do E1 e do E6) e
dos testes diferenciais, não de uma prova de refinamento. Ainda não estão mecanizados:

- registros, variantes e `match`;
- `Text` e refinamentos `{x:σ | φ}` além das políticas decidíveis;
- `retry`, sagas e compensação (o T2);
- timeouts e `reconcile`;
- graus de certeza `σ ~ p`;
- localizações ℓ;
- os teoremas T4 e T5.
