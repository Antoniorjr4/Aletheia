import Aletheia.Eval

/-!
# Rev6 / Teorema T2: confinamento de efeitos na semantica indexada Cap[E]

O avaliador `evalScoped` consulta o Cap[E] do contexto lexico E a
concessao da capacidade do runtime antes de emitir qualquer evento de
ferramenta. Este teorema cobre TODAS as construcoes do nucleo lambda_Ath:
condicionais, sequenciamento, loops, inferencia e endosso.

Limites: a correspondencia do avaliador formal com o codigo Rust do ARX
continua como obrigacao de refinamento, nao provada neste arquivo.
-/

namespace Aletheia

/-- Evento endosso e inofensivo; evento de ferramenta requer permissao da
    capacidade concreta, nao apenas presenca de uma capacidade qualquer. -/
def AuthorizedEvent (A : Authority) : Ev → Prop
  | .act tool id _ => A.permits id tool
  | .endorse _ _ => True

def AuthorizedTrace (A : Authority) (tr : List Ev) : Prop :=
  ∀ ev, ev ∈ tr → AuthorizedEvent A ev

theorem AuthorizedTrace.nil (A : Authority) : AuthorizedTrace A [] := by
  intro ev h
  simp at h

theorem AuthorizedTrace.append {A : Authority} {xs ys : List Ev}
    (hx : AuthorizedTrace A xs) (hy : AuthorizedTrace A ys) :
    AuthorizedTrace A (xs ++ ys) := by
  intro ev h
  rcases List.mem_append.mp h with h | h
  · exact hx ev h
  · exact hy ev h

theorem AuthorizedTrace.endorse (A : Authority) (p : Nat) (v : Option Val) :
    AuthorizedTrace A [.endorse p v] := by
  intro ev h
  simp only [List.mem_singleton] at h
  cases h
  trivial

theorem AuthorizedTrace.consEndorse {A : Authority} {tr : List Ev}
    (p : Nat) (v : Option Val) (h : AuthorizedTrace A tr) :
    AuthorizedTrace A (.endorse p v :: tr) := by
  simpa using (AuthorizedTrace.append (AuthorizedTrace.endorse A p v) h)

theorem AuthorizedTrace.loop {A : Authority} {f : Val → St → Except Err Out}
    (hf : ∀ v s o, f v s = .ok o → AuthorizedTrace A o.tr) :
    ∀ (vs : List Val) (s : St) (r : St × List Ev),
      loop f vs s = .ok r → AuthorizedTrace A r.2 := by
  intro vs
  induction vs with
  | nil =>
      intro s r h
      simp only [loop] at h
      cases h
      exact AuthorizedTrace.nil A
  | cons v vs ih =>
      intro s r h
      simp only [loop, bnd_eq_ok] at h
      obtain ⟨o, ho, r', hr', h⟩ := h
      cases h
      exact AuthorizedTrace.append (hf v s o ho) (ih o.st r' hr')

/-- A guarda de Cap[E] do avaliador nao libera uma acao sem a
    concessao id→E; a unica nova entrada no journal e a ferramenta t. -/
theorem doScopedAct_authorized {S : Sig} {c : Cfg} {A : Authority}
    {Γ : List ScopedBind} {t k : Nat} {ρ : List Val} {o r : Out}
    (h : doScopedAct S c A Γ t k ρ o = .ok r)
    (hprev : AuthorizedTrace A o.tr) :
    AuthorizedTrace A r.tr := by
  cases hΓ : Γ[k]? with
  | none => simp [doScopedAct, hΓ] at h
  | some b =>
    cases b with
    | data => simp [doScopedAct, hΓ] at h
    | cap effects =>
      cases hρ : ρ[k]? with
      | none => simp [doScopedAct, hΓ, hρ] at h
      | some v =>
        cases v with
        | int n => simp [doScopedAct, hΓ, hρ] at h
        | bool b => simp [doScopedAct, hΓ, hρ] at h
        | unit => simp [doScopedAct, hΓ, hρ] at h
        | list vs => simp [doScopedAct, hΓ, hρ] at h
        | cap id =>
          by_cases hp : A.effectOf t ∈ effects ∧ A.effectOf t ∈ A.grants id
          · have hact : doAct S c t (some (.cap id)) o = .ok r := by
              simpa [doScopedAct, hΓ, hρ, hp] using h
            obtain ⟨id', hcv, _, _, hr⟩ := doAct_ok hact
            cases hcv
            cases hr
            apply AuthorizedTrace.append hprev
            intro ev hev
            simp only [List.mem_singleton] at hev
            cases hev
            exact hp.2
          · simp [doScopedAct, hΓ, hρ, hp] at h

/-- T2: todos os eventos de ferramenta de uma execucao do nucleo
    instrumentado sao autorizados, inclusive sob oraculo neural arbitrario. -/
theorem evalScoped_t2 (S : Sig) (c : Cfg) (A : Authority) :
    ∀ (e : Exp) (Γ : List ScopedBind) (ρ : List Val) (s : St) (o : Out),
      evalScoped S c A e Γ ρ s = .ok o → AuthorizedTrace A o.tr := by
  intro e
  induction e with
  | val v =>
      intro Γ ρ s o h
      simp only [evalScoped] at h
      cases h
      exact AuthorizedTrace.nil A
  | var k =>
      intro Γ ρ s o h
      simp only [evalScoped] at h
      split at h
      · cases h
        exact AuthorizedTrace.nil A
      · cases h
  | let_ e₁ e₂ ih₁ ih₂ =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
      cases h
      exact AuthorizedTrace.append (ih₁ Γ ρ s o₁ h₁)
        (ih₂ (.data :: Γ) (o₁.v :: ρ) o₁.st o₂ h₂)
  | ite condition then_ else_ ihc iht ihf =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨oc, hc, h⟩ := h
      split at h
      · obtain ⟨o', h', ho⟩ := bnd_eq_ok.mp h
        cases ho
        rcases cond_ok h' with ⟨_, ht⟩ | ⟨_, hf⟩
        · exact AuthorizedTrace.append (ihc Γ ρ s oc hc) (iht Γ ρ oc.st o' ht)
        · exact AuthorizedTrace.append (ihc Γ ρ s oc hc) (ihf Γ ρ oc.st o' hf)
      · cases h
  | prim op a b iha ihb =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨o₁, h₁, o₂, h₂, h⟩ := h
      split at h
      · cases h
        exact AuthorizedTrace.append (iha Γ ρ s o₁ h₁) (ihb Γ ρ o₁.st o₂ h₂)
      · cases h
  | endorse p e d ihe ihd =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨oe, he, h⟩ := h
      split at h
      · cases h
        exact AuthorizedTrace.append (ihe Γ ρ s oe he)
          (AuthorizedTrace.endorse A p (some oe.v))
      · obtain ⟨od, hd, h⟩ := bnd_eq_ok.mp h
        cases h
        exact AuthorizedTrace.append (ihe Γ ρ s oe he)
          (AuthorizedTrace.consEndorse p none (ihd Γ ρ oe.st od hd))
  | act tool k arg iha =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨oa, ha, hact⟩ := h
      exact doScopedAct_authorized hact (iha Γ ρ s oa ha)
  | for_ n xs body ihx ihbody =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨ox, hx, h⟩ := h
      split at h
      · obtain ⟨r, hr, ho⟩ := bnd_eq_ok.mp h
        cases ho
        exact AuthorizedTrace.append (ihx Γ ρ s ox hx)
          (AuthorizedTrace.loop
            (fun v st out hout => ihbody (.data :: Γ) (v :: ρ) st out hout)
            _ _ r hr)
      · cases h
  | infer m ty a iha =>
      intro Γ ρ s o h
      simp only [evalScoped, bnd_eq_ok] at h
      obtain ⟨oa, ha, hi⟩ := h
      obtain ⟨_, hr⟩ := doInfer_ok hi
      cases hr
      exact iha Γ ρ s oa ha

/-- Teorema T2 na interface de execucao que exige o checker estatico
    e a guarda operacional. Nenhum efeito no trace tem permissao ausente. -/
theorem T2_confinement {S : Sig} {c : Cfg} {A : Authority}
    {e : Exp} {Γ : List ScopedBind} {ρ : List Val} {s : St} {o : Out}
    (h : runScoped S c A e Γ ρ s = .ok o) : AuthorizedTrace A o.tr := by
  unfold runScoped at h
  split at h
  · exact evalScoped_t2 S c A e Γ ρ s o h
  · cases h

end Aletheia
