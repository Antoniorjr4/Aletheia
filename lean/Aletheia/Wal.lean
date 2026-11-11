import Aletheia.Syntax

/-!
# M-III: write-ahead log, queda do processo e recuperação

Uma operação remota com chave atravessa cinco pontos: **persistir** a intenção no log
(`intent`), **enviar**, a **aplicação remota** (no provedor, fora do processo), a **confirmação**
(a resposta chega) e o **encerramento** (o desfecho é persistido). O processo pode cair entre
quaisquer dois deles. Depois do reinício, o log é tudo o que sobrou do lado de cá; o provedor
continua com o que fez.

Este arquivo modela esse protocolo para uma operação e prova, para **todo ponto de queda** e
todo comportamento admitido do provedor e da rede, que o log recuperado:

* tem exatamente um desfecho para cada intenção persistida (`close` ou `escalate`), e nenhum
  para uma operação que nem chegou a ser persistida — nenhuma obrigação some;
* só afirma `Done` com uma resposta `Done` do provedor e só afirma `Failed` com uma resposta
  `Failed`; sob o contrato do provedor (`Truthful`), `Done` implica efeito aplicado e `Failed`
  implica efeito ausente — nenhuma queda vira sucesso ou falha sem evidência;
* deixa o efeito aplicado **no máximo uma vez** (o provedor `idempotent` deduplica pela chave;
  `queryable` e `opaque` nunca recebem reenvio);
* nunca fecha uma operação de provedor `opaque` que caiu em aberto: ela é escalada.

E prova que **não há** "exatamente uma vez" para o provedor `opaque`: dois mundos, um com o
efeito e outro sem, produzem o mesmo log e as mesmas observações (`opaque_not_exactly_once`).

As hipóteses são as de um log real e estão no modelo, não escondidas: o registro é uma lista e
a queda é um prefixo dela (**persistência**: um registro escrito e sincronizado sobrevive;
**atomicidade**: um registro está inteiro ou ausente — o runtime descarta uma última linha
rasgada pela cadeia de hashes); a intenção é persistida **antes** do envio; o provedor decide
uma vez por chave (`accept`) e, se `idempotent`, deduplica pela chave.
-/

namespace Aletheia.Wal

/-- Um registro do log. -/
inductive Rec where
  /-- a intenção, persistida antes do envio: chave, ferramenta, argumento, classe do provedor -/
  | intent (k t : Nat) (v : Int) (p : Prov)
  /-- o desfecho com evidência: `true` = o provedor respondeu `Done`, `false` = `Failed` -/
  | close (k : Nat) (done : Bool)
  /-- sem desfecho conclusivo: a obrigação é escalada (transferida a quem pode decidir) -/
  | escalate (k : Nat)
  deriving DecidableEq, Repr

abbrev Log := List Rec

/-- O que a rede fez com uma tentativa: não chegou ao provedor; chegou, mas a resposta se
perdeu; ou chegou e a resposta voltou. -/
inductive Net where
  | dropped | noReply | ok
  deriving DecidableEq, Repr

/-- O provedor e a rede de uma operação. `accept` é a decisão do provedor para a chave (a mesma
em toda tentativa, como o contrato de idempotência exige); `net i` é o destino da tentativa
`i` (0 é o envio original; 1, 2, … os reenvios ou consultas). -/
structure World where
  accept : Bool
  net : Nat → Net

-- O estado do provedor remoto é um `Nat`: quantas vezes o efeito da chave foi aplicado.


/-- A aplicação remota de uma tentativa que chegou. O provedor `idempotent` deduplica pela
chave; os outros não (por isso nunca recebem reenvio). -/
def applyR (p : Prov) (W : World) (r : Nat) : Nat :=
  if W.accept then (if p = .idempotent ∧ 1 ≤ r then r else r + 1) else r

/-- A resposta a uma tentativa que voltou: a decisão do provedor. -/
def answer (W : World) : Bool := W.accept

/-- Uma tentativa de reconciliação depois da queda (ou da resposta perdida), pela classe do
provedor. Devolve o novo estado remoto e a resposta, se houve: `idempotent` reenvia com a mesma
chave; `queryable` consulta o status (que é a verdade do provedor: aplicado ou não); `opaque` não
tem protocolo. -/
def attempt (p : Prov) (W : World) (i : Nat) (r : Nat) : Nat × Option Bool :=
  match p with
  | .idempotent =>
    match W.net i with
    | .dropped => (r, none)
    | .noReply => (applyR p W r, none)
    | .ok => (applyR p W r, some (answer W))
  | .queryable =>
    match W.net i with
    | .ok => (r, some (decide (1 ≤ r)))
    | _ => (r, none)
  | .opaque => (r, none)

/-- A reconciliação com `fuel` tentativas a partir da tentativa `i`: o estado remoto e o
desfecho, se alguma tentativa respondeu. -/
def reconcileW (p : Prov) (W : World) : Nat → Nat → Nat → Nat × Option Bool
  | _, 0, r => (r, none)
  | i, n + 1, r =>
    match attempt p W i r with
    | (r', some b) => (r', some b)
    | (r', none) => reconcileW p W (i + 1) n r'

/-- O registro de desfecho de uma reconciliação. -/
def outRec (k : Nat) : Option Bool → Rec
  | some b => .close k b
  | none => .escalate k

/-- Uma intenção está em aberto no log: persistida, sem desfecho. -/
def isOpen (L : Log) (k : Nat) : Bool :=
  L.any (fun r => match r with | .intent k' _ _ _ => k' == k | _ => false) &&
    !L.any (fun r => match r with | .close k' _ => k' == k | .escalate k' => k' == k | _ => false)

/-- **A recuperação depois do reinício.** Para a intenção persistida da operação `(k, t, v, p)`:
se ela está em aberto, reconcilia pelo protocolo do provedor e persiste o desfecho; senão, não
faz nada. Nunca descarta, nunca fecha sem resposta. -/
def recover (k _t : Nat) (_v : Int) (p : Prov) (W : World) (fuel : Nat) (L : Log) (r : Nat) : Log × Nat :=
  if isOpen L k then (L ++ [outRec k (reconcileW p W 1 fuel r).2], (reconcileW p W 1 fuel r).1)
  else (L, r)

/-- A intenção persistida, se o ponto 1 foi alcançado. -/
def logI (k t : Nat) (v : Int) (p : Prov) (c : Nat) : Log := if 1 ≤ c then [.intent k t v p] else []

/-- O envio original (tentativa 0): chega ou não; se chega, o provedor decide. -/
def sent (p : Prov) (W : World) (c : Nat) : Nat := if 2 ≤ c ∧ W.net 0 ≠ .dropped then applyR p W 0 else 0

/-- A confirmação: a resposta original, ou a reconciliação em processo (tentativas 1, 2, …). -/
def conf (p : Prov) (W : World) (fuel c : Nat) : Nat × Option Bool :=
  if W.net 0 = .ok then (sent p W c, some (answer W)) else reconcileW p W 1 fuel (sent p W c)

/-- **A execução normal, cortada no ponto `c`.** Os pontos: 0 = nada feito; 1 = intenção
persistida; 2 = enviada; 3 = aplicada no provedor (se a tentativa chegou); 4 = confirmada (a
resposta chegou, ou a reconciliação em processo terminou); 5 = desfecho persistido. O
provedor aplica uma tentativa que chegou mesmo que o processo caia logo depois do envio: a
aplicação remota não depende do processo. Devolve o log persistido e o estado remoto. -/
def crashAt (k t : Nat) (v : Int) (p : Prov) (W : World) (fuel : Nat) (c : Nat) : Log × Nat :=
  if 5 ≤ c then (logI k t v p c ++ [outRec k (conf p W fuel c).2], (conf p W fuel c).1)
  else if 4 ≤ c then (logI k t v p c, (conf p W fuel c).1)
  else (logI k t v p c, sent p W c)

/-- A queda em `c` seguida da recuperação. -/
def crashRecover (k t : Nat) (v : Int) (p : Prov) (W : World) (fuel : Nat) (c : Nat) : Log × Nat :=
  recover k t v p W fuel (crashAt k t v p W fuel c).1 (crashAt k t v p W fuel c).2

/-! ## Os desfechos de uma chave no log -/

/-- Os registros de desfecho da chave `k`. -/
def outcomes (L : Log) (k : Nat) : List Rec :=
  L.filter fun r => match r with | .close k' _ => k' == k | .escalate k' => k' == k | _ => false

/-- As intenções da chave `k`. -/
def intents (L : Log) (k : Nat) : List Rec :=
  L.filter fun r => match r with | .intent k' _ _ _ => k' == k | _ => false

/-! ## Lemas do protocolo -/

/-- O provedor que aceita aplica pelo menos uma vez; o que recusa nunca aplica. -/
theorem applyR_pos {p : Prov} {W : World} {r : Nat} (h : W.accept = true) : 1 ≤ applyR p W r := by
  unfold applyR; rw [if_pos h]; split
  · rename_i hc; exact hc.2
  · exact Nat.le_add_left 1 r

theorem applyR_zero (p : Prov) (W : World) : applyR p W 0 = if W.accept then 1 else 0 := by
  cases ha : W.accept <;> simp [applyR, ha]

theorem applyR_refuse {p : Prov} {W : World} {r : Nat} (h : W.accept = false) : applyR p W r = r := by
  simp [applyR, h]

/-- O reenvio ao provedor `idempotent` nunca aplica duas vezes. -/
theorem applyR_idem {W : World} {r : Nat} (h : r ≤ 1) : applyR .idempotent W r ≤ 1 := by
  cases ha : W.accept
  · rw [applyR_refuse ha]; exact h
  · unfold applyR; rw [if_pos ha]; split
    · exact h
    · rename_i hc
      have h1 : ¬ 1 ≤ r := fun h1 => hc ⟨rfl, h1⟩
      omega

/-- Uma tentativa não aplica de novo um efeito já aplicado (`idempotent`) e só consulta
(`queryable`, `opaque`): o estado remoto fica `≤ 1` se estava. -/
theorem attempt_le {p : Prov} {W : World} {i : Nat} {r : Nat} (h : r ≤ 1) : (attempt p W i r).1 ≤ 1 := by
  cases p <;> cases hn : W.net i <;> simp [attempt, hn, applyR_idem h, h]

theorem reconcileW_le (p : Prov) (W : World) : ∀ (i n : Nat) (r : Nat), r ≤ 1 →
    (reconcileW p W i n r).1 ≤ 1
  | _, 0, r, h => h
  | i, n + 1, r, h => by
    simp only [reconcileW]
    have ha := attempt_le (p := p) (W := W) (i := i) h
    split
    · rename_i r' b he; rw [he] at ha; exact ha
    · rename_i r' he; rw [he] at ha; exact reconcileW_le p W (i + 1) n r' ha

/-- **Evidência.** Uma resposta da reconciliação é a verdade do provedor: `Done` só se o efeito
está aplicado, `Failed` só se não está. Para `idempotent` é a decisão do provedor (que aplicou
na tentativa que respondeu, se aceitou); para `queryable` é o status consultado. -/
theorem attempt_truth {p : Prov} {W : World} {i : Nat} {r : Nat} {r' : Nat} {b : Bool}
    (h : attempt p W i r = (r', some b)) (hr : W.accept = false → r = 0) :
    (b = true ↔ 1 ≤ r') := by
  cases p <;> cases hn : W.net i <;> simp [attempt, hn] at h
  · obtain ⟨rfl, rfl⟩ := h
    cases ha : W.accept
    · rw [applyR_refuse ha, hr ha]; simp [answer, ha]
    · simp [answer, ha, applyR_pos ha]
  · obtain ⟨rfl, rfl⟩ := h; simp

/-- A invariante do provedor que recusa: o efeito nunca existe. -/
theorem attempt_refuse {p : Prov} {W : World} {i : Nat} {r : Nat} (ha : W.accept = false) (hr : r = 0) :
    (attempt p W i r).1 = 0 := by
  cases p <;> cases hn : W.net i <;> simp [attempt, hn, applyR_refuse ha, hr]

theorem reconcileW_refuse (p : Prov) (W : World) (ha : W.accept = false) :
    ∀ (i n : Nat) (r : Nat), r = 0 → (reconcileW p W i n r).1 = 0
  | _, 0, r, h => h
  | i, n + 1, r, h => by
    simp only [reconcileW]
    have h0 := attempt_refuse (p := p) (W := W) (i := i) ha h
    split
    · rename_i r' b he; rw [he] at h0; exact h0
    · rename_i r' he; rw [he] at h0; exact reconcileW_refuse p W ha (i + 1) n r' h0

theorem reconcileW_truth (p : Prov) (W : World) :
    ∀ (i n : Nat) (r : Nat) (b : Bool), (W.accept = false → r = 0) →
      (reconcileW p W i n r).2 = some b → (b = true ↔ 1 ≤ (reconcileW p W i n r).1)
  | _, 0, r, b, _, h => by simp [reconcileW] at h
  | i, n + 1, r, b, hr, h => by
    simp only [reconcileW] at h ⊢
    split at h
    · rename_i r' b' he
      simp only at h; cases h
      simp only [he]
      exact attempt_truth he hr
    · rename_i r' he
      refine reconcileW_truth p W (i + 1) n r' b (fun ha => ?_) h
      have := attempt_refuse (p := p) (W := W) (i := i) ha (hr ha)
      rw [he] at this; exact this

/-- O provedor `opaque` nunca responde à reconciliação. -/
theorem reconcileW_opaque (W : World) : ∀ (i n : Nat) (r : Nat), reconcileW .opaque W i n r = (r, none)
  | _, 0, _ => rfl
  | i, n + 1, r => by simp only [reconcileW, attempt]; exact reconcileW_opaque W (i + 1) n r

theorem sent_le (p : Prov) (W : World) (c : Nat) : sent p W c ≤ 1 := by
  unfold sent; rw [applyR_zero]; split
  · split <;> simp
  · simp

theorem sent_refuse (p : Prov) (W : World) (c : Nat) (ha : W.accept = false) : sent p W c = 0 := by
  unfold sent; split
  · exact applyR_refuse ha
  · rfl

theorem conf_le (p : Prov) (W : World) (fuel c : Nat) : (conf p W fuel c).1 ≤ 1 := by
  unfold conf; split
  · exact sent_le p W c
  · exact reconcileW_le p W 1 fuel _ (sent_le p W c)

theorem conf_refuse (p : Prov) (W : World) (fuel c : Nat) (ha : W.accept = false) : (conf p W fuel c).1 = 0 := by
  unfold conf; split
  · exact sent_refuse p W c ha
  · exact reconcileW_refuse p W ha 1 fuel _ (sent_refuse p W c ha)

/-- A confirmação depois do envio é a verdade do provedor. -/
theorem conf_truth (p : Prov) (W : World) (fuel c : Nat) (hc : 2 ≤ c) {b : Bool}
    (h : (conf p W fuel c).2 = some b) : (b = true ↔ 1 ≤ (conf p W fuel c).1) := by
  unfold conf at h ⊢
  split at h
  · rename_i hok
    rw [if_pos hok]; simp only at h ⊢; cases h
    have hs : sent p W c = applyR p W 0 := by
      unfold sent; rw [if_pos ⟨hc, by rw [hok]; decide⟩]
    rw [hs]
    cases ha : W.accept
    · rw [applyR_refuse ha]; simp [answer, ha]
    · simp [answer, ha, applyR_pos ha]
  · rename_i hok
    rw [if_neg hok]
    exact reconcileW_truth p W 1 fuel _ b (sent_refuse p W c) h

/-! ## Os teoremas -/

section
variable (k t : Nat) (v : Int) (p : Prov) (W : World) (fuel c : Nat)

/-- O estado remoto depois da queda nunca passa de uma aplicação, e um provedor que recusa nunca
aplica. -/
theorem crashAt_remote :
    (crashAt k t v p W fuel c).2 ≤ 1 ∧ (W.accept = false → (crashAt k t v p W fuel c).2 = 0) := by
  unfold crashAt
  split
  · exact ⟨conf_le p W fuel c, conf_refuse p W fuel c⟩
  · split
    · exact ⟨conf_le p W fuel c, conf_refuse p W fuel c⟩
    · exact ⟨sent_le p W c, sent_refuse p W c⟩

theorem outcomes_logI : outcomes (logI k t v p c) k = [] := by
  unfold logI; split <;> simp [outcomes]

theorem outcomes_outRec (o : Option Bool) : outcomes [outRec k o] k = [outRec k o] := by
  cases o <;> simp [outcomes, outRec]

theorem isOpen_logI : isOpen (logI k t v p c) k = decide (1 ≤ c) := by
  unfold logI; by_cases h : 1 ≤ c <;> simp [h, isOpen]

theorem isOpen_closed (o : Option Bool) : isOpen (logI k t v p c ++ [outRec k o]) k = false := by
  cases o <;> simp [isOpen, outRec]

/-- **Nenhuma obrigação some.** Depois da queda em qualquer ponto e da recuperação, a operação
tem exatamente um desfecho no log se a intenção foi persistida (`1 ≤ c`), e nenhum se ela nunca
existiu. Um registro de desfecho nunca é escrito duas vezes. -/
theorem outcomes_app (L₁ L₂ : Log) : outcomes (L₁ ++ L₂) k = outcomes L₁ k ++ outcomes L₂ k := by
  simp [outcomes, List.filter_append]

theorem no_silent_loss :
    (outcomes (crashRecover k t v p W fuel c).1 k).length = if 1 ≤ c then 1 else 0 := by
  unfold crashRecover recover crashAt
  by_cases h5 : 5 ≤ c
  · have h1 : 1 ≤ c := by omega
    simp only [h5, if_true, isOpen_closed, Bool.false_eq_true, if_false, h1, outcomes_app,
      outcomes_logI, outcomes_outRec, List.nil_append, List.length_singleton]
  · by_cases h4 : 4 ≤ c
    · have h1 : 1 ≤ c := by omega
      simp only [h5, h4, if_true, if_false, isOpen_logI, h1, decide_true, outcomes_app,
        outcomes_logI, outcomes_outRec, List.nil_append, List.length_singleton]
    · by_cases h1 : 1 ≤ c
      · simp only [h5, h4, if_false, isOpen_logI, h1, decide_true, if_true, outcomes_app,
          outcomes_logI, outcomes_outRec, List.nil_append, List.length_singleton]
      · simp only [h5, h4, if_false, isOpen_logI, h1, decide_false, Bool.false_eq_true, outcomes_logI,
          List.length_nil]

/-- **Evidência dos desfechos.** Sob o contrato do provedor — a decisão é uma por chave e o
status consultado é a verdade —, depois da queda e da recuperação: `close k true` só se o efeito
existe no provedor, `close k false` só se não existe. Nenhuma queda converte uma obrigação
desconhecida em sucesso ou falha. -/
theorem evidence {b : Bool} (h : Rec.close k b ∈ (crashRecover k t v p W fuel c).1) :
    (b = true ↔ 1 ≤ (crashRecover k t v p W fuel c).2) := by
  have hz := (crashAt_remote k t v p W fuel c).2
  have nI : ∀ b', Rec.close k b' ∉ logI k t v p c := by intro b' hm; unfold logI at hm; split at hm <;> simp at hm
  unfold crashRecover recover at h ⊢
  split at h
  · rename_i ho
    rw [if_pos ho]
    simp only [List.mem_append, List.mem_singleton] at h
    rcases h with h | h
    · -- um desfecho já persistido fecharia a intenção
      exfalso
      unfold crashAt at h ho
      split at h
      · rw [if_pos (by assumption)] at ho; rw [isOpen_closed] at ho; exact Bool.false_ne_true ho
      · split at h <;> exact nI b h
    · unfold outRec at h; split at h
      · rename_i b' hb; cases h
        exact reconcileW_truth p W 1 fuel _ b hz hb
      · cases h
  · rename_i ho
    rw [if_neg ho]
    unfold crashAt at h ⊢
    split at h
    · rename_i h5
      rw [if_pos h5]
      simp only [List.mem_append, List.mem_singleton] at h
      rcases h with h | h
      · exact absurd h (nI b)
      · unfold outRec at h; split at h
        · rename_i b' hb; cases h
          exact conf_truth p W fuel c (by omega) hb
        · cases h
    · split at h <;> exact absurd h (nI b)

/-- **No máximo uma vez.** O efeito nunca é aplicado duas vezes, qualquer que seja o ponto de
queda, a rede e a classe do provedor: o `idempotent` deduplica o reenvio pela chave, e os outros
nunca recebem reenvio. -/
theorem at_most_once : (crashRecover k t v p W fuel c).2 ≤ 1 := by
  have hle := (crashAt_remote k t v p W fuel c).1
  unfold crashRecover recover
  split
  · exact reconcileW_le p W 1 fuel _ hle
  · exact hle

/-- **O provedor `opaque` em aberto é escalado.** Se a queda deixou a operação sem desfecho
persistido, a recuperação não a fecha: escreve `escalate`. -/
theorem opaque_escalates (hc : 1 ≤ c) (h5 : c < 5) :
    (crashRecover k t v .opaque W fuel c).1 = [.intent k t v .opaque, .escalate k] := by
  have h5' : ¬ 5 ≤ c := by omega
  unfold crashRecover recover crashAt
  rw [if_neg h5']
  have hI : logI k t v .opaque c = [.intent k t v .opaque] := by simp [logI, hc]
  split
  · rw [isOpen_logI, if_pos (by simp [hc])]
    simp only [hI, reconcileW_opaque, outRec]; rfl
  · rw [isOpen_logI, if_pos (by simp [hc])]
    simp only [hI, reconcileW_opaque, outRec]; rfl

/-- **O registro de compensação depois da recuperação.** Compensar a operação é estorná-la se
ela foi ou pode ter sido aplicada: com `close k true` ou escalada. A que a reconciliação refutou
(`close k false`) sai do registro, e pela evidência ela não está aplicada: não há o que
estornar. -/
def compensates (L : Log) (k : Nat) : Bool :=
  !(intents L k).isEmpty && !(L.contains (.close k false))

theorem refuted_not_applied (h : Rec.close k false ∈ (crashRecover k t v p W fuel c).1) :
    (crashRecover k t v p W fuel c).2 = 0 := by
  have e := evidence k t v p W fuel c h
  by_cases hx : 1 ≤ (crashRecover k t v p W fuel c).2
  · exact absurd (e.mpr hx) (by simp)
  · omega

/-- A operação que a recuperação tira do registro não tem efeito no provedor; a que fica foi
aplicada ou pode ter sido (escalada). -/
theorem registry_after_recovery (hc : 1 ≤ c) :
    compensates (crashRecover k t v p W fuel c).1 k = false → (crashRecover k t v p W fuel c).2 = 0 := by
  intro h
  have hi : (intents (crashRecover k t v p W fuel c).1 k).isEmpty = false := by
    have hI : Rec.intent k t v p ∈ (crashRecover k t v p W fuel c).1 := by
      unfold crashRecover recover crashAt
      have : Rec.intent k t v p ∈ logI k t v p c := by simp [logI, hc]
      split <;> (try split) <;> (try split) <;> simp_all
    cases hI' : (intents (crashRecover k t v p W fuel c).1 k)
    · exfalso
      have : Rec.intent k t v p ∈ intents (crashRecover k t v p W fuel c).1 k :=
        List.mem_filter.mpr ⟨hI, by simp⟩
      rw [hI'] at this; simp at this
    · rfl
  simp only [compensates, hi, Bool.not_false, Bool.true_and, Bool.not_eq_false'] at h
  exact refuted_not_applied k t v p W fuel c (by simpa using h)

end

/-! ## Não há "exatamente uma vez" para o provedor `opaque` -/

/-- Dois mundos: a tentativa original chega ao provedor (e o efeito é aplicado, a resposta se
perde) ou não chega. -/
def Wapplied : World := ⟨true, fun i => if i = 0 then .noReply else .dropped⟩
def Wlost : World := ⟨true, fun _ => .dropped⟩

/-- **`opaque_not_exactly_once`.** Com o provedor `opaque` e a queda depois do envio, os dois
mundos produzem o mesmo log recuperado — a mesma evidência —, mas num o efeito existe e no outro
não. Nenhuma recuperação que dependa só do log e das respostas pode afirmar "aplicado
exatamente uma vez" (nem "não aplicado"): a Aletheia escala, e não alega o que não sabe. -/
theorem opaque_not_exactly_once :
    (crashRecover 1 0 5 .opaque Wapplied 3 2).1 = (crashRecover 1 0 5 .opaque Wlost 3 2).1 ∧
    (crashRecover 1 0 5 .opaque Wapplied 3 2).2 = 1 ∧ (crashRecover 1 0 5 .opaque Wlost 3 2).2 = 0 := by
  decide

/-- O mesmo acontece com o `queryable` cujas consultas nunca respondem: a evidência não decide,
e a operação é escalada nos dois mundos. -/
theorem queryable_silent_escalates :
    (crashRecover 1 0 5 .queryable Wapplied 3 2).1 = [.intent 1 0 5 .queryable, .escalate 1] ∧
    (crashRecover 1 0 5 .queryable Wlost 3 2).1 = [.intent 1 0 5 .queryable, .escalate 1] := by
  decide

/-- Com o provedor `idempotent` e a rede de volta, o reenvio pela mesma chave fecha a operação
com evidência nos dois mundos e o efeito fica aplicado uma única vez em ambos: aí sim, exatamente
uma vez — sob a hipótese de deduplicação pela chave. -/
def Wback (applied0 : Bool) : World := ⟨true, fun i => if i = 0 then (if applied0 then .noReply else .dropped) else .ok⟩

theorem idempotent_exactly_once :
    (crashRecover 1 0 5 .idempotent (Wback true) 3 2) = ([.intent 1 0 5 .idempotent, .close 1 true], 1) ∧
    (crashRecover 1 0 5 .idempotent (Wback false) 3 2) = ([.intent 1 0 5 .idempotent, .close 1 true], 1) := by
  decide

/-- **Sem deduplicação, o reenvio aplicaria duas vezes (`resend_without_dedup_duplicates`).** É
por isso que a recuperação só reenvia ao `idempotent`: um provedor que aplica cada tentativa que
chega, reenviado depois de uma resposta perdida, termina com o efeito em dobro. -/
def applyNoDedup (W : World) (r : Nat) : Nat := if W.accept then r + 1 else r

theorem resend_without_dedup_duplicates :
    applyNoDedup (Wback true) (applyNoDedup (Wback true) 0) = 2 := by decide

end Aletheia.Wal
