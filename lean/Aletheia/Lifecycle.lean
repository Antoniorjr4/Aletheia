import Aletheia.Anchor

/-!
# v1.0-RC4: o ciclo de vida verificado do efeito

Um único sistema de transição para o protocolo de `arx::lifecycle`: autoridade e escopo,
validade temporal, consumo linear, âncora monotônica, estados de envio, evidência de desfecho e
recuperação. O adversário é parte do sistema: **dois passos dele** (`clockSet`, que põe o
relógio do executor onde quiser, e `walSet`, que reescreve o WAL e qualquer registro local)
podem ocorrer a qualquer momento, quantas vezes quiser.

## O que o executável faz e o modelo representa

| `arx::lifecycle` | aqui |
|---|---|
| o tempo atestado `t`, a tolerância `tol`: o instante real está em `[t − 1, t + tol]` | `Att` com `lo ≤ now ≤ hi` (`window_brackets` deriva isso das hipóteses da testemunha) |
| `grant_valid`: escopo, `max_age`, `issued ≤ lo`, `hi ≤ expires` | `valid` |
| `reserve` (concessão livre, chave nova, válida no tempo atestado) | `Step.reserve` |
| `send` (com reserva, sem liberação, revalidada imediatamente antes) | `Step.send` |
| o efeito sai (primeiro envio ou reenvio, sempre revalidado) | `Step.emit` |
| o provedor aplica | `Step.apply` |
| `outcome` só com recibo assinado; o recibo diz a verdade | `Step.outcome` |
| `release` só sem `send` | `Step.release` |
| `withdraw` | `Step.withdraw` |
| recuperação: `lc_recover` | `decide` |

## Hipóteses (as do sistema, não demonstradas aqui)

* **H1 testemunha**: a testemunha de tempo é honesta (assina o instante real, arredondado para
  cima), a assinatura não é forjável e o nonce é novo; o relógio **monotônico** do executor mede
  o atraso de ida e volta. Com isso o instante real está na janela atestada (`window_brackets`).
* **H2 âncora**: o registro unificado é durável, só cresce e aplica `accept`.
* **H3 provedor**: o recibo assinado diz a verdade, e um recibo de não aplicação é definitivo
  (o provedor não aplica aquela chave depois).
* **H4 hash**: os registros são identificados pelo conteúdo (o hash não colide).

O relógio de parede do executor **não é hipótese**: ele está no estado (`localClock`), o
adversário o controla, e nenhum passo do protocolo o lê (`local_clock_irrelevant`).
-/

namespace Aletheia.Lifecycle

/-- Uma concessão, como a autoridade a assinou. -/
structure Grant where
  id : Nat
  op : Nat
  issued : Nat
  expires : Nat
  deriving DecidableEq

/-- Uma autoridade temporal: o escopo e a validade máxima. -/
structure Auth where
  ops : List Nat
  maxAge : Nat

/-- Uma janela de tempo atestada. -/
structure Att where
  lo : Nat
  hi : Nat

/-- `grant_valid` de `arx::lifecycle`, sem a parte criptográfica. -/
def valid (a : Auth) (g : Grant) (w : Att) : Prop :=
  g.op ∈ a.ops ∧ g.expires - g.issued ≤ a.maxAge ∧ g.issued ≤ w.lo ∧ w.hi ≤ g.expires

/-- Os registros do ciclo de vida. -/
inductive Rec
  | reserve (k : Nat) (g : Grant) (t : Nat)
  | send (k : Nat) (t : Nat)
  | outcome (k : Nat) (applied : Bool)
  | release (k : Nat)
  | withdraw (k : Nat)
  deriving DecidableEq

/-- O estado: o instante real, o registro ancorado, o que saiu, o que o provedor aplicou, e o
que o adversário controla. -/
structure Sys where
  now : Nat
  anchor : List Rec
  sent : List Nat
  applied : List Nat
  localClock : Nat
  wal : List Rec

def init : Sys := ⟨0, [], [], [], 0, []⟩

/-- Uma concessão está livre se toda reserva dela foi liberada. -/
def free (A : List Rec) (g : Grant) : Prop :=
  ∀ k t, Rec.reserve k g t ∈ A → Rec.release k ∈ A

def sendOf (A : List Rec) (k : Nat) : Prop := ∃ t, Rec.send k t ∈ A

inductive Step (a : Auth) : Sys → Sys → Prop
  /-- o tempo passa -/
  | tick (s : Sys) : Step a s { s with now := s.now + 1 }
  /-- o adversário: o relógio do executor -/
  | clockSet (s : Sys) (c : Nat) : Step a s { s with localClock := c }
  /-- o adversário: o WAL e todo registro local -/
  | walSet (s : Sys) (l : List Rec) : Step a s { s with wal := l }
  | reserve (s : Sys) (k : Nat) (g : Grant) (w : Att) :
      w.lo ≤ s.now → s.now ≤ w.hi → valid a g w →
      (∀ g' t, Rec.reserve k g' t ∉ s.anchor) → free s.anchor g →
      Step a s { s with anchor := s.anchor ++ [Rec.reserve k g s.now] }
  | send (s : Sys) (k : Nat) (g : Grant) (t : Nat) (w : Att) :
      Rec.reserve k g t ∈ s.anchor → ¬ sendOf s.anchor k → Rec.release k ∉ s.anchor →
      w.lo ≤ s.now → s.now ≤ w.hi → valid a g w →
      Step a s { s with anchor := s.anchor ++ [Rec.send k s.now] }
  /-- o efeito sai: com `send` ancorado e, para **toda** reserva da chave, a concessão válida
  numa janela que contém o instante real (o primeiro envio e cada reenvio) -/
  | emit (s : Sys) (k : Nat) (w : Att) :
      sendOf s.anchor k → (∃ g t, Rec.reserve k g t ∈ s.anchor) →
      w.lo ≤ s.now → s.now ≤ w.hi → (∀ g t, Rec.reserve k g t ∈ s.anchor → valid a g w) →
      Step a s { s with sent := k :: s.sent }
  | apply (s : Sys) (k : Nat) :
      k ∈ s.sent → Rec.outcome k false ∉ s.anchor →
      Step a s { s with applied := k :: s.applied }
  | outcome (s : Sys) (k : Nat) (b : Bool) :
      sendOf s.anchor k → (∀ b', Rec.outcome k b' ∉ s.anchor) →
      (b = true → k ∈ s.applied) → (b = false → k ∉ s.applied) →
      Step a s { s with anchor := s.anchor ++ [Rec.outcome k b] }
  | release (s : Sys) (k : Nat) (g : Grant) (t : Nat) :
      Rec.reserve k g t ∈ s.anchor → ¬ sendOf s.anchor k → Rec.release k ∉ s.anchor →
      Step a s { s with anchor := s.anchor ++ [Rec.release k] }
  | withdraw (s : Sys) (k : Nat) :
      sendOf s.anchor k → Rec.withdraw k ∉ s.anchor →
      Step a s { s with anchor := s.anchor ++ [Rec.withdraw k] }

inductive Reach (a : Auth) : Sys → Prop
  | init : Reach a init
  | step {s t : Sys} : Reach a s → Step a s t → Reach a t

/-! ## O tempo -/

/-- **A janela atestada contém o instante real** (H1). A testemunha assina `t`, o instante da
assinatura `σ` arredondado para cima (`t − 1 ≤ σ ≤ t`); o pedido saiu em `q`, antes da
assinatura (o nonce é novo: `q ≤ σ`); a conferência acontece em `c`, depois dela, e o relógio
monotônico mediu `c − q ≤ tol`. Então `t − 1 ≤ c ≤ t + tol`. -/
theorem window_brackets {t σ q c tol : Nat}
    (h1 : t - 1 ≤ σ) (h2 : σ ≤ t) (h3 : q ≤ σ) (h4 : σ ≤ c) (h5 : c ≤ q + tol) :
    t - 1 ≤ c ∧ c ≤ t + tol := by omega

/-- O que é válido numa janela que contém o instante real é válido no instante real. -/
theorem valid_at_now {a : Auth} {g : Grant} {w : Att} {now : Nat}
    (hv : valid a g w) (h1 : w.lo ≤ now) (h2 : now ≤ w.hi) :
    g.issued ≤ now ∧ now ≤ g.expires := by
  obtain ⟨_, _, h3, h4⟩ := hv
  exact ⟨Nat.le_trans h3 h1, Nat.le_trans h2 h4⟩

/-- **A composição com a RC3.** A validade da RC4, na janela atestada, implica o predicado
`fresh` de `Anchor.lean` avaliado no **instante real**, não no relógio do executor. -/
theorem valid_implies_fresh {a : Auth} {g : Grant} {w : Att} {now p r : Nat}
    (hv : valid a g w) (h1 : w.lo ≤ now) (h2 : now ≤ w.hi) :
    Anchor.fresh p r now a.maxAge ⟨p, r, g.issued, g.expires⟩ := by
  obtain ⟨hi, he⟩ := valid_at_now hv h1 h2
  exact ⟨rfl, rfl, hi, he, hv.2.1⟩

/-- **O relógio local não decide.** Todo passo do protocolo continua sendo um passo com o
relógio local em qualquer valor: nenhuma premissa o lê. -/
theorem local_clock_irrelevant {a : Auth} {s t : Sys} (h : Step a s t) (c : Nat) :
    Step a { s with localClock := c } { t with localClock := c } := by
  cases h with
  | tick => exact Step.tick _
  | clockSet => exact Step.clockSet _ c
  | walSet l => exact Step.walSet _ l
  | reserve k g w h1 h2 h3 h4 h5 => exact Step.reserve _ k g w h1 h2 h3 h4 h5
  | send k g t w h1 h2 h3 h4 h5 h6 => exact Step.send _ k g t w h1 h2 h3 h4 h5 h6
  | emit k w h1 h2 h3 h4 h5 => exact Step.emit _ k w h1 h2 h3 h4 h5
  | apply k h1 h2 => exact Step.apply _ k h1 h2
  | outcome k b h1 h2 h3 h4 => exact Step.outcome _ k b h1 h2 h3 h4
  | release k g t h1 h2 h3 => exact Step.release _ k g t h1 h2 h3
  | withdraw k h1 h2 => exact Step.withdraw _ k h1 h2

theorem now_mono {a : Auth} {s t : Sys} (h : Step a s t) : s.now ≤ t.now := by
  cases h <;> simp

theorem anchor_mono {a : Auth} {s t : Sys} (h : Step a s t) : ∃ l, t.anchor = s.anchor ++ l := by
  cases h with
  | reserve k g => exact ⟨_, rfl⟩
  | send k => exact ⟨_, rfl⟩
  | outcome k b => exact ⟨_, rfl⟩
  | release k => exact ⟨_, rfl⟩
  | withdraw k => exact ⟨_, rfl⟩
  | _ => exact ⟨[], by simp⟩

/-! ## O invariante -/

/-- O invariante do protocolo. -/
structure Inv (s : Sys) : Prop where
  /-- o que saiu tem autorização de envio ancorada -/
  sent_send : ∀ k, k ∈ s.sent → sendOf s.anchor k
  /-- autorização de envio e liberação se excluem -/
  send_release : ∀ k, sendOf s.anchor k → Rec.release k ∉ s.anchor
  /-- toda autorização de envio tem reserva -/
  send_reserve : ∀ k t, Rec.send k t ∈ s.anchor → ∃ g t', Rec.reserve k g t' ∈ s.anchor
  /-- todo desfecho tem autorização de envio, e diz a verdade sobre o provedor -/
  outcome_ok : ∀ k b, Rec.outcome k b ∈ s.anchor →
    sendOf s.anchor k ∧ (b = true → k ∈ s.applied) ∧ (b = false → k ∉ s.applied)
  /-- o provedor só aplica o que saiu -/
  applied_sent : ∀ k, k ∈ s.applied → k ∈ s.sent
  /-- no máximo uma reserva viva por concessão -/
  one_live : ∀ k k' g t t', Rec.reserve k g t ∈ s.anchor → Rec.reserve k' g t' ∈ s.anchor →
    Rec.release k ∉ s.anchor → Rec.release k' ∉ s.anchor → k = k'

theorem inv_init : Inv init := by
  constructor <;> intros <;> simp_all [init, sendOf]

private theorem send_app {A : List Rec} {k : Nat} {r : Rec} (h : sendOf A k) : sendOf (A ++ [r]) k := by
  obtain ⟨t, ht⟩ := h
  exact ⟨t, List.mem_append_left _ ht⟩

theorem inv_step {a : Auth} {s t : Sys} (hi : Inv s) (h : Step a s t) : Inv t := by
  obtain ⟨i1, i2, i3, i4, i5, i6⟩ := hi
  cases h with
  | tick => exact ⟨i1, i2, i3, i4, i5, i6⟩
  | clockSet => exact ⟨i1, i2, i3, i4, i5, i6⟩
  | walSet => exact ⟨i1, i2, i3, i4, i5, i6⟩
  | reserve k g w _ _ _ hnew hfree =>
    refine ⟨fun k' hk => send_app (i1 k' hk), ?_, ?_, ?_, i5, ?_⟩
    · intro k' ⟨t', ht'⟩ hr
      simp at ht' hr
      exact i2 k' ⟨t', ht'⟩ hr
    · intro k' t' ht'
      simp at ht'
      obtain ⟨g', t'', h⟩ := i3 k' t' ht'
      exact ⟨g', t'', List.mem_append_left _ h⟩
    · intro k' b hb
      simp at hb
      obtain ⟨h1, h2, h3⟩ := i4 k' b hb
      exact ⟨send_app h1, h2, h3⟩
    · intro k1 k2 g' t1 t2 h1 h2 r1 r2
      simp only [List.mem_append, List.mem_singleton, Rec.reserve.injEq] at h1 h2 r1 r2
      simp at r1 r2
      rcases h1 with h1 | ⟨e1, f1, _⟩ <;> rcases h2 with h2 | ⟨e2, f2, _⟩
      · exact i6 k1 k2 g' t1 t2 h1 h2 r1 r2
      · rw [f2] at h1
        exact absurd (hfree k1 t1 h1) r1
      · rw [f1] at h2
        exact absurd (hfree k2 t2 h2) r2
      · rw [e1, e2]
  | send k g t' w hres hns hnr _ _ _ =>
    refine ⟨fun k' hk => send_app (i1 k' hk), ?_, ?_, ?_, i5, ?_⟩
    · intro k' ⟨t'', ht''⟩ hr
      simp at ht'' hr
      rcases ht'' with ht'' | ⟨rfl, _⟩
      · exact i2 k' ⟨t'', ht''⟩ hr
      · exact hnr hr
    · intro k' t'' ht''
      simp at ht''
      rcases ht'' with ht'' | ⟨rfl, _⟩
      · obtain ⟨g', t3, h⟩ := i3 k' t'' ht''
        exact ⟨g', t3, List.mem_append_left _ h⟩
      · exact ⟨g, t', List.mem_append_left _ hres⟩
    · intro k' b hb
      simp at hb
      obtain ⟨h1, h2, h3⟩ := i4 k' b hb
      exact ⟨send_app h1, h2, h3⟩
    · intro k1 k2 g' t1 t2 h1 h2 r1 r2
      simp at h1 h2 r1 r2
      exact i6 k1 k2 g' t1 t2 h1 h2 r1 r2
  | emit k w hs _ _ _ _ =>
    refine ⟨?_, i2, i3, i4, ?_, i6⟩
    · intro k' hk
      simp at hk
      rcases hk with rfl | hk
      · exact hs
      · exact i1 k' hk
    · intro k' hk
      exact List.mem_cons_of_mem _ (i5 k' hk)
  | apply k hk hno =>
    refine ⟨i1, i2, i3, ?_, ?_, i6⟩
    · intro k' b hb
      obtain ⟨h1, h2, h3⟩ := i4 k' b hb
      refine ⟨h1, fun hb' => List.mem_cons_of_mem _ (h2 hb'), fun hb' hm => ?_⟩
      simp at hm
      rcases hm with rfl | hm
      · subst hb'
        exact hno hb
      · exact h3 hb' hm
    · intro k' hk'
      simp at hk'
      rcases hk' with rfl | hk'
      · exact hk
      · exact i5 k' hk'
  | outcome k b hs _ hb1 hb2 =>
    refine ⟨fun k' hk => send_app (i1 k' hk), ?_, ?_, ?_, i5, ?_⟩
    · intro k' ⟨t', ht'⟩ hr
      simp at ht' hr
      exact i2 k' ⟨t', ht'⟩ hr
    · intro k' t' ht'
      simp at ht'
      obtain ⟨g', t'', h⟩ := i3 k' t' ht'
      exact ⟨g', t'', List.mem_append_left _ h⟩
    · intro k' b' hb'
      simp at hb'
      rcases hb' with hb' | ⟨rfl, rfl⟩
      · obtain ⟨h1, h2, h3⟩ := i4 k' b' hb'
        exact ⟨send_app h1, h2, h3⟩
      · exact ⟨send_app hs, hb1, hb2⟩
    · intro k1 k2 g' t1 t2 h1 h2 r1 r2
      simp at h1 h2 r1 r2
      exact i6 k1 k2 g' t1 t2 h1 h2 r1 r2
  | release k g t' hres hns hnr =>
    refine ⟨?_, ?_, ?_, ?_, i5, ?_⟩
    · intro k' hk
      exact send_app (i1 k' hk)
    · intro k' ⟨t'', ht''⟩ hr
      simp at ht'' hr
      rcases hr with hr | rfl
      · exact i2 k' ⟨t'', ht''⟩ hr
      · exact hns ⟨t'', ht''⟩
    · intro k' t'' ht''
      simp at ht''
      obtain ⟨g', t3, h⟩ := i3 k' t'' ht''
      exact ⟨g', t3, List.mem_append_left _ h⟩
    · intro k' b hb
      simp at hb
      obtain ⟨h1, h2, h3⟩ := i4 k' b hb
      exact ⟨send_app h1, h2, h3⟩
    · intro k1 k2 g' t1 t2 h1 h2 r1 r2
      simp at h1 h2 r1 r2
      exact i6 k1 k2 g' t1 t2 h1 h2 r1.1 r2.1
  | withdraw k hs _ =>
    refine ⟨fun k' hk => send_app (i1 k' hk), ?_, ?_, ?_, i5, ?_⟩
    · intro k' ⟨t', ht'⟩ hr
      simp at ht' hr
      exact i2 k' ⟨t', ht'⟩ hr
    · intro k' t' ht'
      simp at ht'
      obtain ⟨g', t'', h⟩ := i3 k' t' ht'
      exact ⟨g', t'', List.mem_append_left _ h⟩
    · intro k' b hb
      simp at hb
      obtain ⟨h1, h2, h3⟩ := i4 k' b hb
      exact ⟨send_app h1, h2, h3⟩
    · intro k1 k2 g' t1 t2 h1 h2 r1 r2
      simp at h1 h2 r1 r2
      exact i6 k1 k2 g' t1 t2 h1 h2 r1 r2

theorem inv_of_reach {a : Auth} {s : Sys} (h : Reach a s) : Inv s := by
  induction h with
  | init => exact inv_init
  | step _ st ih => exact inv_step ih st

/-! ## Os teoremas-alvo -/

/-- **Nenhum efeito sai com concessão expirada, qualquer que seja o relógio local.** Todo passo
que faz um efeito sair o faz com cada concessão reservada para a chave válida no instante real:
`issued ≤ now ≤ expires`. O relógio do executor não aparece. -/
theorem no_expired_effect {a : Auth} {s t : Sys} {k : Nat}
    (h : Step a s t) (hk : k ∈ t.sent) (hn : k ∉ s.sent) :
    ∀ g t', Rec.reserve k g t' ∈ s.anchor → g.issued ≤ s.now ∧ s.now ≤ g.expires := by
  cases h with
  | emit k' w _ _ h1 h2 hv =>
    simp at hk
    rcases hk with rfl | hk
    · intro g t' hr
      exact valid_at_now (hv g t' hr) h1 h2
    · exact absurd hk hn
  | _ => simp_all

/-- O tempo nunca volta, ao longo de qualquer execução. -/
inductive Star (a : Auth) : Sys → Sys → Prop
  | refl (s : Sys) : Star a s s
  | step {s t u : Sys} : Star a s t → Step a t u → Star a s u

theorem star_now {a : Auth} {s t : Sys} (h : Star a s t) : s.now ≤ t.now := by
  induction h with
  | refl => exact Nat.le_refl _
  | step _ st ih => exact Nat.le_trans ih (now_mono st)

theorem star_anchor {a : Auth} {s t : Sys} (h : Star a s t) : ∀ r, r ∈ s.anchor → r ∈ t.anchor := by
  induction h with
  | refl => exact fun _ h => h
  | step _ st ih =>
    intro r hr
    obtain ⟨l, hl⟩ := anchor_mono st
    rw [hl]
    exact List.mem_append_left _ (ih r hr)

/-- **O avanço do tempo não revalida uma concessão expirada.** Se, num estado, uma concessão
reservada para a chave `k` já expirou, nenhum passo de nenhuma continuação faz o efeito de `k`
sair (nem o primeiro envio, nem um reenvio). -/
theorem expired_never_revalidates {a : Auth} {s s' u : Sys} {k : Nat} {g : Grant} {t0 : Nat}
    (hr : Rec.reserve k g t0 ∈ s.anchor) (hexp : g.expires < s.now)
    (hs : Star a s s') (h : Step a s' u) (hk : k ∈ u.sent) (hn : k ∉ s'.sent) : False := by
  have hr' := star_anchor hs _ hr
  have ⟨_, h2⟩ := no_expired_effect h hk hn g t0 hr'
  have := star_now hs
  omega

/-- **Todo efeito enviado tem autorização e histórico ancorado**: reserva (com a concessão) e
autorização de envio. -/
theorem sent_has_history {a : Auth} {s : Sys} {k : Nat} (h : Reach a s) (hk : k ∈ s.sent) :
    sendOf s.anchor k ∧ ∃ g t, Rec.reserve k g t ∈ s.anchor := by
  have i := inv_of_reach h
  obtain ⟨t, ht⟩ := i.sent_send k hk
  exact ⟨⟨t, ht⟩, i.send_reserve k t ht⟩

/-- **Todo desfecho aceito tem evidência associada à operação original**: autorização de envio
e reserva ancoradas, e o que ele diz é o que o provedor fez. -/
theorem outcome_has_evidence {a : Auth} {s : Sys} {k : Nat} {b : Bool} (h : Reach a s)
    (ho : Rec.outcome k b ∈ s.anchor) :
    sendOf s.anchor k ∧ (∃ g t, Rec.reserve k g t ∈ s.anchor) ∧ (b = true ↔ k ∈ s.applied) := by
  have i := inv_of_reach h
  obtain ⟨⟨t, ht⟩, h2, h3⟩ := i.outcome_ok k b ho
  refine ⟨⟨t, ht⟩, i.send_reserve k t ht, ⟨h2, fun hm => ?_⟩⟩
  cases b with
  | true => rfl
  | false => exact absurd hm (h3 rfl)

/-- **Nenhum retrocesso local reintegra uma autorização gasta.** A concessão de um efeito que
saiu não está livre, e isso só depende do registro ancorado: o WAL e os registros locais podem
ser qualquer coisa (o adversário os reescreve à vontade em `walSet`). -/
theorem no_rollback_reauthorizes {a : Auth} {s : Sys} {k : Nat} {g : Grant} {t : Nat}
    (h : Reach a s) (hk : k ∈ s.sent) (hr : Rec.reserve k g t ∈ s.anchor) :
    ¬ free s.anchor g := by
  have i := inv_of_reach h
  intro hf
  exact i.send_release k (i.sent_send k hk) (hf k t hr)

/-- **A liberação é segura**: uma chave liberada nunca saiu. -/
theorem release_sound {a : Auth} {s : Sys} {k : Nat} (h : Reach a s) (hr : Rec.release k ∈ s.anchor) :
    k ∉ s.sent := by
  have i := inv_of_reach h
  intro hk
  exact i.send_release k (i.sent_send k hk) hr

/-- **Consumo linear**: uma concessão paga no máximo um efeito que saiu. -/
theorem one_effect_per_grant {a : Auth} {s : Sys} {k k' : Nat} {g : Grant} {t t' : Nat}
    (h : Reach a s) (hr : Rec.reserve k g t ∈ s.anchor) (hr' : Rec.reserve k' g t' ∈ s.anchor)
    (hk : k ∈ s.sent) (hk' : k' ∈ s.sent) : k = k' := by
  have i := inv_of_reach h
  exact i.one_live k k' g t t' hr hr' (release_sound h · |>.elim hk) (release_sound h · |>.elim hk')

/-! ## A recuperação -/

inductive Decision
  /-- há desfecho ancorado: restaurar o `close` dele, sem consultar o provedor -/
  | restore
  /-- já liberada -/
  | released
  /-- autorização de envio sem desfecho: pode ter saído; o protocolo da classe do provedor -/
  | byClass
  /-- reserva sem autorização de envio: provadamente não saiu; liberar -/
  | release
  /-- sem reserva: provadamente não saiu; nenhuma concessão foi consumida -/
  | unreserved
  deriving DecidableEq, Repr

def hasOutcome (A : List Rec) (k : Nat) : Bool := A.any fun | .outcome k' _ => k' == k | _ => false
def hasRelease (A : List Rec) (k : Nat) : Bool := A.any fun | .release k' => k' == k | _ => false
def hasSend (A : List Rec) (k : Nat) : Bool := A.any fun | .send k' _ => k' == k | _ => false
def hasReserve (A : List Rec) (k : Nat) : Bool := A.any fun | .reserve k' _ _ => k' == k | _ => false

/-- `lc_recover`: a decisão sobre a chave `k` é uma função **só** do registro ancorado. -/
def decide (A : List Rec) (k : Nat) : Decision :=
  if hasOutcome A k then .restore
  else if hasRelease A k then .released
  else if hasSend A k then .byClass
  else if hasReserve A k then .release
  else .unreserved

theorem hasSend_iff {A : List Rec} {k : Nat} : hasSend A k = true ↔ sendOf A k := by
  unfold hasSend sendOf
  rw [List.any_eq_true]
  constructor
  · rintro ⟨r, hr, he⟩
    cases r <;> simp at he
    subst he
    exact ⟨_, hr⟩
  · rintro ⟨t, ht⟩
    exact ⟨_, ht, by simp⟩

/-- **A recuperação é determinística e ignora o que é local**: o WAL e o relógio do executor
não entram na decisão. -/
theorem decide_ignores_local (s : Sys) (l : List Rec) (c k : Nat) :
    decide { s with wal := l, localClock := c }.anchor k = decide s.anchor k := rfl

/-- O que a recuperação libera, ou encerra sem reserva, provadamente não saiu. -/
theorem decide_sound {a : Auth} {s : Sys} {k : Nat} (h : Reach a s)
    (hd : decide s.anchor k = .release ∨ decide s.anchor k = .unreserved) : k ∉ s.sent := by
  intro hk
  have hs : hasSend s.anchor k = true := hasSend_iff.mpr ((inv_of_reach h).sent_send k hk)
  unfold decide at hd
  split at hd
  · simp at hd
  · split at hd
    · simp at hd
    · simp_all

/-- **O limite do provedor opaco.** Há dois estados alcançáveis com o mesmo registro ancorado,
o mesmo WAL e o mesmo relógio local, em que o efeito saiu nos dois, foi aplicado num e não no
outro. Sem recibo, nenhuma evidência durável os distingue. -/
theorem opaque_ambiguity (a : Auth) (g : Grant) (hop : g.op ∈ a.ops) (hage : g.expires - g.issued ≤ a.maxAge)
    (hwin : g.issued = 0) (hexp : 0 < g.expires) :
    ∃ s1 s2, Reach a s1 ∧ Reach a s2 ∧ s1.anchor = s2.anchor ∧ s1.wal = s2.wal ∧
      s1.localClock = s2.localClock ∧ s1.sent = s2.sent ∧ 0 ∈ s1.applied ∧ 0 ∉ s2.applied := by
  let w : Att := ⟨0, 0⟩
  have hv : valid a g w := ⟨hop, hage, by simp [w, hwin], by simp [w]⟩
  let r1 : Sys := { init with anchor := [Rec.reserve 0 g 0] }
  let r2 : Sys := { r1 with anchor := r1.anchor ++ [Rec.send 0 0] }
  let r3 : Sys := { r2 with sent := [0] }
  let r4 : Sys := { r3 with applied := [0] }
  have h1 : Reach a r1 :=
    .step .init (Step.reserve init 0 g w (by simp [w, init]) (by simp [w, init]) hv (by simp [init]) (by simp [free, init]))
  have h2 : Reach a r2 :=
    .step h1 (Step.send r1 0 g 0 w (by simp [r1]) (by simp [sendOf, r1]) (by simp [r1]) (by simp [w, r1, init]) (by simp [w, r1, init]) hv)
  have h3 : Reach a r3 := by
    refine .step h2 (Step.emit r2 0 w ⟨0, by simp [r2, r1]⟩ ⟨g, 0, by simp [r2, r1]⟩ (by simp [w, r2, r1, init]) (by simp [w, r2, r1, init]) ?_)
    intro g' t' hr
    simp [r2, r1] at hr
    obtain ⟨rfl, rfl⟩ := hr
    exact hv
  have h4 : Reach a r4 := .step h3 (Step.apply r3 0 (by simp [r3]) (by simp [r3, r2, r1]))
  exact ⟨r4, r3, h4, h3, rfl, rfl, rfl, rfl, by simp [r4], by simp [r3, r2, r1, init]⟩

/-- E, por isso, nenhuma recuperação que só olhe a evidência durável decide se o efeito de um
provedor opaco foi aplicado. -/
theorem opaque_undecidable (a : Auth) (g : Grant) (hop : g.op ∈ a.ops) (hage : g.expires - g.issued ≤ a.maxAge)
    (hwin : g.issued = 0) (hexp : 0 < g.expires) :
    ¬ ∃ f : List Rec → List Rec → Bool, ∀ s, Reach a s → (f s.anchor s.wal = true ↔ 0 ∈ s.applied) := by
  rintro ⟨f, hf⟩
  obtain ⟨s1, s2, h1, h2, ha, hw, _, _, hin, hout⟩ := opaque_ambiguity a g hop hage hwin hexp
  have e1 := (hf s1 h1).mpr hin
  rw [ha, hw] at e1
  exact hout ((hf s2 h2).mp e1)

/-! ## A tabela de decisões, confrontada com o executável -/

/-- Os pontos de `--kill-at` do executável, na fronteira de uma operação autorizada. -/
inductive Point
  | authorized | logged | reserved | intent | sent | confirmed | closed
  deriving DecidableEq, Repr

/-- O registro ancorado em que a queda no ponto deixa a operação `0`. -/
def anchorAt (g : Grant) : Point → List Rec
  | .authorized => []
  | .logged => []
  | .reserved => [Rec.reserve 0 g 0]
  | .intent => [Rec.reserve 0 g 0, Rec.send 0 0]
  | .sent => [Rec.reserve 0 g 0, Rec.send 0 0]
  | .confirmed => [Rec.reserve 0 g 0, Rec.send 0 0, Rec.outcome 0 true]
  | .closed => [Rec.reserve 0 g 0, Rec.send 0 0, Rec.outcome 0 true]

/-- A tabela que `crates/athc/tests/rc4.rs` lê deste arquivo e confronta com o ARX. -/
def expected : List (Point × Decision) :=
  [(.authorized, .unreserved), (.logged, .unreserved), (.reserved, .release), (.intent, .byClass),
   (.sent, .byClass), (.confirmed, .restore), (.closed, .restore)]

theorem expected_is_decide (g : Grant) : ∀ pd ∈ expected, decide (anchorAt g pd.1) 0 = pd.2 := by
  intro pd h
  simp [expected] at h
  rcases h with h | h | h | h | h | h | h <;> subst h <;>
    simp [decide, anchorAt, hasOutcome, hasRelease, hasSend, hasReserve]

/-! ## O contraexemplo da RC3, no modelo -/

/-- A RC3 conferia o prazo no relógio do executor. Há um estado alcançável em que a concessão
está expirada no instante real e válida no relógio local: o adversário só precisou de `clockSet`. -/
theorem rc3_local_clock_counterexample (a : Auth) (g : Grant) (hexp : g.issued ≤ g.expires) :
    ∃ s, Reach a s ∧ g.expires < s.now ∧ (g.issued ≤ s.localClock ∧ s.localClock ≤ g.expires) := by
  -- o tempo real passa além do prazo; o relógio local é posto dentro dele
  have tick : ∀ n, Reach a { init with now := n } := by
    intro n
    induction n with
    | zero => exact .init
    | succ n ih => exact .step ih (Step.tick _)
  refine ⟨{ init with now := g.expires + 1, localClock := g.issued }, ?_, by simp, ?_⟩
  · exact .step (tick (g.expires + 1)) (Step.clockSet _ g.issued)
  · refine ⟨?_, ?_⟩ <;> simp <;> omega

end Aletheia.Lifecycle
