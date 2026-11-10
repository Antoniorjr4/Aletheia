# Aletheia Rev6 — plano executivo e critérios de aceite

Base imutável: `Antoniorjr4/Aletheia@eb59d0e784abbad5a79186f0d11f505e84417241`.
Área exclusiva de desenvolvimento: `Antoniorjr4/Aletheia-Lab:rev6-dev`.
Nenhuma operação de escrita será feita no repositório original.

## 0. Estado verificado da Rev5

- `athc`/Rust: 7 crates; E0/E1/E2/E3/E6 implementados parcialmente. A AST já inclui
  `atomic`, `reconcile`, `transfer`, `CompensationFailed` e ferramentas com classe de efeito.
- `lean/`: T1 e T3 comprovados para núcleo abstrato `λ_Ath`, não para o programa Rust completo.
- CI principal em `Aletheia-Lab@eb59d0e`: fmt, clippy, testes Rust e Python,
  replay diferencial de 15 cassetes reais da Groq, `lake build --wfail` e M1 passaram.
- Não há SMT real em E5, `certify`, `Bounded`, monitor de deriva ou AgentDojo oficial.
- A saga Rust executa compensações, mas não descarrega formalmente contratos restoring
  sobre o mundo abstrato nem reconstitui pendências do journal após crash.
- `Bind.cap` no Lean atual não contém escopo de efeitos: a prova T2 de confinamento
  de **classes específicas** de efeitos depende de enriquecimento do cálculo.

## 1. T6 — saga e recuperação (PR #1: fase inicial implementada)

A spec v0.2 §3.4 requer:
`{P} A {Q}`, `{P ∨ Q} C {R}`, conjunção dos `R_i` implicando `Inv`.
O modelo não exige reconstruir o mesmo mundo anterior; exige restaurar o invariante.

- [x] `lean/Aletheia/Recovery.lean`: contrato de compensação total sobre
  `P ∨ Q`, reconciliação finita, confirmação não presumida, teste de invariante
  e `CompensationFailed` abstrato.
- [ ] `lean/Aletheia/Saga.lean`: semântica operacional para `atomic recover`;
  mundo abstrato `W`, `footprint`, `restoring`, composição reversa e
  `CompensationFailed(residual)` sem descarte de obrigações.
- [ ] T6 completo: sob hipóteses explícitas de contratos de ferramentas e sondas
  fidedignas, toda conclusão `Recovered` satisfaz `Inv`; uma compensação sem
  prova válida sinaliza `CompensationFailed` e preserva o residual.
- [ ] T9 (§3.5): modelar `Pending[k]` linear, `Exhausted(p)`, `transfer(p)`
  durável e reidratação após crash. **Importante:** a função `resolve` atual
  abstrai escalonamento automático no esgotamento; ainda não prova persistência
  nem passagem de posse do token real.
- [ ] `crates/arx/src/journal.rs` e `crates/arx/src/lib.rs`:
  recovery scan idempotente, estado pendente reidratável, handoff durável.

Aceite: `lake build --wfail`; exemplos de compensação positiva/negativa, timeout,
crash/restart sem perda de Pending; teste de propriedade para efeito compensável
em ambos os mundos possíveis.

## 2. T2 — efeitos confinados à autoridade

- [ ] `lean/Aletheia/Syntax.lean`: `Effect`/conjuntos finitos de efeitos;
  `Bind.cap E`; contratos de ferramentas com escopo.
- [ ] `lean/Aletheia/Typing.lean`: exigir `tool.effect ⊆ cap.effects` na
  regra `act`, preservando linearidade e integridade.
- [ ] `lean/Aletheia/Eval.lean`/`Confinement.lean`: trace safety:
  para todo evento de ferramenta, existe autorização não forjada,
  compatível com a classe de efeito, não consumida antes.
- [ ] `crates/ath-check/src/check.rs`/`linear.rs`: cotejar a prova
  abstrata e comportamento Rust, casos negativos de capacidades atenuadas.

Aceite: ferramenta de efeito E fora do escopo `Cap[E]` rejeitada tanto pelo
checker quanto pela interpretação de referência.

## 3. E5 — SMT real (QF_LIA primeiro)

- [ ] Criar `crates/ath-check/src/smt.rs` e interfaces para Z3/cvc5;
  tradução canônica de `{x: τ | φ}`, intervalos, limites, cláusulas de mundo
  `P/Q/R` e budgets inteiros para SMT-LIB 2.
- [ ] Somente resultados `unsat` das obrigações de contraexemplo passam;
  `sat`, `unknown`, timeout ou solver ausente = não comprovado (fail closed).
- [ ] Cacher obrigações por hash BLAKE3 no Codex com versão do solver e contrato Δ.
- [ ] `athc check --smt=z3|cvc5 --explain` com diagnósticos de contraexemplo.

Aceite: testes com ambos os solvers e mutações dos contratos; nem um timeout
pode virar aprovação.

## 4. Terceiro backend diferencial Lean

- [ ] Elaborar subconjunto comum da AST `ath-syntax` para `λ_Ath`.
- [ ] Compilar um executável de `lean/Aletheia/Eval.lean` com entrada estruturada,
  oráculo de replay e saída de eventos canônicos.
- [ ] `crates/athc/tests/differential.rs`: matriz ARX × Python × Lean para
  programas do núcleo; mesmo valor, mesma ordem de eventos, mesmo consumo;
  journal BLAKE3 somente após normalização idêntica e especificada.
- [ ] Sagas entram na comparação **após** sua semântica estar no núcleo Lean;
  não fabricar equivalência com recursos ainda não modelados.

Aceite: todas as diferenças têm contraexemplos mínimos; nenhum caso é
marcado como coberto sem compilação real no terceiro backend.

## 5. Certificados e monitor de deriva (§2.3)

- [ ] `crates/ath-syntax/src/ast.rs`: `Dist<T>`,
  `Cert[φ,α,δ,D,h]`, `Bounded(ρ,ε,x₀)`,
  `ExprKind::Certify`; nenhuma autorização de efeito embutida em `Cert`.
- [ ] `crates/ath-check`: hash do gerador `h` obrigatório;
  fontes `Trusted` ou `Bounded` com certificado robusto; caso contrário E0421;
  alteração do gerador invalida certificado E0430.
- [ ] `crates/ath-grammar`: serialização/codificação restrita dos novos nós;
  `ath-emit`: representação fiel dos mesmos contratos.
- [ ] `crates/arx/src/cert.rs`: estados
  `Valid → Suspect → AuditDue → Revoked`, duas e-martingales
  separadas (escores não rotulados e perdas auditadas); trilha de auditoria.
- [ ] Regras de risco: Σα por uso, Σδ por certificado **distinto**,
  com a hipótese de permutabilidade registrada. Sob deriva não detectada,
  não afirmar garantia incondicional.

Aceite: `certify` falha fechado com fonte Untrusted, certificado revogado
ou gerador trocado; ensaio de alarme de Ville e limite entre auditorias.

## 6. AgentDojo oficial — benchmark real, não cenários semelhantes

Fonte: https://github.com/ethz-spylab/agentdojo

- [ ] `bench/agentdojo/`: instalação reproduzível e versão fixada
  (partir de `v1.2.2`, verificar disponibilidade), adaptador de ferramentas
  sob políticas Aletheia, armazenamento de cassetes e manifesto.
- [ ] Suites: `workspace`, `slack`, `banking`, `travel`.
- [ ] Comparação justa com pipeline Python puro: mesma tarefa, modelo,
  ferramenta, orçamento, payload e semente quando aplicável.
- [ ] Relatar `attack success rate`, `legitimate utility`, violações de
  política, custo/latência, recusas legítimas, falhas de integração e
  métricas por suite; separar pares que pedem ferramentas já autorizadas.
- [ ] Executar primeiro canário offline/fixture; liberar chamadas remotas
  somente com segredo de provedor explicitamente configurado.

Aceite: harness nativo, `run manifest` fixando versões e parâmetros, resultados
reproduzíveis; jamais chamar M1 sintético de resultado oficial AgentDojo.

## 7. Ordem de ataque

R6.0: T6 kernel e CI (iniciado) → R6.1: T2 effect scopes →
R6.2: contrato de mundo e T6 pleno → R6.3: SMT E5 →
R6.4: backend Lean diferencial → R6.5: certificados/deriva →
R6.6: AgentDojo completo.

Toda revisão exige CI verde, sem `sorry`, nenhum retrocesso nos 15 cassetes,
regressões negativas de segurança e evidências separadas para provas vs medições.
