# Rev12 — Convergência semântica

Branch `rev12`, a partir de `rev11` (c8e4ceb). Gasto Groq: US$ 0,00 (nenhuma chamada paga, nenhum
workflow pago, agendamento pausado intocado). Os experimentos oficiais, os journals e a matriz
adversarial da Rev11 não mudam.

## Reproduzir

```
cd lean && lake build --wfail                     # provas + auditoria de axiomas (71 guardas) + ath-lean-eval
ATH_REQUIRE_LEAN=1 ATH_TRIPOD_N=400 cargo test -p athc --test differential   # quatro backends
python3 bench/adversarial/harness.py --contracts --athc target/release/athc  # M4
```

## 1. O que está provado (Lean 4, sem `sorry`, axiomas fixados em `Axioms.lean`)

**M1 — autoridade e identidade independentes.** `call t c κ a n h` separa a capacidade (`c`) da
chave (`κ`); `mint e` cunha uma chave fresca pelo contador `St.next` e a aposenta (`St.keys`) na
saída. A guarda de `call` recusa uma chave usada, aposentada ou igual à capacidade.

- T1 (`sound`, `soundness_total`): reconstruído com o invariante de frescor `KF` (identidades em
  uso abaixo do contador); `Start` exige ambiente sem chaves.
- T2 (`effects_lexical_total`, `caps_exactly_once`): enunciados inalterados.
- T3 e T3-saga (`noninterference`, `noninterference_saga`): o contador de cunhagem é estado do
  host, igual nas duas execuções (`next_preserves`, `eval_next`, `next_agree`).
- T9 (`T9_outcome`, `T9_honest`): `KeysOk` sobre identidades gastas (`used ∪ keys`); o provedor
  responde à operação `(t, k)`; `Lifecycle.pending_unique`.
- Exemplos por `rfl`: chave = capacidade recusada; chave reusada recusada; chave aposentada recusada.

**M2 — o ARX como refinamento do λ_Ath** (`Arx.lean`). Um modelo do interpretador ARX com a
estrutura do Rust (journal global só de acréscimos, pilha de quadros de saga, `Unwind`, evento
`tool` que funde ação e `Done`, laço de reconciliação com `break`, compensador liquidado no medidor)
e a vista do núcleo (`core_view_with`, que reconstrói compensações, sonda e chave recusada).

- `sim`: simulação para a frente. Todo `eval` que termina em valor ou falha capturável é realizado
  pelo modelo, com o mesmo valor ou causa, o mesmo mundo, oráculo e contador, **gasto do ARX ≤
  gasto do λ_Ath**, e vista do núcleo = traço módulo `nv` (o que o journal do ARX não registra).
- `arx_sound`: **T1 no ARX**. Bem tipado, linear, a partir de `Start` ⇒ o ARX termina em valor ou
  falha capturável, dentro do orçamento estático, com a vista igual ao traço do λ_Ath módulo `nv`.
  O que T2–T9 dizem do traço vale para o que o ARX registra.
- `recA_eq` (o `for … break` do ARX é a recursão de `reconcile`), `compA_sim` (compensar o quadro
  do topo é compensar o registro).

**M3 — T6 com chaves, sem `keyless`** (`Saga.lean`). O registro passa a ser o que foi *emitido*
(`opsOf`: `act` e `pending`), cada entrada com a verdade oculta da aplicação.

- `T6_keys`: compensações = operações emitidas, nenhuma recusada, mundo `E`-restaurado, aplicada ou
  não cada operação em aberto — sob `TotalUndo` só para as ferramentas em aberto no traço.
  `opsOf_keyless`: contém o T6 da Rev11.

**M4 — por que o contrato fica no efeito** (`Contracts.lean`). `endorsed_amplified` é bem tipado e
linear; a política aceita 64, a ação recebe 64064. `policyOk_but_not_contract`: o `PolicyOk` vale e
o argumento do efeito está fora da política. O rótulo `Endorsed` é fechado sob computação confiável;
o predicado da política não é. Essa é a raiz comum de R1, R2 e RS1.

## 2. O que está demonstrado por execução

| evidência | resultado |
|---|---|
| tripé com chaves, bytes crus Lean = ARX | 600/600 (Rev11: só módulo renomeação) |
| quarta perna: modelo Lean do ARX = binário do ARX | 400 + 600 + 400 programas e os 7 cenários, byte a byte |
| mutantes do protocolo de chaves | todos recusados; a troca entre eventos isolados agora é detectada (o `mint` ancora a identidade) |
| contratos, journal completo ARX = Python | `contracts_monitor` |
| matriz `K` (SafePlan + contratos) | 15/15 ataques recusados antes do efeito (R1 e R2 pelo contrato), 4/4 controles, 1 custo (U1) |
| matriz `KP` (política que aceita tudo, RS1) | 15/15 ataques recusados, 0 efeitos proibidos, 3/3 controles, 2 custos (U1, N3): só os contratos decidem |

A cadeia M2 fica: binário do ARX = modelo (teste em todo programa gerado) e modelo ⊑ λ_Ath (prova).

## 3. O que ainda é condicional

Cada hipótese tem contraexemplo por `rfl`:

- **Compensações gratuitas** (`comp_cost_breaks_budget`): o ARX liquida o compensador sem reserva;
  com custo 1, gasta 2 com limite 1. A garantia de orçamento passa ao ARX só se as compensações forem
  gratuitas ou reservadas antes.
- **Igualdade byte a byte, e não só módulo `nv`**: exige tratador puro (`impure_handler_reorders`:
  o ARX fecha `Transferred` no `transfer`, o λ_Ath depois do tratador) e `await` dentro de saga
  (`top_level_await_differs`). As duas condições são as de E1107 e E0601.
- **`kdOk`**: ferramentas com chave só por `call`.
- **T6 com chaves exige compensador total** (`blind_compensator_corrupts`: estornar um pagamento que
  não existiu dá −5 em vez de 0; `bound_compensator_restores`: o compensador vinculado à operação
  restaura nos dois mundos). O ARX já assume isso num comentário; agora é obrigação explícita.
- **Contratos limitam o dano, não o eliminam**: R3 (valor indevido abaixo do limite declarado)
  executa. Custos medidos: U1 (endereço legítimo lido de arquivo não confiável é recusado) e, sem
  política, KP-N3 (o `else` do `refine` não roda porque a recusa vem no efeito).
- O modelo do ARX é uma transcrição do Rust; a ligação com o binário é empírica (o tripé), não
  provada.

## 4. Problemas abertos

1. **Descarga estática de contratos**: provar no compilador que a política que endossa um valor
   implica o `requires` da ferramenta que o recebe (o monitor nunca dispara). `Contracts.lean` mostra
   por que não basta o rótulo: o predicado não atravessa a computação confiável; é preciso
   rastrear refinamentos `{x | φ}` pelo fluxo de dados até o efeito.
2. **WAL e recuperação após queda**: registrar a intenção `{chave, ferramenta, argumentos}` antes do
   host e provar que todo prefixo de journal (ponto de queda) contém, para todo efeito no mundo, uma
   obrigação reconciliável pela chave. Hoje o `mint` precede o efeito, mas não carrega a ferramenta.
3. **Registro com evidência**: tirar do registro a operação que a reconciliação confirmou não
   aplicada (`R0611`), para que `TotalUndo` seja exigido só das transferidas.
4. **Orçamento das compensações**: reservar o custo dos compensadores no início da saga e estender
   `arx_sound` a compensadores pagos.
5. **Simulação reversa**: `sim` é para a frente; com T1 e o determinismo ela cobre os programas
   bem tipados. Para programas que o λ_Ath recusa (`reuse`, `budget`), o ARX pode seguir outro caminho
   (a saga do ARX captura erros de programa); não há afirmação sobre eles.
