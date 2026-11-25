# Aletheia v1.0-RC3: autoridade durável e integridade temporal

O RC2 (`docs/RC2.md`) fechou a fronteira entre autorização, persistência e efeito, e deixou
escrita a sua principal hipótese: o registro de consumo era confiado. O RC3 troca essa
hipótese por um mecanismo verificável e dá prazo e contexto ao que a autoridade assina. A
autorização deixa de ser verdadeira só no instante da concessão: a sua história (concessão,
intenção, consumo, âncora, desfecho) é conferida a cada consumo e a cada recuperação.

```sh
scripts/rc3-smoke.sh              # a demonstração, num comando (exit 0 = todas as conferências)
cargo test -p athc --test rc3     # nove testes com processos reais
```

O RC1 e o RC2 continuam passando sem alteração: `tests/rc1.rs`, `tests/rc2.rs`,
`scripts/rc-smoke.sh`, `scripts/rc2-smoke.sh` e os programas de `examples/rc1` e `examples/rc2`
(a raiz do Codex do programa do RC2 é a mesma: `#4877f356ba52`).

## 1. Modelo de confiança

| Quem | Pode | Não pode |
|---|---|---|
| **Adversário** | ler e escrever o disco do executor: registro de consumo, WAL, arquivos de concessões, fotografia, assinaturas apresentadas; restaurar cópias antigas; rodar o executor quando e quantas vezes quiser | obter a chave privada da autoridade; alterar o programa (o Codex muda); fazer a âncora voltar |
| **Autoridade** | assinar concessões, fotografia e pedido, para um programa, uma execução e um prazo | — |
| **Âncora** | guardar, por escopo, uma marca que só avança | conceder: ela só vê hashes |
| **Relógio do executor** | dizer que horas são | (é confiado; ver limitações) |

Fora desse modelo nada é prometido. Em particular, quem escreve no banco da âncora ou altera o
relógio do executor derrota as garantias correspondentes.

## 2. Os quatro contraexemplos, reproduzidos antes de corrigir

O commit `RC3: quatro contraexemplos do RC2 como testes que falham` só acrescenta testes, contra
o programa do RC2. Os quatro **falham** nele; a saída está em
`docs/evidence/rc3-contraexemplos-no-rc2.txt`.

| Contraexemplo | No RC2 | Fechado por |
|---|---|---|
| Retrocesso do registro | apagar o registro devolvia as concessões; uma nova execução repetia pagamentos e agendamento | cadeia + âncora (`L0002`) |
| Recuperação que reintegra | com a linha do consumo apagada, `athc recover` fechava como "não enviada" um agendamento que tinha saído e liberava a concessão | a recuperação confronta o registro com a âncora antes de decidir |
| Fotografia antiga | a fotografia autêntica de uma execução valia em qualquer outra, para sempre | carimbo: programa, execução, prazo (`R0635`) |
| Outro programa | a concessão valia em qualquer programa com a mesma autoridade e o mesmo identificador de execução | o carimbo da concessão cita a raiz do Codex (`G0001`) |

## 3. O registro de consumo: cadeia e âncora

**Cadeia.** Cada consumo é uma linha com `seq`, `prev` e `hash` (BLAKE3 do conteúdo), e cita o
programa (raiz do Codex), a execução, a ferramenta, a chave da operação e o hash da intenção do
WAL. Alterar, remover ou reordenar uma linha do meio quebra a cadeia: `L0001`.

**O que a cadeia não vê.** Um prefixo de uma cadeia íntegra é uma cadeia íntegra. Apagar o fim,
ou tudo, passa por qualquer verificação que só olhe o arquivo
(`the_chain_sees_the_middle_and_the_anchor_sees_the_end`, e `rc2_chain_accepts_rollback` no Lean).

**Âncora** (`crates/arx/src/anchor.rs`). Um estado fora do disco do executor que guarda, por
escopo (programa e execução), a marca do registro: quantos consumos e o hash do último. Só tem
`read` e `advance(de, para)`, um compare-and-swap que anda um passo e nunca volta. No
repositório ela é a operação `anchor` de `ath-world serve`, num processo e num banco SQLite
separados dos do provedor.

A ordem passa a ser **intenção → consumo → âncora → envio**, sob a trava do registro:

1. trava o registro, confere a cadeia e confronta com a âncora (`settle`);
2. persiste a intenção no WAL (citando as concessões);
3. persiste o consumo no registro (citando a intenção e o programa);
4. avança a âncora para a marca do novo consumo; solta a trava;
5. envia o efeito.

Como o envio vem depois do avanço, **todo efeito enviado tem o consumo ancorado**. A
conferência do passo 1, com `n` consumos no registro e a âncora em `a`:

| Situação | Resultado |
|---|---|
| `a = n`, mesma cabeça | em dia |
| `a = n − 1` e o registro passa pela marca da âncora | queda entre os passos 3 e 4: esse consumo não enviou nada; a âncora é adiantada |
| `a > n` | `L0002` retrocesso: nada é enviado nem escrito |
| o resto | `L0003` bifurcação |
| linha sem cadeia, ou de outro programa ou execução | `L0004` |
| âncora inalcançável | `L0005`: sem âncora não se consome nem se recupera |

A mesma conferência roda em `athc run` antes de a execução começar, em cada consumo e em
`athc recover` antes de qualquer decisão. O WAL registra que a execução é ancorada; a
recuperação é obrigada a usar a âncora.

A âncora também arbitra entre **cópias** do registro (duas máquinas, um backup em uso): a trava
de arquivo não as separa; o compare-and-swap sim
(`the_anchor_arbitrates_between_copies_of_the_ledger`).

**Desfecho.** O registro `close` do WAL passa a citar o hash do consumo que pagou a operação.
A cadeia completa de um efeito: concessão assinada → intenção (cita a concessão) → consumo (cita
a intenção e o programa) → marca da âncora → desfecho (cita o consumo). O teste
`every_effect_has_a_verifiable_history` percorre essa cadeia nos arquivos de uma execução real.

## 4. A autoridade: identidade, contexto, validade

```
authority treasury {
  key = "ed25519:d75a…511a"       -- identidade
  ops = "pay, schedule, wire"     -- contexto: o que ela pode conceder
  max_age = 900                   -- validade: duração máxima (s) do que ela assina
  ledger = "anchored"             -- história: o consumo exige âncora
}
```

* **`ops` é conferido pelo compilador (`E0427`).** Se toda autoridade do módulo declara `ops`,
  um contrato `requires s.authorizes("op", …)` que nenhuma delas cobre é um erro de tipo: o
  efeito nunca seria autorizado. Um `ops` que cita operação que nenhum contrato usa também.
* **Os vínculos entram no Codex.** Trocar escopo, prazo ou a exigência de âncora é trocar de
  programa.
* **Com vínculos, o que ela assina tem carimbo:** a raiz do Codex do programa, a execução,
  `issued` e `expires`. A assinatura cobre a carga e o carimbo. `Arx::attest` confere, nesta
  ordem: programa, execução, instante dentro do prazo, duração ≤ `max_age`, operação em `ops`.

| Fonte | Recusa | Código |
|---|---|---|
| concessão | outro programa, expirada, ainda não válida, prazo maior que `max_age`, fora do escopo de quem assinou, carimbo adulterado, formato do RC2 | `G0001` (carga) e `refused` no journal |
| fotografia confiável | idem, e assinada para outra execução | `R0635` |
| pedido `Trusted` | idem | `R0636` |
| consumo sem âncora | autoridade com `ledger = "anchored"` | `R0637` |

Uma autoridade só com `key` mantém a semântica do RC2. É a compatibilidade que mantém o RC2
passando; nada no compilador avisa que ela é mais fraca (limitação).

Comandos: `athc grant … --sign chave --program P --ttl S`,
`athc sign --key chave --trusted F --run R --program P --ttl S`,
`athc sign --key chave --run R --arg … --program P --ttl S`, `athc run … --anchor tcp://…`,
`athc recover … [--anchor tcp://…]`.

## 5. Modelo formal

`lean/Aletheia/Anchor.lean`, com o adversário como um passo do sistema (`tamper`: o registro
passa a ser qualquer lista):

| Teorema | Afirma |
|---|---|
| `anchor_monotone` | a âncora só cresce |
| `sent_anchored` | todo efeito enviado tem o consumo ancorado |
| `accepted_keeps_spent` | um registro aceito pela conferência contém o consumo de todo efeito enviado |
| `rollback_rejected`, `fork_rejected` | registro mais curto que a âncora, ou do mesmo tamanho e diferente, nunca é aceito |
| `rc2_chain_accepts_rollback` | o contraexemplo é alcançável quando só a cadeia é conferida |
| `rc3_rejects_that_rollback` | e a conferência do RC3 recusa aquele estado |
| `fresh_is_recent`, `fresh_is_scoped`, `other_run_is_stale`, `expired_stays_expired` | o que a conferência de validade aceita foi emitido há no máximo `max_age`, para este programa e esta execução; expirado não volta a valer |

Lean 4.20.0, sem `sorry`. 117 guardas em `Axioms.lean` (eram 106); os teoremas novos usam só os
axiomas padrão do Lean (`propext`, `Quot.sound` e, em `fork_rejected`, `Classical.choice`, que
vem do `omega`). T1, T2, T3, T6 e T9 não foram tocados: `lake build --wfail` compila o núcleo
inteiro com as guardas anteriores intactas.

**O que o modelo assume, e o executável não prova:** o registro é identificado pelo conteúdo
(o hash da cadeia não colide); a âncora é de fato monotônica e durável; a assinatura é
autêntica; o relógio diz a verdade.

## 6. Correspondência experimental

| Evidência | O que confronta |
|---|---|
| `tests/rc3.rs` (9 testes) | os contraexemplos e as propriedades acima com `athc`, `ath-world`, SQLite e `SIGKILL` |
| `grant.rs` (3 testes novos) | cadeia × âncora, bifurcação, avanço de um passo, cópias do registro, âncora fora do ar |
| `stamped_authority_decides_the_same_in_arx_and_python` | ARX × Python: 11 casos de escopo e validade, journals iguais byte a byte |
| `the_grant_transaction_is_the_same_in_arx_and_python` (RC2) | continua passando com a cadeia: o `close` cita o mesmo hash de consumo nos dois runtimes, e a tabela de `Txn.lean` continua sendo o oráculo |
| `scripts/rc3-smoke.sh` | o cenário inteiro num comando |

Os testes do executável e os teoremas do Lean falam das mesmas propriedades, mas a ligação é por
correspondência de enunciado (e, para a transação do RC2, pela tabela lida do Lean). Não há
extração de código nem prova sobre o Rust.

## 7. Runtime Python: o que é equivalente e o que não é

Equivalente (comparado byte a byte): a ordem intenção → consumo; o formato e o hash da linha de
consumo (a cadeia é calculada em memória); o `spend` citado no desfecho; a decisão de
recuperação; a verificação de assinaturas com e sem carimbo, de programa, execução, prazo,
`max_age` e escopo; os journals.

**Não equivalente:**

* não tem registro de consumo durável nem travas entre processos;
* **não tem âncora**: com `ledger = "anchored"`, recusa todo consumo (`R0637`), como o ARX sem
  `--anchor`. Não detecta retrocesso nem bifurcação;
* não recebe a assinatura da fotografia confiável: com qualquer autoridade declarada, recusa
  `s.member`/`s.inventory` (`R0635`);
* a verificação Ed25519 é a de referência da RFC 8032; a do ARX (`verify_strict`) é mais estrita;
* o instante é um argumento (`--now`); o ARX lê o relógio do sistema.

## 8. Limitações

* **A âncora do repositório é uma demonstração do protocolo.** É um `ath-world` na mesma
  máquina, em TCP sem autenticação nem TLS. A separação de domínios de confiança depende de
  onde ela é implantada; quem escreve no banco dela, ou responde no lugar dela, derrota a
  garantia. Quem a alcança pode adiantá-la (isso queima concessões; não devolve).
* **Um consumo forjado e bem encadeado, acrescentado ao fim, é ancorado** na próxima abertura
  (é indistinguível da queda entre o consumo e a âncora). Queima uma concessão; nunca libera uma.
* **A âncora cobre o registro de consumo, não o WAL.** Apagar o `close` do WAL reabre a intenção;
  a recuperação a resolve de novo pelo protocolo do provedor (reenvio com a mesma chave,
  consulta, ou escalada). Não há efeito duplicado nem concessão devolvida, mas o desfecho
  original não é provado pela âncora.
* **O relógio do executor é confiado.** Atrasá-lo revalida uma assinatura expirada. A âncora
  não guarda tempo.
* **A validade é conferida uma vez por fonte**, no primeiro uso. Uma execução longa pode
  terminar depois de `expires`.
* **Não há revogação** além do prazo.
* **Queda entre o consumo e a âncora num provedor que não deduplica:** a recuperação adianta a
  âncora e consulta o provedor; se o efeito não tinha saído, a concessão fica consumida sem
  efeito e o principal precisa conceder de novo. É conservador, não é ideal.
* **Um registro ancorado é de um só escopo** (programa, execução): `L0004` para o resto.
* **`ops` é texto separado por vírgulas**, não uma lista da linguagem.
* **Autoridade só com `key`** continua valendo como no RC2, sem aviso.
* **`athc recover` continua não retomando o programa.** Queda de energia continua não
  exercitada.

## 9. Códigos novos

| Código | Significado |
|---|---|
| `E0427` | o escopo `ops` das autoridades não fecha com os contratos |
| `L0001` | registro de consumo adulterado ou fora da cadeia |
| `L0002` | retrocesso: a âncora registra consumos que o registro não tem |
| `L0003` | bifurcação: o registro não passa pela marca da âncora |
| `L0004` | registro sem cadeia ou de outro escopo, numa execução ancorada |
| `L0005` | âncora inalcançável, ou não avançou |
| `R0637` | a autoridade exige registro ancorado e não há âncora |
