# Aletheia v1.0-RC4: autoridade temporal e ciclo de vida verificado do efeito

A RC3 (`docs/RC3.md`) ancorou o consumo das concessões e deixou três hipóteses abertas: o
relógio do executor decidia o prazo, a âncora não cobria os desfechos, e o escopo da autoridade
era texto. A RC4 troca as três por mecanismos da linguagem e do runtime, prova as propriedades
num modelo integrado em Lean e confronta o modelo com o executável.

```sh
scripts/rc4-smoke.sh              # a demonstração, num comando (exit 0 = todas as conferências)
cargo test -p athc --test rc4     # quinze testes com processos reais
cd lean && lake build --wfail     # Lifecycle.lean e as 135 guardas de axiomas
```

As RC1, RC2 e RC3 continuam passando sem alteração: `tests/rc1.rs` a `tests/rc3.rs`, os três
scripts de demonstração e os programas de `examples/rc1` a `examples/rc3`. A raiz do Codex da
tesouraria da RC2 continua `#4877f356ba52`.

## 1. Os contraexemplos, reproduzidos antes de corrigir

O commit `RC4: quatro contraexemplos da RC3 como testes que falham` só acrescenta testes (e o
registro de toda consulta de `status` no `ath-world`, sem o qual o segundo não era observável).
Os quatro **falham** nele; a saída está em `docs/evidence/rc4-contraexemplos-na-rc3.txt`.

| Contraexemplo | Na RC3 | Fechado por | Teste |
|---|---|---|---|
| **Relógio local** | concessões expiradas há 1 h 50 min eram aceitas com o relógio do processo atrasado 2 h | o tempo vem da testemunha declarada; o relógio de parede do executor não é lido | `a_rewound_local_clock_does_not_revalidate_an_expired_grant` |
| **Desfecho apagado** | apagar o `close` do WAL passava sem aviso, e a recuperação consultava o provedor de novo | o desfecho ancorado com o recibo; a recuperação o restaura sem consultar ninguém | `a_confirmed_outcome_does_not_vanish_with_the_wal` |
| **Registro forjado** | uma linha bem encadeada no fim do registro de consumo era ancorada | a âncora aceita por evidência (assinaturas, tempo, ligação com a operação), não por forma | `a_fabricated_record_is_never_anchored` |
| **Não utilização provada** | a queda entre o consumo e a âncora deixava a concessão consumida, sem efeito | `release`: reserva sem autorização de envio é liberada na âncora | `a_provably_unsent_operation_releases_its_grant` |

O primeiro também está formalizado: `rc3_local_clock_counterexample` mostra um estado alcançável
em que a concessão está expirada no instante real e válida no relógio local.

## 2. Modelo de confiança

| Papel | Confiado para | Não confiado |
|---|---|---|
| **Autoridade** (`authority`) | assinar concessões, fotografia e pedido para um programa, uma execução e um prazo | — |
| **Testemunha de tempo** (`witness`, usada como `clock`) | assinar o instante real, arredondado para cima, sobre o nonce do pedido | não decide nada além do instante: o executor confere atraso, piso e congelamento |
| **Testemunha de desfecho** (`witness`, usada como `outcomes`) | assinar recibos verdadeiros; um recibo de não aplicação é definitivo | — |
| **Âncora** (o registro unificado) | ser durável, só crescer e aplicar as regras de `accept` | conceder: ela confere evidência que não pode produzir |
| **Relógio monotônico do executor** | medir intervalos (o atraso de ida e volta, o tempo desde a última leitura) | — |
| **Relógio de parede do executor** | **nada** | o adversário o põe onde quiser |
| **Disco do executor** (WAL, registro de consumo, arquivos) | **nada** | o adversário o reescreve, restaura cópias antigas, forja registros |

As chaves das testemunhas são declaradas no programa e fazem parte do Codex. O escopo de um
registro na âncora é o hash da configuração de confiança (programa, execução, autoridades,
testemunhas, tolerância): quem abrisse o escopo com outras chaves abriria outro escopo, que
nenhum executor deste programa usa.

## 3. Missão I: tempo autenticado

**A fonte.** `witness notary { key = "ed25519:…", tolerance = 2 }`, referida pela autoridade
como `clock = notary`. Cada leitura (`arx::lifecycle::TimeKeeper::attest`):

1. o executor sorteia um sal e forma o nonce `BLAKE3(escopo, cabeça do registro, sal)`: a
   resposta só serve ao pedido e à posição do registro para os quais foi emitida;
2. a testemunha assina `nonce` e `t` (o instante, arredondado para cima);
3. o executor confere a assinatura e o eco do nonce (`T0001`), o atraso de ida e volta medido
   pelo relógio **monotônico** contra a tolerância declarada (`T0003`), o piso (o maior instante
   já ancorado ou já aceito; `T0002`) e a coerência com o relógio monotônico desde a leitura
   anterior (`T0004`, congelamento); sem resposta, `T0005`.

No momento da conferência, o instante real está em `[t − 1, t + tolerância]` (o teorema
`window_brackets` deriva isso das hipóteses da testemunha). Uma concessão vale se
`issued ≤ t − 1` e `t + tolerância ≤ expires`. A janela é conservadora: a garantia é sobre o
instante real, não sobre `t`.

**Onde é conferido.** No início da execução (antes de aceitar o pedido `Trusted` e as
concessões), em cada fronteira de efeito (duas leituras: uma para a reserva, outra
**imediatamente antes** da autorização de envio), numa conferência final sem rede antes de o
efeito sair (a janela da última leitura mais o relógio monotônico desde ela), antes de cada
reenvio na reconciliação dentro da execução e antes de cada reenvio na recuperação. A âncora
reconfere cada instante atestado (assinatura, nonce preso à posição, piso) ao aceitar `reserve`
e `send`.

**O que é recusado.** Prazo expirado ou ainda não começado no tempo atestado (`G0001` na carga,
`R0638` na fronteira), a fonte `Trusted` expirada na fronteira (`R0638`), e a execução inteira se
o tempo não pode ser demonstrado (`T000x`). Num programa temporal o executor zera o relógio de
parede: sem tempo atestado, nada é válido.

## 4. Missão II: o ciclo de vida ancorado

A âncora passa a ser o **registro unificado** das operações autorizadas por concessão. Cada
registro é aceito por `arx::lifecycle::accept`, as mesmas regras na âncora (`ath-world`) e no
executor, que relê e reaceita o registro inteiro do zero (`verify_chain`, `L0006`).

```text
Grant ─▶ Intent ─▶ reserve ─▶ send ─▶ (o efeito) ─▶ outcome
  assinada   WAL     âncora     âncora                  âncora, com recibo assinado
                       └──────▶ release                 provadamente não enviada
                                send ─▶ withdraw        o executor desiste do reenvio
```

| Registro | A âncora aceita se |
|---|---|
| `reserve` | a chave é nova no escopo; cada concessão é assinada por uma autoridade do escopo, para este programa e esta execução, no escopo `ops` e no `max_age` dela, válida no tempo atestado, e não está reservada (linear); o tempo é da testemunha, para esta posição do registro, não abaixo do piso; o hash dos argumentos confere |
| `send` | há reserva e não há `send` nem `release`; as concessões ainda valem no novo tempo atestado |
| `outcome` | há `send` e não há desfecho; o recibo é assinado pela testemunha de desfecho e é **desta** operação: chave de fio, ferramenta e hash dos argumentos |
| `release` | há reserva e **não** há `send` |
| `withdraw` | há `send` e não há desfecho |

**A ordem no executor** (`lc_boundary`): tempo atestado e concessões conferidas → intenção no WAL
→ `reserve` → nova leitura de tempo e reconferência → `send` → conferência final → efeito →
recibo → `outcome` → `close` no WAL citando o hash do desfecho ancorado. O efeito só sai depois do
`send` ancorado; logo a **ausência de `send` no registro confiável** prova que nada saiu. A
ausência de um registro **local** nunca é consultada.

**Recuperação** (`lc_recover`), antes de qualquer contato com o provedor: o registro é
reaceito (`L0006`), o tempo é atestado (`T000x`) e o WAL é confrontado com a âncora: um `close`
que cita um desfecho que a âncora não tem, ou que diz outra coisa, é recusado (`W0008`). Depois,
para cada operação ancorada:

| Estado ancorado | Decisão | Contato com o provedor |
|---|---|---|
| com desfecho | restaura o `close` dele (`anchor:outcome`) | nenhum |
| liberada | restaura a liberação | nenhum |
| reserva sem `send` | libera na âncora (`anchor:unsent`); a concessão volta a valer | nenhum |
| `send` sem desfecho, `idempotent` | reenvia **se** as concessões valem no tempo atestado agora; senão escala (`R0638`) | reenvio com a mesma chave |
| `send` sem desfecho, `queryable` | consulta; o recibo vira desfecho ancorado | consulta |
| `send` sem desfecho, `opaque` (ou retirada) | **escala**: o estado desconhecido é preservado | nenhum |
| intenção do WAL sem reserva | encerra (`anchor:unreserved`); nenhuma concessão foi consumida | nenhum |

As chaves são do escopo: uma nova execução na mesma `run` numera depois da maior chave ancorada,
e uma chave liberada nunca é reaproveitada.

## 5. Missão III: a autoridade como elemento tipado

```
witness notary { key = "ed25519:…", tolerance = 2 }
witness bank   { key = "ed25519:…" }

authority treasury {
  key      = "ed25519:…"
  ops      = [pay, schedule, wire]
  max_age  = 900
  ledger   = anchored
  clock    = notary
  outcomes = bank
}
```

* **AST**: `WitnessDecl`; `AuthorityDecl` com `op_idents` (cada operação com o seu lugar),
  `typed`, `clock` e `outcomes` como referências; `Guarantee` (identidade, textual, tipada
  incompleta, temporal).
* **Resolução** (`ath-check`): cada operação de `ops = [..]` contra os contratos
  `requires s.authorizes("op", …)` do módulo. `E0427` para operação inexistente (com a lista das
  concedíveis), para `ops = []` e para contrato que nenhuma autoridade cobre; `E0428` para
  operação repetida; `E0429` para testemunha não declarada ou testemunha de tempo sem
  `tolerance`. A forma textual e a tipada não se misturam numa autoridade (erro de sintaxe).
* **Codex**: forma canônica `(authority/typed key (guarantee …) (ops ordenadas) (max_age) (ledger)
  (clock #hash-da-testemunha) (outcomes #hash))`. A ordem das operações não muda a identidade;
  o conteúdo, sim. As formas da RC2 e da RC3 serializam como antes.
* **Nível de garantia explícito**: `athc check` imprime o nível de cada autoridade e o que lhe
  falta; `athc check --strict-authority` recusa (`E0430`) qualquer autoridade mais fraca que a
  temporal. O journal de cada execução registra o tempo atestado.

## 6. Missão IV: o modelo integrado (`lean/Aletheia/Lifecycle.lean`)

Um único sistema de transição com os passos do protocolo e dois passos do adversário
(`clockSet`, `walSet`), que podem ocorrer a qualquer momento. O invariante `Inv` (seis
cláusulas) é provado preservado por todos os passos (`inv_of_reach`).

| Teorema | Formulação | Hipóteses usadas | Contraexemplo que elimina |
|---|---|---|---|
| `window_brackets` | `t − 1 ≤ σ ≤ t`, `q ≤ σ ≤ c ≤ q + tol` ⇒ `t − 1 ≤ c ≤ t + tol` | H1 (testemunha honesta, nonce novo, relógio monotônico) | — |
| `valid_at_now` | válida numa janela que contém o instante real ⇒ `issued ≤ now ≤ expires` | — | — |
| `valid_implies_fresh` | a validade da RC4 implica `Anchor.fresh` no **instante real** | — | composição com a RC3 |
| `local_clock_irrelevant` | todo passo continua passo com qualquer valor do relógio local | — | relógio local |
| `no_expired_effect` | todo passo que faz um efeito sair o faz com cada concessão da chave válida no instante real | — | relógio local |
| `expired_never_revalidates` | expirada num estado ⇒ nenhuma continuação faz o efeito sair (nem reenvio) | — | revalidação pelo tempo |
| `sent_has_history` | efeito que saiu ⇒ reserva e `send` ancorados | H2 | — |
| `outcome_has_evidence` | desfecho ancorado ⇒ `send`, reserva, e diz o que o provedor fez | H2, H3 | desfecho fabricado |
| `no_rollback_reauthorizes` | efeito que saiu ⇒ a concessão não está livre, qualquer que seja o estado local | H2 | retrocesso local |
| `release_sound` | chave liberada ⇒ nunca saiu | H2 | não utilização provada |
| `one_effect_per_grant` | uma concessão paga no máximo um efeito que saiu | H2 | corrida |
| `decide_ignores_local`, `decide_sound` | a recuperação é função só do registro ancorado; o que ela libera não saiu | H2 | ausência local como prova |
| `opaque_ambiguity`, `opaque_undecidable` | há dois estados alcançáveis com a mesma evidência durável, aplicado num e não no outro; nenhuma função da evidência decide | — | **o limite** |
| `expected_is_decide` | a tabela ponto de queda → decisão é `decide` | — | — |
| `rc3_local_clock_counterexample` | na conferência da RC3, há estado alcançável expirado no real e válido no local | — | (reproduz o contraexemplo) |

Hipóteses do modelo: **H1** testemunha de tempo honesta, assinatura não forjável, nonce novo,
relógio monotônico que mede o atraso; **H2** âncora durável, monotônica, que aplica `accept`;
**H3** recibo verdadeiro e recibo de não aplicação definitivo; **H4** registros identificados
pelo conteúdo (o hash não colide). Nenhum `sorry`, nenhum axioma além dos padrões do Lean
(`propext`, `Quot.sound`, `Classical.choice` em `window_brackets` via `omega`). T1, T2, T3, T6 e
T9 não foram tocados; `lake build --wfail` compila o núcleo inteiro com as 135 guardas.

O modelo representa uma concessão por reserva e o tempo como um número. Ele não modela a
criptografia, nem o WAL como estrutura (o adversário o substitui por qualquer coisa), nem a
saga: as compensações não são autorizadas por concessão.

## 7. Missão V: o confronto com o executável

| Evidência | Confronta |
|---|---|
| `tests/rc4.rs` (15 testes, processos reais) | os quatro contraexemplos; relógio local adiantado e atrasado (interposição de `clock_gettime`, `tests/clockshim.c`); repetição, retrocesso, atraso e congelamento da testemunha; prazo que vence antes do primeiro efeito e entre a reserva e o envio; recuperação depois do prazo; WAL que perde desfechos; WAL com desfecho alterado ou fabricado; registros forjados mandados direto à âncora; `SIGKILL` em todos os pontos; provedor opaco; quatro processos concorrentes; os níveis de garantia e a resolução tipada |
| `recovery_decides_as_the_lean_table` | lê `Lifecycle.expected` do fonte Lean e confere a decisão do `athc recover` nos sete pontos de queda |
| `lifecycle.rs` (testes de unidade) | as regras de `accept` e o verificador de tempo, sem rede |
| `a_temporal_program_is_refused_identically_without_the_protocol` | ARX × Python no limite do domínio comum |
| os diferenciais da RC2 e da RC3 | continuam iguais byte a byte (programas sem autoridade temporal) |
| `scripts/rc4-smoke.sh` | o cenário inteiro, num comando |

A ligação entre o Lean e o Rust é por correspondência de enunciado e pela tabela lida do fonte;
não há extração de código nem prova sobre o Rust.

## 8. O runtime Python

**Equivalente** (comparado byte a byte): tudo o que não depende de autoridade temporal,
inclusive as concessões, os carimbos e a ordem intenção → consumo das RC2 e RC3.

**Não equivalente, e recusado explicitamente**: o runtime Python não tem registro unificado,
testemunha de tempo, recibos assinados, travas nem registro durável. Diante de um programa com
autoridade temporal ele recusa a execução com `R0639`, o mesmo código e o mesmo texto do ARX sem o
protocolo. Ele não simula a garantia. A tabela de decisões do Lean é confrontada com o ARX, não
com o Python.

## 9. Limites

* **O provedor opaco.** Depois de um envio possível, sem recibo, nenhuma evidência durável
  decide se o efeito foi aplicado (`opaque_undecidable`). A operação fica escalada; o estado
  desconhecido é preservado. Não há como fazer melhor sem o provedor.
* **As hipóteses H1 a H4 não são demonstradas aqui.** A testemunha de tempo e a de desfecho são
  confiadas, e na demonstração são processos locais (`ath-world`) em TCP sem autenticação nem TLS;
  a separação de domínios de confiança depende de onde forem implantadas.
* **O relógio monotônico do executor é confiado** para medir intervalos. Ele não é ajustável
  por `settimeofday`, mas um executor comprometido no núcleo o controla.
* **Granularidade de um segundo** no instante atestado: a janela é conservadora por isso.
* **Disponibilidade.** Sem a âncora ou sem a testemunha, nada executa e nada é recuperado
  (`L0005`, `T0005`, `R0639`). É falha fechada, não tolerância a falhas.
* **Só operações autorizadas por concessão** têm o ciclo de vida ancorado. As compensações das
  sagas (`refund`) e as operações sem `requires s.authorizes` (o comprovante) seguem a semântica
  da RC3: o desfecho delas fica no WAL local.
* **Retirada (`withdraw`).** Quando o provedor não é alcançado na primeira tentativa, ou o prazo
  vence entre a última leitura e o envio, o executor retira a operação; ela fica com envio
  possível e não é reenviada, mesmo que o provedor seja idempotente. É conservador.
* **Não há revogação** de concessão além do prazo.
* **As autoridades temporais de um programa compartilham as testemunhas.**
* **A recuperação fecha obrigações; não retoma o programa.** Queda de energia não foi exercitada.

## 10. Códigos novos

| Código | Significado |
|---|---|
| `E0427` | (ampliado) operação inexistente em `ops = [..]`, `ops = []`, contrato sem autoridade |
| `E0428` | operação repetida em `ops = [..]` |
| `E0429` | testemunha não declarada, ou testemunha de tempo sem `tolerance` |
| `E0430` | autoridade mais fraca que a temporal, com `athc check --strict-authority` |
| `T0001` | tempo não autenticado: assinatura que não confere, ou resposta a outro pedido |
| `T0002` | retrocesso temporal: abaixo do piso |
| `T0003` | resposta de tempo acima da tolerância de atraso |
| `T0004` | congelamento: a testemunha andou menos que o relógio monotônico |
| `T0005` | testemunha de tempo indisponível |
| `R0638` | concessão ou fonte `Trusted` expirada na fronteira do efeito (ou num reenvio) |
| `R0639` | programa temporal sem o protocolo (âncora, testemunha, recibos) |
| `L0006` | o registro ancorado tem um registro sem evidência, ou está quebrado |
| `L0007` | a âncora recusou um registro do executor |
| `W0008` | desfecho local que não confere com o registro ancorado |
