# Aletheia · Marco Zero

Primeira implementação executável de Aletheia: o subconjunto **Aletheia Core 0**,
com o compilador `athc` e o runtime `ARX` em Rust. O Marco Zero existe para
provar uma coisa: que integridade, efeitos, capacidades, linearidade e orçamento
podem ser verificados **juntos, antes de rodar**, num programa de agente real.

```text
$ athc check examples/remediate.ath
✓ E0 estrutura       7 definições, todas resolvidas
✓ E1 multiplicidade  1 vinculação(ões) IdemKey consumidas exatamente uma vez
✓ E2 efeitos         {Cluster.Read, Cluster.Write, Infer, Page} cobertos por capacidades e linhas de efeito
✓ E3 integridade     1 exigência(s) de integridade satisfeitas, inclusive sob pc
✓ E6 orçamento       Remediator.alert: pior caminho 7.290 tok · US$ 0,0327 · 32,5 s
                     ≤ [20.000 tok · US$ 0,50 · 60,0 s]
                     caminho: logs 2,0 → infer planner ×3 28,5 → apply 1,0 → page 1,0
✓ Codex              7 objetos BLAKE3, raiz #00aa72f552eb
```

## Começar

Requer Rust ≥ 1.87.

```sh
cargo build --release
cargo test                                    # 16 testes, incluindo os ponta a ponta

# verificar
./target/release/athc check examples/remediate.ath
./target/release/athc check examples/errors/e0412_injection.ath     # rejeitado: E0412

# executar no ARX (mundo simulado e modelo roteirizado em examples/fixtures)
./target/release/athc run examples/remediate.ath --arg '"api"'

# Codex: formas canônicas endereçadas por BLAKE3
./target/release/athc hash examples/remediate.ath

# auditar a cadeia de custódia gravada pelo run
./target/release/athc verify-journal .arx/journal.jsonl
```

O cenário de `examples/fixtures` é um ataque: o log da `api` contém uma linha com
injeção de prompt pedindo para escalar `payments` a zero réplicas. O modelo
roteirizado "cai" na primeira tentativa; a política rejeita; a segunda proposta é
endossada e aplicada; `payments` continua intacto.

```text
▶ Remediator.alert("api")
      t      +Δt
    0,0     +1,4  logs(⟨cap⟩, "api")  5 linhas de texto
    1,4     +1,7  infer planner · tentativa 1  → Scale { deploy: "payments", to: 0 }
                  ✗ rejeitada por SafeRemediation
    3,1     +1,6  infer planner · tentativa 2  → Restart { deploy: "api" }
                  ✓ Endorsed(SafeRemediation)
    4,7     +0,6  apply(⟨cap⟩, Restart { deploy: "api" }, k1)  "restart api"
    5,3     +0,2  commit page(⟨cap⟩, "remediado: restart api", k2)  ok
◀ resultado: "resolvido"
```

## Estrutura

```text
crates/
  ath-syntax/   lexer, AST com spans, parser descendente recursivo
  ath-check/    verificador estratificado E0 · E1 · E2 · E3 · E6, diagnósticos
  ath-codex/    forma canônica, DAG de Merkle, BLAKE3, alfa-normalização
  arx/          interpretador, medidor por reserva, journal encadeado, host e modelo
  athc/         CLI, renderização de diagnósticos, testes ponta a ponta
examples/
  remediate.ath           agente de remediação (aceito)
  errors/*.ath            9 variantes, cada uma com uma única edição que a torna inválida
  fixtures/               mundo simulado (com a injeção) e roteiro do modelo
```

## O que o Marco Zero implementa

| Spec v0.2 | Marco Zero |
| --- | --- |
| 1.3 Codex | forma canônica por definição; referências por hash; nomes de definições e de locais não entram no hash |
| 2.5 Modelos e inferência | `model` como valor com contrato Δ; `infer` produz um rascunho lazy; saída decodificada no tipo declarado |
| 2.3 `refine … by policy retry n` | políticas puras; só o que a política aceita ganha `Endorsed`; saída fora do tipo é descartada |
| 2.6 Capacidades e integridade | capacidades só vêm dos parâmetros do agente, com atenuação; reticulado Untrusted ⊑ Endorsed ⊑ Trusted; `pc` para fluxos implícitos; `quote` obrigatório para dado Untrusted no prompt |
| 2.7 Orçamento | E6 estático no pior caminho, com caminhos que retornam; reserva antes, liquidação depois no ARX; `ContractBreach` quando a latência observada excede a declarada |
| 2.10 Estratos | E0, E1, E2, E3 e E6, relatando só o primeiro estrato que falha |
| 3.4 Classes de efeito | `reversible`, `compensable`, `irrevocable`; irrevogável só com `commit` |
| 3.5 Idempotência | `IdemKey` linear verificada em E1; o host não repete efeito com chave já aplicada |
| 3.6 Execução durável | journal append-only encadeado por BLAKE3; `verify-journal` detecta adulteração |

### Erros que o compilador já pega

| Código | Estrato | Situação |
| --- | --- | --- |
| E0412 | E3 | valor Untrusted, ou ramo decidido por Untrusted, chega a um parâmetro `^ Endorsed` |
| E0415 | E3 | dado Untrusted entra no prompt sem `quote(…)` |
| E1104 / E1105 | E1 | `IdemKey` consumida mais de uma vez, ou não consumida em algum caminho |
| E0501 | E2 | efeito usado e não declarado na linha do handler |
| E0502 | E2 | capacidade insuficiente para a ferramenta ou para `mint()` |
| E0500 | E2 | ferramenta declara efeito sem parâmetro `Cap[…]` que o cubra |
| E0420 | E2 | política impura |
| T0301 / T0303 | E2 | irrevogável sem `commit`, ou `commit` em efeito revogável |
| B0110 | E6 | `Text` sem limite de tokens no prompt (use `fit(x, N)`) |
| B0201 / B0202 / B0207 | E6 | tokens, dólares ou tempo do pior caminho acima do orçamento |
| B0301 | E6 | prompt + saída máxima acima da janela do modelo |

## O que ainda não existe

Dito sem rodeio, para orientar os próximos marcos:

- **Backend:** interpretador de árvore. Não há MLIR, LLVM nem emissão para Python.
- **Estratos E4 e E5:** sem formas de tensor e sem refinamentos via SMT; `Text[≤ n]` é aproximado por `fit`.
- **Tokenizer:** estimativa de 4 caracteres por token, usada igualmente no compilador e no runtime.
- **Modelo e mundo:** provedor roteirizado e cluster simulado. O trait `ModelProvider` é o ponto de entrada do provedor HTTP real.
- **Sagas:** sem `atomic`/`recover`, compensações, `TimeoutUnknown`, `Pending` nem reconciliação.
- **Certificados:** sem `certify`, `Bounded` nem monitor de deriva.
- **Capacidades** não são lineares nem revogáveis; **tipos recursivos** não são suportados no Codex.
- **Teoremas:** nenhum está mecanizado. As garantias acima são testadas, não provadas.

## Próximos marcos

1. **Núcleo formal em Lean 4**, em paralelo: λ_Ath restrito ao que o Marco Zero implementa, com T1 (segurança de tipos) e T2 (confinamento de efeitos); depois T3 (não-interferência com endosso robusto), que é o teorema que sustenta a tese.
2. **Provedor real + decodificação restrita derivada do tipo**: gramática do ADT de saída compilada para o formato de saída estruturada do provedor.
3. **Métrica M1**: portar cenários do AgentDojo e medir ações fora da política contra um baseline em Python.
4. **Sagas da seção 3.4–3.5**: `atomic … recover`, `CompensationFailed`, `TimeoutUnknown` com `Pending` linear.
5. **E5 com SMT** (refinamentos e cobertura de pós-condições) e cache de obrigações no Codex.
6. **Emissor para Python + asyncio**, o backend da fase F1 da spec.

## Licença

Apache-2.0.
