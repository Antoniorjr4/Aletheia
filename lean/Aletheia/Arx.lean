import Aletheia.Keys

/-!
# M2: o ARX como refinamento do λ_Ath

`eval` (`Eval.lean`) é a semântica sobre a qual T1–T9 estão provados: funcional, devolve o
traço como valor, guarda o registro da saga corrente no estado e o restaura na saída. O ARX
(`crates/arx/src/lib.rs`) é outra máquina: imperativa, escreve um journal **global** só de
acréscimos, empilha um quadro por saga (`self.sagas`), desenrola falhas (`Unwind`), funde a
ação e o fechamento `Done` num único evento `tool`, deixa a reconciliação num laço com
`break` e não registra os argumentos de uma operação sem resposta. A vista do núcleo
(`Journal::core_view_with`) reconstrói, a partir desse journal grosso, a pilha de
compensações, a sonda corrente e a chave recusada.

Este arquivo modela essa máquina em Lean (`run`), com a estrutura do código Rust, e a vista
do núcleo (`cvStep`/`cvRun`). O teorema `sim` é uma **simulação para a frente**: toda
execução de `eval` que termina com valor ou com falha capturável é realizada pelo modelo do
ARX, com

* o mesmo valor e a mesma causa de falha;
* o mesmo mundo, o mesmo índice de oráculo e o mesmo contador de chaves;
* gasto do ARX ≤ gasto contabilizado pelo λ_Ath (o ARX não liquida uma chamada recusada);
* a vista do núcleo do journal do ARX igual ao traço do λ_Ath módulo `nv`, que apaga o que o
  journal do ARX não registra (o argumento de uma operação pendente e o de uma compensação
  com chave, o valor do encerramento) e põe o fechamento `Transferred` junto da entrega.

As hipóteses do teorema são as obrigações que sobram, e cada uma tem contraexemplo no fim do
arquivo: compensações gratuitas (`comp_cost_breaks_budget`), ferramentas com chave usadas só
por `call` (`KdOk`), e, para a igualdade byte a byte e não só módulo `nv`, um tratador puro
(`impure_handler_reorders`) e `await` dentro de saga (`top_level_await_differs`).

O que este arquivo **não** prova: que o código Rust é este modelo. Essa ligação continua
empírica (o tripé roda o binário do ARX). O que ele prova é que a forma do ARX (quadros,
journal global, vista do núcleo, laço de reconciliação) não altera nada do que T1–T9
garantem, desde que as hipóteses valham.
-/

namespace Aletheia.Arx

open Aletheia Lab Budget

/-! ## O journal do ARX (o fragmento) -/

/-- Os códigos de falha que o ARX desenrola e que a saga captura. -/
inductive Code where
  /-- `R0600`: o provedor recusou a chamada com chave (`tool_failed`) -/
  | refusedNow (k : Nat)
  /-- `R0611`: a reconciliação confirmou que a operação não foi aplicada -/
  | refusedRec (k : Nat)
  /-- `R0910`: a pós-condição não foi observada -/
  | probe (q : Nat)
  /-- erros de programa (`R01xx`, `R05xx`, `R08xx`), que o T1 exclui -/
  | other
  deriving DecidableEq, Repr

/-- O `Unwind` do ARX: uma falha com código ou o encerramento pelo tratador. -/
inductive AU where
  | err (c : Code)
  | abort (v : Val)

/-- Os eventos do journal completo do ARX, os do fragmento. -/
inductive AEv where
  /-- `tool {tool, args: [cap, v, key?], class, key?}` -/
  | tool (t id : Nat) (v : Val) (key : Option Nat) (compensable : Bool)
  /-- `tool` sem capacidade: a sonda `q` e o seu resultado -/
  | probe (q : Nat) (ok : Bool)
  | toolFailed (t k : Nat)
  | timeoutUnknown (t k : Nat)
  | reconcile (k i : Nat) (r : Option Reply)
  | resolved (k : Nat) (done : Bool)
  | exhausted (k : Nat)
  | transfer (k : Nat)
  | endorse (p : Nat) (v : Val)
  | refineExhausted (p : Nat)
  | awaitTimeout
  | sagaCommit (n : Nat)
  | sagaAbort (u : AU) (n : Nat)
  | compensate (t : Nat) (key : Option Nat) (ok : Bool)
  | sagaRecovered
  | compensationFailed
  | mint (k : Nat)

/-- Uma operação no quadro da saga: a ferramenta, o argumento e a chave (se houver). -/
abbrev SOp := Nat × Val × Option Nat

/-- O estado do ARX: o mundo, o medidor, o índice do oráculo, o contador de chaves, a pilha de
quadros de saga (o mais interno à frente; cada quadro com a operação mais recente à frente) e
o journal, só de acréscimos. -/
structure AS where
  w : World
  spent : Budget
  calls : Nat
  next : Nat
  frames : List (List SOp)
  jr : List AEv

def AS.push (σ : AS) (e : AEv) : AS := { σ with jr := σ.jr ++ [e] }

/-- `self.sagas.last_mut().push(op)`: só dentro de uma saga, e só se a ferramenta é
`compensable`. -/
def pushOp (S : Sig) (op : SOp) (fs : List (List SOp)) : List (List SOp) :=
  if S.compensable op.1 then
    match fs with
    | top :: rest => (op :: top) :: rest
    | [] => []
  else fs

/-- O estado depois de uma chamada aplicada: o mundo, o medidor e o quadro. -/
def AS.step (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) : AS :=
  { σ with w := (if b then S.step t v σ.w else σ.w), spent := σ.spent + S.cost t,
           frames := pushOp S (t, v, ko) σ.frames }

@[simp] theorem AS.step_jr (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) :
    (σ.step S t v ko b).jr = σ.jr := rfl
@[simp] theorem AS.step_frames (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) :
    (σ.step S t v ko b).frames = pushOp S (t, v, ko) σ.frames := rfl
@[simp] theorem AS.step_calls (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) :
    (σ.step S t v ko b).calls = σ.calls := rfl
@[simp] theorem AS.step_next (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) :
    (σ.step S t v ko b).next = σ.next := rfl
@[simp] theorem AS.step_spent (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) :
    (σ.step S t v ko b).spent = σ.spent + S.cost t := rfl
@[simp] theorem AS.step_w (S : Sig) (t : Nat) (v : Val) (ko : Option Nat) (b : Bool) (σ : AS) :
    (σ.step S t v ko b).w = if b then S.step t v σ.w else σ.w := rfl
@[simp] theorem AS.push_jr (σ : AS) (e : AEv) : (σ.push e).jr = σ.jr ++ [e] := rfl
@[simp] theorem AS.push_frames (σ : AS) (e : AEv) : (σ.push e).frames = σ.frames := rfl
@[simp] theorem AS.push_w (σ : AS) (e : AEv) : (σ.push e).w = σ.w := rfl
@[simp] theorem AS.push_spent (σ : AS) (e : AEv) : (σ.push e).spent = σ.spent := rfl
@[simp] theorem AS.push_calls (σ : AS) (e : AEv) : (σ.push e).calls = σ.calls := rfl
@[simp] theorem AS.push_next (σ : AS) (e : AEv) : (σ.push e).next = σ.next := rfl

/-- O desfecho de um passo do ARX: o valor ou o `Unwind`, e o estado (que persiste nos dois). -/
abbrev Res := Except AU Val × AS

def seqA (r : Res) (k : Val → AS → Res) : Res :=
  match r with
  | (.ok v, σ) => k v σ
  | (.error u, σ) => (.error u, σ)


section
variable (S : Sig) (c : Cfg) (cc : Nat → Budget)

/-- O laço de reconciliação do ARX: `for attempt in 1..=fuel` com `break`. O estado é o
journal até aqui e o desfecho: `none` continua; `some o` parou, com `o = some b` (resolvida,
`b` = aplicada) ou `o = none` (provedor `opaque`). -/
def recStep (t k : Nat) (acc : List AEv × Option (Option Bool)) (i : Nat) :
    List AEv × Option (Option Bool) :=
  match acc with
  | (evs, some o) => (evs, some o)
  | (evs, none) =>
    if S.prov t = .opaque then (evs ++ [.reconcile k i none], some none)
    else
      match S.reply t k i with
      | .done => (evs ++ [.reconcile k i (some .done)], some (some true))
      | .failed => (evs ++ [.reconcile k i (some .failed)], some (some false))
      | .unknown => (evs ++ [.reconcile k i (some .unknown)], none)

/-- O desfecho de quem parou: resolvida (`some b`) ou sem desfecho. -/
def collapse : Option (Option Bool) → Option Bool
  | some (some b) => some b
  | _ => none

def recA (t k fuel : Nat) : List AEv × Option Bool :=
  let r := (List.range' 1 fuel).foldl (recStep S t k) ([], none)
  (r.1, collapse r.2)

/-- As compensações do quadro, da mais recente à mais antiga: cada uma chama o compensador e
escreve `compensate`; o medidor liquida o custo do compensador (`cc`) quando ele responde. -/
def compA : List SOp → AS → Bool × AS
  | [], σ => (true, σ)
  | (t, v, ko) :: rest, σ =>
    match S.undo t v σ.w with
    | some w' => compA rest ({ σ with w := w', spent := σ.spent + cc t }.push (.compensate t ko true))
    | none => ((compA rest (σ.push (.compensate t ko false))).2 |> fun σ' => (false, σ'))

/-- O laço `for`. -/
def loopA (f : Val → AS → Res) : List Val → AS → Res
  | [], σ => (.ok .unit, σ)
  | v :: vs, σ => seqA (f v σ) fun _ σ' => loopA f vs σ'

/-- O estado do ARX depois de `timeout_unknown` e das tentativas da reconciliação. -/
def pendSt (S : Sig) (σ : AS) (t : Nat) (v : Val) (kid n : Nat) : AS :=
  let σ₁ := (σ.step S t v (some kid) (S.applied t kid)).push (.timeoutUnknown t kid)
  { σ₁ with jr := σ₁.jr ++ (recA S t kid n).1 }

/-- O modelo do ARX: a mesma árvore de programa, a máquina do Rust. -/
def run : Exp → List Val → AS → Res
  | .val v, _, σ => (.ok v, σ)
  | .var k, ρ, σ =>
    match ρ[k]? with
    | some v => (.ok v, σ)
    | none => (.error (.err .other), σ)
  | .let_ a b, ρ, σ => seqA (run a ρ σ) fun v σ => run b (v :: ρ) σ
  | .ite x t f, ρ, σ =>
    seqA (run x ρ σ) fun v σ =>
      match v with
      | .bool true => run t ρ σ
      | .bool false => run f ρ σ
      | _ => (.error (.err .other), σ)
  | .prim op a b, ρ, σ =>
    seqA (run a ρ σ) fun x σ =>
    seqA (run b ρ σ) fun y σ =>
      match x, y with
      | .int m, .int n => (.ok (op.apply m n), σ)
      | _, _ => (.error (.err .other), σ)
  -- `refine e by p else d`: `endorse` ou `refine_exhausted`, depois o `else`
  | .endorse p e d, ρ, σ =>
    seqA (run e ρ σ) fun v σ =>
      if S.pol p v then (.ok v, σ.push (.endorse p v)) else run d ρ (σ.push (.refineExhausted p))
  -- `call_tool`: a capacidade, a reserva no medidor, o host, o quadro da saga, o journal
  | .act t k a, ρ, σ =>
    seqA (run a ρ σ) fun v σ =>
      match ρ[k]? with
      | some (.cap id) =>
        if S.effect t ∈ c.grants id then
          if ble (σ.spent + S.cost t) c.limit then
            (.ok .unit, (σ.step S t v none true).push (.tool t id v none (S.compensable t)))
          else (.error (.err .other), σ)
        else (.error (.err .other), σ)
      | _ => (.error (.err .other), σ)
  | .drop k, ρ, σ =>
    match ρ[k]? with
    | some (.cap _) => (.ok .unit, σ)
    | _ => (.error (.err .other), σ)
  | .for_ _ xs body, ρ, σ =>
    seqA (run xs ρ σ) fun v σ =>
      match v with
      | .list vs => loopA (fun x σ => run body (x :: ρ) σ) vs σ
      | _ => (.error (.err .other), σ)
  | .infer m t a, ρ, σ =>
    seqA (run a ρ σ) fun v σ =>
      if ble (σ.spent + S.mcost m) c.limit then
        (.ok (c.oracle σ.calls m t v), { σ with spent := σ.spent + S.mcost m, calls := σ.calls + 1 })
      else (.error (.err .other), σ)
  -- `await_cond`: a sonda; se falsa, `await_timeout` e `R0910`
  | .await q, _, σ =>
    let σ₁ := σ.push (.probe q (S.probe q σ.w))
    if S.probe q σ.w then (.ok .unit, σ₁) else (.error (.err (.probe q)), σ₁.push .awaitTimeout)
  -- `atomic`: empilha um quadro, roda o corpo, desempilha; compensa o quadro, sonda e escolhe
  | .atomic body r f g, ρ, σ =>
    match run body ρ { σ with frames := [] :: σ.frames } with
    | (.ok v, σ₁) =>
      (.ok v, { σ₁ with frames := σ₁.frames.tail }.push (.sagaCommit (σ₁.frames.headD []).length))
    | (.error u, σ₁) =>
      let ops := σ₁.frames.headD []
      let σ₂ := { σ₁ with frames := σ₁.frames.tail }.push (.sagaAbort u ops.length)
      let cr := compA S cc ops σ₂
      let p := S.probe r cr.2.w
      let σ₄ := cr.2.push (.probe r p)
      if cr.1 && p then
        match u with
        | .abort v => (.error (.abort v), σ₄.push .sagaRecovered)
        | .err _ => run f ρ (σ₄.push .sagaRecovered)
      else run g ρ (σ₄.push .compensationFailed)
  | .call t j κ a n h, ρ, σ =>
    seqA (run a ρ σ) fun v σ =>
      match ρ[j]?, ρ[κ]? with
      | some (.cap id), some (.cap kid) =>
        if S.effect t ∈ c.grants id then
          if ble (σ.spent + S.cost t) c.limit then
            match S.reply t kid 0 with
            | .done =>
              (.ok .unit, (σ.step S t v (some kid) true).push (.tool t id v (some kid) (S.compensable t)))
            -- a recusa não liquida o medidor (só a reserva foi conferida)
            | .failed => (.error (.err (.refusedNow kid)), σ.push (.toolFailed t kid))
            | .unknown =>
              let σ₂ := pendSt S σ t v kid n
              match (recA S t kid n).2 with
              | some true => (.ok .unit, σ₂.push (.resolved kid true))
              | some false => (.error (.err (.refusedRec kid)), σ₂.push (.resolved kid false))
              | none =>
                match run h [.pend t kid] (σ₂.push (.exhausted kid)) with
                | (.ok w, σ₃) => (.error (.abort w), σ₃)
                | (.error (.abort w), σ₃) => (.error (.abort w), σ₃)
                | (.error u, σ₃) => (.error u, σ₃)
          else (.error (.err .other), σ)
        else (.error (.err .other), σ)
      | _, _ => (.error (.err .other), σ)
  -- `keys.mint()`: o contador e o evento `mint`
  | .mint e, ρ, σ => run e (.cap σ.next :: ρ) ({ σ with next := σ.next + 1 }.push (.mint σ.next))
  | .transfer p, ρ, σ =>
    match ρ[p]? with
    | some (.pend _ k) => (.ok .unit, σ.push (.transfer k))
    | _ => (.error (.err .other), σ)

end

/-! ## A vista do núcleo -/

/-- O estado da vista: a pilha reconstruída de argumentos das compensações (a mais recente à
frente), a sonda corrente e a chave da última recusa. -/
structure CV where
  comp : List (List Val)
  probe : Nat × Bool
  refused : Nat

def CV.init : CV := ⟨[], (0, false), 0⟩

/-- `Journal::core_view_with`, um evento por vez. -/
def cvStep (S : Sig) (cv : CV) : AEv → CV × List Ev
  | .tool t id v key comp =>
    ({ cv with comp := if comp then [v] :: cv.comp else cv.comp },
      .act t id v :: (match key with | some k => [.settle k .done] | none => []))
  | .probe q ok => ({ cv with probe := (q, ok) }, [])
  | .toolFailed _ k => ({ cv with refused := k }, [.settle k .failed])
  | .timeoutUnknown t k =>
    ({ cv with comp := if S.compensable t then [] :: cv.comp else cv.comp }, [.pending t k .unit])
  | .reconcile k i r => (cv, [.query k i r])
  | .resolved k true => (cv, [.settle k .done])
  | .resolved k false => ({ cv with refused := k }, [.settle k .failed])
  | .exhausted _ => (cv, [])
  | .transfer k => (cv, [.handoff k, .settle k .transferred])
  | .endorse p v => (cv, [.endorse p (some v)])
  | .refineExhausted p => (cv, [.endorse p none])
  | .awaitTimeout => (cv, [.fail (.probe cv.probe.1)])
  | .sagaCommit n => ({ cv with comp := cv.comp.drop n }, [])
  | .sagaAbort u _ =>
    (cv, match u with
      | .err (.refusedNow _) | .err (.refusedRec _) => [.fail (.refused cv.refused)]
      | .abort _ => [.fail (.abort .unit)]
      | .err _ => [])
  | .compensate t key ok =>
    ({ cv with comp := cv.comp.tail },
      [match key with
        | some _ => .undo t .unit ok
        | none => .undo t ((cv.comp.headD []).headD .unit) ok])
  | .sagaRecovered | .compensationFailed => (cv, [.recover cv.probe.1 cv.probe.2])
  | .mint k => (cv, [.mint k])

def cvAcc (S : Sig) (acc : CV × List Ev) (e : AEv) : CV × List Ev :=
  ((cvStep S acc.1 e).1, acc.2 ++ (cvStep S acc.1 e).2)

def cvRun (S : Sig) (J : List AEv) : CV × List Ev := J.foldl (cvAcc S) (CV.init, [])

@[simp] theorem cvRun_push (S : Sig) (J : List AEv) (e : AEv) :
    cvRun S (J ++ [e]) = cvAcc S (cvRun S J) e := by
  simp [cvRun, List.foldl_append]

theorem cvRun_append (S : Sig) (J Δ : List AEv) :
    cvRun S (J ++ Δ) = Δ.foldl (cvAcc S) (cvRun S J) := by
  simp [cvRun, List.foldl_append]

/-! ## A normalização: o que o journal do ARX não registra -/

/-- Uma ferramenta é `kd` se tem chave (`own IdemKey`). -/
def nv (kd : Nat → Bool) : Ev → List Ev
  | .pending t k _ => [.pending t k .unit]
  | .undo t v ok => [if kd t then .undo t .unit ok else .undo t v ok]
  | .fail (.abort _) => [.fail (.abort .unit)]
  | .handoff k => [.handoff k, .settle k .transferred]
  | .settle _ .transferred => []
  | e => [e]

def nvs (kd : Nat → Bool) (tr : List Ev) : List Ev := tr.flatMap (nv kd)

@[simp] theorem nvs_nil (kd : Nat → Bool) : nvs kd [] = [] := rfl
@[simp] theorem nvs_append (kd : Nat → Bool) (a b : List Ev) : nvs kd (a ++ b) = nvs kd a ++ nvs kd b := by
  simp [nvs, List.flatMap_append]
@[simp] theorem nvs_cons (kd : Nat → Bool) (e : Ev) (l : List Ev) : nvs kd (e :: l) = nv kd e ++ nvs kd l := by
  simp [nvs]

/-- A falha de pós-condição já está na vista quando a saga a captura (`await_timeout`). -/
def failEv : Cause → List Ev
  | .probe q => [.fail (.probe q)]
  | _ => []

/-! ## O laço de reconciliação do ARX é a reconciliação do λ_Ath -/

/-- As tentativas do λ_Ath como eventos do journal do ARX. -/
def qA : Ev → AEv
  | .query k i r => .reconcile k i r
  | _ => .exhausted 0

/-- O estado em que o laço do ARX para, na forma da recursão de `reconcile`. -/
def recS (S : Sig) (t k : Nat) : Nat → Nat → Option (Option Bool)
  | _, 0 => none
  | i, n + 1 =>
    if S.prov t = .opaque then some none
    else
      match S.reply t k i with
      | .done => some (some true)
      | .failed => some (some false)
      | .unknown => recS S t k (i + 1) n

theorem recStep_stopped (S : Sig) (t k : Nat) (evs : List AEv) (o : Option Bool) :
    ∀ l : List Nat, l.foldl (recStep S t k) (evs, some o) = (evs, some o)
  | [] => rfl
  | _ :: l => by simp only [List.foldl, recStep]; exact recStep_stopped S t k evs o l

theorem recA_fold (S : Sig) (t k : Nat) :
    ∀ (i n : Nat) (evs : List AEv), (List.range' i n).foldl (recStep S t k) (evs, none) =
      (evs ++ (reconcile S t k i n).1.map qA, recS S t k i n)
  | i, 0, evs => by simp [reconcile, recS]
  | i, n + 1, evs => by
    rw [List.range'_succ, List.foldl_cons]
    simp only [recStep, reconcile, recS]
    by_cases ho : S.prov t = .opaque
    · simp only [ho, if_true]
      rw [recStep_stopped]; simp [qA]
    · simp only [ho, if_false]
      cases hr : S.reply t k i with
      | done => simp only []; rw [recStep_stopped]; simp [qA]
      | failed => simp only []; rw [recStep_stopped]; simp [qA]
      | unknown =>
        simp only []
        rw [recA_fold S t k (i + 1) n]
        simp [qA]

/-- O desfecho do laço: `Done`, `Failed` ou o combustível esgotado (inclusive pelo provedor
`opaque`, que para o laço sem desfecho), como `reconcile` decide. -/
def endOf : KEnd → Option Bool
  | .done => some true
  | .failed => some false
  | .transferred => none

theorem recS_end (S : Sig) (t k : Nat) :
    ∀ i n, collapse (recS S t k i n) = endOf (reconcile S t k i n).2
  | i, 0 => by simp [recS, reconcile, collapse, endOf]
  | i, n + 1 => by
    simp only [recS, reconcile]
    by_cases ho : S.prov t = .opaque
    · simp [ho, collapse, endOf]
    · simp only [ho, if_false]
      cases hr : S.reply t k i with
      | done => rfl
      | failed => rfl
      | unknown => exact recS_end S t k (i + 1) n

theorem recA_eq (S : Sig) (t k n : Nat) :
    recA S t k n = ((reconcile S t k 1 n).1.map qA, endOf (reconcile S t k 1 n).2) := by
  unfold recA
  rw [recA_fold]
  simp only [List.nil_append]
  rw [← recS_end]

/-- A vista do núcleo das tentativas do ARX são as tentativas do λ_Ath. -/
theorem cv_queries (S : Sig) (t k n : Nat) :
    ∀ (acc : CV × List Ev), ((reconcile S t k 1 n).1.map qA).foldl (cvAcc S) acc =
      (acc.1, acc.2 ++ (reconcile S t k 1 n).1) := by
  have hq := reconcile_queries S t k 1 n
  generalize (reconcile S t k 1 n).1 = l at hq ⊢
  induction l with
  | nil => intro acc; simp
  | cons e l ih =>
    intro acc
    obtain ⟨j, r, rfl⟩ := hq e (List.mem_cons_self ..)
    simp only [List.map_cons, List.foldl_cons, qA, cvAcc, cvStep]
    rw [ih (fun x hx => hq x (List.mem_cons_of_mem _ hx))]
    simp

theorem nvs_queries (kd : Nat → Bool) {l : List Ev} {k : Nat} (h : ∀ e ∈ l, ∃ j r, e = .query k j r) :
    nvs kd l = l := by
  induction l with
  | nil => rfl
  | cons e l ih =>
    obtain ⟨j, r, rfl⟩ := h e (List.mem_cons_self ..)
    simp [nv, ih (fun x hx => h x (List.mem_cons_of_mem _ hx))]

/-! ## A relação de simulação -/

/-- O quadro do topo do ARX é o registro da saga do λ_Ath, com a chave quando a ferramenta é
`kd`. -/
def FrameOk (kd : Nat → Bool) (top : List SOp) (comp : List (Nat × Val)) : Prop :=
  top.map (fun o => (o.1, o.2.1)) = comp ∧ ∀ o ∈ top, o.2.2.isSome = kd o.1

/-- A pilha reconstruída pela vista concorda com o registro do λ_Ath: o argumento de uma
operação sem chave; qualquer coisa para uma com chave (a vista não o usa). -/
inductive Agree (kd : Nat → Bool) : List (List Val) → List (Nat × Val) → Prop where
  | nil : Agree kd [] []
  | cons {a : List Val} {X : List (List Val)} {t : Nat} {v : Val} {c : List (Nat × Val)} :
      (kd t = true ∨ a = [v]) → Agree kd X c → Agree kd (a :: X) ((t, v) :: c)

theorem Agree.length {kd : Nat → Bool} {X : List (List Val)} {c : List (Nat × Val)}
    (h : Agree kd X c) : X.length = c.length := by
  induction h with
  | nil => rfl
  | cons _ _ ih => simp [ih]

/-- O estado do λ_Ath e o do ARX: o mesmo mundo, o mesmo oráculo, o mesmo contador; o gasto do
ARX não passa do contabilizado; o quadro do topo é o registro. -/
structure Rel (kd : Nat → Bool) (s : St) (σ : AS) : Prop where
  w : σ.w = s.w
  spent : σ.spent ≤ s.spent
  calls : σ.calls = s.calls
  next : σ.next = s.next
  top : ∀ top rest, σ.frames = top :: rest → FrameOk kd top s.comp

/-- A pós-condição da simulação: a relação no fim, a pilha de quadros de fora intacta, a pilha
da vista com o mesmo fundo `P`, e a vista com `out` acrescentado. -/
structure Post (S : Sig) (kd : Nat → Bool) (s' : St) (σ σ' : AS) (P : List (List Val))
    (out : List Ev) : Prop where
  rel : Rel kd s' σ'
  flen : σ'.frames.length = σ.frames.length
  ftail : σ'.frames.tail = σ.frames.tail
  cv : ∃ X', Agree kd X' s'.comp ∧ (cvRun S σ'.jr).1.comp = X' ++ P
  out : (cvRun S σ'.jr).2 = (cvRun S σ.jr).2 ++ out

theorem Post.refl {S : Sig} {kd : Nat → Bool} {s : St} {σ : AS} {X P : List (List Val)}
    (hr : Rel kd s σ) (hX : Agree kd X s.comp) (hc : (cvRun S σ.jr).1.comp = X ++ P) :
    Post S kd s σ σ P [] :=
  ⟨hr, rfl, rfl, ⟨X, hX, hc⟩, by simp⟩

theorem Post.trans {S : Sig} {kd : Nat → Bool} {s₁ s₂ : St} {σ σ₁ σ₂ : AS} {P : List (List Val)}
    {o₁ o₂ : List Ev} (h₁ : Post S kd s₁ σ σ₁ P o₁) (h₂ : Post S kd s₂ σ₁ σ₂ P o₂) :
    Post S kd s₂ σ σ₂ P (o₁ ++ o₂) :=
  ⟨h₂.rel, h₂.flen.trans h₁.flen, h₂.ftail.trans h₁.ftail, h₂.cv, by rw [h₂.out, h₁.out]; simp⟩

theorem Rel.push {kd : Nat → Bool} {s : St} {σ : AS} (h : Rel kd s σ) (e : AEv) : Rel kd s (σ.push e) :=
  ⟨h.w, h.spent, h.calls, h.next, h.top⟩

/-- Um evento que não mexe na pilha da vista. -/
theorem Post.push {S : Sig} {kd : Nat → Bool} {s : St} {σ : AS} {X P : List (List Val)} {e : AEv}
    (hr : Rel kd s σ) (hX : Agree kd X s.comp) (hc : (cvRun S σ.jr).1.comp = X ++ P)
    (he : (cvStep S (cvRun S σ.jr).1 e).1.comp = (cvRun S σ.jr).1.comp) :
    Post S kd s σ (σ.push e) P (cvStep S (cvRun S σ.jr).1 e).2 :=
  ⟨hr.push e, rfl, rfl, ⟨X, hX, by simp [AS.push, cvAcc, he, hc]⟩, by simp [AS.push, cvAcc]⟩

/-- Como o desenrolar corresponde à causa da falha. -/
def UQ : Cause → AU → Prop
  | .refused k, .err (.refusedNow k') => k = k'
  | .refused k, .err (.refusedRec k') => k = k'
  | .abort v, .abort v' => v = v'
  | .probe q, .err (.probe q') => q = q'
  | _, _ => False

/-- O que a vista precisa saber de uma falha para nomeá-la quando a saga a capturar. -/
def CauseCV : Cause → CV → Prop
  | .refused k, cv => cv.refused = k
  | _, _ => True

/-- As ferramentas `kd` só rodam por `call`, as outras só por `act`. -/
def kdOk (kd : Nat → Bool) : Exp → Bool
  | .act t _ a => !kd t && kdOk kd a
  | .call t _ _ a _ h => kd t && kdOk kd a && kdOk kd h
  | .let_ a b | .prim _ a b => kdOk kd a && kdOk kd b
  | .ite x t f => kdOk kd x && kdOk kd t && kdOk kd f
  | .endorse _ e d => kdOk kd e && kdOk kd d
  | .for_ _ xs b => kdOk kd xs && kdOk kd b
  | .infer _ _ a => kdOk kd a
  | .atomic b _ f g => kdOk kd b && kdOk kd f && kdOk kd g
  | .mint e => kdOk kd e
  | .val _ | .var _ | .drop _ | .await _ | .transfer _ => true

/-! ## As compensações do quadro são as do registro -/

theorem budget_add_zero (b : Budget) : b + (fun _ => 0 : Nat → Budget) 0 = b := by
  funext d; simp

/-- Com compensadores gratuitos, compensar o quadro do topo é compensar o registro: o mesmo
mundo, o mesmo desfecho, o medidor parado, e a vista emite as mesmas compensações (módulo
`nv`), desempilhando exatamente a parte da saga. -/
theorem compA_sim (S : Sig) (kd : Nat → Bool) (P : List (List Val)) :
    ∀ (top : List SOp) (comp : List (Nat × Val)) (X : List (List Val)) (σ : AS),
      FrameOk kd top comp → Agree kd X comp → (cvRun S σ.jr).1.comp = X ++ P →
      (compA S (fun _ => 0) top σ).1 = (compensate S comp σ.w).ok ∧
      (compA S (fun _ => 0) top σ).2.w = (compensate S comp σ.w).w ∧
      (compA S (fun _ => 0) top σ).2.spent = σ.spent ∧
      (compA S (fun _ => 0) top σ).2.calls = σ.calls ∧
      (compA S (fun _ => 0) top σ).2.next = σ.next ∧
      (compA S (fun _ => 0) top σ).2.frames = σ.frames ∧
      (cvRun S (compA S (fun _ => 0) top σ).2.jr).1 = { (cvRun S σ.jr).1 with comp := P } ∧
      (cvRun S (compA S (fun _ => 0) top σ).2.jr).2 =
        (cvRun S σ.jr).2 ++ nvs kd (compensate S comp σ.w).evs
  | [], comp, X, σ, ⟨hm, _⟩, hX, hc => by
    simp only [List.map_nil] at hm; subst hm
    cases hX
    simp only [List.nil_append] at hc
    simp [compA, compensate, ← hc]
  | (t, v, ko) :: top, comp, X, σ, ⟨hm, hk⟩, hX, hc => by
    simp only [List.map_cons] at hm; subst hm
    cases hX with
    | @cons a X' _ _ _ ha hX' =>
    have hkt : ko.isSome = kd t := hk (t, v, ko) (List.mem_cons_self ..)
    have hF : FrameOk kd top (top.map fun o => (o.1, o.2.1)) :=
      ⟨rfl, fun o ho => hk o (List.mem_cons_of_mem _ ho)⟩
    -- o evento de compensação desempilha `a` e emite o `undo` normalizado
    have hund : ∀ (b : Bool) (σ₀ : AS), (cvRun S σ₀.jr).1 = (cvRun S σ.jr).1 →
        (cvRun S (σ₀.push (.compensate t ko b)).jr).1 = { (cvRun S σ.jr).1 with comp := X' ++ P } ∧
        (cvRun S (σ₀.push (.compensate t ko b)).jr).2 = (cvRun S σ₀.jr).2 ++ nv kd (.undo t v b) := by
      intro b σ₀ h₀
      simp only [AS.push, cvRun_push, cvAcc, cvStep, h₀, hc, List.cons_append, List.tail_cons]
      refine ⟨trivial, ?_⟩
      cases ko with
      | none =>
        have : kd t = false := by simpa using hkt.symm
        rcases ha with ha | rfl
        · rw [this] at ha; cases ha
        · simp [nv, this]
      | some k =>
        have : kd t = true := by simpa using hkt.symm
        simp [nv, this]
    simp only [compA, compensate]
    cases hu : S.undo t v σ.w with
    | some w' =>
      simp only []
      obtain ⟨h₁, h₂⟩ := hund true { σ with w := w', spent := σ.spent + (fun _ => 0 : Nat → Budget) t } rfl
      obtain ⟨r1, r2, r3, r4, r5, r6, r7, r8⟩ := compA_sim S kd P top _ X' _ hF hX' (by rw [h₁])
      refine ⟨r1, r2, ?_, r4, r5, r6, ?_, ?_⟩
      · rw [r3]; simp [AS.push]; funext d; simp
      · rw [r7, h₁]
      · rw [r8, h₂]; simp [AS.push, nvs]
    | none =>
      simp only []
      obtain ⟨h₁, h₂⟩ := hund false σ rfl
      obtain ⟨r1, r2, r3, r4, r5, r6, r7, r8⟩ := compA_sim S kd P top _ X' _ hF hX' (by rw [h₁])
      refine ⟨trivial, r2, r3, r4, r5, r6, ?_, ?_⟩
      · rw [r7, h₁]
      · rw [r8, h₂]; simp [AS.push, nvs]

/-! ## O teorema de simulação -/

theorem ble_mono {a b c L : Budget} (h : a ≤ b) (hb : ble (b + c) L = true) : ble (a + c) L = true := by
  rw [Budget.ble_iff] at hb ⊢
  intro d; have := hb d; have := h d; simp at *; omega

theorem add_le_add {a b : Budget} (c : Budget) (h : a ≤ b) : a + c ≤ b + c := by
  intro d; have := h d; simp; omega

/-- Empilhar uma operação no quadro preserva a forma da pilha. -/
theorem pushOp_length (S : Sig) (op : SOp) (fs : List (List SOp)) : (pushOp S op fs).length = fs.length := by
  unfold pushOp; split
  · split <;> rfl
  · rfl

theorem pushOp_tail (S : Sig) (op : SOp) (fs : List (List SOp)) : (pushOp S op fs).tail = fs.tail := by
  unfold pushOp; split
  · split <;> rfl
  · rfl

/-- O quadro do topo depois de uma operação registrada é o registro depois de `logAct`. -/
theorem pushOp_top {S : Sig} {kd : Nat → Bool} {t : Nat} {v : Val} {ko : Option Nat} {fs : List (List SOp)}
    {comp : List (Nat × Val)} (hk : ko.isSome = kd t)
    (h : ∀ top rest, fs = top :: rest → FrameOk kd top comp) :
    ∀ top rest, pushOp S (t, v, ko) fs = top :: rest → FrameOk kd top (logAct S t v comp) := by
  intro top rest he
  unfold pushOp at he
  by_cases hc : S.compensable t = true
  · simp only [hc, if_true] at he
    cases fs with
    | nil => cases he
    | cons top₀ rest₀ =>
      obtain ⟨hm, hks⟩ := h top₀ rest₀ rfl
      cases he
      refine ⟨by simp [hm, logAct, hc], fun o ho => ?_⟩
      rcases List.mem_cons.mp ho with rfl | ho
      · exact hk
      · exact hks o ho
  · simp only [hc, Bool.false_eq_true, if_false] at he
    have := h top rest he
    simpa [logAct, hc] using this

/-- A pilha da vista depois de um evento `tool` ou `timeout_unknown` de uma operação registrada. -/
theorem agree_log {kd : Nat → Bool} {S : Sig} {t : Nat} {v : Val} {a : List Val} {X : List (List Val)}
    {comp : List (Nat × Val)} (hX : Agree kd X comp) (ha : kd t = true ∨ a = [v]) :
    Agree kd (if S.compensable t then a :: X else X) (logAct S t v comp) := by
  unfold logAct
  by_cases hc : S.compensable t = true
  · simp only [hc, if_true]; exact .cons ha hX
  · simp only [hc, Bool.false_eq_true, if_false]; exact hX

/-- O enunciado da simulação para uma expressão. -/
def SimT (S : Sig) (c : Cfg) (kd : Nat → Bool) (e : Exp) : Prop :=
  ∀ (ρ : List Val) (s : St) (σ : AS) (X P : List (List Val)),
    Rel kd s σ → Agree kd X s.comp → (cvRun S σ.jr).1.comp = X ++ P →
    (∀ o, eval S c e ρ s = .ok o →
      ∃ σ', run S c (fun _ => 0) e ρ σ = (.ok o.v, σ') ∧ Post S kd o.st σ σ' P (nvs kd o.tr)) ∧
    (∀ q s₁ tr, eval S c e ρ s = .error (.timeout q s₁ tr) →
      ∃ u σ', run S c (fun _ => 0) e ρ σ = (.error u, σ') ∧ UQ q u ∧ CauseCV q (cvRun S σ'.jr).1 ∧
        Post S kd s₁ σ σ' P (nvs kd tr ++ failEv q))

section
variable {S : Sig} {c : Cfg} {kd : Nat → Bool}

/-- A composição sequencial: uma continuação depois de uma parte que terminou com valor. -/
theorem seq_ok {e : Exp} {ρ : List Val} {s : St} {σ : AS} {X P : List (List Val)}
    (ih : SimT S c kd e) (hr : Rel kd s σ) (hX : Agree kd X s.comp) (hc : (cvRun S σ.jr).1.comp = X ++ P)
    {o : Out} (he : eval S c e ρ s = .ok o) :
    ∃ σ₁ X₁, run S c (fun _ => 0) e ρ σ = (.ok o.v, σ₁) ∧ Post S kd o.st σ σ₁ P (nvs kd o.tr) ∧
      Rel kd o.st σ₁ ∧ Agree kd X₁ o.st.comp ∧ (cvRun S σ₁.jr).1.comp = X₁ ++ P := by
  obtain ⟨σ₁, h₁, hp⟩ := (ih ρ s σ X P hr hX hc).1 o he
  obtain ⟨X₁, hX₁, hc₁⟩ := hp.cv
  exact ⟨σ₁, X₁, h₁, hp, hp.rel, hX₁, hc₁⟩

theorem sim_val (v : Val) : SimT S c kd (.val v) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp [eval] at he; subst he; exact ⟨σ, rfl, Post.refl hr hX hc⟩
  · simp [eval] at he

theorem sim_var (k : Nat) : SimT S c kd (.var k) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval] at he
    split at he
    · rename_i v hv
      cases he
      exact ⟨σ, by simp [run, hv], Post.refl hr hX hc⟩
    · cases he
  · simp only [eval] at he; split at he <;> cases he

theorem sim_let {a b : Exp} (iha : SimT S c kd a) (ihb : SimT S c kd b) : SimT S c kd (.let_ a b) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
    cases he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
    obtain ⟨σ₂, r₂, p₂⟩ := (ihb _ _ σ₁ X₁ P hr₁ hX₁ hc₁).1 o₂ h₂
    exact ⟨σ₂, by simp [run, seqA, r₁, r₂], by simpa using p₁.trans p₂⟩
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (iha ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
      · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
        obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ihb _ _ σ₁ X₁ P hr₁ hX₁ hc₁).2 q s₁ t h₃
        exact ⟨u, σ', by simp [run, seqA, r₁, r], hu, hcv, by simpa [HasTr.tr] using p₁.trans hp⟩
      · cases h₄

theorem sim_ite {x t f : Exp} (ihx : SimT S c kd x) (iht : SimT S c kd t) (ihf : SimT S c kd f) :
    SimT S c kd (.ite x t f) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨ox, hx, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok ihx hr hX hc hx
    split at he
    · rename_i b hb
      obtain ⟨o', h', he⟩ := bnd_eq_ok.mp he
      cases he
      cases b
      · obtain ⟨σ₂, r₂, p₂⟩ := (ihf ρ _ σ₁ X₁ P hr₁ hX₁ hc₁).1 o' h'
        exact ⟨σ₂, by simp [run, seqA, r₁, hb, r₂], by simpa using p₁.trans p₂⟩
      · obtain ⟨σ₂, r₂, p₂⟩ := (iht ρ _ σ₁ X₁ P hr₁ hX₁ hc₁).1 o' h'
        exact ⟨σ₂, by simp [run, seqA, r₁, hb, r₂], by simpa using p₁.trans p₂⟩
    · cases he
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨ox, t', hx, h₁, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ihx ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok ihx hr hX hc hx
      revert h₁; split
      · rename_i b hb
        intro h₁
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · cases b
          · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ihf ρ _ σ₁ X₁ P hr₁ hX₁ hc₁).2 q s₁ t' h₂
            exact ⟨u, σ', by simp [run, seqA, r₁, hb, r], hu, hcv, by simpa [HasTr.tr] using p₁.trans hp⟩
          · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (iht ρ _ σ₁ X₁ P hr₁ hX₁ hc₁).2 q s₁ t' h₂
            exact ⟨u, σ', by simp [run, seqA, r₁, hb, r], hu, hcv, by simpa [HasTr.tr] using p₁.trans hp⟩
        · cases h₃
      · intro h₁; cases h₁

theorem sim_prim {op : Op} {a b : Exp} (iha : SimT S c kd a) (ihb : SimT S c kd b) :
    SimT S c kd (.prim op a b) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
    obtain ⟨σ₂, r₂, p₂⟩ := (ihb ρ _ σ₁ X₁ P hr₁ hX₁ hc₁).1 o₂ h₂
    split at he
    · rename_i x y hx hy
      cases he
      exact ⟨σ₂, by simp [run, seqA, r₁, r₂, hx, hy], by simpa using p₁.trans p₂⟩
    · cases he
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (iha ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
      rcases bnd_eq_timeout h₂ with h₃ | ⟨o₂, _, h₃, h₄, _⟩
      · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ihb ρ _ σ₁ X₁ P hr₁ hX₁ hc₁).2 q s₁ t h₃
        exact ⟨u, σ', by simp [run, seqA, r₁, r], hu, hcv, by simpa [HasTr.tr] using p₁.trans hp⟩
      · revert h₄; split <;> intro h₄ <;> cases h₄

theorem sim_endorse {p : Nat} {e d : Exp} (ihe : SimT S c kd e) (ihd : SimT S c kd d) :
    SimT S c kd (.endorse p e d) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨oe, h₁, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok ihe hr hX hc h₁
    by_cases hp : S.pol p oe.v = true
    · rw [if_pos hp] at he; cases he
      refine ⟨σ₁.push (.endorse p oe.v), by simp [run, seqA, r₁, hp], ?_⟩
      have := p₁.trans (Post.push (e := .endorse p oe.v) hr₁ hX₁ hc₁ rfl)
      simpa [cvStep, nv] using this
    · rw [if_neg hp] at he
      obtain ⟨o', h₂, he⟩ := bnd_eq_ok.mp (pre_eq_ok.mp he)
      cases he
      have pe := Post.push (e := .refineExhausted p) hr₁ hX₁ hc₁ rfl
      obtain ⟨σ₂, r₂, p₂⟩ := (ihd ρ _ (σ₁.push (.refineExhausted p)) X₁ P (hr₁.push _) hX₁ (by simp [AS.push, cvAcc, cvStep, hc₁])).1 o' h₂
      refine ⟨σ₂, by simp [run, seqA, r₁, hp, r₂], ?_⟩
      have := (p₁.trans pe).trans p₂
      simpa [cvStep, nv] using this
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨oe, t, h₁, h₂, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ihe ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok ihe hr hX hc h₁
      by_cases hp : S.pol p oe.v = true
      · rw [if_pos hp] at h₂; cases h₂
      · rw [if_neg hp] at h₂
        obtain ⟨t', h₃, rfl⟩ := pre_eq_timeout h₂
        rcases bnd_eq_timeout h₃ with h₄ | ⟨_, _, _, h₅, _⟩
        · have pe := Post.push (e := .refineExhausted p) hr₁ hX₁ hc₁ rfl
          obtain ⟨u, σ', r, hu, hcv, hp'⟩ :=
            (ihd ρ _ (σ₁.push (.refineExhausted p)) X₁ P (hr₁.push _) hX₁ (by simp [AS.push, cvAcc, cvStep, hc₁])).2 q s₁ _ h₄
          refine ⟨u, σ', by simp [run, seqA, r₁, hp, r], hu, hcv, ?_⟩
          have := (p₁.trans pe).trans hp'
          simpa [cvStep, nv, HasTr.tr] using this
        · cases h₅

theorem sim_act {t k : Nat} {a : Exp} (hk : kd t = false) (iha : SimT S c kd a) :
    SimT S c kd (.act t k a) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨oa, h₁, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
    obtain ⟨id, hid, hg, _, hb, rfl⟩ := doAct_ok he
    have hb' := ble_mono hr₁.spent hb
    refine ⟨(σ₁.step S t oa.v none true).push (.tool t id oa.v none (S.compensable t)),
      by simp [run, seqA, r₁, hid, hg, hb'], ?_⟩
    have h2 : Post S kd ⟨id :: oa.st.used, oa.st.spent + S.cost t, oa.st.calls, actW S t id oa.v oa.st.w,
        actC S t id oa.v oa.st.comp, oa.st.next, oa.st.keys⟩ σ₁
        ((σ₁.step S t oa.v none true).push (.tool t id oa.v none (S.compensable t))) P [.act t id oa.v] := by
      refine ⟨⟨?_, ?_, hr₁.calls, hr₁.next, ?_⟩, ?_, ?_, ?_, ?_⟩
      · simp [AS.push, AS.step, actW, hr₁.w]
      · exact add_le_add _ hr₁.spent
      · exact pushOp_top (by simp [hk]) hr₁.top
      · simp [AS.push, AS.step, pushOp_length]
      · simp [AS.push, AS.step, pushOp_tail]
      · exact ⟨_, agree_log (S := S) hX₁ (.inr rfl), by
          simp only [AS.push_jr, AS.step_jr, cvRun_push, cvAcc, cvStep, hc₁, actC]; split <;> rfl⟩
      · simp [cvAcc, cvStep]
    simpa [actEvs, nv] using p₁.trans h2
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨oa, t', h₁, h₂, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (iha ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · exact absurd h₂ doAct_ne_timeout

theorem sim_drop (k : Nat) : SimT S c kd (.drop k) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval] at he
    obtain ⟨id, hid, _, rfl⟩ := doDrop_ok he
    exact ⟨σ, by simp [run, hid], Post.refl ⟨hr.w, hr.spent, hr.calls, hr.next, hr.top⟩ hX hc⟩
  · simp only [eval] at he; exact absurd he doDrop_ne_timeout

theorem sim_infer {m : Nat} {ty : Ty} {a : Exp} (iha : SimT S c kd a) : SimT S c kd (.infer m ty a) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨oa, h₁, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
    obtain ⟨hb, rfl⟩ := doInfer_ok he
    have hb' := ble_mono hr₁.spent hb
    let σ₂ : AS := { σ₁ with spent := σ₁.spent + S.mcost m, calls := σ₁.calls + 1 }
    refine ⟨σ₂, by simp [σ₂, run, seqA, r₁, hb', hr₁.calls], ?_⟩
    have h2 : Post S kd ⟨oa.st.used, oa.st.spent + S.mcost m, oa.st.calls + 1, oa.st.w, oa.st.comp, oa.st.next, oa.st.keys⟩ σ₁ σ₂ P [] :=
      ⟨⟨hr₁.w, add_le_add _ hr₁.spent, by simp [σ₂, hr₁.calls], hr₁.next, hr₁.top⟩, rfl, rfl, ⟨X₁, hX₁, hc₁⟩, by simp [σ₂]⟩
    simpa using p₁.trans h2
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨oa, t', h₁, h₂, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (iha ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · exact absurd h₂ doInfer_ne_timeout

theorem sim_await (q₀ : Nat) : SimT S c kd (.await q₀) := by
  intro ρ s σ X P hr hX hc
  have hpp := Post.push (e := .probe q₀ (S.probe q₀ σ.w)) hr hX hc rfl
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval] at he
    split at he
    · rename_i hp
      cases he
      refine ⟨σ.push (.probe q₀ (S.probe q₀ σ.w)), by simp [run, hr.w, hp], ?_⟩
      simpa [cvStep] using hpp
    · cases he
  · simp only [eval] at he
    split at he
    · cases he
    · rename_i hp
      cases he
      refine ⟨.err (.probe q₀), (σ.push (.probe q₀ (S.probe q₀ σ.w))).push .awaitTimeout,
        by simp [run, hr.w, hp], rfl, trivial, ?_⟩
      have := hpp.trans (Post.push (e := .awaitTimeout) (hr.push _) hpp.cv.choose_spec.1
        hpp.cv.choose_spec.2 rfl)
      simpa [cvStep, AS.push, cvAcc, failEv, hr.w, hp] using this

theorem sim_transfer (p : Nat) : SimT S c kd (.transfer p) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval] at he
    split at he
    · rename_i t k hp
      cases he
      refine ⟨σ.push (.transfer k), by simp [run, hp], ?_⟩
      simpa [cvStep, nv] using Post.push (e := .transfer k) hr hX hc rfl
    · cases he
  · simp only [eval] at he; split at he <;> cases he

theorem sim_mint {e : Exp} (ih : SimT S c kd e) : SimT S c kd (.mint e) := by
  intro ρ s σ X P hr hX hc
  let σ₀ : AS := { σ with next := σ.next + 1 }.push (.mint σ.next)
  have hr₀ : Rel kd { s with next := s.next + 1 } σ₀ :=
    ⟨hr.w, hr.spent, hr.calls, by simp [σ₀, AS.push, hr.next], hr.top⟩
  have hc₀ : (cvRun S σ₀.jr).1.comp = X ++ P := by simp [σ₀, AS.push, cvAcc, cvStep, hc]
  have hout : (cvRun S σ₀.jr).2 = (cvRun S σ.jr).2 ++ [.mint s.next] := by
    simp [σ₀, AS.push, cvAcc, cvStep, hr.next]
  have hrun : ∀ ρ', run S c (fun _ => 0) (.mint e) ρ' σ = run S c (fun _ => 0) e (.cap s.next :: ρ') σ₀ := by
    intro ρ'; simp [run, σ₀, hr.next]
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · obtain ⟨_, _, o', h', rfl⟩ := eval_mint_ok he
    obtain ⟨σ', r, hp⟩ := (ih _ _ σ₀ X P hr₀ hX hc₀).1 o' h'
    refine ⟨σ', by rw [hrun, r], ⟨⟨hp.rel.w, hp.rel.spent, hp.rel.calls, hp.rel.next, hp.rel.top⟩,
      hp.flen, hp.ftail, hp.cv, ?_⟩⟩
    rw [hp.out, hout]; simp [nv]
  · obtain ⟨_, _, s', tr', h', rfl, rfl⟩ := eval_mint_timeout he
    obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ih _ _ σ₀ X P hr₀ hX hc₀).2 q s' tr' h'
    refine ⟨u, σ', by rw [hrun, r], hu, hcv, ⟨⟨hp.rel.w, hp.rel.spent, hp.rel.calls, hp.rel.next, hp.rel.top⟩,
      hp.flen, hp.ftail, hp.cv, ?_⟩⟩
    rw [hp.out, hout]; simp [nv]

theorem sim_loop {body : Exp} {ρ : List Val} (ih : SimT S c kd body) :
    ∀ (vs : List Val) (s : St) (σ : AS) (X P : List (List Val)),
      Rel kd s σ → Agree kd X s.comp → (cvRun S σ.jr).1.comp = X ++ P →
      (∀ r, loop (fun v s' => eval S c body (v :: ρ) s') vs s = .ok r →
        ∃ σ', loopA (fun x σ => run S c (fun _ => 0) body (x :: ρ) σ) vs σ = (.ok .unit, σ') ∧
          Post S kd r.1 σ σ' P (nvs kd r.2)) ∧
      (∀ q s₁ tr, loop (fun v s' => eval S c body (v :: ρ) s') vs s = .error (.timeout q s₁ tr) →
        ∃ u σ', loopA (fun x σ => run S c (fun _ => 0) body (x :: ρ) σ) vs σ = (.error u, σ') ∧
          UQ q u ∧ CauseCV q (cvRun S σ'.jr).1 ∧ Post S kd s₁ σ σ' P (nvs kd tr ++ failEv q))
  | [], s, σ, X, P, hr, hX, hc => by
    refine ⟨fun r he => ?_, fun q s₁ tr he => ?_⟩
    · simp [loop] at he; subst he; exact ⟨σ, rfl, Post.refl hr hX hc⟩
    · simp [loop] at he
  | v :: vs, s, σ, X, P, hr, hX, hc => by
    refine ⟨fun r he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [loop, bnd_eq_ok] at he
      obtain ⟨o, ho, r', hr', he⟩ := he
      cases he
      obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok (ρ := v :: ρ) ih hr hX hc ho
      obtain ⟨σ₂, r₂, p₂⟩ := (sim_loop ih vs _ σ₁ X₁ P hr₁ hX₁ hc₁).1 r' hr'
      exact ⟨σ₂, by simp [loopA, seqA, r₁, r₂], by simpa using p₁.trans p₂⟩
    · simp only [loop] at he
      rcases bnd_eq_timeout he with h₀ | ⟨o, t, ho, h₁, rfl⟩
      · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ih (v :: ρ) s σ X P hr hX hc).2 q s₁ tr h₀
        exact ⟨u, σ', by simp [loopA, seqA, r], hu, hcv, hp⟩
      · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok (ρ := v :: ρ) ih hr hX hc ho
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (sim_loop ih vs _ σ₁ X₁ P hr₁ hX₁ hc₁).2 q s₁ t h₂
          exact ⟨u, σ', by simp [loopA, seqA, r₁, r], hu, hcv, by simpa [HasTr.tr] using p₁.trans hp⟩
        · cases h₃

theorem sim_for {n : Nat} {xs body : Exp} (ihx : SimT S c kd xs) (ihb : SimT S c kd body) :
    SimT S c kd (.for_ n xs body) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨ox, hx, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok ihx hr hX hc hx
    split at he
    · rename_i vs hvs
      obtain ⟨r, hl, he⟩ := bnd_eq_ok.mp he
      cases he
      obtain ⟨σ₂, r₂, p₂⟩ := (sim_loop (ρ := ρ) ihb vs _ σ₁ X₁ P hr₁ hX₁ hc₁).1 r hl
      exact ⟨σ₂, by simp [run, seqA, r₁, hvs, r₂], by simpa using p₁.trans p₂⟩
    · cases he
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨ox, t, hx, h₁, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (ihx ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok ihx hr hX hc hx
      revert h₁; split
      · rename_i vs hvs
        intro h₁
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (sim_loop (ρ := ρ) ihb vs _ σ₁ X₁ P hr₁ hX₁ hc₁).2 q s₁ t h₂
          exact ⟨u, σ', by simp [run, seqA, r₁, hvs, r], hu, hcv, by simpa [HasTr.tr] using p₁.trans hp⟩
        · cases h₃
      · intro h₁; cases h₁

theorem le_add_right' {a b : Budget} (c : Budget) (h : a ≤ b) : a ≤ b + c := by
  intro d; have := h d; simp; omega

/-- Os eventos da chamada até a reconciliação: `timeout_unknown` e as tentativas. -/
theorem call_pending {t id kid n : Nat} {v : Val} {σ : AS} {o : St} {X P : List (List Val)}
    (hkd : kd t = true) (hr : Rel kd o σ) (hX : Agree kd X o.comp) (hc : (cvRun S σ.jr).1.comp = X ++ P) :
    Post S kd (callSt S t id kid v o (S.applied t kid)) σ (pendSt S σ t v kid n) P
        (.pending t kid .unit :: (reconcile S t kid 1 n).1) ∧
      (cvRun S (pendSt S σ t v kid n).jr).1 =
        { (cvRun S σ.jr).1 with comp := (cvRun S (pendSt S σ t v kid n).jr).1.comp } := by
  let σ₁ := (σ.step S t v (some kid) (S.applied t kid)).push (.timeoutUnknown t kid)
  let σ₂ : AS := pendSt S σ t v kid n
  show Post S kd _ σ σ₂ P _ ∧ (cvRun S σ₂.jr).1 = { (cvRun S σ.jr).1 with comp := (cvRun S σ₂.jr).1.comp }
  have hj : σ₂.jr = σ.jr ++ [.timeoutUnknown t kid] ++ (reconcile S t kid 1 n).1.map qA := by
    simp [σ₂, pendSt, recA_eq]
  have hcv : cvRun S σ₂.jr = ({ (cvRun S σ.jr).1 with
      comp := if S.compensable t then [] :: (cvRun S σ.jr).1.comp else (cvRun S σ.jr).1.comp },
      (cvRun S σ.jr).2 ++ [.pending t kid .unit] ++ (reconcile S t kid 1 n).1) := by
    rw [hj, cvRun_append, cv_queries]; simp [cvAcc, cvStep]
  refine ⟨⟨⟨?_, ?_, hr.calls, hr.next, ?_⟩, ?_, ?_, ?_, ?_⟩, by rw [hcv]⟩
  · simp [σ₂, pendSt, callSt, hr.w]
  · simp only [σ₂, pendSt, AS.push_spent, AS.step_spent, callSt]; exact add_le_add _ hr.spent
  · simpa [σ₂, pendSt, callSt] using pushOp_top (S := S) (v := v) (ko := some kid) (by simp [hkd]) hr.top
  · simp [σ₂, pendSt, pushOp_length]
  · simp [σ₂, pendSt, pushOp_tail]
  · refine ⟨_, agree_log (S := S) (a := []) hX (.inl hkd), ?_⟩
    rw [hcv]; simp only [hc]; split <;> rfl
  · rw [hcv]; simp

theorem sim_call {t j κ n : Nat} {a h : Exp} (hkd : kd t = true) (iha : SimT S c kd a)
    (ihh : SimT S c kd h) : SimT S c kd (.call t j κ a n h) := by
  intro ρ s σ X P hr hX hc
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · simp only [eval, bnd_eq_ok] at he
    obtain ⟨oa, h₁, he⟩ := he
    obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
    obtain ⟨id, kid, hid, hkid, hg, _, _, _, hb, hcs⟩ := doCall_ok he
    have hb' := ble_mono hr₁.spent hb
    rcases hcs with ⟨h0, rfl⟩ | ⟨h0, hrc, rfl⟩
    · -- `Done` na hora: o evento `tool` com a chave
      refine ⟨(σ₁.step S t oa.v (some kid) true).push (.tool t id oa.v (some kid) (S.compensable t)),
        by simp [run, seqA, r₁, hid, hkid, hg, hb', h0], ?_⟩
      have h2 : Post S kd (callSt S t id kid oa.v oa.st true) σ₁
          ((σ₁.step S t oa.v (some kid) true).push (.tool t id oa.v (some kid) (S.compensable t))) P
          [.act t id oa.v, .settle kid .done] := by
        refine ⟨⟨?_, ?_, hr₁.calls, hr₁.next, ?_⟩, ?_, ?_, ?_, ?_⟩
        · simp [callSt, hr₁.w]
        · simp only [AS.push_spent, AS.step_spent, callSt]; exact add_le_add _ hr₁.spent
        · simpa [callSt] using pushOp_top (S := S) (v := oa.v) (ko := some kid) (by simp [hkd]) hr₁.top
        · simp [pushOp_length]
        · simp [pushOp_tail]
        · exact ⟨_, agree_log (S := S) hX₁ (.inr rfl), by
            simp only [AS.push_jr, AS.step_jr, cvRun_push, cvAcc, cvStep, hc₁, callSt]; split <;> rfl⟩
        · simp [cvAcc, cvStep]
      simpa [nv] using p₁.trans h2
    · -- `TimeoutUnknown` e a reconciliação confirma: `Done`
      obtain ⟨pp, hcv⟩ := call_pending (S := S) (kd := kd) (id := id) (kid := kid) (n := n) (v := oa.v) hkd hr₁ hX₁ hc₁
      refine ⟨(pendSt S σ₁ t oa.v kid n).push (.resolved kid true),
        by simp [run, seqA, r₁, hid, hkid, hg, hb', h0, recA_eq, hrc, endOf], ?_⟩
      obtain ⟨X₂, hX₂, hc₂⟩ := pp.cv
      have h3 := Post.push (e := .resolved kid true) pp.rel hX₂ hc₂ (by simp [cvStep])
      have := (p₁.trans pp).trans h3
      simpa [cvStep, nv, nvs_queries kd (reconcile_queries S t kid 1 n)] using this
  · simp only [eval] at he
    rcases bnd_eq_timeout he with h₀ | ⟨oa, t', h₁, h₂, rfl⟩
    · obtain ⟨u, σ', r, hu, hcv, hp⟩ := (iha ρ s σ X P hr hX hc).2 q s₁ tr h₀
      exact ⟨u, σ', by simp [run, seqA, r], hu, hcv, hp⟩
    · obtain ⟨σ₁, X₁, r₁, p₁, hr₁, hX₁, hc₁⟩ := seq_ok iha hr hX hc h₁
      obtain ⟨id, kid, hid, hkid, hg, _, _, _, hb, hcs⟩ := doCall_timeout h₂
      have hb' := ble_mono hr₁.spent hb
      rcases hcs with ⟨h0, rfl, rfl, rfl⟩ | ⟨h0, hrc, rfl, rfl, rfl⟩ | ⟨h0, hrc, oh, hoh, _, rfl, rfl, rfl⟩
      · -- recusa na hora: `tool_failed`; o ARX não liquida o medidor
        refine ⟨.err (.refusedNow kid), σ₁.push (.toolFailed t kid),
          by simp [run, seqA, r₁, hid, hkid, hg, hb', h0], rfl, by simp [CauseCV, cvAcc, cvStep], ?_⟩
        have h2 : Post S kd (callSt0 S t id kid oa.st) σ₁ (σ₁.push (.toolFailed t kid)) P [.settle kid .failed] :=
          ⟨⟨by simp [callSt0, hr₁.w], by simpa [callSt0] using le_add_right' (S.cost t) hr₁.spent,
            by simp [callSt0, hr₁.calls], by simp [callSt0, hr₁.next], by simpa [callSt0] using hr₁.top⟩,
            rfl, rfl, ⟨X₁, by simpa [callSt0] using hX₁, by simp [cvAcc, cvStep, hc₁]⟩, by simp [cvAcc, cvStep]⟩
        simpa [nv, failEv, HasTr.tr] using p₁.trans h2
      · -- a reconciliação confirma que não foi aplicada: `R0611`
        obtain ⟨pp, hcv⟩ := call_pending (S := S) (kd := kd) (id := id) (kid := kid) (n := n) (v := oa.v) hkd hr₁ hX₁ hc₁
        obtain ⟨X₂, hX₂, hc₂⟩ := pp.cv
        refine ⟨.err (.refusedRec kid), (pendSt S σ₁ t oa.v kid n).push (.resolved kid false),
          by simp [run, seqA, r₁, hid, hkid, hg, hb', h0, recA_eq, hrc, endOf],
          rfl, by simp [CauseCV, cvAcc, cvStep], ?_⟩
        have h3 := Post.push (e := .resolved kid false) pp.rel hX₂ hc₂ (by simp [cvStep])
        have := (p₁.trans pp).trans h3
        simpa [cvStep, nv, failEv, HasTr.tr, nvs_queries kd (reconcile_queries S t kid 1 n)] using this
      · -- combustível esgotado: o tratador entrega a obrigação e encerra o evento
        obtain ⟨pp, hcv⟩ := call_pending (S := S) (kd := kd) (id := id) (kid := kid) (n := n) (v := oa.v) hkd hr₁ hX₁ hc₁
        obtain ⟨X₂, hX₂, hc₂⟩ := pp.cv
        have h3 := Post.push (e := .exhausted kid) pp.rel hX₂ hc₂ (by simp [cvStep])
        obtain ⟨X₃, hX₃, hc₃⟩ := h3.cv
        obtain ⟨σ₄, r₄, p₄⟩ := (ihh [.pend t kid] _ _ X₃ P h3.rel hX₃ hc₃).1 oh hoh
        refine ⟨.abort oh.v, σ₄, by simp [run, seqA, r₁, hid, hkid, hg, hb', h0, recA_eq, hrc, endOf, r₄],
          rfl, trivial, ?_⟩
        have := ((p₁.trans pp).trans h3).trans p₄
        simpa [cvStep, nv, failEv, HasTr.tr, nvs_queries kd (reconcile_queries S t kid 1 n)] using this

/-- O `fail` da saga: emitido na hora (`await_timeout`) para a pós-condição, no `saga_abort`
para as outras causas. -/
def abortOut (cv : CV) : AU → List Ev
  | .err (.refusedNow _) | .err (.refusedRec _) => [.fail (.refused cv.refused)]
  | .abort _ => [.fail (.abort .unit)]
  | .err _ => []

theorem fail_split {q : Cause} {u : AU} {cv : CV} (hu : UQ q u) (hcv : CauseCV q cv) :
    failEv q ++ abortOut cv u = nv kd (.fail q) := by
  cases q with
  | refused k =>
    simp only [CauseCV] at hcv
    cases u with
    | err c => cases c <;> simp_all [UQ, abortOut, failEv, nv]
    | abort _ => simp [UQ] at hu
  | abort v => cases u with
    | err c => cases c <;> simp [UQ] at hu
    | abort _ => simp [abortOut, failEv, nv]
  | probe p => cases u with
    | err c => cases c <;> simp_all [UQ, abortOut, failEv, nv]
    | abort _ => simp [UQ] at hu

theorem UQ.isAbort {q : Cause} {u : AU} (hu : UQ q u) :
    (q.isAbort = true ∧ ∃ v, u = .abort v ∧ q = .abort v) ∨ (q.isAbort = false ∧ ∃ c, u = .err c) := by
  cases q <;> cases u <;> simp_all [UQ, Cause.isAbort]
  all_goals (rename_i c; cases c <;> simp_all)

/-- O ARX numa saga cujo corpo desenrolou: as compensações, a sonda e a escolha. -/
theorem run_atomic_err (cc : Nat → Budget) {body f g : Exp} {r : Nat} {ρ : List Val} {σ σ₁ : AS} {u : AU}
    (h : run S c cc body ρ { σ with frames := [] :: σ.frames } = (.error u, σ₁)) :
    run S c cc (.atomic body r f g) ρ σ =
      let cr := compA S cc (σ₁.frames.headD [])
        ({ σ₁ with frames := σ₁.frames.tail }.push (.sagaAbort u (σ₁.frames.headD []).length))
      let p := S.probe r cr.2.w
      if cr.1 && p then
        match u with
        | .abort v => (.error (.abort v), (cr.2.push (.probe r p)).push .sagaRecovered)
        | .err _ => run S c cc f ρ ((cr.2.push (.probe r p)).push .sagaRecovered)
      else run S c cc g ρ ((cr.2.push (.probe r p)).push .compensationFailed) := by
  simp only [run, h]
  cases u <;> rfl

theorem run_atomic_ok (cc : Nat → Budget) {body f g : Exp} {r : Nat} {ρ : List Val} {σ σ₁ : AS} {v : Val}
    (h : run S c cc body ρ { σ with frames := [] :: σ.frames } = (.ok v, σ₁)) :
    run S c cc (.atomic body r f g) ρ σ =
      (.ok v, { σ₁ with frames := σ₁.frames.tail }.push (.sagaCommit (σ₁.frames.headD []).length)) := by
  simp only [run, h]

theorem sim_atomic {body f g : Exp} {r : Nat} (ihb : SimT S c kd body) (ihf : SimT S c kd f)
    (ihg : SimT S c kd g) : SimT S c kd (.atomic body r f g) := by
  intro ρ s σ X P hr hX hc
  -- a saga empilha um quadro vazio e começa o registro do zero
  let σ₀ : AS := { σ with frames := [] :: σ.frames }
  have hr₀ : Rel kd { s with comp := [] } σ₀ :=
    ⟨hr.w, hr.spent, hr.calls, hr.next, fun top rest h => by
      simp only [σ₀, List.cons.injEq] at h; obtain ⟨rfl, _⟩ := h; exact ⟨rfl, by simp⟩⟩
  have hc₀ : (cvRun S σ₀.jr).1.comp = [] ++ (X ++ P) := by simp [σ₀, hc]
  -- o quadro do corpo, no fim: o topo da pilha, sobre os quadros de fora
  have frame : ∀ {s' : St} {σ₁ : AS} {out : List Ev}, Post S kd s' σ₀ σ₁ (X ++ P) out →
      ∃ top, σ₁.frames = top :: σ.frames ∧ FrameOk kd top s'.comp := by
    intro s' σ₁ out hp
    have hl := hp.flen; have ht := hp.ftail
    simp only [σ₀, List.length_cons, List.tail_cons] at hl ht
    cases h₁ : σ₁.frames with
    | nil => rw [h₁] at hl; simp at hl
    | cons top rest =>
      rw [h₁] at ht; simp at ht; subst ht
      exact ⟨top, rfl, hp.rel.top top _ h₁⟩
  -- o corpo falhou: compensa, sonda e escolhe; vale para os dois desfechos da saga
  have abortCase : ∀ q' s₁' tr', eval S c body ρ { s with comp := [] } = .error (.timeout q' s₁' tr') →
      (∀ o, eval S c (.atomic body r f g) ρ s = .ok o →
        ∃ σ', run S c (fun _ => 0) (.atomic body r f g) ρ σ = (.ok o.v, σ') ∧ Post S kd o.st σ σ' P (nvs kd o.tr)) ∧
      (∀ q s₁ tr, eval S c (.atomic body r f g) ρ s = .error (.timeout q s₁ tr) →
        ∃ u σ', run S c (fun _ => 0) (.atomic body r f g) ρ σ = (.error u, σ') ∧ UQ q u ∧
          CauseCV q (cvRun S σ'.jr).1 ∧ Post S kd s₁ σ σ' P (nvs kd tr ++ failEv q)) := by
    intro q' s₁' tr' hb
    obtain ⟨u, σ₁, r₁, hu, hcv, p₁⟩ := (ihb ρ _ σ₀ [] (X ++ P) hr₀ .nil hc₀).2 q' s₁' tr' hb
    obtain ⟨top, hf, hfo⟩ := frame p₁
    obtain ⟨X₁, hX₁, hc₁⟩ := p₁.cv
    -- `saga_abort`
    let σ₂ : AS := { σ₁ with frames := σ₁.frames.tail }.push (.sagaAbort u top.length)
    have hc₂ : (cvRun S σ₂.jr).1.comp = X₁ ++ (X ++ P) := by simp [σ₂, cvAcc, cvStep, hc₁]
    have ho₂ : (cvRun S σ₂.jr).2 = (cvRun S σ.jr).2 ++ nvs kd tr' ++ nv kd (.fail q') := by
      have hs : (cvStep S (cvRun S σ₁.jr).1 (.sagaAbort u top.length)).2 = abortOut (cvRun S σ₁.jr).1 u := by
        cases u with
        | err c => cases c <;> rfl
        | abort _ => rfl
      simp only [σ₂, AS.push_jr, cvRun_push, cvAcc, hs, p₁.out]
      rw [← fail_split hu hcv]
      simp [σ₀, List.append_assoc]
    -- as compensações do quadro
    obtain ⟨k1, k2, k3, k4, k5, k6, k7, k8⟩ := compA_sim S kd (X ++ P) top s₁'.comp X₁ σ₂ hfo hX₁ hc₂
    have hw₂ : σ₂.w = s₁'.w := by simp [σ₂, p₁.rel.w]
    rw [hw₂] at k1 k2 k8
    let K := compensate S s₁'.comp s₁'.w
    let σ₃ := (compA S (fun _ => 0) top σ₂).2
    let p := S.probe r K.w
    have hp3 : S.probe r σ₃.w = p := by simp [σ₃, p, k2, K]
    have hrun₀ := run_atomic_err (S := S) (c := c) (r := r) (f := f) (g := g) (fun _ => 0) (σ := σ) r₁
    rw [hf] at hrun₀
    simp only [List.headD_cons, List.tail_cons] at hrun₀
    have hσ₂ : ({ σ₁ with frames := σ.frames } : AS).push (.sagaAbort u top.length) = σ₂ := by
      simp [σ₂, hf]
    rw [hσ₂, k1, hp3] at hrun₀
    -- o estado depois da sonda de recuperação
    let s₂ : St := { s₁' with w := K.w, comp := s.comp }
    let evs := tr' ++ .fail q' :: K.evs ++ [.recover r p]
    have k78 : cvRun S σ₃.jr = ({ (cvRun S σ₂.jr).1 with comp := X ++ P }, (cvRun S σ₂.jr).2 ++ nvs kd K.evs) :=
      Prod.ext k7 k8
    have hfr : σ₃.frames = σ.frames := by simp only [σ₃]; rw [k6]; simp [σ₂, hf]
    have post4 : ∀ (e : AEv), (e = .sagaRecovered ∨ e = .compensationFailed) →
        Post S kd s₂ σ ((σ₃.push (.probe r p)).push e) P (nvs kd evs) := by
      intro e he'
      have hcv4 : cvRun S ((σ₃.push (.probe r p)).push e).jr =
          ({ (cvRun S σ₂.jr).1 with comp := X ++ P, probe := (r, p) },
            (cvRun S σ₂.jr).2 ++ nvs kd K.evs ++ [.recover r p]) := by
        simp only [AS.push_jr, List.append_assoc, List.singleton_append]
        rw [cvRun_append, k78]
        rcases he' with rfl | rfl <;> simp [cvAcc, cvStep]
      refine ⟨⟨?_, ?_, ?_, ?_, ?_⟩, ?_, ?_, ⟨X, hX, by rw [hcv4]⟩, ?_⟩
      · simp only [AS.push_w, σ₃]; rw [k2]
      · simp only [AS.push_spent, σ₃]; rw [k3]; simpa [σ₂, s₂] using p₁.rel.spent
      · simp only [AS.push_calls, σ₃]; rw [k4]; simpa [σ₂, s₂] using p₁.rel.calls
      · simp only [AS.push_next, σ₃]; rw [k5]; simpa [σ₂, s₂] using p₁.rel.next
      · intro top' rest' h'
        simp only [AS.push_frames, hfr] at h'
        exact hr.top top' rest' h'
      · simp [hfr]
      · simp [hfr]
      · rw [hcv4, ho₂]; simp [evs, nv, List.append_assoc]
    have hfin : ∀ (hd : Exp), SimT S c kd hd → ∀ (e : AEv), (e = .sagaRecovered ∨ e = .compensationFailed) →
        (∀ o : Out, (pre evs (bnd (eval S c hd ρ s₂) fun o' =>
            (.ok ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, evs ++ o'.tr⟩ : Except Err Out))) = .ok o →
          ∃ σ', run S c (fun _ => 0) hd ρ ((σ₃.push (.probe r p)).push e) = (.ok o.v, σ') ∧
            Post S kd o.st σ σ' P (nvs kd o.tr)) ∧
        (∀ (q : Cause) (s₁ : St) (tr : List Ev), (pre evs (bnd (eval S c hd ρ s₂) fun o' =>
            (.ok ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, evs ++ o'.tr⟩ : Except Err Out))) = .error (.timeout q s₁ tr) →
          ∃ u σ', run S c (fun _ => 0) hd ρ ((σ₃.push (.probe r p)).push e) = (.error u, σ') ∧ UQ q u ∧
            CauseCV q (cvRun S σ'.jr).1 ∧ Post S kd s₁ σ σ' P (nvs kd tr ++ failEv q)) := by
      intro hd ih e he'
      have p4 := post4 e he'
      obtain ⟨X₄, hX₄, hc₄⟩ := p4.cv
      refine ⟨fun o h => ?_, fun q s₁ tr h => ?_⟩
      · obtain ⟨o', h', h⟩ := bnd_eq_ok.mp (pre_eq_ok.mp h)
        cases h
        obtain ⟨σ', r', p'⟩ := (ih ρ s₂ _ X₄ P p4.rel hX₄ hc₄).1 o' h'
        refine ⟨σ', r', ?_⟩
        have := p4.trans p'
        exact ⟨⟨by simpa [exitSaga] using this.rel.w, by simpa [exitSaga] using this.rel.spent,
          by simpa [exitSaga] using this.rel.calls, by simpa [exitSaga] using this.rel.next,
          by simpa [exitSaga] using this.rel.top⟩, this.flen, this.ftail,
          by simpa [exitSaga] using this.cv, by simpa using this.out⟩
      · obtain ⟨t, h', rfl⟩ := pre_eq_timeout h
        rcases bnd_eq_timeout h' with h'' | ⟨_, _, _, h₃, _⟩
        · obtain ⟨u', σ', r', hu', hcv', p'⟩ := (ih ρ s₂ _ X₄ P p4.rel hX₄ hc₄).2 q s₁ t h''
          exact ⟨u', σ', r', hu', hcv', by simpa using p4.trans p'⟩
        · cases h₃
    -- a escolha: o encerramento atravessa a saga recuperada; senão roda `f` ou `g`
    have hev : ∀ b : Bool, (q'.isAbort && (K.ok && p)) = false → (K.ok && p) = b →
        eval S c (.atomic body r f g) ρ s =
          pre evs (bnd (eval S c (cond b f g) ρ s₂) fun o' =>
            .ok ⟨o'.v, exitSaga ρ (.atomic body r f g) o'.st, evs ++ o'.tr⟩) := by
      intro b hn hb'
      rw [eval_atomic_fail hb hn]; rw [hb']
    rcases hu.isAbort with ⟨hab, v, rfl, rfl⟩ | ⟨hab, c', rfl⟩
    · by_cases hy : (K.ok && p) = true
      · have he := eval_atomic_reraise (r := r) (f := f) (g := g) hb (by rw [hab, Bool.true_and]; exact hy)
        refine ⟨fun o h => (by rw [he] at h; cases h), fun q s₁ tr h => ?_⟩
        rw [he] at h
        simp only [Except.error.injEq, Err.timeout.injEq] at h
        obtain ⟨rfl, rfl, rfl⟩ := h
        refine ⟨.abort v, _, by rw [hrun₀, if_pos hy], rfl, trivial, ?_⟩
        simpa [evs, s₂, failEv] using post4 .sagaRecovered (.inl rfl)
      · have hy' : (K.ok && p) = false := by simpa using hy
        rw [hev false (by simp [hy']) hy']
        rw [hrun₀, if_neg hy]
        exact hfin g ihg .compensationFailed (.inr rfl)
    · by_cases hy : (K.ok && p) = true
      · rw [hev true (by simp [hab]) hy, hrun₀, if_pos hy]
        exact hfin f ihf .sagaRecovered (.inl rfl)
      · have hy' : (K.ok && p) = false := by simpa using hy
        rw [hev false (by simp [hab]) hy', hrun₀, if_neg hy]
        exact hfin g ihg .compensationFailed (.inr rfl)
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · have he' := he
    simp only [eval] at he'
    split at he'
    · rename_i ob hb
      cases he'
      obtain ⟨σ₁, r₁, p₁⟩ := (ihb ρ _ σ₀ [] (X ++ P) hr₀ .nil hc₀).1 ob hb
      obtain ⟨top, hf, hfo⟩ := frame p₁
      obtain ⟨X₁, hX₁, hc₁⟩ := p₁.cv
      have hlen : top.length = X₁.length := by
        rw [hX₁.length, ← hfo.1]; simp
      refine ⟨{ σ₁ with frames := σ₁.frames.tail }.push (.sagaCommit (σ₁.frames.headD []).length),
        run_atomic_ok (fun _ => 0) r₁, ?_⟩
      refine ⟨⟨by simp [exitSaga, p₁.rel.w], by simpa [exitSaga] using p₁.rel.spent,
        by simp [exitSaga, p₁.rel.calls], by simp [exitSaga, p₁.rel.next], ?_⟩, ?_, ?_, ?_, ?_⟩
      · intro top' rest' h'; simp [hf] at h'; simpa [exitSaga] using hr.top top' rest' h'
      · simp [hf]
      · simp [hf]
      · exact ⟨X, by simpa [exitSaga] using hX, by simp [cvAcc, cvStep, hf, hc₁, hlen]⟩
      · simp [cvAcc, cvStep, p₁.out, σ₀]
    · rename_i q' s₁' tr' hb
      exact (abortCase q' s₁' tr' hb).1 o he
    · cases he'
  · have he' := he
    simp only [eval] at he'
    split at he'
    · cases he'
    · rename_i q' s₁' tr' hb
      exact (abortCase q' s₁' tr' hb).2 q s₁ tr he
    · rename_i hne _
      cases he'
      exact absurd rfl (hne _ _ _)

/-- **O teorema de simulação.** Toda execução de `eval` que termina com valor ou com falha
capturável é realizada pelo modelo do ARX, para qualquer programa em que as ferramentas com
chave só rodem por `call`, com compensadores gratuitos. -/
theorem sim (e : Exp) (hk : kdOk kd e = true) : SimT S c kd e := by
  induction e with
  | val v => exact sim_val v
  | var k => exact sim_var k
  | let_ a b iha ihb =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_let (iha hk.1) (ihb hk.2)
  | ite x t f ihx iht ihf =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_ite (ihx hk.1.1) (iht hk.1.2) (ihf hk.2)
  | prim op a b iha ihb =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_prim (iha hk.1) (ihb hk.2)
  | endorse p e d ihe ihd =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_endorse (ihe hk.1) (ihd hk.2)
  | act t k a iha =>
    simp only [kdOk, Bool.and_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true] at hk
    exact sim_act hk.1 (iha hk.2)
  | drop k => exact sim_drop k
  | for_ n xs body ihx ihb =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_for (ihx hk.1) (ihb hk.2)
  | infer m t a iha => simp only [kdOk] at hk; exact sim_infer (iha hk)
  | await q => exact sim_await q
  | atomic body r f g ihb ihf ihg =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_atomic (ihb hk.1.1) (ihf hk.1.2) (ihg hk.2)
  | call t j κ a n h iha ihh =>
    simp only [kdOk, Bool.and_eq_true] at hk; exact sim_call hk.1.1 (iha hk.1.2) (ihh hk.2)
  | mint e ih => simp only [kdOk] at hk; exact sim_mint (ih hk)
  | transfer p => exact sim_transfer p

/-- O ARX começa vazio: sem quadros, sem journal, com o mundo, o medidor, o oráculo e o
contador do λ_Ath. -/
def AS.ofSt (s : St) : AS := ⟨s.w, s.spent, s.calls, s.next, [], []⟩

/-- **O ARX refina o λ_Ath (o programa inteiro).** Partindo do mesmo estado, o que o λ_Ath faz
o ARX faz: o mesmo valor ou a mesma causa, o mesmo mundo, gasto não maior, e a vista do núcleo
do seu journal é o traço do λ_Ath módulo `nv`. -/
theorem arx_refines (e : Exp) (hk : kdOk kd e = true) (ρ : List Val) (s : St) (hcomp : s.comp = []) :
    (∀ o, eval S c e ρ s = .ok o → ∃ σ', run S c (fun _ => 0) e ρ (AS.ofSt s) = (.ok o.v, σ') ∧
      σ'.w = o.st.w ∧ σ'.spent ≤ o.st.spent ∧ (cvRun S σ'.jr).2 = nvs kd o.tr) ∧
    (∀ q s₁ tr, eval S c e ρ s = .error (.timeout q s₁ tr) →
      ∃ u σ', run S c (fun _ => 0) e ρ (AS.ofSt s) = (.error u, σ') ∧ UQ q u ∧
        σ'.w = s₁.w ∧ σ'.spent ≤ s₁.spent ∧ (cvRun S σ'.jr).2 = nvs kd tr ++ failEv q) := by
  have hr : Rel kd s (AS.ofSt s) :=
    ⟨rfl, fun _ => Nat.le_refl _, rfl, rfl, fun _ _ h => by simp [AS.ofSt] at h⟩
  have key := sim (S := S) (c := c) e hk ρ s (AS.ofSt s) [] [] hr (by rw [hcomp]; exact .nil)
    (by simp [AS.ofSt, cvRun, CV.init])
  refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
  · obtain ⟨σ', r, hp⟩ := key.1 o he
    exact ⟨σ', r, hp.rel.w, hp.rel.spent, by rw [hp.out]; simp [AS.ofSt, cvRun]⟩
  · obtain ⟨u, σ', r, hu, _, hp⟩ := key.2 q s₁ tr he
    exact ⟨u, σ', r, hu, hp.rel.w, hp.rel.spent, by rw [hp.out]; simp [AS.ofSt, cvRun]⟩

/-- **O T1 vale no ARX.** Um programa bem tipado e linear, partindo de `Start`, roda no ARX até
um valor ou uma falha capturável, com o gasto do ARX dentro do orçamento estático e a vista do
seu journal igual ao traço do λ_Ath módulo `nv`: tudo o que T2–T9 dizem desse traço vale para o
que o ARX registra. -/
theorem arx_sound (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) (hcomp : s.comp = [])
    (hk : kdOk kd e = true) :
    (∃ o σ', eval S c e ρ s = .ok o ∧ run S c (fun _ => 0) e ρ (AS.ofSt s) = (.ok o.v, σ') ∧
      σ'.spent ≤ c.limit ∧ σ'.w = o.st.w ∧ (cvRun S σ'.jr).2 = nvs kd o.tr) ∨
    (∃ q s₁ tr u σ', eval S c e ρ s = .error (.timeout q s₁ tr) ∧
      run S c (fun _ => 0) e ρ (AS.ofSt s) = (.error u, σ') ∧ UQ q u ∧
      σ'.spent ≤ c.limit ∧ σ'.w = s₁.w ∧ (cvRun S σ'.jr).2 = nvs kd tr ++ failEv q) := by
  have hr := arx_refines (S := S) (c := c) (kd := kd) e hk ρ s hcomp
  rcases soundness_total hO h hlin h0 hB with ⟨o, he, _, hw, _⟩ | ⟨q, s₁, tr, he, hw, _, _⟩
  · obtain ⟨σ', r, h1, h2, h3⟩ := hr.1 o he
    refine .inl ⟨o, σ', he, r, fun d => ?_, h1, h3⟩
    have := h2 d; have := (hw d).2; have := hB d; simp at *; omega
  · obtain ⟨u, σ', r, hu, h1, h2, h3⟩ := hr.2 q s₁ tr he
    refine .inr ⟨q, s₁, tr, u, σ', he, r, hu, fun d => ?_, h1, h3⟩
    have := h2 d; have := (hw d).2; have := hB d; simp at *; omega

end

/-! ## As obrigações que sobram: contraexemplos

Cada hipótese de `sim`/`arx_sound` é necessária. Os exemplos abaixo são programas concretos,
avaliados por `rfl`/`decide`. -/

namespace Counter

/-- Uma ferramenta compensável de custo 1 (em toda dimensão), uma sonda que nunca confirma. -/
def S : Sig where
  cost := fun _ _ => 1
  mcost := fun _ _ => 0
  arg := fun _ => .int
  effect := fun _ => 0
  pol := fun _ _ => true
  probe := fun _ _ => false

def c : Cfg := ⟨fun _ _ _ _ => .unit, fun _ => 1, fun _ => [0]⟩

def s₀ : St := ⟨[], fun _ => 0, 0, fun _ => 0, [], 10, []⟩

/-- `atomic { t0(c, 1); await q0 } recover q0 else ()`: a ação é feita, a pós-condição falha, a
saga compensa. -/
def prog : Exp := .atomic (.let_ (.act 0 0 (.val (.int 1))) (.await 0)) 0 (.val .unit) (.val .unit)

/-- No λ_Ath o programa gasta 1 = o limite: o T1 vale. -/
example : (match eval S c prog [.cap 0] s₀ with | .ok o => some (o.st.spent .usd) | _ => none) = some 1 := rfl

/-- **Compensações custam (`comp_cost_breaks_budget`).** No ARX o compensador é liquidado no
medidor sem reserva (`obligation`): com custo 1, o ARX gasta 2 > 1 = o limite. A garantia de
orçamento do T1 só passa ao ARX se as compensações forem gratuitas ou reservadas antes. -/
theorem comp_cost_breaks_budget :
    ((run S c (fun _ _ => 1) prog [.cap 0] (AS.ofSt s₀)).2.spent .usd = 2) ∧ c.limit .usd = 1 := ⟨rfl, rfl⟩

/-- Um tratador impuro: depois de `transfer`, ainda emite um evento (`endorse`). -/
def S' : Sig := { S with reply := fun _ _ _ => .unknown, probe := fun _ _ => true }

def hImpure : Exp := .let_ (.transfer 0) (.endorse 0 (.val .unit) (.val .unit))

def progK : Exp := .mint (.call 0 1 0 (.val (.int 5)) 1 hImpure)

/-- **A ordem depois de `transfer` (`impure_handler_reorders`).** O λ_Ath fecha `Transferred`
depois de todo o tratador; o ARX, no próprio `transfer`. Com um tratador puro (o que E1107 e o
`pureH` do T1 impõem) as duas ordens coincidem; com um impuro, a vista do ARX e o traço do λ_Ath
diferem byte a byte, e só coincidem módulo `nv`. -/
theorem impure_handler_reorders :
    (match eval S' c progK [.cap 7] s₀ with
      | .error (.timeout _ _ tr) => some tr | _ => none) =
      some [.mint 10, .pending 0 10 (.int 5), .query 10 1 (some .unknown), .handoff 10,
        .endorse 0 (some .unit), .settle 10 .transferred] ∧
    (cvRun S' (run S' c (fun _ => 0) progK [.cap 7] (AS.ofSt s₀)).2.jr).2 =
      [.mint 10, .pending 0 10 .unit, .query 10 1 (some .unknown), .handoff 10,
        .settle 10 .transferred, .endorse 0 (some .unit)] := ⟨rfl, rfl⟩

/-- **`await` fora de saga (`top_level_await_differs`).** O λ_Ath desenrola sem evento (a falha
só vira `fail` quando uma saga a captura); o ARX escreve `await_timeout`, que a vista já mostra
como `fail`. Dentro de uma saga (E0601) as duas coincidem. -/
theorem top_level_await_differs :
    (match eval S c (.await 0) [] s₀ with | .error (.timeout _ _ tr) => some tr | _ => none) = some [] ∧
    (cvRun S (run S c (fun _ => 0) (.await 0) [] (AS.ofSt s₀)).2.jr).2 = [.fail (.probe 0)] := ⟨rfl, rfl⟩

/-- **Ferramenta com chave por `act` (`kdOk`).** A vista do ARX só sabe que uma compensação é de
uma operação com chave pela chave no quadro: se `kd` não separa as ferramentas, a normalização
`nv` e a vista discordam no argumento da compensação. -/
example : kdOk (fun _ => true) prog = false := rfl

end Counter

end Aletheia.Arx
