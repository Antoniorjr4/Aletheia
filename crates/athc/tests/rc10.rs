//! **v1.0-RC10: confidencialidade — o fluxo tóxico barrado por tipo.**
//!
//! A dimensão de confidencialidade (`Public ⊑ Internal ⊑ Secret`), dual da integridade: um dado
//! lido de uma fonte privada é `^ Secret`; um canal público declara `^ Public`; e nenhum caminho
//! de um `^ Secret` a um `^ Public` passa na verificação. É a perna que faltava contra o "lethal
//! trifecta" (dado privado + conteúdo não confiável + canal de saída) que a reportagem de
//! nov/2026 e o achado da Invariant Labs no servidor MCP do GitHub descrevem.
//!
//! Testes de verificação pura (`athc::compile`), sem runtime.

use ath_check::Stratum;
use athc::compile;

/// Compila e devolve os códigos dos diagnósticos; pânico se for aceito.
fn rejected(src: &str) -> (Stratum, Vec<String>) {
    match compile(src) {
        Ok(_) => panic!("programa aceito, mas deveria ser rejeitado:\n{src}"),
        Err(d) => (d[0].stratum, d.iter().map(|x| x.code.clone()).collect()),
    }
}

fn accepted(src: &str) {
    if let Err(d) = compile(src) {
        panic!("rejeitado: {:?}", d.iter().map(|x| format!("{} {} {}", x.code, x.msg, x.label)).collect::<Vec<_>>());
    }
}

/// Um módulo com um leitor de fonte privada (`-> Text ^ Secret`), um sink público
/// (`body: ^ Public`) e um sink interno (`^ Internal`), e um agente cujo corpo de `go` é `$BODY`.
fn prog(body: &str) -> String {
    format!(
        r#"module t

tool read_private(c: Cap[Repo.Read], issue: Text[..8]) -> Text[..600] ^ Secret
  ! {{ Repo.Read }} reversible latency 1.0

tool read_internal(c: Cap[Repo.Read], issue: Text[..8]) -> Text[..600] ^ Internal
  ! {{ Repo.Read }} reversible latency 1.0

tool read_public(c: Cap[Repo.Read], issue: Text[..8]) -> Text[..600]
  ! {{ Repo.Read }} reversible latency 1.0

tool post_public(c: Cap[Repo.Write], body: Text[..600] ^ Public) -> Unit
  ! {{ Repo.Write }} irrevocable latency 1.0

tool post_internal(c: Cap[Repo.Write], body: Text[..600] ^ Internal) -> Unit
  ! {{ Repo.Write }} irrevocable latency 1.0

tool sink_any(c: Cap[Repo.Write], body: Text[..600]) -> Unit
  ! {{ Repo.Write }} irrevocable latency 1.0

agent A(r: Cap[Repo.Read], w: Cap[Repo.Write])
  budget [tok <= 100, usd <= 1.0, wall <= 100.0]
{{
  on go(n: Text[..8] ^ Trusted) -> Unit ! {{ Repo.Read, Repo.Write }} {{
{body}
  }}
}}
"#
    )
}

fn leak(code: &str) -> Vec<String> {
    let (st, codes) = rejected(&prog(code));
    assert_eq!(st, Stratum::E3, "estrato errado para:\n{code}");
    codes
}

/// **O fluxo direto é recusado.** Ler privado e publicar num canal público.
#[test]
fn secret_into_a_public_sink_is_refused() {
    assert!(leak("    let s = read_private(r, n)\n    commit post_public(w, s)").contains(&"E0440".to_string()));
}

/// **A derivação carrega o rótulo.** Concatenar, projetar ou compor a partir do segredo não o
/// torna público: o texto derivado continua `Secret`.
#[test]
fn deriving_from_secret_does_not_launder_it() {
    assert!(leak("    let s = read_private(r, n)\n    commit post_public(w, \"vazou: \" ++ s)")
        .contains(&"E0440".to_string()));
    assert!(leak("    let s = read_private(r, n)\n    commit post_public(w, s ++ s)").contains(&"E0440".to_string()));
}

/// **O fluxo implícito é recusado.** Ramificar sobre o segredo e agir no ramo expõe pela escolha
/// do ramo (o `pc` de confidencialidade), mesmo que o argumento do sink seja público.
#[test]
fn an_implicit_flow_through_a_secret_branch_is_refused() {
    let codes = leak(
        "    let s = read_private(r, n)\n    \
         if s == \"segredo\" {\n      commit post_public(w, \"ping\")\n    } else {\n      commit post_public(w, \"pong\")\n    }",
    );
    assert!(codes.contains(&"E0440".to_string()), "{codes:?}");
}

/// **O reticulado é respeitado.** `Secret` não entra num canal `Internal`; `Internal` não entra
/// num canal `Public`; mas `Public` entra em qualquer um, e `Secret`/`Internal` entram num canal
/// sem rótulo (que tolera tudo).
#[test]
fn the_lattice_is_respected() {
    assert!(leak("    let s = read_private(r, n)\n    commit post_internal(w, s)").contains(&"E0440".to_string()));
    assert!(leak("    let s = read_internal(r, n)\n    commit post_public(w, s)").contains(&"E0440".to_string()));
    // Public em canal Internal: ok. Public em canal público: ok. Secret em sink sem rótulo: ok.
    accepted(&prog(
        "    let p = read_public(r, n)\n    commit post_internal(w, p)\n    commit post_public(w, p)\n    let s = read_private(r, n)\n    commit sink_any(w, s)",
    ));
}

/// **O canal público aceita dado público.** A triagem legítima (compor e publicar texto que não
/// vem de fonte secreta) continua passando: a dimensão não atrapalha o trabalho real.
#[test]
fn a_public_channel_accepts_public_data() {
    accepted(&prog("    let p = read_public(r, n)\n    commit post_public(w, p)"));
}

/// **Rótulos repetidos ou desconhecidos são recusados na sintaxe.**
#[test]
fn malformed_labels_are_rejected() {
    let bad = |ty: &str| {
        let src = format!(
            "module t\ntool f(c: Cap[X.R]) -> {ty} ! {{ X.R }} reversible latency 1.0\n\nagent A(c: Cap[X.R]) budget [tok<=1, usd<=0.1, wall<=1.0] {{ on go() -> Unit ! {{ X.R }} {{ let _ = f(c) }} }}"
        );
        compile(&src).is_err()
    };
    assert!(bad("Text ^ Secret ^ Internal")); // confidencialidade repetida
    assert!(bad("Text ^ Public ^ Public"));
    assert!(bad("Text ^ Segredo")); // desconhecido
}

/// **Os dois eixos são independentes.** Um valor pode ser `Untrusted` e `Public` (o texto de um
/// issue público: não escolhe ação, mas pode ser repostado) e `Trusted` e `Secret` (um segredo
/// de configuração confiável que não pode sair). Um tipo carrega os dois rótulos.
#[test]
fn integrity_and_confidentiality_are_orthogonal() {
    // Trusted ^ Secret num sink público: recusado por confidencialidade, não por integridade.
    let src = r#"module t
tool keyvault(c: Cap[Vault.Read]) -> Text[..64] ^ Trusted ^ Secret
  ! { Vault.Read } reversible latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text[..64] ^ Public) -> Unit
  ! { Repo.Write } irrevocable latency 1.0
agent A(v: Cap[Vault.Read], w: Cap[Repo.Write]) budget [tok<=10, usd<=1.0, wall<=10.0] {
  on go() -> Unit ! { Vault.Read, Repo.Write } {
    let token = keyvault(v)
    commit post_public(w, token)
  }
}
"#;
    let (st, codes) = rejected(src);
    assert_eq!(st, Stratum::E3);
    assert!(codes.contains(&"E0440".to_string()) && !codes.contains(&"E0412".to_string()), "{codes:?}");
}
