/-!
# v1.0-RC2: concessão e intenção, uma transação

O RC1 consumia a concessão no registro de consumo e **depois** persistia a intenção no WAL, e
cada executor decidia pela leitura que fez do registro na carga. Dois contraexemplos, aqui como
teoremas sobre o modelo do RC1:

* `rc1_window`: há um estado alcançável com a concessão consumida e **nenhuma** intenção. Uma
  queda ali perde a autorização do principal sem efeito e sem obrigação recuperável;
* `rc1_race`: há um estado alcançável em que **dois** executores enviaram com a mesma concessão.

O protocolo do RC2 (`arx::grant::{begin, commit}`, `Arx::call_tool`), para cada executor `e`:

1. `acquire`   toma a trava do registro de consumo (`flock`);
2. `logIntent` com a trava e a concessão livre, persiste a intenção no WAL, citando a concessão;
3. `spend`     com a trava, a intenção e a concessão ainda livre, persiste o consumo e solta a trava;
4. `send`      só o dono do consumo envia.

`release` solta a trava sem consumir: é a recusa (`R0632`, outro executor consumiu antes) e é
também a morte do processo, porque o núcleo solta um `flock` quando o processo morre. Uma queda
é o executor parar de dar passos: todo estado alcançável é um estado em que uma queda pode
deixar o sistema.

Provado sobre **todo** estado alcançável, para qualquer número de executores:

* `spent_has_intent`: concessão consumida ⇒ a intenção do dono está no WAL (a janela fechou);
* `at_most_one_sender`: dois executores nunca enviam com a mesma concessão (a corrida fechou);
* `unsent_never_left`, `nothing_never_left`: quando a recuperação decide "não enviada" ou "nada
  a fazer", a operação não saiu do processo. É o que autoriza fechar uma intenção sem consumo
  sem perguntar ao provedor, inclusive um provedor opaco;
* `consumed_is_recoverable`: concessão consumida ⇒ a recuperação segue o protocolo do provedor;
* `unsent_leaves_grant`: a decisão "não enviada" nunca é tomada sobre uma concessão que este
  executor consumiu: a autorização não é descartada.

`recover` é uma função do estado durável: a recuperação é determinística por construção.

A tabela `expected` (ponto de queda → decisão) é provada igual a `recover` (`expected_is_recover`)
e é lida deste arquivo pelo teste `the_grant_transaction_is_the_same_in_arx_and_python`
(`crates/athc/tests/differential.rs`), que a confronta com o ARX e com o runtime Python.

Fora do modelo: a durabilidade do `fsync`, a exclusão do `flock` e a integridade do registro em
disco são hipóteses do sistema; a autenticidade (assinaturas) é verificada por teste.
-/

namespace Aletheia.Txn

/-- O estado durável de uma concessão: quem a consumiu, quem detém a trava do registro, e, por
executor, se a intenção está no seu WAL e se a operação saiu do processo. -/
structure Sys where
  owner : Option Nat
  lock : Option Nat
  intent : Nat → Bool
  sent : Nat → Bool

def init : Sys := ⟨none, none, fun _ => false, fun _ => false⟩

def mark (f : Nat → Bool) (e : Nat) : Nat → Bool := fun x => if x = e then true else f x

/-- Os passos do protocolo do RC2. -/
inductive Step : Sys → Sys → Prop
  | acquire (s : Sys) (e : Nat) : s.lock = none → Step s { s with lock := some e }
  | logIntent (s : Sys) (e : Nat) :
      s.lock = some e → s.owner = none → Step s { s with intent := mark s.intent e }
  | spend (s : Sys) (e : Nat) :
      s.lock = some e → s.owner = none → s.intent e = true →
      Step s { s with owner := some e, lock := none }
  | release (s : Sys) (e : Nat) : s.lock = some e → Step s { s with lock := none }
  | send (s : Sys) (e : Nat) : s.owner = some e → Step s { s with sent := mark s.sent e }

inductive Reach : Sys → Prop
  | init : Reach init
  | step {s t : Sys} : Reach s → Step s t → Reach t

/-- O invariante: o dono do consumo tem a intenção no WAL, e só o dono envia. -/
def Inv (s : Sys) : Prop :=
  (∀ e, s.owner = some e → s.intent e = true) ∧ (∀ e, s.sent e = true → s.owner = some e)

theorem inv_of_reach {s : Sys} (h : Reach s) : Inv s := by
  induction h with
  | init =>
    refine ⟨fun e h => ?_, fun e h => ?_⟩
    · simp [init] at h
    · simp [init] at h
  | step _ st ih =>
    obtain ⟨i1, i2⟩ := ih
    cases st with
    | acquire e _ => exact ⟨i1, i2⟩
    | release e _ => exact ⟨i1, i2⟩
    | logIntent e _ hfree =>
      refine ⟨fun e' h => ?_, i2⟩
      simp [hfree] at h
    | spend e _ hfree hint =>
      refine ⟨fun e' h => ?_, fun e' h => ?_⟩
      · simp at h
        subst h
        exact hint
      · have := i2 e' h
        simp [hfree] at this
    | send e hown =>
      refine ⟨i1, fun e' h => ?_⟩
      simp only [mark] at h
      by_cases he : e' = e
      · subst he
        exact hown
      · simp [he] at h
        exact i2 e' h

/-- **A janela fechou.** Concessão consumida ⇒ a intenção do dono está no WAL. -/
theorem spent_has_intent {s : Sys} {e : Nat} (h : Reach s) (ho : s.owner = some e) :
    s.intent e = true :=
  (inv_of_reach h).1 e ho

/-- Só o dono do consumo envia. -/
theorem sender_is_owner {s : Sys} {e : Nat} (h : Reach s) (hs : s.sent e = true) :
    s.owner = some e :=
  (inv_of_reach h).2 e hs

/-- **A corrida fechou.** Dois executores nunca enviam com a mesma concessão. -/
theorem at_most_one_sender {s : Sys} {a b : Nat} (h : Reach s)
    (ha : s.sent a = true) (hb : s.sent b = true) : a = b := by
  have h1 := sender_is_owner h ha
  have h2 := sender_is_owner h hb
  rw [h1] at h2
  exact Option.some.inj h2

/-- O que a recuperação faz com a operação do executor `e`, lendo só o estado durável. -/
inductive Decision
  | nothing   -- não há intenção: nada a recuperar
  | unsent    -- intenção sem consumo: fechada como não enviada; a concessão não foi gasta por `e`
  | byClass   -- intenção e consumo: o protocolo da classe do provedor
  deriving DecidableEq, Repr

def recover (s : Sys) (e : Nat) : Decision :=
  if s.owner = some e then .byClass else if s.intent e = true then .unsent else .nothing

/-- Fechar como "não enviada" é correto: a operação não saiu do processo. -/
theorem unsent_never_left {s : Sys} {e : Nat} (h : Reach s) (hr : recover s e = .unsent) :
    s.sent e = false := by
  cases hs : s.sent e with
  | false => rfl
  | true =>
    have ho := sender_is_owner h hs
    simp [recover, ho] at hr

theorem nothing_never_left {s : Sys} {e : Nat} (h : Reach s) (hr : recover s e = .nothing) :
    s.sent e = false := by
  cases hs : s.sent e with
  | false => rfl
  | true =>
    have ho := sender_is_owner h hs
    simp [recover, ho] at hr

/-- Uma concessão consumida sempre tem obrigação recuperável. -/
theorem consumed_is_recoverable {s : Sys} {e : Nat} (h : Reach s) (ho : s.owner = some e) :
    recover s e = .byClass ∧ s.intent e = true :=
  ⟨by simp [recover, ho], spent_has_intent h ho⟩

/-- A decisão "não enviada" nunca recai sobre uma concessão que `e` consumiu. -/
theorem unsent_leaves_grant {s : Sys} {e : Nat} (hr : recover s e = .unsent) :
    s.owner ≠ some e := by
  intro ho
  simp [recover, ho] at hr

/-! ## Os pontos de queda do executável -/

/-- Os pontos de `--kill-at` na fronteira da autorização (`arx::wal::CrashPoint`). -/
inductive Point
  | authorized  -- o contrato passou; nada em disco
  | logged      -- a intenção está no WAL; o consumo não
  | intent      -- a intenção e o consumo estão persistidos; nada enviado
  | sent        -- enviado
  deriving DecidableEq, Repr

def s0 : Sys := init
def s1 : Sys := { s0 with lock := some 0 }
def s2 : Sys := { s1 with intent := mark s1.intent 0 }
def s3 : Sys := { s2 with owner := some 0, lock := none }
def s4 : Sys := { s3 with sent := mark s3.sent 0 }

/-- O estado em que a queda no ponto `p` deixa o executor 0 (a trava some com o processo). -/
def crashAt : Point → Sys
  | .authorized => s0
  | .logged => { s2 with lock := none }
  | .intent => s3
  | .sent => s4

theorem reach_s2 : Reach s2 :=
  .step (.step .init (.acquire s0 0 rfl)) (.logIntent s1 0 rfl rfl)

theorem reach_s3 : Reach s3 := .step reach_s2 (.spend s2 0 rfl rfl rfl)

theorem crash_reachable (p : Point) : Reach (crashAt p) := by
  cases p with
  | authorized => exact .init
  | logged => exact .step reach_s2 (.release s2 0 rfl)
  | intent => exact reach_s3
  | sent => exact .step reach_s3 (.send s3 0 rfl)

/-- A tabela que o teste diferencial lê deste arquivo e confronta com o ARX e o Python. -/
def expected : List (Point × Decision) :=
  [(.authorized, .nothing), (.logged, .unsent), (.intent, .byClass), (.sent, .byClass)]

theorem expected_is_recover : ∀ pd ∈ expected, recover (crashAt pd.1) 0 = pd.2 := by
  intro pd h
  simp [expected] at h
  rcases h with h | h | h | h <;> subst h <;>
    simp [recover, crashAt, s4, s3, s2, s1, s0, init, mark]

/-! ## O RC1, e por que ele não bastava -/

/-- O RC1: cada executor decide pela leitura da carga (`seen`), o consumo vem antes da intenção,
e nada impede dois consumos. -/
structure Sys1 where
  ledger : List Nat
  seen : Nat → Bool
  intent : Nat → Bool
  sent : Nat → Bool

def init1 : Sys1 := ⟨[], fun _ => false, fun _ => false, fun _ => false⟩

inductive Step1 : Sys1 → Sys1 → Prop
  | load (s : Sys1) (e : Nat) : s.ledger = [] → Step1 s { s with seen := mark s.seen e }
  | spend (s : Sys1) (e : Nat) : s.seen e = true → Step1 s { s with ledger := e :: s.ledger }
  | logIntent (s : Sys1) (e : Nat) : e ∈ s.ledger → Step1 s { s with intent := mark s.intent e }
  | send (s : Sys1) (e : Nat) : s.intent e = true → Step1 s { s with sent := mark s.sent e }

inductive Reach1 : Sys1 → Prop
  | init : Reach1 init1
  | step {s t : Sys1} : Reach1 s → Step1 s t → Reach1 t

/-- **Contraexemplo 1 (a janela).** No RC1 há um estado alcançável com a concessão consumida e
nenhuma intenção no WAL. -/
theorem rc1_window : ∃ s, Reach1 s ∧ 0 ∈ s.ledger ∧ s.intent 0 = false := by
  let a : Sys1 := { init1 with seen := mark init1.seen 0 }
  let b : Sys1 := { a with ledger := 0 :: a.ledger }
  refine ⟨b, .step (.step .init (.load init1 0 rfl)) (.spend a 0 ?_), ?_, ?_⟩
  · simp [a, mark]
  · simp [b]
  · simp [b, a, init1]

/-- **Contraexemplo 2 (a corrida).** No RC1 há um estado alcançável em que dois executores
enviaram com a mesma concessão. -/
theorem rc1_race : ∃ s, Reach1 s ∧ s.sent 0 = true ∧ s.sent 1 = true := by
  let a : Sys1 := { init1 with seen := mark init1.seen 0 }
  let b : Sys1 := { a with seen := mark a.seen 1 }
  let c : Sys1 := { b with ledger := 0 :: b.ledger }
  let d : Sys1 := { c with ledger := 1 :: c.ledger }
  let e : Sys1 := { d with intent := mark d.intent 0 }
  let f : Sys1 := { e with intent := mark e.intent 1 }
  let g : Sys1 := { f with sent := mark f.sent 0 }
  let h : Sys1 := { g with sent := mark g.sent 1 }
  have r : Reach1 h := by
    refine .step (.step (.step (.step (.step (.step (.step (.step .init
      (.load init1 0 rfl)) (.load a 1 rfl)) (.spend b 0 ?_)) (.spend c 1 ?_))
      (.logIntent d 0 ?_)) (.logIntent e 1 ?_)) (.send f 0 ?_)) (.send g 1 ?_)
    all_goals simp [g, f, e, d, c, b, a, init1, mark]
  exact ⟨h, r, by simp [h, g, mark], by simp [h, mark]⟩

end Aletheia.Txn
