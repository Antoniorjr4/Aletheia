//! **Defeito registrado (28/11/2026): o resultado de ferramenta sem rótulo não herda os argumentos.**
//!
//! `bench/ferramentas/DEFEITO.md`. Um retorno sem rótulo é `Trusted` e `Public`: um dado
//! `Untrusted` ou `Secret` que passa por uma ferramenta assim chega a parâmetro `^ Endorsed`, decide
//! efeito irrevogável ou sai por sink `^ Public`. Não corrigido. Este teste fixa as reproduções: os
//! controles diretos são recusados; os fluxos pela ferramenta, aceitos. Se a correção for aplicada,
//! ele falha e o registro deve ser atualizado.

use athc::compile;
use std::path::PathBuf;

fn codigos(nome: &str) -> Vec<String> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../bench/ferramentas/{nome}.ath"));
    compile(&std::fs::read_to_string(p).unwrap()).err().unwrap_or_default().into_iter().map(|d| d.code).collect()
}

#[test]
fn os_controles_diretos_sao_recusados() {
    assert_eq!(codigos("t0_direto"), vec!["E0412", "E0412"]);
    assert_eq!(codigos("t2_retorno_untrusted"), vec!["E0412", "E0412"]);
    assert_eq!(codigos("t5b_segredo_direto"), vec!["E0440"]);
}

#[test]
fn o_fluxo_pela_ferramenta_sem_rotulo_e_aceito_defeito_registrado() {
    for nome in [
        "t1_untrusted_por_ferramenta",
        "t1b_untrusted_decide_efeito",
        "t1c_inline_na_condicao",
        "t4_recusado_pela_politica_por_ferramenta",
        "t5_segredo_por_ferramenta",
    ] {
        assert!(codigos(nome).is_empty(), "{nome}: mudou; atualize bench/ferramentas/DEFEITO.md");
    }
    // a asserção explícita do programador e o endossado que perde a origem: aceitos por desenho
    assert!(codigos("t2b_retorno_trusted_explicito").is_empty());
    assert!(codigos("t3_endossado_por_ferramenta").is_empty());
}
