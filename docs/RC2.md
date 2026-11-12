# Aletheia v1.0-RC2: a fronteira entre autorização, persistência e efeito

O v1.0-RC1 (`docs/RC1.md`) demonstrou a linguagem em execução real e deixou, por escrito, três
hipóteses frágeis nessa fronteira. O RC2 as transforma em propriedades do ARX, com testes que
falhavam e passaram a passar, e um modelo em Lean que o executável é obrigado a seguir.

```sh
scripts/rc2-smoke.sh     # a demonstração, num comando (exit 0 = todas as conferências)
cargo test -p athc --test rc2
```

O RC1 continua intacto como referência: `examples/rc1/`, `crates/athc/tests/rc1.rs` e
`scripts/rc-smoke.sh` não foram alterados e continuam passando.

## 1. Os dois contraexemplos, reproduzidos antes de corrigir

O commit `RC2: os dois contraexemplos do RC1 como testes que falham` acrescenta só um ponto de
queda (`authorized`) e dois testes. Nesse commit os dois **falham**; a saída está em
`docs/evidence/rc2-contraexemplos-no-rc1.txt`. Para reproduzir: faça checkout desse commit e
rode `cargo test -p athc --test rc2`.

| Contraexemplo | O que acontecia no RC1 | Teste |
|---|---|---|
| **A janela** | A concessão era consumida (`<concessões>.spent`, com `fsync`) *antes* de a intenção ir para o WAL. `SIGKILL` entre os dois: concessão gasta, nenhuma obrigação recuperável. A autorização do principal sumia sem efeito e sem registro. Pela leitura do código, uma recusa de orçamento no mesmo intervalo tinha o mesmo resultado sem queda alguma (isto não foi reproduzido em teste; a nova ordem reserva o orçamento antes). | `sigkill_between_authorization_and_intent_never_strands_a_grant` |
| **A corrida** | O registro de consumo era lido uma vez, na carga. Dois processos com o mesmo arquivo viam a concessão livre e os dois a gastavam. Com um provedor que não deduplica: dois agendamentos para uma autorização. | `two_processes_cannot_spend_the_same_grant` |

## 2. O protocolo: intenção → consumo → envio

A seção crítica da autorização está em `Arx::call_tool` (`crates/arx/src/lib.rs`) e em
`arx::grant::{begin, commit}`:

1. o contrato `requires s.authorizes(…)` passa: a concessão é só **reivindicada**, em memória;
2. o orçamento é reservado (uma recusa aqui não consome nada);
3. `begin`: `flock` exclusivo no registro de consumo; o registro é **relido**; se outro
   executor consumiu a concessão, `R0632`, sem nada escrito;
4. a **intenção** vai para o WAL (`fsync`), citando as concessões que vai consumir;
5. `commit`: o **consumo** vai para o registro (`fsync`), numa linha para o efeito inteiro,
   citando o hash da intenção; a trava é solta;
6. o efeito é enviado.

Em nenhum instante existe consumo sem intenção. O estado intermediário possível, intenção sem
consumo, tem um significado preciso: a operação **não saiu do processo**. A recuperação o fecha
como não enviada (`close … evidence: "recovery:unspent"`, com as concessões liberadas no campo
`released`) e a concessão continua livre para uma nova execução.

| Queda em | Em disco | `athc recover` |
|---|---|---|
| `authorized:N` | nada da operação | nada a fazer; a concessão está livre |
| `logged:N` | intenção, sem consumo | fecha como não enviada; a concessão está livre; nada é enviado |
| `intent:N` | intenção e consumo | o protocolo da classe do provedor (reenvio, consulta ou escalada) |
| `sent:N` e depois | idem | idem (como no RC1) |

Consequência para o provedor **opaco**: o RC1 escalava toda remessa interrompida, porque o WAL
não distinguia "antes do envio" de "depois". Agora a queda em `logged` prova que a remessa não
saiu e é fechada sem intervenção; a queda em `intent` ou depois continua escalada.

**Exclusão entre executores.** Duas travas `flock(2)`, que o núcleo solta quando o processo
morre (um `SIGKILL` não deixa trava órfã):

* registro de consumo: bloqueante, só durante os passos 3 a 5;
* WAL (`<wal>.lock`): não bloqueante, pela vida do processo. Um segundo `athc run` ou
  `athc recover` no mesmo WAL para com `W0006`.

**Recuperação sem adivinhar.** O WAL registra o caminho do registro de consumo. Se há intenção
em aberto que cita concessões e o registro não existe ou é ilegível, `athc recover` para com
`W0007` e não envia nem escreve nada.

## 3. Autenticidade: a autoridade é declarada no programa

```
authority treasury {
  key = "ed25519:d75a…511a"
}
```

`authority` é uma declaração da linguagem: está no AST, é conferida pelo `athc` (`E0426`) e
entra no hash do programa (Codex), de modo que o WAL de uma execução só é recuperado pelo
programa com a mesma autoridade. Um módulo que declara uma autoridade só aceita como fonte de
autoridade o que ela assinou (Ed25519, `crates/arx/src/sign.rs`):

| Fonte | O que a assinatura cobre | Sem assinatura válida |
|---|---|---|
| concessão (`s.authorizes`) | a identidade: execução, operação, argumentos, origem, nonce | `G0001` na carga (o arquivo inteiro é recusado); no runtime, a concessão é barrada e listada em `unauthenticated` no journal |
| fotografia confiável (`s.member`, `s.inventory`, …) | o BLAKE3 dos bytes do arquivo | `R0635`: `s.…` não é consultado |
| argumentos `Trusted` do evento | a execução e os argumentos | `R0636`: o tratador não começa |

Comandos: `athc keygen --out chave`, `athc grant … --sign chave`,
`athc sign --key chave --trusted arquivo`, `athc sign --key chave --run R --arg …`,
`athc run … --args-sig assinatura`. A chave privada nunca chega ao runtime. O journal registra
quem atestou cada fonte (`attested`) e a autoridade de cada concessão que pagou um efeito.

Um módulo **sem** `authority` se comporta como no RC1 (é o que mantém os testes do RC1 como
referência). `examples/rc2/treasury.ath` é a tesouraria do RC1 com a autoridade declarada; a
chave de demonstração é o vetor de teste 1 da RFC 8032, pública por definição.

## 4. Modelo formal e confronto com o executável

`lean/Aletheia/Txn.lean` modela o protocolo para qualquer número de executores, com a trava, e
prova sobre **todo** estado alcançável (logo, sobre todo ponto de queda):

| Teorema | Afirma |
|---|---|
| `spent_has_intent` | concessão consumida ⇒ a intenção do dono está no WAL |
| `at_most_one_sender` | dois executores nunca enviam com a mesma concessão |
| `unsent_never_left`, `nothing_never_left` | quando a recuperação decide "não enviada" ou "nada", a operação não saiu |
| `consumed_is_recoverable` | concessão consumida ⇒ a recuperação segue o protocolo do provedor |
| `unsent_leaves_grant` | "não enviada" nunca recai sobre concessão que o executor consumiu |
| `rc1_window`, `rc1_race` | os dois contraexemplos são alcançáveis no modelo do RC1 |
| `expected_is_recover` | a tabela ponto de queda → decisão é a função `recover` |

Lean 4.20.0, sem `sorry`, sem axiomas além de `propext`; 106 guardas em `Axioms.lean` (eram 96).

**Confronto.** O teste `the_grant_transaction_is_the_same_in_arx_and_python` lê a tabela
`expected` do fonte Lean e a usa como oráculo: para os quatro pontos de queda e as três classes
de provedor (12 casos), a decisão do ARX é a da tabela e o WAL recuperado é igual, byte a byte,
ao do runtime Python. `only_what_the_declared_authority_signed_confers_authority` compara os
journals dos dois runtimes nos casos de assinatura (7 casos).

## 5. Testes

`crates/athc/tests/rc2.rs`, dez testes com processos reais (`athc`, `ath-world`, SQLite):

| Teste | Verifica |
|---|---|
| `sigkill_between_authorization_and_intent_never_strands_a_grant` | `SIGKILL` em `authorized` e `logged` × 3 operações: nenhum consumo sem intenção; a operação não chega ao serviço; a recuperação não a envia; a segunda recuperação não escreve |
| `an_authorization_survives_a_crash_before_its_effect` | depois da queda, uma nova execução com as mesmas concessões faz o confirmado, uma vez |
| `an_opaque_operation_that_provably_never_left_is_not_escalated` | remessa: `logged` fecha sem intervenção; `intent` escala |
| `a_missing_spend_ledger_stops_recovery` | `W0007`, nada enviado, WAL intacto; com o registro de volta, recupera |
| `recovery_is_a_function_of_the_durable_state` | duas cópias do mesmo estado, mesmas decisões na mesma ordem |
| `two_processes_cannot_spend_the_same_grant` | 4 processos × 3 rodadas: cada concessão consumida uma vez; o serviço termina no estado de uma execução |
| `one_executor_per_wal` | `W0006` para um segundo `run` e para `recover` com a execução viva |
| `a_run_signed_by_the_declared_authority_executes` | execução e recuperação com tudo assinado; `W0004` para o programa sem a autoridade |
| `only_the_declared_authority_can_grant` | sem assinatura, outra chave, valor adulterado, concessão acrescentada, outra execução: nenhum efeito |
| `trusted_sources_must_be_signed_by_the_declared_authority` | fotografia adulterada / sem assinatura / outra chave (`R0635`); pedido diferente / outra execução / outra chave / sem assinatura (`R0636`) |

Mais: testes de unidade em `grant.rs` (leitura velha não gasta; guarda abandonada e rabo rasgado
não consomem; formato do RC1 continua lido), `wal.rs` (`W0006`) e `sign.rs`.

## 6. O que muda nas hipóteses do RC1

| Hipótese do RC1 | No RC2 |
|---|---|
| 4. "O arquivo de concessões não é assinado: quem escreve nele concede. O mesmo vale para `--trusted`." | Com `authority` declarada, eliminada: só a chave privada concede. |
| 5. "Um executor por WAL. Não há trava." | Eliminada: `W0006`. |
| (não escrita) consumo e intenção em dois passos | Eliminada: seção 2. |
| 1 a 3 (classe do provedor, `fsync`, `SIGKILL` ≠ queda de energia) | Continuam. |

## 7. Limitações

* **O registro de consumo é confiado quanto à integridade.** Uma linha ilegível o recusa e um
  rabo rasgado é aparado, mas apagar linhas válidas (rollback) devolve concessões e não é
  detectado: o registro não é encadeado nem assinado. É a principal hipótese que resta.
* **Reuso de assinaturas dentro do que elas cobrem.** A fotografia confiável assinada não é
  ligada a uma execução nem a uma data: uma versão antiga, autêntica, é aceita. A concessão é
  ligada à execução, mas não ao hash do programa: dois programas com a mesma autoridade e o
  mesmo identificador de execução aceitariam a mesma concessão.
* **`flock` é consultivo** e depende do sistema de arquivos (não foi exercitado em rede). Fora
  de Unix, a trava falha fechada: o ARX não executa com WAL.
* **Saga.** Uma concessão consumida por um pagamento que a saga depois estorna continua
  consumida: o principal precisa confirmar de novo. É a semântica do RC1, agora explícita.
* **`athc recover` fecha obrigações; não retoma o programa.** A operação fechada como não
  enviada só acontece numa nova execução (WAL novo, mesmas concessões).
* **Runtime Python.** Segue a mesma ordem, a mesma decisão de recuperação e a mesma verificação
  de assinatura de concessões e argumentos (os diferenciais comparam byte a byte), mas não tem
  registro de consumo durável nem travas, e recusa toda fotografia confiável quando há
  autoridade (não tem de onde receber a assinatura). A verificação Ed25519 dele é a de
  referência da RFC 8032; a do ARX (`verify_strict`) é mais estrita.
* **Lean.** O modelo trata uma concessão por operação; `fsync`, `flock` e as assinaturas são
  hipóteses, verificadas por teste e não por teorema.
* **Chave no fonte.** Trocar de autoridade é trocar de programa (outro Codex): não há rotação
  de chave com WALs em aberto. A chave do exemplo é pública e serve só à demonstração.
* **Queda de energia** continua não exercitada (`SIGKILL` mata o processo, não a máquina).

## 8. Códigos novos

| Código | Significado |
|---|---|
| `E0426` | chave inválida numa declaração `authority` |
| `G0001` | concessão sem assinatura válida de uma autoridade do programa (na carga) |
| `R0632` | (ampliado) a concessão foi consumida por outro executor |
| `R0633` | o registro de consumo não pôde ser lido ou travado |
| `R0635` | fotografia confiável sem a assinatura de uma autoridade |
| `R0636` | argumento `Trusted` sem a assinatura de uma autoridade |
| `W0006` | outro processo detém o WAL |
| `W0007` | a recuperação precisa do registro de consumo e ele não está disponível |
