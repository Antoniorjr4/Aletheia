/-!
# v1.0-RC5: a retomada verificada

O modelo de `arx::resume`. Um programa é determinístico **dadas as suas entradas**: é uma
árvore de interação (`Prog`) que pede algo ao mundo e continua com a resposta. A fita (`Tape`)
é a lista das entradas que o programa já observou, na ordem.

| `arx` | aqui |
|---|---|
| a fita `<wal>.tape`: `input` (amostra de modelo) e `ret` (o que a chamada devolveu) | `Tape` |
| a re-execução conferida: cada entrada vem da fita, comparada com o que o programa pede | `replay` |
| divergência (`R0650`): o programa pede outra coisa | `replay … = none` |
| o envio de uma operação com chave a um provedor que deduplica pela chave | `Step.send`, `call` |
| a resposta gravada na fita | `Step.record` |
| o modelo responde (qualquer coisa: ele não é determinístico) | `Step.infer` |
| a queda e `athc resume`: processo novo, do início, com a fita | `Step.crash` |

O adversário do modelo é a **queda**: pode ocorrer em qualquer configuração, quantas vezes
quiser, inclusive entre o envio e a gravação da resposta.

## Hipóteses

* **R1 determinismo**: o programa é função das suas entradas (é o que `Prog` é). A fotografia
  confiável e os argumentos são os mesmos (o executável recusa outros: `R0652`).
* **R2 provedor**: a operação com chave é deduplicada pelo provedor (`idempotent`), ou o
  desfecho da operação interrompida foi decidido pela recuperação antes da re-execução (o
  protocolo de cada classe é o de `Wal.lean` e `Lifecycle.lean`; aqui ele é o `call` idempotente).
* **R3 fita**: a fita é um prefixo do que foi observado. O encadeamento não a autentica: quem
  escreve no disco do executor pode refazê-lo. O executável sustenta R3 com a marca da fita na
  âncora (execuções com autoridade temporal: `Arx::tape_seal`, `L0008`) e, sem âncora, R3 é
  confiança no disco. O tipo dos valores não depende de R3: ver `replayC`.

Não modelado: a chave nova da operação provadamente não enviada (`resume::key_for`), as
concessões e o ciclo de vida ancorado (estão em `Txn.lean`, `Anchor.lean`, `Lifecycle.lean`).
-/

namespace Aletheia.Resume

/-- O que o programa pede: uma amostra de modelo, ou uma operação com chave. -/
inductive Req where
  | infer (prompt : Nat)
  | call (key tool args : Nat)
  deriving DecidableEq

/-- Um programa: termina com um valor, ou pede algo e continua com a resposta. -/
inductive Prog where
  | done (v : Nat)
  | ask (r : Req) (k : Nat → Prog)

/-- A fita das entradas observadas: (o que foi pedido, o que veio). -/
abbrev Tape := List (Req × Nat)

/-- A re-execução conferida: consome a fita, comparando cada entrada com o que o programa
pede. Devolve o programa que resta; `none` é a divergência. **Não toma o mundo**: re-executar
o que já foi observado não envia nada, por construção. -/
def replay : Prog → Tape → Option Prog
  | p, [] => some p
  | .done _, _ :: _ => none
  | .ask r k, (r', a) :: t => if r = r' then replay (k a) t else none

/-- O provedor: as operações aplicadas, por chave, com o que cada uma devolveu. -/
abbrev World := List (Nat × Nat)

def lookup : World → Nat → Option Nat
  | [], _ => none
  | (k, a) :: w, key => if k = key then some a else lookup w key

/-- O envio: o provedor deduplica pela chave. Uma chave já aplicada não gera novo efeito. -/
def call (prov : Nat → Nat → Nat → Nat) (w : World) (key tool args : Nat) : World :=
  match lookup w key with
  | some _ => w
  | none => (key, prov key tool args) :: w

structure Cfg where
  prog : Prog
  world : World
  tape : Tape

/-- O custo de uma entrada (os tokens de uma amostra): o orçamento gasto é função da fita. -/
def spent (cost : Req → Nat → Nat) : Tape → Nat
  | [] => 0
  | (r, a) :: t => cost r a + spent cost t

/-- Os passos do sistema, para um programa `p0` e um provedor `prov`. -/
inductive Step (p0 : Prog) (prov : Nat → Nat → Nat → Nat) : Cfg → Cfg → Prop where
  /-- o modelo responde `a` (qualquer `a`), e a resposta vai para a fita -/
  | infer {c : Cfg} {q : Nat} {k : Nat → Prog} (a : Nat) :
      c.prog = .ask (.infer q) k → Step p0 prov c ⟨k a, c.world, c.tape ++ [(.infer q, a)]⟩
  /-- a operação sai para o provedor; a resposta ainda não está na fita -/
  | send {c : Cfg} {key tool args : Nat} {k : Nat → Prog} :
      c.prog = .ask (.call key tool args) k →
      Step p0 prov c ⟨c.prog, call prov c.world key tool args, c.tape⟩
  /-- a resposta do provedor é gravada e o programa continua -/
  | record {c : Cfg} {key tool args a : Nat} {k : Nat → Prog} :
      c.prog = .ask (.call key tool args) k → lookup c.world key = some a →
      Step p0 prov c ⟨k a, c.world, c.tape ++ [(.call key tool args, a)]⟩
  /-- **a queda e a retomada**: um processo novo re-executa `p0` com a fita -/
  | crash {c : Cfg} {p' : Prog} :
      replay p0 c.tape = some p' → Step p0 prov c ⟨p', c.world, c.tape⟩

inductive Reach (p0 : Prog) (prov : Nat → Nat → Nat → Nat) : Cfg → Prop where
  | init : Reach p0 prov ⟨p0, [], []⟩
  | step {c c' : Cfg} : Reach p0 prov c → Step p0 prov c c' → Reach p0 prov c'

/-! ## A re-execução -/

theorem replay_nil (p : Prog) : replay p [] = some p := by
  cases p <;> rfl

theorem replay_append : ∀ (t : Tape) (p : Prog) (r : Req) (k : Nat → Prog) (a : Nat),
    replay p t = some (.ask r k) → replay p (t ++ [(r, a)]) = some (k a)
  | [], p, r, k, a, h => by
      simp only [replay, Option.some.injEq] at h
      subst h
      simp [replay]
  | (r', a') :: t, .done v, r, k, a, h => by simp [replay] at h
  | (r', a') :: t, .ask r0 k0, r, k, a, h => by
      simp only [replay, List.cons_append] at h ⊢
      by_cases e : r0 = r'
      · simp only [e, if_true] at h ⊢
        exact replay_append t (k0 a') r k a h
      · simp [e] at h

/-- **Fidelidade da re-execução.** Em toda configuração alcançável, com quantas quedas for,
re-executar o programa do início com a fita devolve exatamente o programa em curso. -/
theorem replay_fidelity {p0 : Prog} {prov : Nat → Nat → Nat → Nat} {c : Cfg}
    (h : Reach p0 prov c) : replay p0 c.tape = some c.prog := by
  induction h with
  | init => exact replay_nil p0
  | step _ s ih =>
    cases s with
    | infer a hp => exact replay_append _ _ _ _ a (hp ▸ ih)
    | send hp => exact ih
    | record hp _ => exact replay_append _ _ _ _ _ (hp ▸ ih)
    | crash hr => exact hr

/-- **A queda não muda nada.** O passo de queda e retomada deixa o mundo e a fita como
estavam, e (pela fidelidade) o programa também: a retomada continua a execução original. -/
theorem crash_is_identity {p0 : Prog} {prov : Nat → Nat → Nat → Nat} {c : Cfg} {p' : Prog}
    (h : Reach p0 prov c) (hr : replay p0 c.tape = some p') :
    (⟨p', c.world, c.tape⟩ : Cfg) = c := by
  have := replay_fidelity h
  rw [hr] at this
  cases c
  simp_all

/-- A execução sem quedas. -/
inductive Reach0 (p0 : Prog) (prov : Nat → Nat → Nat → Nat) : Cfg → Prop where
  | init : Reach0 p0 prov ⟨p0, [], []⟩
  | infer {c : Cfg} {q : Nat} {k : Nat → Prog} (a : Nat) : Reach0 p0 prov c →
      c.prog = .ask (.infer q) k → Reach0 p0 prov ⟨k a, c.world, c.tape ++ [(.infer q, a)]⟩
  | send {c : Cfg} {key tool args : Nat} {k : Nat → Prog} : Reach0 p0 prov c →
      c.prog = .ask (.call key tool args) k →
      Reach0 p0 prov ⟨c.prog, call prov c.world key tool args, c.tape⟩
  | record {c : Cfg} {key tool args a : Nat} {k : Nat → Prog} : Reach0 p0 prov c →
      c.prog = .ask (.call key tool args) k → lookup c.world key = some a →
      Reach0 p0 prov ⟨k a, c.world, c.tape ++ [(.call key tool args, a)]⟩

/-- **A retomada refina a execução sem queda.** Tudo o que é alcançável com quedas e
retomadas, em qualquer ponto e em qualquer número, é alcançável sem queda nenhuma: a queda
não cria comportamento. Vale **neste modelo** (provedor que deduplica, sem concessões nem ciclo
de vida ancorado); a composição com as chaves novas, as concessões e a âncora é sustentada
pelos testes, não por uma prova integrada. -/
theorem crash_refines {p0 : Prog} {prov : Nat → Nat → Nat → Nat} {c : Cfg}
    (h : Reach p0 prov c) : Reach0 p0 prov c := by
  induction h with
  | init => exact .init
  | step hc s ih =>
    cases s with
    | infer a hp => exact .infer a ih hp
    | send hp => exact .send ih hp
    | record hp hl => exact .record ih hp hl
    | crash hr => rw [crash_is_identity hc hr]; exact ih

/-! ## No máximo uma vez -/

theorem lookup_none_not_mem : ∀ (w : World) (key : Nat),
    lookup w key = none → key ∉ w.map Prod.fst
  | [], _, _ => by simp
  | (k, a) :: w, key, h => by
      simp only [lookup] at h
      by_cases e : k = key
      · simp [e] at h
      · simp only [e, if_false] at h
        simp only [List.map_cons, List.mem_cons, not_or]
        exact ⟨fun x => e x.symm, lookup_none_not_mem w key h⟩

theorem call_nodup {prov : Nat → Nat → Nat → Nat} {w : World} (key tool args : Nat)
    (h : (w.map Prod.fst).Nodup) : ((call prov w key tool args).map Prod.fst).Nodup := by
  unfold call
  cases e : lookup w key with
  | some a => simpa using h
  | none =>
    simp only [List.map_cons, List.nodup_cons]
    exact ⟨lookup_none_not_mem w key e, h⟩

/-- **No máximo uma vez.** Com quantas quedas e retomadas for, o provedor aplica cada chave
no máximo uma vez: nenhuma operação é duplicada. É uma propriedade de segurança, sob R2. **Não
diz que toda operação termina**: uma operação pode nunca ser aplicada (a concessão consumida
por uma tentativa que o provedor não aplicou, por exemplo). -/
theorem at_most_once {p0 : Prog} {prov : Nat → Nat → Nat → Nat} {c : Cfg}
    (h : Reach p0 prov c) : (c.world.map Prod.fst).Nodup := by
  induction h with
  | init => simp
  | step _ s ih =>
    cases s with
    | infer a hp => exact ih
    | send hp => exact call_nodup _ _ _ ih
    | record hp _ => exact ih
    | crash hr => exact ih

theorem lookup_call {prov : Nat → Nat → Nat → Nat} {w : World} {key a : Nat} (k' tool args : Nat)
    (h : lookup w key = some a) : lookup (call prov w k' tool args) key = some a := by
  unfold call
  cases e : lookup w k' with
  | some b => simpa using h
  | none =>
    simp only [lookup]
    by_cases q : k' = key
    · subst q; rw [e] at h; cases h
    · simp [q, h]

/-- **A retomada não inventa resultados.** Toda resposta de operação que está na fita é a
que o provedor deu para aquela chave. -/
theorem tape_is_evidence {p0 : Prog} {prov : Nat → Nat → Nat → Nat} {c : Cfg}
    (h : Reach p0 prov c) :
    ∀ key tool args a, (Req.call key tool args, a) ∈ c.tape → lookup c.world key = some a := by
  induction h with
  | init => intro _ _ _ _ hm; cases hm
  | step _ s ih =>
    cases s with
    | infer a hp =>
      intro key tool args b hm
      simp only [List.mem_append, List.mem_singleton, Prod.mk.injEq] at hm
      rcases hm with hm | ⟨hm, _⟩
      · exact ih _ _ _ _ hm
      · cases hm
    | send hp =>
      intro key tool args b hm
      exact lookup_call _ _ _ (ih _ _ _ _ hm)
    | record hp hl =>
      intro key tool args b hm
      simp only [List.mem_append, List.mem_singleton, Prod.mk.injEq] at hm
      rcases hm with hm | ⟨hm, hb⟩
      · exact ih _ _ _ _ hm
      · cases hm; subst hb; exact hl
    | crash hr => exact ih

/-! ## O orçamento atravessa a queda -/

theorem spent_append (cost : Req → Nat → Nat) : ∀ (t : Tape) (r : Req) (a : Nat),
    spent cost (t ++ [(r, a)]) = spent cost t + cost r a
  | [], r, a => by simp [spent]
  | (r', a') :: t, r, a => by
      simp only [List.cons_append, spent, spent_append cost t r a]
      omega

/-- **O gasto só cresce, e a queda não o zera.** O orçamento gasto é função da fita: nenhum
passo o diminui, e o passo de queda e retomada o deixa exatamente como estava. Cobre **o que
foi gravado**: uma amostra cuja resposta se perdeu entre o modelo e a fita não está aqui, e a
integridade da fita é hipótese (R3). -/
theorem budget_survives {p0 : Prog} {prov : Nat → Nat → Nat → Nat} (cost : Req → Nat → Nat)
    {c c' : Cfg} (s : Step p0 prov c c') : spent cost c.tape ≤ spent cost c'.tape := by
  cases s with
  | infer a hp => simp only [spent_append]; omega
  | send hp => exact Nat.le_refl _
  | record hp _ => simp only [spent_append]; omega
  | crash hr => exact Nat.le_refl _

/-- **O modelo não é consultado de novo.** A resposta de uma amostra que está na fita é a
que a re-execução entrega ao programa, qualquer que seja o modelo agora: `replay` não tem
modelo. Um programa cuja fita registra a amostra `a` continua com `k a`. -/
theorem model_not_asked_again (q a : Nat) (k : Nat → Prog) (t : Tape) :
    replay (.ask (.infer q) k) ((.infer q, a) :: t) = replay (k a) t := by
  simp [replay]

/-- **A divergência não continua.** Se o programa pede outra coisa que a fita registra, a
re-execução não devolve programa nenhum: não há passo de retomada (`R0650`). -/
theorem divergence_stops (r r' : Req) (a : Nat) (k : Nat → Prog) (t : Tape) (h : r ≠ r') :
    replay (.ask r k) ((r', a) :: t) = none := by
  simp [replay, h]

/-! ## A re-execução conferida pelo monitor (RC5.1) -/

/-- A re-execução que confere cada entrada da fita com `ok` (o monitor de tipo da fronteira)
antes de entregá-la ao programa. É a do executável desde a RC5.1: o valor que vem da fita passa
pela mesma conferência do valor que vem do provedor. -/
def replayC (ok : Req → Nat → Bool) : Prog → Tape → Option Prog
  | p, [] => some p
  | .done _, _ :: _ => none
  | .ask r k, (r', a) :: t => if r = r' ∧ ok r a = true then replayC ok (k a) t else none

/-- **Nada entra pela fita sem passar pelo monitor.** Se a re-execução conferida devolve um
programa, toda entrada que ela entregou satisfaz `ok`, qualquer que seja a fita (inclusive uma
fita reescrita por quem controla o disco). -/
theorem replayC_checked (ok : Req → Nat → Bool) : ∀ (t : Tape) (p p' : Prog),
    replayC ok p t = some p' → ∀ e ∈ t, ok e.1 e.2 = true
  | [], _, _, _ => by intro e he; cases he
  | (r', a) :: t, .done v, p', h => by simp [replayC] at h
  | (r', a) :: t, .ask r k, p', h => by
      simp only [replayC] at h
      by_cases c : r = r' ∧ ok r a = true
      · rw [if_pos c] at h
        intro e he
        simp only [List.mem_cons] at he
        rcases he with he | he
        · rw [he]; exact c.1 ▸ c.2
        · exact replayC_checked ok t (k a) p' h e he
      · simp [c] at h

/-- **Sem culpa indevida.** Numa fita em que toda entrada satisfaz `ok`, a re-execução
conferida é a re-execução: o monitor não muda o que uma fita íntegra retoma. -/
theorem replayC_agrees (ok : Req → Nat → Bool) : ∀ (t : Tape) (p : Prog),
    (∀ e ∈ t, ok e.1 e.2 = true) → replayC ok p t = replay p t
  | [], p, _ => by cases p <;> rfl
  | (r', a) :: t, .done v, _ => by simp [replayC, replay]
  | (r', a) :: t, .ask r k, h => by
      have ha : ok r' a = true := h (r', a) (by simp)
      have ht : ∀ e ∈ t, ok e.1 e.2 = true := fun e he => h e (by simp [he])
      simp only [replayC, replay]
      by_cases c : r = r'
      · subst c
        simp only [ha, and_self, if_true]
        exact replayC_agrees ok t (k a) ht
      · simp [c]

end Aletheia.Resume
