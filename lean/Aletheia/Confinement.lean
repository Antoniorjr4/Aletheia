import Aletheia.Noninterference

/-!
# Teorema T2: efeitos e capacidades lineares `Cap[E]`

O T2 vale sobre o **mesmo** avaliador `eval` e o **mesmo** julgamento `HasType` do T1 e do T3.
Não há avaliador paralelo. Cada ação passa por duas autoridades:

* a **estática**: `act t k a` só tipa se o índice `k` for uma capacidade `Cap[E]` e o efeito
  da ferramenta `t` estiver em `E`;
* a **dinâmica**: o avaliador só emite o evento se a tabela de concessões do runtime
  (`c.grants`, instalada pelo host, nunca pela resposta de um modelo) conceder o efeito à
  identidade daquela capacidade.

Este arquivo prova quatro coisas:

1. `effects_granted`: para **qualquer** programa, bem tipado ou não, toda ação no traço foi
   concedida pelo runtime. A guarda dinâmica é correta.
2. `effects_lexical`: para programa bem tipado, toda ação no traço usou uma capacidade
   `Cap[E]` léxica cujo `E` contém o efeito da ferramenta.
3. `caps_exactly_once`: com `Linear` (multiplicidade 1), cada capacidade do agente é consumida
   **exatamente uma vez**. Nenhuma é duplicada e nenhuma é descartada sem consumo explícito,
   seja por `act` ou por `drop`.
4. `T2`: a verificação estática basta. Para programa bem tipado e linear, com concessões que
   cobrem os tipos `Cap[E]`, a guarda dinâmica **nunca recusa**, e as três propriedades acima
   valem juntas.

Limite: a correspondência deste avaliador com o código Rust do ARX continua sendo uma
obrigação de refinamento, não provada aqui.
-/

namespace Aletheia

open Lab Budget

/-- Toda ação do traço foi concedida pela tabela de concessões `G`. -/
def Granted' (S : Sig) (G : Nat → List EffectId) (tr : List Ev) : Prop :=
  ∀ (t id : Nat) (v : Val), Ev.act t id v ∈ tr → S.effect t ∈ G id

/-- Toda ação do traço usou uma capacidade léxica `Cap[E]` de Γ cujo `E` contém o efeito. -/
def Lexical (S : Sig) (Γ : List Bind) (ρ : List Val) (tr : List Ev) : Prop :=
  ∀ (t id : Nat) (v : Val), Ev.act t id v ∈ tr →
    ∃ (k : Nat) (E : List EffectId), Γ[k]? = some (Bind.cap E) ∧ ρ[k]? = some (Val.cap id) ∧
      S.effect t ∈ E

section
variable {S : Sig}

theorem Granted'.nil (G : Nat → List EffectId) : Granted' S G [] := by intro t id v h; simp at h

theorem Granted'.append {G : Nat → List EffectId} {a b : List Ev} (ha : Granted' S G a)
    (hb : Granted' S G b) : Granted' S G (a ++ b) := by
  intro t id v h
  rcases List.mem_append.mp h with h | h
  · exact ha t id v h
  · exact hb t id v h

theorem Granted'.endorse (G : Nat → List EffectId) (p : Nat) (o : Option Val) (tr : List Ev)
    (h : Granted' S G tr) : Granted' S G (.endorse p o :: tr) := by
  intro t id v hm
  simp at hm
  exact h t id v hm

theorem Lexical.nil (Γ : List Bind) (ρ : List Val) : Lexical S Γ ρ [] := by
  intro t id v h; simp at h

theorem Lexical.append {Γ : List Bind} {ρ : List Val} {a b : List Ev} (ha : Lexical S Γ ρ a)
    (hb : Lexical S Γ ρ b) : Lexical S Γ ρ (a ++ b) := by
  intro t id v h
  rcases List.mem_append.mp h with h | h
  · exact ha t id v h
  · exact hb t id v h

theorem Lexical.endorse {Γ : List Bind} {ρ : List Val} (p : Nat) (o : Option Val) (tr : List Ev)
    (h : Lexical S Γ ρ tr) : Lexical S Γ ρ (.endorse p o :: tr) := by
  intro t id v hm
  simp at hm
  exact h t id v hm

/-- Sob um vínculo de dado, a capacidade usada vem de fora: índice deslocado de um. -/
theorem Lexical.uncons {Γ : List Bind} {ρ : List Val} {τ : Ty} {ι : Lab} {v : Val} {tr : List Ev}
    (h : Lexical S (.data τ ι :: Γ) (v :: ρ) tr) : Lexical S Γ ρ tr := by
  intro t id w hm
  obtain ⟨k, E, hk, hid, he⟩ := h t id w hm
  cases k with
  | zero => simp at hk
  | succ k => exact ⟨k, E, by simpa using hk, by simpa using hid, he⟩

theorem loop_all {f : Val → St → Except Err Out} {P : List Ev → Prop}
    (hnil : P []) (happ : ∀ a b, P a → P b → P (a ++ b))
    (hf : ∀ v (s : St) o, f v s = .ok o → P o.tr) :
    ∀ (vs : List Val) (s : St) r, loop f vs s = .ok r → P r.2
  | [], s, r, h => by simp [loop] at h; subst h; exact hnil
  | v :: vs, s, r, h => by
    simp only [loop, bnd_eq_ok] at h
    obtain ⟨o, ho, r', hr', h⟩ := h
    cases h
    exact happ _ _ (hf v s o ho) (loop_all hnil happ hf vs o.st r' hr')

/-! ## 1. A guarda dinâmica é correta, para qualquer programa -/

/-- Toda ação do traço foi concedida, nos dois desfechos (as compensações e as sondas de uma
saga não são ações). -/
theorem granted_preserves (c : Cfg) : Preserves S c True (fun _ _ tr => Granted' S c.grants tr) where
  refl _ := Granted'.nil _
  trans h₁ h₂ := h₁.append h₂
  endorse _ := Granted'.endorse _ _ _ [] (Granted'.nil _)
  reject := Granted'.endorse _ _ _ [] (Granted'.nil _)
  act hg _ := fun t' id' v' hm => by
    rcases List.mem_singleton.mp hm with ⟨⟩
    exact hg
  drop _ := Granted'.nil _
  infer := Granted'.nil _
  commit _ := fun h => h
  revoke _ := fun _ _ _ h => h
  abort _ := fun q r p h => by
    rw [List.append_assoc]
    refine h.append fun t id v hv => ?_
    simp only [List.cons_append, List.mem_cons, List.mem_append, List.mem_singleton, reduceCtorEq,
      false_or, or_false] at hv
    rcases hv with hv | hv
    · obtain ⟨_, _, _, hu⟩ := compensate_undo S _ _ _ hv
      cases hu
    · simp at hv

theorem effects_granted (c : Cfg) (e : Exp) {ρ : List Val} {s : St} {o : Out}
    (h : eval S c e ρ s = .ok o) : Granted' S c.grants o.tr :=
  (eval_preserves (granted_preserves c) e (.inl trivial)).1 h

/-! ## 2. Toda ação usa uma `Cap[E]` léxica que contém o efeito -/

theorem Lexical.saga {Γ : List Bind} {ρ : List Val} {tr : List Ev} (q r : Nat) (p : Bool)
    (comp : List (Nat × Val)) (w : World) (h : Lexical S Γ ρ tr) :
    Lexical S Γ ρ (tr ++ .fail q :: (compensate S comp w).evs ++ [.recover r p]) := by
  rw [List.append_assoc]
  refine h.append fun t id v hv => ?_
  simp only [List.cons_append, List.mem_cons, List.mem_append, List.mem_singleton, reduceCtorEq,
    false_or, or_false] at hv
  rcases hv with hv | hv
  · obtain ⟨_, _, _, hu⟩ := compensate_undo S _ _ _ hv
    cases hu
  · simp at hv

theorem loop_all_timeout {f : Val → St → Except Err Out} {P : List Ev → Prop}
    (happ : ∀ a b, P a → P b → P (a ++ b))
    (hf : ∀ v (s : St) o, f v s = .ok o → P o.tr)
    (ht : ∀ v (s : St) q s₁ tr, f v s = .error (.timeout q s₁ tr) → P tr) :
    ∀ (vs : List Val) (s : St) q s₁ tr, loop f vs s = .error (.timeout q s₁ tr) → P tr
  | [], s, q, s₁, tr, h => by simp [loop] at h
  | v :: vs, s, q, s₁, tr, h => by
    simp only [loop] at h
    rcases bnd_eq_timeout h with h₀ | ⟨o, t, ho, h₁, rfl⟩
    · exact ht v s q s₁ tr h₀
    · rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
      · exact happ _ _ (hf v s o ho) (loop_all_timeout happ hf ht vs _ q s₁ t h₂)
      · cases h₃

/-- Toda ação usou uma `Cap[E]` léxica, nos dois desfechos: também no traço de um `timeout`
e no de uma saga que compensou. -/
theorem effects_lexical_total (c : Cfg) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ∀ {ρ : List Val} {s : St}, (∀ o, eval S c e ρ s = .ok o → Lexical S Γ ρ o.tr) ∧
      (∀ q s₁ tr, eval S c e ρ s = .error (.timeout q s₁ tr) → Lexical S Γ ρ tr) := by
  induction h with
  | val _ =>
    intro ρ s
    exact ⟨fun o he => by simp [eval] at he; subst he; exact Lexical.nil _ _,
      fun q s₁ tr he => by simp [eval] at he⟩
  | var _ =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval] at he
      split at he
      · cases he; exact Lexical.nil _ _
      · cases he
    · simp only [eval] at he
      split at he <;> cases he
  | let_ _ _ ih₁ ih₂ =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
      cases he
      exact (ih₁.1 _ h₁).append (ih₂.1 _ h₂).uncons
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
      · exact ih₁.2 _ _ _ h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact (ih₁.1 _ h₁).append (ih₂.2 _ _ _ h₃).uncons
        · cases h₄
  | ite _ _ _ ihc iht ihf =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨oc, hc, he⟩ := he
      split at he
      · obtain ⟨o', h', he⟩ := bnd_eq_ok.mp he
        cases he
        rcases cond_ok h' with ⟨_, h'⟩ | ⟨_, h'⟩
        · exact (ihc.1 _ hc).append (iht.1 _ h')
        · exact (ihc.1 _ hc).append (ihf.1 _ h')
      · cases he
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨oc, t, hc, h₁, rfl⟩
      · exact ihc.2 _ _ _ h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
            · exact (ihc.1 _ hc).append (iht.2 _ _ _ h₄)
            · exact (ihc.1 _ hc).append (ihf.2 _ _ _ h₄)
          · cases h₃
        · intro h₁; cases h₁
  | prim _ _ iha ihb =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨o₁, h₁, o₂, h₂, he⟩ := he
      split at he
      · cases he; exact (iha.1 _ h₁).append (ihb.1 _ h₂)
      · cases he
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨o₁, t, h₁, h₂, rfl⟩
      · exact iha.2 _ _ _ h₀
      · rcases bnd_eq_timeout h₂ with h₃ | ⟨_, _, _, h₄, _⟩
        · exact (iha.1 _ h₁).append (ihb.2 _ _ _ h₃)
        · revert h₄; split <;> intro h₄ <;> cases h₄
  | @endorse Γ pc p e d τ ιe ιd βe βd _ _ _ ihe ihd =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨oe, h₁, he⟩ := he
      split at he
      · cases he; exact (ihe.1 _ h₁).append (Lexical.endorse p _ [] (Lexical.nil _ _))
      · obtain ⟨od, h₂, he⟩ := bnd_eq_ok.mp (pre_eq_ok.mp he)
        cases he
        exact (ihe.1 _ h₁).append (Lexical.endorse p none _ (ihd.1 _ h₂))
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨oe, t, h₁, h₂, rfl⟩
      · exact ihe.2 _ _ _ h₀
      · revert h₂; split
        · intro h₂; cases h₂
        · intro h₂
          obtain ⟨t', h₃, rfl⟩ := pre_eq_timeout h₂
          rcases bnd_eq_timeout h₃ with h₄ | ⟨_, _, _, h₅, _⟩
          · exact (ihe.1 _ h₁).append (Lexical.endorse p none _ (ihd.2 _ _ _ h₄))
          · cases h₅
  | @act Γ pc t k a ιa βa E _ hk heff _ _ iha =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨oa, ha, he⟩ := he
      obtain ⟨id, hid, _, _, _, rfl⟩ := doAct_ok he
      refine (iha.1 _ ha).append fun t' id' v' hm => ?_
      simp at hm
      obtain ⟨rfl, rfl, rfl⟩ := hm
      exact ⟨k, E, hk, hid, heff⟩
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨_, _, _, h₁, _⟩
      · exact iha.2 _ _ _ h₀
      · exact absurd h₁ doAct_ne_timeout
  | drop _ =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval] at he
      obtain ⟨_, _, _, rfl⟩ := doDrop_ok he
      exact Lexical.nil _ _
    · simp only [eval] at he; exact absurd he doDrop_ne_timeout
  | for_ _ _ ihx ihb =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨ox, hx, he⟩ := he
      split at he
      · obtain ⟨r, hr, he⟩ := bnd_eq_ok.mp he
        cases he
        exact (ihx.1 _ hx).append
          (loop_all (Lexical.nil _ _) (fun _ _ ha hb => ha.append hb)
            (fun v s o ho => (ihb.1 _ ho).uncons) _ _ r hr)
      · cases he
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨ox, t, hx, h₁, rfl⟩
      · exact ihx.2 _ _ _ h₀
      · revert h₁; split
        · intro h₁
          rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
          · exact (ihx.1 _ hx).append
              (loop_all_timeout (fun _ _ ha hb => ha.append hb)
                (fun v s o ho => (ihb.1 _ ho).uncons)
                (fun v s q s₁ tr ho => (ihb.2 _ _ _ ho).uncons) _ _ _ _ _ h₂)
          · cases h₃
        · intro h₁; cases h₁
  | infer _ iha =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval, bnd_eq_ok] at he
      obtain ⟨oa, ha, he⟩ := he
      obtain ⟨_, rfl⟩ := doInfer_ok he
      exact iha.1 oa ha
    · simp only [eval] at he
      rcases bnd_eq_timeout he with h₀ | ⟨_, _, _, h₁, _⟩
      · exact iha.2 _ _ _ h₀
      · exact absurd h₁ doInfer_ne_timeout
  | await _ =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval] at he
      split at he
      · cases he; exact Lexical.nil _ _
      · cases he
    · simp only [eval] at he
      split at he
      · cases he
      · cases he; exact Lexical.nil _ _
  | atomic _ _ _ _ ihb ihf ihg =>
    intro ρ s
    refine ⟨fun o he => ?_, fun q s₁ tr he => ?_⟩
    · simp only [eval] at he
      split at he
      · rename_i ob hb
        cases he; exact ihb.1 ob hb
      · rename_i q' s' tr' hb
        obtain ⟨o', h₁, he⟩ := bnd_eq_ok.mp (pre_eq_ok.mp he)
        cases he
        refine (Lexical.saga q' _ _ _ _ (ihb.2 _ _ _ hb)).append ?_
        rcases cond_ok h₁ with ⟨_, h₂⟩ | ⟨_, h₂⟩
        · exact ihf.1 _ h₂
        · exact ihg.1 _ h₂
      · cases he
    · simp only [eval] at he
      split at he
      · cases he
      · rename_i q' s' tr' hb
        obtain ⟨t, h₁, rfl⟩ := pre_eq_timeout he
        rcases bnd_eq_timeout h₁ with h₂ | ⟨_, _, _, h₃, _⟩
        · refine (Lexical.saga q' _ _ _ _ (ihb.2 _ _ _ hb)).append ?_
          rcases cond_eq h₂ with ⟨_, h₄⟩ | ⟨_, h₄⟩
          · exact ihf.2 _ _ _ h₄
          · exact ihg.2 _ _ _ h₄
        · cases h₃
      · rename_i hne _
        cases he
        exact absurd rfl (hne _ _ _)

theorem effects_lexical (c : Cfg) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} (h : HasType S Γ pc e τ ι β) :
    ∀ {ρ : List Val} {s : St} {o : Out}, eval S c e ρ s = .ok o → Lexical S Γ ρ o.tr :=
  fun he => (effects_lexical_total c h).1 _ he

/-! ## 3. Multiplicidade 1: cada capacidade é consumida exatamente uma vez -/

/-- **Capacidades lineares.** Um programa bem tipado e linear, a partir de um ponto de
partida válido e com orçamento suficiente, termina tendo consumido cada capacidade do agente
exatamente uma vez. O conjunto `new` acrescentado não repete identidade (nenhuma
duplicação) e contém todas as capacidades do ambiente (nenhum descarte sem consumo). -/
theorem caps_exactly_once {c : Cfg} (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp}
    {τ : Ty} {ι : Lab} {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β)
    (hlin : Linear Γ e) (hg : Guarded e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o new, eval S c e ρ s = .ok o ∧ o.st.used = new ++ s.used ∧ new.Nodup ∧
      ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ[k]? = some (Val.cap id) := by
  obtain ⟨o, he, _, _, new, hnew, hmem⟩ := soundness hO h hlin hg h0 hB
  obtain ⟨new', hnew', hd, _⟩ := eval_fresh e he
  have : new = new' := List.append_cancel_right (hnew.symm.trans hnew')
  subst this
  exact ⟨o, new, he, hnew, hd, hmem⟩

/-! ## 4. O teorema T2 -/

/-- **Teorema T2 (efeitos e capacidades `Cap[E]`).** Para um programa bem tipado e linear,
com concessões do runtime que cobrem os tipos `Cap[E]` e orçamento que cabe no teto:

* a execução termina sem erro: a guarda dinâmica nunca recusa, porque a verificação estática
  basta;
* toda ação do traço usou uma capacidade `Cap[E]` léxica cujo `E` contém o efeito;
* toda ação do traço foi concedida pelo runtime;
* cada capacidade do agente foi consumida exatamente uma vez. -/
theorem T2 {c : Cfg} (hO : c.oracle.Ok) {Γ : List Bind} {pc : Lab} {e : Exp} {τ : Ty} {ι : Lab}
    {β : Budget} {ρ : List Val} {s : St} (h : HasType S Γ pc e τ ι β) (hlin : Linear Γ e)
    (hg : Guarded e) (h0 : Start c.grants Γ ρ s) (hB : s.spent + β ≤ c.limit) :
    ∃ o new, eval S c e ρ s = .ok o ∧ Lexical S Γ ρ o.tr ∧ Granted' S c.grants o.tr ∧
      o.st.used = new ++ s.used ∧ new.Nodup ∧
      ∀ id : Nat, id ∈ new ↔ ∃ k : Nat, ρ[k]? = some (Val.cap id) := by
  obtain ⟨o, new, he, hu, hd, hmem⟩ := caps_exactly_once hO h hlin hg h0 hB
  exact ⟨o, new, he, effects_lexical c h he, effects_granted c e he, hu, hd, hmem⟩

end

end Aletheia
